use config::{AwsInstancesPollerSettingsProvider, AwsSpotPricesPollerSettingsProvider,
             AwsCredentialsProviderType};
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
use std::ascii::AsciiExt;
use std::iter::{Iterator, IntoIterator};
use prometheus::{Opts, GaugeVec, Collector};
use prometheus::Error as PrometheusError;
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

impl From<ec2::DescribeSpotPriceHistoryError> for AwsPollerError {

    fn from(e: ec2::DescribeSpotPriceHistoryError) -> Self {
        match e {
            ec2::DescribeSpotPriceHistoryError::HttpDispatch(dpt) => AwsPollerError::from(dpt),
            ec2::DescribeSpotPriceHistoryError::Credentials(crd) => AwsPollerError::from(crd),
            ec2::DescribeSpotPriceHistoryError::Validation(s) => AwsPollerError::InvalidCredentials(s),
            ec2::DescribeSpotPriceHistoryError::Unknown(s) => {
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

    /// Try to retrieve credentials from provider to be able to fail-fast if credentials
    /// are not available.
    fn test(&self) -> Option<AwsPollerError> {
        self.credentials().err().map(|e| AwsPollerError::from(e))
    }
}

impl ProvideAwsCredentials for CredentialsProviderWrapper {
    fn credentials(&self) -> StdResult<AwsCredentials, CredentialsError> {
        self.inner.credentials()
    }
}

type Ec2Client = ec2::Ec2Client<CredentialsProviderWrapper, ::hyper::Client>;

pub struct AwsInstancesPoller {
    credentials_provider: CredentialsProviderWrapper,
    region: Region,
    max_chunk_size: Option<i32>,
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
            max_chunk_size: settings.max_chunk_size,
            gauges: Self::new_gauges(&settings.expose_tags)?,
            expose_tags: settings.expose_tags,
        };
        if let Some(e) = result.credentials_provider.test() { Err(e)? }
            else if let Some(e) = result.test_describe_instances() { Err(e)? }
        Ok(result)
    }

    fn new_gauges(expose_tags: &Vec<String>) -> Result<GaugeVec, PrometheusError> {
        let opts = Opts::new("AwsInstanceState", "Identifies a running AWS instance");
        let labels: Vec<&str> = vec!["id", "availability_zone", "platform", "type", "lifecycle"].into_iter()
            .chain(expose_tags.iter().map(|s| &**s)).collect();
        GaugeVec::new(opts, labels.as_slice())
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
        {
            let di = PaginatedIterator::new(
                DescribeInstancesRequestor::new(self.get_ec2_client(), vec![running_filter], self.max_chunk_size),
                &mut query_err);

            for instance in di {
                if let Some(tags) = instance.tags {
                    let id = instance.instance_id.unwrap();
                    let mut subsidiary_labels = vec![
                        ("id".to_owned(), id.clone()),
                        ("availability_zone".to_owned(), instance.placement.unwrap().availability_zone.unwrap()),
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
    }

    fn counters(&self) -> Box<Collector> {
        Box::new(self.gauges.clone())
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
                let mut chunk = Vec::with_capacity(self.req.max_results.unwrap_or(0) as usize);
                if let Some(ref mut reservations) = resp.reservations {
                    for r in reservations {
                        if let Some(ref mut instances) = r.instances {
                            chunk.append(instances);
                        }
                    }
                }
                self.req.next_token = resp.next_token.clone();
                Ok(Some(chunk))
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
        req.filters = if filters.is_empty() { None } else { Some(filters) };
        req.max_results = chunk_size;
        DescribeInstancesRequestor {
            client: client,
            req: req,
            first_chunk: true,
        }
    }
}

pub struct AwsSpotPricesPoller {
    credentials_provider: CredentialsProviderWrapper,
    region: Region,
    max_chunk_size: Option<i32>,
    availability_zones: Option<Vec<String>>,
    products: Option<Vec<String>>,
    instance_types: Option<Vec<String>>,
    gauges: GaugeVec
}

impl AwsSpotPricesPoller {
    pub fn new(settings_provider: &AwsSpotPricesPollerSettingsProvider) -> PollerResult<Self> {
        let settings = settings_provider.aws_spot_prices_poller_settings();
        let result = AwsSpotPricesPoller {
            credentials_provider: CredentialsProviderWrapper::from_type(
                settings.credentials_provider.unwrap_or(AwsCredentialsProviderType::Default))?,
            region: Region::from_str(&settings.region)?,
            max_chunk_size: settings.max_chunk_size,
            availability_zones: settings.availability_zones,
            products: settings.products,
            instance_types: settings.instance_types,
            gauges: Self::new_gauges()?,
        };
        if let Some(e) = result.credentials_provider.test() { Err(e)? }
            else if let Some(e) = result.test_describe_spot_prices() { Err(e)? }
        Ok(result)
    }

    fn new_gauges() -> Result<GaugeVec, PrometheusError> {
        let opts = Opts::new("AwsSpotPrices", "Identifies a history of spot prices");
        GaugeVec::new(opts, &["availability_zone", "platform", "type"])
    }

    fn get_ec2_client(&self) -> Ec2Client {
        Ec2Client::new(default_tls_client().unwrap(), self.credentials_provider.clone(), self.region)
    }

    fn test_describe_spot_prices(&self) -> Option<AwsPollerError> {
        let client = self.get_ec2_client();
        let mut req: ec2::DescribeSpotPriceHistoryRequest = Default::default();
        req.dry_run = Some(true);

        match client.describe_spot_price_history(&req) {
            Err(e) => {
                match AwsPollerError::from(e) {
                    AwsPollerError::NoError => None,
                    e => Some(e)
                }
            }
            _ => None
        }
    }

    fn product_to_platform(product: &str) -> Option<&str> {
        match product {
            "Linux/UNIX" => Some("linux"),
            "Windows" => Some("windows"),
            _ => None
        }
    }
}

impl Poller for AwsSpotPricesPoller {
    fn poll(&self) {
        let mut query_err = None;
        {
            let mut filters = Vec::with_capacity(3);
            if let Some(ref az) = self.availability_zones {
                filters.push(ec2::Filter {
                    name: Some("availability-zone".to_owned()),
                    values: Some(az.clone())
                });
            }
            let spot_prices_iterator = PaginatedIterator::new(
                DescribeSpotPricesRequestor::new(self.get_ec2_client(), filters,
                                                 self.products.clone(), self.instance_types.clone(),
                                                 self.max_chunk_size),
                &mut query_err);
            for sp in spot_prices_iterator {
                let labels = vec![
                    ("availability_zone".to_owned(), sp.availability_zone.unwrap()),
                    ("platform".to_owned(), Self::product_to_platform(&sp.product_description.unwrap()).unwrap_or("").to_owned()),
                    ("type".to_owned(), sp.instance_type.unwrap())
                ];
                match self.gauges.get_metric_with(&to_hashmap(&labels)) {
                    Ok(m) => m.set(1.0),
                    Err(e) => println!("Error {:?} on {:?}", e, labels)
                }
            }
        }
    }

    fn counters(&self) -> Box<Collector> {
        Box::new(self.gauges.clone())
    }
}

struct DescribeSpotPricesRequestor {
    client: Ec2Client,
    req: ec2::DescribeSpotPriceHistoryRequest,
    first_chunk: bool
}

impl PaginatedRequestor for DescribeSpotPricesRequestor {
    type Item = ec2::SpotPrice;
    type Error = ec2::DescribeSpotPriceHistoryError;
    fn next_page(&mut self) -> Result<Option<Vec<Self::Item>>, Self::Error> {
        if self.req.next_token.is_none() && !self.first_chunk {
            return Ok(None);
        }
        self.first_chunk = false;
        match self.client.describe_spot_price_history(&self.req) {
            Ok(resp) => {
                // handle empty next_token
                self.req.next_token = if let Some(ref s) = resp.next_token {
                    match s.as_ref() {
                        "" => None,
                        _ => Some(s.clone())
                    }
                } else {
                    None
                };
                Ok(resp.spot_price_history)
            }
            Err(e) => {
                Err(e)
            }
        }
    }
}

impl DescribeSpotPricesRequestor {
    fn new(client: Ec2Client, filters: Vec<ec2::Filter>,
           products: Option<Vec<String>>, instance_types: Option<Vec<String>>,
           chunk_size: Option<i32>) -> Self {
        let mut req: ec2::DescribeSpotPriceHistoryRequest = Default::default();
        req.max_results = chunk_size;
        req.end_time = Some(format!("{}", ::time::now_utc().strftime("%FT%T").unwrap()));
        req.start_time = req.end_time.clone();
        req.filters = if filters.is_empty() { None } else { Some(filters) };
        req.product_descriptions = products;
        req.instance_types = instance_types;
        DescribeSpotPricesRequestor {
            client: client,
            req: req,
            first_chunk: true,
        }
    }
}
