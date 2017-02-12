#[macro_use]
extern crate prometheus;
extern crate hyper;
#[macro_use]
extern crate lazy_static;
extern crate dotenv;
extern crate rusoto;
extern crate ctrlc;

mod config;
mod metrics;
mod server;
mod termination;

use hyper::server::Server;
use config::ExporterConfigurationProvider;
use server::DeucalionHandler;
use metrics::AsyncPeriodicRunner;
use termination::TerminationGuard;

fn inject_environment() {
    match dotenv::dotenv() {
        Ok(_) | Err(dotenv::DotenvError::Io) => // it is ok if the .env file was not found
            return,
        Err(dotenv::DotenvError::Parsing {line}) =>
            panic!(".env file parsing failed at {:?}", line),
        Err(err) => panic!(err)
    }
}

fn main() {
    inject_environment();

    let config = config::EnvConfig::new().unwrap();

    println!("listening address {:?} {:?} {:?}", config.listen_on(),
        config.read_timeout(), config.keep_alive_timeout());
    let mut listening = Server::http(config.listen_on())
        .unwrap()
        .handle(DeucalionHandler::new())
        .unwrap();
    let _poller = AsyncPeriodicRunner::new(std::time::Duration::from_secs(5));
    let _g = TerminationGuard::new();
    println!("In the outer scope");
    let _ = listening.close();
}
