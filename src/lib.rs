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
extern crate openssl;
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
mod rpc;

use std::env::current_dir;
use std::net::ToSocketAddrs;
use std::fs::File;
use std::io::{self, Read, Write};

use capnp_rpc::{RpcSystem, twoparty, rpc_twoparty_capnp, Server};
use futures::{Future, Stream};
use native_tls::{Certificate, Pkcs12, TlsAcceptor, TlsConnector};
use openssl::asn1::Asn1Time;
use openssl::bn::{BigNum, MSB_MAYBE_ZERO};
use openssl::hash::MessageDigest;
use openssl::pkey::PKey;
use openssl::rsa::Rsa;
use openssl::x509::extension::{KeyUsage, SubjectAlternativeName};
use openssl::x509::{X509, X509Builder, X509NameBuilder};
use tokio_core::{net, reactor};
use tokio_io::AsyncRead;
use tokio_tls::{TlsAcceptorExt, TlsConnectorExt};

use errors::*;
use microservice_capnp::microservice;

const PKCS12_PASSWORD: &str = "";
const CERT_FILENAME: &str = "cert.pem";

/// The main microservice structure
pub struct Microservice {
    socket_addr: std::net::SocketAddr,
}

impl Microservice {
    /// Create a new microservice instance
    pub fn new(address: &str) -> Result<Self> {
        // Parse socket address
        let parsed_address = address.to_socket_addrs()?.next().ok_or_else(
            || "Could not parse socket address.",
        )?;
        info!("Parsed socket address: {}", parsed_address);

        // Return service
        Ok(Microservice { socket_addr: parsed_address })
    }

    /// Runs the server
    pub fn serve(&self, domains: &[&str]) -> Result<()> {
        info!("Creating server and binding socket.");
        let mut core = reactor::Core::new()?;
        let handle = core.handle();
        let socket = net::TcpListener::bind(&self.socket_addr, &handle)?;

        // Generate TLS server certificate
        let cert = Self::generate_cert(domains)?;
        info!("Created new server ceritifacte");

        // Create the acceptor
        let tls_acceptor = TlsAcceptor::builder(cert)?.build()?;

        let server_impl = microservice::ToClient::new(rpc::Rpc).from_server::<Server>();
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

        let der_cert = X509::from_pem(&bytes)?.to_der()?;
        let cert = Certificate::from_der(&der_cert)?;

        info!("Creating client.");
        let mut core = reactor::Core::new()?;
        let handle = core.handle();

        let socket = net::TcpStream::connect(&self.socket_addr, &handle);
        let mut builder = TlsConnector::builder()?;
        builder.add_root_certificate(cert)?;
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

    fn generate_cert(domains: &[&str]) -> Result<Pkcs12> {
        // Create private key
        let private_key = PKey::from_rsa(Rsa::generate(4096)?)?;

        // Create cert builder
        let mut cert_builder = X509Builder::new()?;
        cert_builder.set_version(3)?;
        cert_builder.set_pubkey(&private_key)?;

        // Generate serial number
        let mut serial = BigNum::new()?;
        serial.rand(128, MSB_MAYBE_ZERO, false)?;
        let asn1_serial = serial.to_asn1_integer()?;
        cert_builder.set_serial_number(&asn1_serial)?;

        // Set certificate subject
        let mut subject_builder = X509NameBuilder::new()?;
        let first_domain = domains.iter().nth(0).unwrap_or(&"");
        info!("Setting server certificate common name: {}", first_domain);
        subject_builder.append_entry_by_text("CN", first_domain)?;
        let subject = subject_builder.build();
        cert_builder.set_subject_name(&subject)?;
        cert_builder.set_issuer_name(&subject)?;

        // Set certificate validity
        let now = Asn1Time::days_from_now(0)?;
        let then = Asn1Time::days_from_now(365 * 10)?;
        cert_builder.set_not_before(&now)?;
        cert_builder.set_not_after(&then)?;

        // Set key usage extension
        let mut key_usage = KeyUsage::new();
        key_usage.non_repudiation();
        key_usage.digital_signature();
        key_usage.key_encipherment();
        let key_ext = key_usage.build()?;
        cert_builder.append_extension(key_ext)?;

        // Set subject alt names if needed
        if domains.len() > 1 {
            let mut san = SubjectAlternativeName::new();
            for domain in domains.iter().skip(1) {
                info!("Setting server subject alt name: {}", domain);
                san.dns(domain);
            }
            let san_ext = san.build(&cert_builder.x509v3_context(None, None))?;
            cert_builder.append_extension(san_ext)?;
        }

        // Sign the ceritifacte
        cert_builder.sign(&private_key, MessageDigest::sha256())?;

        // Create the Pkcs12 bundle
        let cert = cert_builder.build();
        let openssl_pkcs12 = openssl::pkcs12::Pkcs12::builder().build(
            PKCS12_PASSWORD,
            "server",
            &private_key,
            &cert,
        )?;
        let der_bytes = openssl_pkcs12.to_der()?;
        let native_tls_pkcs12 = Pkcs12::from_der(&der_bytes, PKCS12_PASSWORD)?;

        // Save the cert and key
        let cert_pem = cert.to_pem()?;
        let key_pem = private_key.public_key_to_pem()?;
        let filename = current_dir()?.join(CERT_FILENAME);
        let mut file = File::create(&filename)?;
        file.write_all(&cert_pem)?;
        file.write_all(&key_pem)?;
        info!("Wrote certificate to: {}", filename.display());

        Ok(native_tls_pkcs12)
    }
}
