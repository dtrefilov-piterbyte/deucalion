
use std::net::SocketAddr;
use std::str::FromStr;
use std::result::Result;
use std::env;
use std::time::Duration;
use std::option::Option;

static POLLING_PERIOD_OPTION_NAME : &'static str = "DEUCALION_POLLING_PERIOD";
static AWS_CREDENTIALS_PROVIDER_OPTION_NAME : &'static str = "DEUCALION_AWS_CREDENTIALS_PROVIDER";
static LISTEN_ON_OPTION_NAME : &'static str = "DEUCALION_LISTEN_ON";
static READ_TIMEOUT_OPTION_NAME : &'static str = "DEUCALION_READ_TIMEOUT";
static KEEP_ALIVE_TIMEOUT_OPTION_NAME : &'static str = "DEUCALION_KEEP_ALIVE_TIMEOUT";

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ConfigError {
    OptionNotFound(String),
    OptionFailedToParse(String),
    BadOptionValue(String, String)
}

#[derive(Copy, Debug, PartialEq, Eq, Hash, Clone)]
pub enum AwsCredentialsProvider {
    Environment,
    Profile,
    Instance,
    Container
}

impl Default for AwsCredentialsProvider {
    fn default() -> AwsCredentialsProvider {
        AwsCredentialsProvider::Environment
    }
}

impl FromStr for AwsCredentialsProvider {
    type Err = ();

    fn from_str(s: &str) -> Result<AwsCredentialsProvider, ()> {
        match s {
            "Environment" => Ok(AwsCredentialsProvider::Environment),
            "Profile" => Ok(AwsCredentialsProvider::Profile),
            "Instance" => Ok(AwsCredentialsProvider::Instance),
            "Container" => Ok(AwsCredentialsProvider::Container),
            _ => Err(())
        }
    }
}

pub trait ExporterConfigurationProvider {
    fn polling_period(&self) -> Option<Duration>;
    fn aws_credentials_provider(&self) -> AwsCredentialsProvider;
    fn listen_on(&self) -> SocketAddr;
    fn read_timeout(&self) -> Option<Duration>;
    fn keep_alive_timeout(&self) -> Option<Duration>;
}

pub struct EnvConfig {
    polling_period: Option<u64>,
    aws_credentials_provider: Option<AwsCredentialsProvider>,
    listen_on: SocketAddr,
    read_timeout: Option<u64>,
    keep_alive_timeout: Option<u64>
}

impl ExporterConfigurationProvider for EnvConfig {
    fn polling_period(&self) -> Option<Duration> {
        self.polling_period.map(|s| Duration::from_secs(s))
    }

    fn aws_credentials_provider(&self) -> AwsCredentialsProvider {
        self.aws_credentials_provider.unwrap_or(AwsCredentialsProvider::default())
    }


    fn listen_on(&self) -> SocketAddr {
        self.listen_on
    }

    fn read_timeout(&self) -> Option<Duration> {
        self.read_timeout.map(|s| Duration::from_secs(s))
    }

    fn keep_alive_timeout(&self) -> Option<Duration> {
        self.keep_alive_timeout.map(|s| Duration::from_secs(s))
    }
}

fn get_env_setting<T: FromStr>(name: &'static str) -> Result<T, ConfigError> {
    match env::var(name) {
        Err(env::VarError::NotPresent) => Err(ConfigError::OptionNotFound(name.to_string())),
        Err(env::VarError::NotUnicode(_)) => Err(ConfigError::OptionFailedToParse(name.to_string())),
        Ok(v) => T::from_str(&v).map_err(
            |_| ConfigError::BadOptionValue(name.to_string(), v)
        )
    }
}

fn get_env_option_setting<T: FromStr>(name: &'static str) -> Result<Option<T>, ConfigError> {
    let setting : Result<T, ConfigError> = get_env_setting(name);
    match setting {
        Err(ConfigError::OptionNotFound(_)) => Ok(None),
        Err(e) => Err(e),
        Ok(v) => Ok(Some(v))
    }
}

