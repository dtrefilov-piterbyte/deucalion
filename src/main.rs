extern crate prometheus;
extern crate hyper;
extern crate dotenv;
extern crate rusoto;
extern crate ctrlc;
#[macro_use]
extern crate serde_derive;
extern crate serde_yaml;
extern crate time;

mod config;
mod poller;
mod periodic;
mod server;
mod termination;

use std::time::Duration;
use hyper::server::Server;
use config::{ScrapeSettingsProvider, PollerSettingsProvider};
use server::DeucalionHandler;
use poller::AwsPoller;
use periodic::AsyncPeriodicRunner;
use termination::TerminationGuard;
use prometheus::{TextEncoder, Registry};

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

    let config = config::DeucalionSettings::from_filename("config.yml")
        .expect("Could not load configuration");
    let aws_poller = AwsPoller::new(&config)
        .expect("Could not initialize AWS poller");
    let polling_period = config.polling_period().unwrap_or(Duration::from_secs(10));

    let aws_gauges = aws_poller.counters();
    let registry = Registry::new();
    registry.register(aws_gauges).unwrap();

    let mut listening = Server::http(config.listen_on())
        .unwrap()
        .handle(DeucalionHandler::new(TextEncoder::new(), registry))
        .unwrap();
    let _runner = AsyncPeriodicRunner::new(aws_poller, polling_period);
    TerminationGuard::new();

    let _ = listening.close();
}
