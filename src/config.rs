
use std::net::SocketAddr;
use std::result::Result;
use std::time::Duration;
use std::option::Option;
use std::io;
use serde_yaml;
use std::fs::File;
use std::error::Error;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ConfigError {
    IoError(String),
    SyntaxError(String),
}

impl From<io::Error> for ConfigError {
    fn from(e: io::Error) -> Self {
        ConfigError::IoError(String::from(e.description()))
    }
}

impl From<serde_yaml::Error> for ConfigError {
    fn from(e: serde_yaml::Error) -> Self {
        ConfigError::SyntaxError(String::from(e.description()))
    }
}

#[derive(Copy, Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub enum AwsCredentialsProviderType {
    Default,
    Environment,
    Profile,
    Instance,
    Container
}

impl Default for AwsCredentialsProviderType {
    fn default() -> AwsCredentialsProviderType {
        AwsCredentialsProviderType::Default
    }
}

pub trait AwsInstancesPollerSettingsProvider {
    fn aws_instances_poller_settings(&self) -> AwsInstancesPollerSettings;
}

pub trait ScrapeSettingsProvider {
    fn listen_on(&self) -> SocketAddr;
    fn read_timeout(&self) -> Option<Duration>;
    fn keep_alive_timeout(&self) -> Option<Duration>;
    fn polling_period(&self) -> Option<Duration>;
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AwsInstancesPollerSettings {
    pub credentials_provider: Option<AwsCredentialsProviderType>,
    pub region: String,
    pub expose_tags: Vec<String>,
    pub describe_instances_chunk_size: Option<i32>,
}

#[derive(Serialize, Deserialize)]
struct  ScrapeSettings {
    polling_period: Option<u64>,
    listen_on: SocketAddr,
    read_timeout: Option<u64>,
    keep_alive_timeout: Option<u64>,
}

#[derive(Serialize, Deserialize)]
pub struct DeucalionSettings {
    aws_instances_poller_settings: AwsInstancesPollerSettings,
    scrape_settings: ScrapeSettings
}

impl DeucalionSettings {
    pub fn from_filename(filename: &str) -> Result<Self, ConfigError>
    {
        Ok(serde_yaml::from_reader(File::open(filename)?)?)
    }
}

impl AwsInstancesPollerSettingsProvider for DeucalionSettings {
    fn aws_instances_poller_settings(&self) -> AwsInstancesPollerSettings {
        self.aws_instances_poller_settings.clone()
    }
}

impl ScrapeSettingsProvider for DeucalionSettings {
    fn listen_on(&self) -> SocketAddr {
        self.scrape_settings.listen_on
    }

    fn read_timeout(&self) -> Option<Duration> {
        self.scrape_settings.read_timeout.map(|s| Duration::from_secs(s))
    }

    fn keep_alive_timeout(&self) -> Option<Duration> {
        self.scrape_settings.keep_alive_timeout.map(|s| Duration::from_secs(s))
    }

    fn polling_period(&self) -> Option<Duration> {
        self.scrape_settings.polling_period.map(Duration::from_secs)
    }
}