impl EnvConfig{
    pub fn new() -> Result<Self, ConfigError>
    {
        Ok(EnvConfig{
                polling_period: get_env_option_setting(POLLING_PERIOD_OPTION_NAME)?,
                aws_credentials_provider: get_env_option_setting(AWS_CREDENTIALS_PROVIDER_OPTION_NAME)?,
                listen_on: get_env_setting(LISTEN_ON_OPTION_NAME)?,
                read_timeout: get_env_option_setting(READ_TIMEOUT_OPTION_NAME)?,
                keep_alive_timeout: get_env_option_setting(KEEP_ALIVE_TIMEOUT_OPTION_NAME)?
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn set_up() {
        env::set_var(LISTEN_ON_OPTION_NAME, "127.0.0.1:80");
        env::set_var(READ_TIMEOUT_OPTION_NAME, "60");
        env::set_var(KEEP_ALIVE_TIMEOUT_OPTION_NAME, "180")
    }

    fn tear_down() {
        for n in vec![LISTEN_ON_OPTION_NAME, READ_TIMEOUT_OPTION_NAME, KEEP_ALIVE_TIMEOUT_OPTION_NAME].into_iter() {
            env::remove_var(n);
        }
    }

    macro_rules! unit_test {
        ($name:ident $expr:expr) => (
            #[test]
            fn $name() {
                set_up();
                $expr;
                tear_down();
            }
        )
    }

    unit_test!(test_listen_on_valid_option {
        env::set_var(LISTEN_ON_OPTION_NAME, "0.0.0.0:9090");
        assert_eq!(SocketAddr::from_str("0.0.0.0:9090").unwrap(),
            EnvConfig::new().unwrap().listen_on());
    });

    unit_test!(test_listen_on_invalid_option {
        env::set_var(LISTEN_ON_OPTION_NAME, "asdad");
        assert_eq!(ConfigError::BadOptionValue(LISTEN_ON_OPTION_NAME.to_string(), "asdad".to_string()),
            EnvConfig::new().err().unwrap());
    });

    unit_test!(test_listen_on_not_exists {
        env::remove_var(LISTEN_ON_OPTION_NAME);
        assert_eq!(ConfigError::OptionNotFound(LISTEN_ON_OPTION_NAME.to_string()),
            EnvConfig::new().err().unwrap());
    });

    unit_test!(test_read_timeout_valid_option {
        env::set_var(READ_TIMEOUT_OPTION_NAME, "30");
        assert_eq!(Duration::from_secs(30), EnvConfig::new().unwrap().read_timeout().unwrap());
    });

    unit_test!(test_read_timeout_invalid_option {
        env::set_var(READ_TIMEOUT_OPTION_NAME, "das");
        assert_eq!(ConfigError::BadOptionValue(READ_TIMEOUT_OPTION_NAME.to_string(), "das".to_string()),
            EnvConfig::new().err().unwrap());
    });

    unit_test!(test_read_timeout_not_exists {
        env::remove_var(READ_TIMEOUT_OPTION_NAME);
        assert_eq!(None, EnvConfig::new().unwrap().read_timeout());
    });

    unit_test!(test_keep_alive_timeout_valid_option {
        env::set_var(KEEP_ALIVE_TIMEOUT_OPTION_NAME, "300");
        assert_eq!(Duration::from_secs(300), EnvConfig::new().unwrap().keep_alive_timeout().unwrap());
    });

    unit_test!(test_keep_alive_timeout_invalid_option {
        env::set_var(KEEP_ALIVE_TIMEOUT_OPTION_NAME, "sad");
        assert_eq!(ConfigError::BadOptionValue(KEEP_ALIVE_TIMEOUT_OPTION_NAME.to_string(), "sad".to_string()),
            EnvConfig::new().err().unwrap());
    });

    unit_test!(test_keep_alive_timeout_not_exists {
        env::remove_var(KEEP_ALIVE_TIMEOUT_OPTION_NAME);
        assert_eq!(None, EnvConfig::new().unwrap().keep_alive_timeout());
    });
}
