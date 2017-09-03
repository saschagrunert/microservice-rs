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
extern crate native_tls;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_tls;

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
use std::fs::File;
use std::io::{self, Read};

use capnp::capability::Promise;
use capnp_rpc::{RpcSystem, twoparty, rpc_twoparty_capnp, Server};
use futures::{Future, Stream};
use native_tls::{Certificate, Pkcs12, TlsAcceptor, TlsConnector};
use tokio_core::{net, reactor};
use tokio_io::AsyncRead;
use tokio_tls::{TlsAcceptorExt, TlsConnectorExt};

use errors::*;
use microservice_capnp::microservice;

/// The main microservice structure
pub struct Microservice {
    socket_addr: std::net::SocketAddr,
    cert_filename: String,
}

impl Microservice {
    /// Create a new microservice instance
    pub fn new(address: &str, cert_filename: &str) -> Result<Self> {
        // Parse socket address
        let parsed_address = address.to_socket_addrs()?.next().ok_or_else(
            || "Could not parse socket address.",
        )?;

        // Process TLS settings
        info!("Parsed socket address: {:}", parsed_address);

        // Return service
        Ok(Microservice {
            socket_addr: parsed_address,
            cert_filename: cert_filename.to_owned(),
        })
    }

    /// Runs the server
    pub fn serve(&self) -> Result<()> {
        info!("Creating server and binding socket.");
        let mut core = reactor::Core::new()?;
        let handle = core.handle();
        let socket = net::TcpListener::bind(&self.socket_addr, &handle)?;

        // Prepare the vector
        info!("Opening server certificate");
        let mut bytes = vec![];
        File::open(&self.cert_filename)?.read_to_end(&mut bytes)?;

        // Create the certificate
        let cert = Pkcs12::from_der(&bytes, "")?;

        // Create the acceptor
        let tls_acceptor = TlsAcceptor::builder(cert)?.build()?;

        let server_impl = microservice::ToClient::new(ServerImplementation).from_server::<Server>();
        let connections = socket.incoming();

        let tls_handshake = connections.map(|(socket, _addr)| {
            if let Err(e) = socket.set_nodelay(true) {
                error!("Unable to set socket to nodelay: {:}", e);
            }
            tls_acceptor.accept_async(socket)
        });

        let server = tls_handshake.map(|acceptor| {
            let handle = handle.clone();
            let server_impl = server_impl.clone();
            acceptor.and_then(move |socket| {
                let (reader, writer) = socket.split();

                let network = twoparty::VatNetwork::new(
                    reader,
                    writer,
                    rpc_twoparty_capnp::Side::Server,
                    Default::default(),
                );

                let rpc_system = RpcSystem::new(Box::new(network), Some(server_impl.client));
                handle.spawn(rpc_system.map_err(|e| error!("{}", e)));
                Ok(())
            })
        });

        info!("Running server");
        Ok(core.run(server.for_each(|client| {
            handle.spawn(client.map_err(|e| error!("{}", e)));
            Ok(())
        }))?)
    }

    /// Retrieve a client to the microservice instance
    pub fn get_client(&self, client_cert_file: &str) -> Result<(microservice::Client, reactor::Core)> {
        info!("Opening TLS client certificate.");
        let mut bytes = vec![];
        File::open(client_cert_file)?.read_to_end(&mut bytes)?;
        let ca_cert = Certificate::from_der(&bytes)?;

        info!("Creating client.");
        let mut core = reactor::Core::new()?;
        let handle = core.handle();

        let socket = net::TcpStream::connect(&self.socket_addr, &handle);
        let mut builder = TlsConnector::builder()?;
        builder.add_root_certificate(ca_cert)?;
        let cx = builder.build()?;
        let tls_handshake = socket.and_then(|socket| {
            if let Err(e) = socket.set_nodelay(true) {
                error!("Unable to set socket to nodelay: {:}", e);
            }
            cx.connect_async("localhost", socket).map_err(|e| {
                io::Error::new(io::ErrorKind::Other, e)
            })
        });

        let stream = core.run(tls_handshake)?;
        let (reader, writer) = stream.split();

        let network = Box::new(twoparty::VatNetwork::new(
            reader,
            writer,
            rpc_twoparty_capnp::Side::Client,
            Default::default(),
        ));
        let mut rpc_system = RpcSystem::new(network, None);
        let client: microservice::Client = rpc_system.bootstrap(rpc_twoparty_capnp::Side::Server);
        handle.spawn(rpc_system.map_err(|e| error!("{}", e)));

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
