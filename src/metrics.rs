
//use prometheus::gauge::GaugeVec;
use std::time::Duration;
use std::thread;
use std::sync::Condvar;


pub trait Poller {
    fn poll(&mut self);
}

pub struct AsyncPeriodicRunner
{
    thread: Option<thread::JoinHandle<()>>
}

impl AsyncPeriodicRunner
{
    pub fn new(poll_period: Duration) -> AsyncPeriodicRunner
    {
        let result = AsyncPeriodicRunner{thread: Some(thread::spawn(move || -> () {
            println!("Started periodic poller {:?}", poll_period);
            thread::sleep(poll_period);
            println!("Exiting poller...");
        }))};
        return result;
    }
}

impl Drop for AsyncPeriodicRunner
{
    fn drop(&mut self)
    {
        println!("Waiting for poller thread to exit...");
        if let Some(h) = self.thread.take() {
            h.join().unwrap();
        };
    }
}
