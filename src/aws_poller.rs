use config::{AwsInstancesPollerSettingsProvider, AwsCredentialsProviderType};
use std::result::Result as StdResult;
use std::error::Error as StdError;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use std::io::{stderr, Write};
use rusoto::{ProvideAwsCredentials, AwsCredentials, DefaultCredentialsProviderSync, EnvironmentProvider,
             ProfileProvider, InstanceMetadataProvider, ContainerProvider, CredentialsError,
             Region, ParseRegionError, HttpDispatchError};
use rusoto::ec2;
use rusoto::default_tls_client;
use hyper;
use std::ascii::AsciiExt;
use std::iter::{Iterator, IntoIterator};
use prometheus::{Opts, GaugeVec, Collector};
use prometheus::Error as PrometheusError;
use time::precise_time_s;
use std::collections::HashMap;
use pagination::{PaginatedIterator, PaginatedRequestor};
use poller::Poller;

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum AwsPollerError {
    InvalidCredentials(String),
    InsufficientPermissions(String),
    BadRegion(String),
    NetworkError(String),
    UnknownError(String),
    NoError
}

impl fmt::Display for AwsPollerError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<CredentialsError> for AwsPollerError {
    fn from(error: CredentialsError) -> Self {
        AwsPollerError::InvalidCredentials(String::from(error.description()))
    }
}

impl From<ParseRegionError> for AwsPollerError {
    fn from(error: ParseRegionError) -> Self {
        AwsPollerError::BadRegion(String::from(error.description()))
    }
}

impl From<HttpDispatchError> for AwsPollerError {
    fn from(error: HttpDispatchError) -> Self {
        AwsPollerError::NetworkError(String::from(error.description()))
    }
}

impl From<PrometheusError> for AwsPollerError {
    fn from(error: PrometheusError) -> Self {
        AwsPollerError::UnknownError(String::from(error.description()))
    }
}

impl From<ec2::DescribeInstancesError> for AwsPollerError {
    fn from(e: ec2::DescribeInstancesError) -> Self {
        match e {
            ec2::DescribeInstancesError::HttpDispatch(dpt) => AwsPollerError::from(dpt),
            ec2::DescribeInstancesError::Credentials(crd) => AwsPollerError::from(crd),
            ec2::DescribeInstancesError::Validation(s) => AwsPollerError::InvalidCredentials(s),
            ec2::DescribeInstancesError::Unknown(s) => {
                if s.contains("DryRunOperation") {
                    AwsPollerError::NoError
                } else if s.contains("UnauthorizedOperation") {
                    AwsPollerError::InsufficientPermissions(String::from("DescribeInstances"))
                } else if s.contains("AuthFailure") {
                    AwsPollerError::InvalidCredentials(s)
                } else {
                    AwsPollerError::UnknownError(s)
                }
            }
        }
    }
}

impl StdError for AwsPollerError {
    fn description(&self) -> &str {
        match *self {
            AwsPollerError::InvalidCredentials(ref m) => &m,
            AwsPollerError::InsufficientPermissions(ref m) => &m,
            AwsPollerError::BadRegion(ref m) => &m,
            AwsPollerError::NetworkError(ref m) => &m,
            AwsPollerError::UnknownError(ref m) => &m,
            AwsPollerError::NoError => "No error",
        }
    }
}

type PollerResult<T> = StdResult<T, AwsPollerError>;

#[derive(Clone)]
struct CredentialsProviderWrapper {
    inner: Arc<ProvideAwsCredentials + Send + Sync>
}

impl CredentialsProviderWrapper {
    fn from_type(provider_type: AwsCredentialsProviderType)
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
}

impl ProvideAwsCredentials for CredentialsProviderWrapper {
    fn credentials(&self) -> StdResult<AwsCredentials, CredentialsError> {
        self.inner.credentials()
    }
}

type Ec2Client = ec2::Ec2Client<CredentialsProviderWrapper, hyper::Client>;

pub struct AwsInstancesPoller {
    credentials_provider: CredentialsProviderWrapper,
    region: Region,
    di_chunk_size: Option<i32>,
    expose_tags: Vec<String>,
    gauges: GaugeVec
}

impl AwsInstancesPoller {
    pub fn new(settings_provider: &AwsInstancesPollerSettingsProvider) -> PollerResult<AwsInstancesPoller> {
        let settings = settings_provider.aws_instances_poller_settings();
        let result = AwsInstancesPoller {
            credentials_provider: CredentialsProviderWrapper::from_type(
                settings.credentials_provider.unwrap_or(AwsCredentialsProviderType::Default))?,
            region: Region::from_str(&settings.region)?,
            di_chunk_size: settings.describe_instances_chunk_size,
            gauges: Self::new_gauges(&settings.expose_tags)?,
            expose_tags: settings.expose_tags,
        };
        if let Some(e) = result.test_credentials() { Err(e)? } else if let Some(e) = result.test_describe_instances() { Err(e)? }
        Ok(result)
    }

    fn new_gauges(expose_tags: &Vec<String>) -> Result<GaugeVec, PrometheusError> {
        let opts = Opts::new("AwsInstanceState", "Identifies a running AWS instance");
        let labels: Vec<&str> = vec!["id", "platform", "type", "lifecycle"].into_iter()
            .chain(expose_tags.iter().map(|s| &**s)).collect();
        GaugeVec::new(opts, labels.as_slice())
    }

