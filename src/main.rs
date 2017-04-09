extern crate prometheus;
extern crate hyper;
extern crate dotenv;
extern crate rusoto;
extern crate ctrlc;
#[macro_use]
extern crate serde_derive;
extern crate serde_yaml;
extern crate time;
extern crate env_logger;

mod config;
mod poller;
mod periodic;
mod server;
mod termination;
mod pagination;
mod aws_poller;

use std::time::Duration;
use hyper::server::Server;
use config::{ScrapeSettingsProvider};
use server::DeucalionHandler;
use poller::Poller;
use aws_poller::{AwsInstancesPoller, AwsSpotPricesPoller};
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
    env_logger::init().unwrap();

    let config = config::DeucalionSettings::from_filename("config.yml")
        .expect("Could not load configuration");
    let polling_period = config.polling_period()
        .unwrap_or(Duration::from_secs(60));
    let aws_instances_poller = AwsInstancesPoller::new(&config)
        .expect("Could not initialize AWS Instances poller");
    let aws_spot_prices_poller = AwsSpotPricesPoller::new(&config)
        .expect("Could not initialize AWS Spot Prices poller");

    let registry = Registry::new();
    registry.register(aws_instances_poller.counters()).unwrap();
    registry.register(aws_spot_prices_poller.counters()).unwrap();

    let mut listening = Server::http(config.listen_on())
        .unwrap()
        .handle(DeucalionHandler::new(TextEncoder::new(), registry))
        .unwrap();
    let _aws_instances_runner = AsyncPeriodicRunner::new(aws_instances_poller, polling_period.clone());
    let _aws_spot_prices_runner = AsyncPeriodicRunner::new(aws_spot_prices_poller, polling_period.clone());
    TerminationGuard::new();

    let _ = listening.close();
}
