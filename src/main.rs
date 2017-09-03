#[macro_use]
extern crate clap;

#[macro_use]
extern crate log;
extern crate futures;
extern crate microservice;
extern crate mowl;

use std::error::Error;
use std::process::exit;

use clap::App;
use futures::Future;
use log::LogLevel;

use microservice::Microservice;
use microservice::errors::*;

fn error_and_exit(string: &str, error: &Error) {
    error!("{}: {}", string, error);
    exit(1);
}

pub fn main() {
    if let Err(error) = run() {
        error_and_exit("Main", &error);
    }
}

fn run() -> Result<()> {
    // Load the CLI parameters from the yaml file
    let yaml = load_yaml!("cli.yaml");
    let app = App::from_yaml(yaml).version(crate_version!());
    let matches = app.clone().get_matches();

    // Set the verbosity level
    let log_level = match matches.occurrences_of("verbose") {
        0 => LogLevel::Info, // Default value
        1 => LogLevel::Debug,
        _ => LogLevel::Trace,
    };

    // Init the logging
    match mowl::init_with_level(log_level) {
        Err(_) => warn!("Log level already set"),
        Ok(_) => info!("Log level set to: {}", log_level),
    }

    let address = matches.value_of("address").ok_or_else(
        || "No CLI 'address' provided",
    )?;

    // Create the microservice instance
    let microservice = Microservice::new(address)?;

    // Check if testing is enabled
    if matches.is_present("test") {
        // Get a client to the microservice
        let (client, mut rpc) = microservice.get_client()?;

        // Assemble the request
        let mut request = client.hello_request();
        request.get().set_request("Hello");

        // Run the RPC
        info!("Running the RPC.");
        rpc.run(request.send().promise.and_then(|message| {
            // Get the response content
            let response = message.get()?.get_response()?;
            info!("Got response: {}", response);

            // Check the result
            assert_eq!(response, "olleH");
            Ok(())
        }))?;

        info!("Test passed.");
    } else {
        // Start the server
        microservice.serve()?;
    }

    Ok(())
}
