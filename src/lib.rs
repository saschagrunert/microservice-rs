//! # Microservice Template
//!
//! This crate contains the library for the basic microservice template using Cap'n Proto and Rust.
//!
#![cfg_attr(feature="clippy", feature(plugin))]
#![deny(missing_docs)]

#[macro_use]
extern crate capnp_rpc;
extern crate capnp;
extern crate futures;
extern crate tokio_core;
extern crate tokio_io;

#[macro_use]
extern crate error_chain;

#[macro_use]
extern crate log;

#[macro_use]
pub mod errors;
pub mod microservice_capnp {
    #![allow(missing_docs)]
    include!(concat!(env!("OUT_DIR"), "/proto/microservice_capnp.rs"));
}

use std::net::ToSocketAddrs;

use capnp::capability::Promise;
use capnp_rpc::{RpcSystem, twoparty, rpc_twoparty_capnp, Server};
use futures::{Future, Stream};
use tokio_core::{net, reactor};
use tokio_io::AsyncRead;

use errors::*;
use microservice_capnp::microservice;

#[derive(Debug)]
/// The main microservice structure
pub struct Microservice {
    socket_addr: std::net::SocketAddr,
}

impl Microservice {
    /// Create a new microservice instance
    pub fn new(address: &str) -> Result<Self> {
        let parsed_address = address.to_socket_addrs()?.next().ok_or_else(
            || "Could not parse socket address.",
        )?;

        info!("Parsed socket address: {:}", parsed_address);
        Ok(Microservice { socket_addr: parsed_address })
    }

    /// Runs the server
    pub fn serve(&self) -> Result<()> {
        info!("Creating server.");
        let mut core = reactor::Core::new()?;
        let handle = core.handle();
        let socket = net::TcpListener::bind(&self.socket_addr, &handle)?;
        info!("Binding socket.");

        let calc = microservice::ToClient::new(ServerImplementation).from_server::<Server>();

        let done = socket.incoming().for_each(move |(socket, _addr)| {
            try!(socket.set_nodelay(true));
            let (reader, writer) = socket.split();

            let handle = handle.clone();

            let network = twoparty::VatNetwork::new(
                reader,
                writer,
                rpc_twoparty_capnp::Side::Server,
                Default::default(),
            );

            let rpc_system = RpcSystem::new(Box::new(network), Some(calc.clone().client));
            handle.spawn(rpc_system.map_err(|e| println!("error: {:?}", e)));
            Ok(())
        });

        info!("Running server.");
        Ok(core.run(done)?)
    }

    /// Retrieve a client to the microservice instance
    pub fn get_client(&self) -> Result<(microservice::Client, reactor::Core)> {
        info!("Creating client.");
        let mut core = reactor::Core::new()?;
        let handle = core.handle();
        let stream = core.run(
            net::TcpStream::connect(&self.socket_addr, &handle),
        )?;
        stream.set_nodelay(true)?;
        let (reader, writer) = stream.split();

        let network = Box::new(twoparty::VatNetwork::new(
            reader,
            writer,
            rpc_twoparty_capnp::Side::Client,
            Default::default(),
        ));
        let mut rpc_system = RpcSystem::new(network, None);
        let client: microservice::Client = rpc_system.bootstrap(rpc_twoparty_capnp::Side::Server);
        handle.spawn(rpc_system.map_err(|_e| ()));

        info!("Client creation successful.");
        Ok((client, core))
    }
}

struct ServerImplementation;

impl microservice::Server for ServerImplementation {
    fn hello(
        &mut self,
        params: microservice::HelloParams,
        mut results: microservice::HelloResults,
    ) -> Promise<(), capnp::Error> {
        // Get the request
        let request = pry!(pry!(params.get()).get_request());
        info!("Got request: {}", request);

        // Create the response
        let response: String = request.chars().rev().collect();
        results.get().set_response(&response);
        info!("Returned reponse: {}", response);

        // Finish the future
        Promise::ok(())
    }
}
