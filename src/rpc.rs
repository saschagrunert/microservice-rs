use capnp;
use capnp::capability::Promise;
use microservice_capnp::microservice;

/// The main RPC implementation structure
pub struct Rpc;

impl microservice::Server for Rpc {
    fn hello(&mut self,
             params: microservice::HelloParams,
             mut results: microservice::HelloResults)
             -> Promise<(), capnp::Error> {
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
