extern crate capnpc;

use capnpc::CompilerCommand;

fn main() {
    CompilerCommand::new()
        .file("proto/microservice.capnp")
        .run()
        .unwrap();
}