    /// Try to retrieve credentials from provider to be able to fail-fast if the credentials
    /// are not available.
    fn test_credentials(&self) -> Option<AwsPollerError> {
        self.credentials_provider.credentials().err().map(|e| AwsPollerError::from(e))
    }

    fn get_ec2_client(&self) -> Ec2Client {
        Ec2Client::new(default_tls_client().unwrap(), self.credentials_provider.clone(), self.region)
    }

    fn test_describe_instances(&self) -> Option<AwsPollerError> {
        let client = self.get_ec2_client();
        let mut req: ec2::DescribeInstancesRequest = Default::default();
        req.dry_run = Some(true);

        match client.describe_instances(&req) {
            Err(e) => {
                match AwsPollerError::from(e) {
                    AwsPollerError::NoError => None,
                    e => Some(e)
                }
            }
            _ => None
        }
    }
}

struct DescribeInstancesRequestor {
    client: Ec2Client,
    req: ec2::DescribeInstancesRequest,
    first_chunk: bool
}

impl PaginatedRequestor for DescribeInstancesRequestor {
    type Item = ec2::Instance;
    type Error = ec2::DescribeInstancesError;
    fn next_page(&mut self) -> Result<Option<Vec<Self::Item>>, Self::Error> {
        if self.req.next_token.is_none() && !self.first_chunk {
            return Ok(None);
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
                Ok(Some(chunk.into_iter().rev().collect()))
            }
            Err(e) => {
                Err(e)
            }
        }
    }
}

impl DescribeInstancesRequestor {
    fn new(client: Ec2Client, filters: Vec<ec2::Filter>, chunk_size: Option<i32>) -> Self {
        let mut req: ec2::DescribeInstancesRequest = Default::default();
        req.filters = Some(filters);
        req.max_results = chunk_size;
        DescribeInstancesRequestor {
            client: client,
            req: req,
            first_chunk: true,
        }
    }
}

fn to_hashmap(labels: &Vec<(String, String)>) -> HashMap<&str, &str> {
    let literals: Vec<(&str, &str)> = labels.iter().map(|l| -> (&str, &str)
        { (&l.0, &l.1) }).collect();
    literals.iter().cloned().collect()
}

impl Poller for AwsInstancesPoller {
    fn poll(&self) {
        let running_filter = ec2::Filter {
            name: Some(String::from("instance-state-code")),
            values: Some(vec![String::from("16")])
        };
        let mut current_metrics: Vec<_> = self.gauges.collect().iter().next().unwrap().get_metric().iter()
            .map(|m| m.get_label().iter()
                .map(|l| (l.get_name().to_owned(), l.get_value().to_owned())).collect::<HashMap<_, _>>())
            .collect();
        let mut query_err = None;
        let started = precise_time_s();
        {
            let di = PaginatedIterator::new(
                DescribeInstancesRequestor::new(self.get_ec2_client(), vec![running_filter], self.di_chunk_size),
                &mut query_err);

            for instance in di {
                if let Some(tags) = instance.tags {
                    let id = instance.instance_id.unwrap();
                    let mut subsidiary_labels = vec![
                        ("id".to_owned(), id.clone()),
                        ("platform".to_owned(), instance.platform.unwrap_or("linux".to_owned())),
                        ("type".to_owned(), instance.instance_type.unwrap()),
                        ("lifecycle".to_owned(), instance.instance_lifecycle.unwrap_or("ondemand".to_owned()))
                    ];
                    current_metrics.retain(|m| m[&"id".to_owned()] != id);
                    let mut labels = Vec::with_capacity(subsidiary_labels.len() + self.expose_tags.len());
                    labels.append(&mut subsidiary_labels);
                    for e in self.expose_tags.iter() {
                        let m = match tags.iter().find(|&t| e.eq_ignore_ascii_case(t.key.as_ref().unwrap())) {
                            Some(ft) => (e.clone(), ft.clone().value.unwrap()),
                            None => (e.clone(), "".to_owned())
                        };
                        labels.push(m);
                    }
                    match self.gauges.get_metric_with(&to_hashmap(&labels)) {
                        Ok(m) => m.set(1.0),
                        Err(e) => println!("Error {:?} on {:?}", e, labels)
                    }
                }
            }
        }
        if query_err.is_some() {
            let _ = writeln!(&mut stderr(), "Unexpected error during instance enumeration: {:?}",
                             query_err);
        } else {
            // Delete instances that are not in running state anymore
            for m in current_metrics.iter() {
                let labels = m.iter().map(|t| (t.0.as_str(), t.1.as_str())).collect::<HashMap<_, _>>();
                println!("Deleting {:?}", labels["id"]);
                if self.gauges.remove(&labels).is_err() {
                    let _ = writeln!(&mut stderr(), "Instance disappeared?");
                }
            }
        }
        println!("Total time spent on query: {:?} sec", precise_time_s() - started);
    }

    fn counters(&self) -> Box<Collector> {
        Box::new(self.gauges.clone())
    }
}
