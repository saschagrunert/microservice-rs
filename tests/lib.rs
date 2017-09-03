extern crate futures;
extern crate microservice;

use futures::Future;
use microservice::Microservice;
use std::{thread, time};

#[test]
fn hello_success() {
    let addr = "127.0.0.1:30080";
    let server_cert = "tests/certificate.pfx";
    let client_cert = "tests/certificate.der";

    // Run the server in a differenc instance
    thread::spawn(move || {
                      Microservice::new(addr)
                          .unwrap()
                          .serve(server_cert)
                          .unwrap();
                  });

    // Wait for the server to become ready
    let time = time::Duration::from_secs(1);
    thread::sleep(time);

    // Get a client to the microservice
    let (client, mut rpc) = Microservice::new(addr)
        .unwrap()
        .get_client(client_cert)
        .unwrap();

    // Assemble the request
    let mut request = client.hello_request();
    request.get().set_request("Hello");

    // Run the RPC
    rpc.run(request
                 .send()
                 .promise
                 .and_then(|message| {
                               // Get the response content
                               let response = message.get()?.get_response()?;

                               // Check the result
                               assert_eq!(response, "olleH");
                               Ok(())
                           }))
        .unwrap();
}
