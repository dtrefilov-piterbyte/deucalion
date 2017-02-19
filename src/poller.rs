use config::{PollerSettingsProvider, AwsCredentialsProviderType};
use std::result::Result;
use std::error::Error;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use rusoto::{ProvideAwsCredentials, AwsCredentials, DefaultCredentialsProviderSync, EnvironmentProvider,
             ProfileProvider, InstanceMetadataProvider, ContainerProvider, CredentialsError,
             Region, ParseRegionError, HttpDispatchError};
use rusoto::ec2;
use rusoto::default_tls_client;
use hyper;

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum PollerError {
    InvalidCredentials(String),
    InsufficientPermissions(String),
    BadRegion(String),
    NetworkError(String),
    UnknownError(String)
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

impl From<HttpDispatchError> for PollerError {
    fn from(error: HttpDispatchError) -> Self {
        PollerError::NetworkError(String::from(error.description()))
    }
}

impl Error for PollerError {
    fn description(&self) -> &str {
        match *self {
            PollerError::InvalidCredentials(ref m) => &m,
            PollerError::InsufficientPermissions(ref m) => &m,
            PollerError::BadRegion(ref m) => &m,
            PollerError::NetworkError(ref m) => &m,
            PollerError::UnknownError(ref m) => &m
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

type Ec2Client = ec2::Ec2Client<CredentialsProviderWrapper, hyper::Client>;

pub struct AwsPoller {
    credentials_provider: CredentialsProviderWrapper,
    region: Region
}

impl AwsPoller {
    pub fn new(settings: &PollerSettingsProvider) -> PollerResult<AwsPoller> {
        let result = AwsPoller{
            credentials_provider: Self::new_credentials_provider(settings.credentials_provider())?,
            region : Region::from_str(settings.region().as_str())?
        };
        if let Some(e) = result.test_credentials() { Err(e)? }
        else if let Some(e) = result.test_describe_instances() { Err(e)? }
        else { Ok(result) }
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
    fn test_credentials(&self) -> Option<PollerError> {
        self.credentials_provider.credentials().err().map(|e| PollerError::from(e))
    }

    fn get_ec2_client(&self) -> Ec2Client {
        Ec2Client::new(default_tls_client().unwrap(), self.credentials_provider.clone(), self.region)
    }

    fn test_describe_instances(&self) -> Option<PollerError> {
        let client = self.get_ec2_client();
        let mut req : ec2::DescribeInstancesRequest = Default::default();
        req.dry_run = Some(true);

        match client.describe_instances(&req) {
            Err(e) => {
                match e {
                    ec2::DescribeInstancesError::HttpDispatch(dpt) =>
                        Some(PollerError::from(dpt)),
                    ec2::DescribeInstancesError::Credentials(crd) =>
                        Some(PollerError::from(crd)),
                    ec2::DescribeInstancesError::Validation(s) =>
                        Some(PollerError::UnknownError(s)),
                    ec2::DescribeInstancesError::Unknown(s) => {
                        if s.contains("DryRunOperation") {
                            None
                        } else if s.contains("UnauthorizedOperation") {
                            Some(PollerError::InsufficientPermissions(String::from("DescribeInstances")))
                        } else {
                            Some(PollerError::UnknownError(s))
                        }
                    }
                }
            }
            _ => None
        }
    }

}

struct DescribeInstances {
    client: Ec2Client,
    req: ec2::DescribeInstancesRequest
}

impl DescribeInstances {
    fn new(client: Ec2Client) -> Self {
        DescribeInstances {
            client: client,
            req: Default::default()
        }
    }
}

impl Poller for AwsPoller {
    fn poll(&self) {
        let client = self.get_ec2_client();
        let mut req : ec2::DescribeInstancesRequest = Default::default();
        let running_filter = ec2::Filter{
            name: Some(String::from("instance-state-code")),
            values: Some(vec![String::from("16")])
        };
        req.filters = Some(vec![running_filter]);
        req.max_results = Some(50);
        loop {
            match client.describe_instances(&req) {
                Ok(resp) => {
                    if let Some(reservations) = resp.reservations {
                        for r in reservations {
                            if let Some(instances) = r.instances {
                                for i in instances {
                                    println!("{:?}: {:?}", i.instance_id, i);
                                }
                            }
                        }
                    }
                    if let Some(token) = resp.next_token {
                        req.next_token = Some(token)
                    } else {
                        break
                    }
                },
                Err(e) => {
                    println!("Err {:?}", e);
                    break
                }
            }
        }
        println!("Exiting");
    }
}
