use config::{AwsSettingsProvider, AwsCredentialsProviderType};
use std::result::Result;
use std::error::Error;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use rusoto::{ProvideAwsCredentials, AwsCredentials, DefaultCredentialsProviderSync, EnvironmentProvider,
             ProfileProvider, InstanceMetadataProvider, ContainerProvider, CredentialsError,
             Region, ParseRegionError};
use rusoto::cloudwatch::{CloudWatchClient, ListMetricsInput};
use rusoto::default_tls_client;

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum PollerError {
    InvalidCredentials(String),
    BadRegion(String)
}

impl fmt::Display for PollerError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<CredentialsError> for PollerError {
    fn from(error: CredentialsError) -> Self {
        PollerError::InvalidCredentials(String::from(error.description()))
    }
}

impl From<ParseRegionError> for PollerError {
    fn from(error: ParseRegionError) -> Self {
        PollerError::BadRegion(String::from(error.description()))
    }
}

impl Error for PollerError {
    fn description(&self) -> &str {
        match *self {
            PollerError::InvalidCredentials(ref m) => m.as_str(),
            PollerError::BadRegion(ref m) => m.as_str()
        }
    }
}

type PollerResult<T> = Result<T, PollerError>;

pub trait Poller : Sync + Send {
    fn poll(&self);
}

#[derive(Clone)]
struct CredentialsProviderWrapper {
    inner: Arc<ProvideAwsCredentials + Send + Sync>
}

impl ProvideAwsCredentials for CredentialsProviderWrapper {
    fn credentials(&self) -> Result<AwsCredentials, CredentialsError> {
        self.inner.credentials()
    }
}

pub struct AwsPoller {
    credentials_provider: CredentialsProviderWrapper,
    region: Region
}

impl AwsPoller {
    pub fn new(settings: &AwsSettingsProvider) -> PollerResult<AwsPoller> {
        let result = AwsPoller{
            credentials_provider: Self::new_credentials_provider(settings.aws_credentials_provider())?,
            region : Region::from_str(settings.aws_region().as_str())?
        };
        result.test_credentials()
    }

    fn new_credentials_provider(provider_type: AwsCredentialsProviderType)
        -> Result<CredentialsProviderWrapper, CredentialsError> {
        Ok(CredentialsProviderWrapper {
            inner: match provider_type {
                AwsCredentialsProviderType::Default =>
                    Arc::new(DefaultCredentialsProviderSync::new()?),
                AwsCredentialsProviderType::Environment =>
                    Arc::new(EnvironmentProvider {}),
                AwsCredentialsProviderType::Profile =>
                    Arc::new(ProfileProvider::new()?),
                AwsCredentialsProviderType::Instance =>
                    Arc::new(InstanceMetadataProvider {}),
                AwsCredentialsProviderType::Container =>
                    Arc::new(ContainerProvider {})
            }
        })
    }

    /// Try to retrieve credentials from provider to be able to fail-fast if the credentials
    /// are not available.
    pub fn test_credentials(self) -> PollerResult<AwsPoller> {
        let credentials = self.credentials_provider.credentials()?;
        println!("{:?}", credentials);
        Ok(self)
    }
}


impl Poller for AwsPoller {
    fn poll(&self) {
        let client = CloudWatchClient::new(default_tls_client().unwrap(),
                                           self.credentials_provider.clone(), self.region);
        let mut list_metrics : ListMetricsInput = Default::default();
        list_metrics.namespace = Some(String::from("AWS/EFS"));
        match client.list_metrics(&list_metrics) {
            Ok(r) => println!("Ok {:?}", r),
            Err(e) => println!("Err {:?}", e)
        }
    }
}
