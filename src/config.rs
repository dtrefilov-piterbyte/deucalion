
use std::net::SocketAddr;
use std::str::FromStr;
use std::result::Result;
use std::time::Duration;
use std::option::Option;
use std::io;
use std::io::Read;
use std::error::Error;
use yaml_rust::{YamlLoader, Yaml, ScanError};
use std::fs::File;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ConfigError {
    IoError(String),
    SyntaxError(String),
    OptionNotFound(String),
    BadOptionValue(String)
}

impl From<io::Error> for ConfigError {
    fn from(e: io::Error) -> Self {
        ConfigError::IoError(String::from(e.description()))
    }
}

impl From<ScanError> for ConfigError {
    fn from(e: ScanError) -> Self {
        ConfigError::SyntaxError(String::from(e.description()))
    }
}

/*
impl Error for ConfigError {
    fn description(&self) -> &str {
        match *self {
            ConfigError::IoError(s) => &s,
            ConfigError::SyntaxError(s) => &s,
            ConfigError::OptionNotFound(s) => &s,
            ConfigError::BadOptionValue(s) => &s
        }
    }

    fn cause(&self) -> Option<&Error> {
        None
    }
}
*/

#[derive(Copy, Debug, PartialEq, Eq, Hash, Clone)]
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

impl FromStr for AwsCredentialsProviderType {
    type Err = ();

    fn from_str(s: &str) -> Result<AwsCredentialsProviderType, ()> {
        match s {
            "Environment" => Ok(AwsCredentialsProviderType::Environment),
            "Profile" => Ok(AwsCredentialsProviderType::Profile),
            "Instance" => Ok(AwsCredentialsProviderType::Instance),
            "Container" => Ok(AwsCredentialsProviderType::Container),
            "Default" => Ok(AwsCredentialsProviderType::Default),
            _ => Err(())
        }
    }
}

pub trait PollerSettingsProvider {
    fn polling_period(&self) -> Option<Duration>;
    fn credentials_provider(&self) -> AwsCredentialsProviderType;
    fn region(&self) -> String;
}

pub trait ScrapeSettingsProvider {
    fn listen_on(&self) -> SocketAddr;
    fn read_timeout(&self) -> Option<Duration>;
    fn keep_alive_timeout(&self) -> Option<Duration>;
}

struct PollerSettings {
    polling_period: Option<u64>,
    credentials_provider: Option<AwsCredentialsProviderType>,
    region: String,
}

struct  ScrapeSettings {
    listen_on: SocketAddr,
    read_timeout: Option<u64>,
    keep_alive_timeout: Option<u64>
}

pub struct DeucalionSettings {
    aws_settings: PollerSettings,
    scrape_settings: ScrapeSettings
}

impl PollerSettingsProvider for DeucalionSettings {
    fn polling_period(&self) -> Option<Duration> {
        self.aws_settings.polling_period.map(|s| Duration::from_secs(s))
    }

    fn credentials_provider(&self) -> AwsCredentialsProviderType {
        self.aws_settings.credentials_provider.unwrap_or(AwsCredentialsProviderType::default())
    }

    fn region(&self) -> String {
        self.aws_settings.region.clone()
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
}

impl DeucalionSettings {
    fn from_doc<T: FromStr>(doc: &Yaml, option_name: &str) -> Result<T, ConfigError> {
        let s = doc[option_name].as_str().ok_or(ConfigError::OptionNotFound(String::from(option_name)))?;
        T::from_str(s).map_err(|_|ConfigError::BadOptionValue(String::from(option_name)))
    }

    fn option_from_doc<T: FromStr>(doc: &Yaml, option_name: &str) -> Result<Option<T>, ConfigError> {
        match Self::from_doc(doc, option_name) {
            Ok(v) => Ok(Some(v)),
            Err(ConfigError::OptionNotFound(_)) => Ok(None),
            Err(e) => Err(e)
        }
    }

    fn from_yaml_string(s: &str) -> Result<Self, ConfigError>
    {
        let doc = &YamlLoader::load_from_str(s).unwrap()[0];
        let scrape_doc = &doc["scrape"];
        let poller_doc = &doc["poller"];
        if scrape_doc.is_badvalue() {
            Err(ConfigError::OptionNotFound(String::from("Missing scrape configuration section")))?
        }
        if poller_doc.is_badvalue() {
            Err(ConfigError::OptionNotFound(String::from("Missing aws configuration section")))?
        }
        Ok(DeucalionSettings {
            scrape_settings: ScrapeSettings {
                listen_on: Self::from_doc(scrape_doc, "listen_address")?,
                keep_alive_timeout: Self::option_from_doc(scrape_doc, "keep_alive_timeout")?,
                read_timeout: Self::option_from_doc(scrape_doc, "read_timeout")?
            },
            aws_settings: PollerSettings {
                credentials_provider: Self::option_from_doc(poller_doc, "credentials_provider")?,
                polling_period: Self::option_from_doc(poller_doc, "polling_period")?,
                region: Self::from_doc(poller_doc, "region")?,
            }
        })
    }

    pub fn from_filename(filename: &str) -> Result<Self, ConfigError>
    {
        let mut f = File::open(filename)?;
        let mut buffer = String::new();
        f.read_to_string(&mut buffer)?;
        Self::from_yaml_string(buffer.as_ref())
    }
}
