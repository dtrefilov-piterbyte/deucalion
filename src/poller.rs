use prometheus::Collector;

pub trait Poller: Sync + Send {
    fn poll(&self);
    fn counters(&self) -> Box<Collector>;
}
