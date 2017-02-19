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
use std::ascii::AsciiExt;
use std::iter::Iterator;

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum PollerError {
    InvalidCredentials(String),
    InsufficientPermissions(String),
    BadRegion(String),
    NetworkError(String),
    UnknownError(String),
    NoError
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

impl From<ec2::DescribeInstancesError> for PollerError {
    fn from(e: ec2::DescribeInstancesError) -> Self {
        match e {
            ec2::DescribeInstancesError::HttpDispatch(dpt) => PollerError::from(dpt),
            ec2::DescribeInstancesError::Credentials(crd) => PollerError::from(crd),
            ec2::DescribeInstancesError::Validation(s) => PollerError::UnknownError(s),
            ec2::DescribeInstancesError::Unknown(s) => {
                if s.contains("DryRunOperation") {
                    PollerError::NoError
                } else if s.contains("UnauthorizedOperation") {
                    PollerError::InsufficientPermissions(String::from("DescribeInstances"))
                } else {
                    PollerError::UnknownError(s)
                }
            }
        }
    }
}

impl Error for PollerError {
    fn description(&self) -> &str {
        match *self {
            PollerError::InvalidCredentials(ref m) => &m,
            PollerError::InsufficientPermissions(ref m) => &m,
            PollerError::BadRegion(ref m) => &m,
            PollerError::NetworkError(ref m) => &m,
            PollerError::UnknownError(ref m) => &m,
            PollerError::NoError => "No error",
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
    region: Region,
    di_chunk_size: Option<i32>,
    expose_tags: Vec<String>,
}

impl AwsPoller {
    pub fn new(settings: &PollerSettingsProvider) -> PollerResult<AwsPoller> {
        let result = AwsPoller{
            credentials_provider: Self::new_credentials_provider(settings.credentials_provider())?,
            region : Region::from_str(settings.region())?,
            di_chunk_size: settings.describe_instances_chunk_size(),
            expose_tags: settings.expose_tags(),
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
                match PollerError::from(e) {
                    PollerError::NoError => None,
                    e => Some(e)
                }
            }
            _ => None
        }
    }

}

struct DescribeInstancesIterator {
    current_page: Option<Vec<ec2::Instance>>,
    client: Ec2Client,
    req: ec2::DescribeInstancesRequest,
    error: Option<ec2::DescribeInstancesError>,
    first_chunk: bool,
}

impl DescribeInstancesIterator {
    fn new(client: Ec2Client, filters: Vec<ec2::Filter>, chunk_size: Option<i32>) -> Self {
        let mut req: ec2::DescribeInstancesRequest = Default::default();
        req.filters = Some(filters);
        req.max_results = chunk_size;
        DescribeInstancesIterator {
            current_page: None,
            client: client,
            req: req,
            error: None,
            first_chunk: true,
        }
    }

    fn next_page(&mut self) -> Option<Vec<ec2::Instance>> {
        if self.req.next_token.is_none() && !self.first_chunk {
            return None;
        }
        self.first_chunk = false;
        match self.client.describe_instances(&self.req) {
            Ok(ref mut resp) => {
                let mut chunk = vec![];
                if let Some(ref mut reservations) = resp.reservations {
                    for r in reservations {
                        if let Some(ref mut instances) = r.instances {
                            chunk.append(instances);
                        }
                    }
                }
                self.req.next_token = resp.next_token.clone();
                Some(chunk.into_iter().rev().collect())
            }
            Err(e) => {
                self.error = Some(e);
                None
            }
        }
    }
}

impl Iterator for DescribeInstancesIterator {
    type Item = ec2::Instance;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_page.is_none() {
            self.current_page = self.next_page();
            if self.current_page.is_none() {
                return None;
            }
        }
        match self.current_page.as_mut().unwrap().pop() {
            Some(i) => Some(i),
            None => {
                self.current_page = self.next_page();
                match self.current_page {
                    Some(_) => self.next(),
                    None => None
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MetricLabel {
    key: String,
    value: String,
}

impl From<ec2::Tag> for MetricLabel {
    fn from(tag: ec2::Tag) -> Self {
        MetricLabel {
            key: tag.key.unwrap(),
            value: tag.value.unwrap(),
        }
    }
}

impl Poller for AwsPoller {
    fn poll(&self) {
        let running_filter = ec2::Filter{
            name: Some(String::from("instance-state-code")),
            values: Some(vec![String::from("16")])
        };
        let di = DescribeInstancesIterator::new(self.get_ec2_client(), vec![running_filter],
                                        self.di_chunk_size);
        for instance in di {
            if let Some(tags) = instance.tags {
                let mut labels = vec![MetricLabel
                    {
                        key: "id".to_owned(),
                        value: instance.instance_id.unwrap(),
                    }];
                let mut used_tags = tags.into_iter().filter(|t| {
                    self.expose_tags.clone().into_iter().any(
                        |e| t.clone().key.unwrap().eq_ignore_ascii_case(&e))
                }).map(|t| MetricLabel::from(t))
                    .collect::<Vec<MetricLabel>>();
                labels.append(&mut used_tags);
                println!("{:?}" , labels);
            }
        }
    }
}
