//! Basic error handling mechanisms
#![allow(unused_doc_comment)]

use std::io;
use capnp;

error_chain! {
    foreign_links {
         Capnp(capnp::Error) #[doc = "A Cap'n Proto error."];
         Io(io::Error) #[doc = "A I/O error."];
    }
}
