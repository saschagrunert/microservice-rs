//! Basic error handling mechanisms
#![allow(unused_doc_comment)]

use std::io;
use {capnp, native_tls, openssl};

error_chain! {
    foreign_links {
         Capnp(capnp::Error) #[doc = "A Cap'n Proto error."];
         Io(io::Error) #[doc = "A I/O error."];
         OpenSSL(openssl::error::ErrorStack) #[doc = "An OpenSSL error."];
         Tls(native_tls::Error) #[doc = "A TLS error."];
    }
}
