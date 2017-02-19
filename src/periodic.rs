
use std::time::{Duration, Instant};
use std::thread;
use std::sync::{Arc, Mutex, Condvar};
use poller::Poller;

pub struct AsyncPeriodicRunner
{
    terminate: Arc<(Mutex<bool>, Condvar)>,
    thread: Option<thread::JoinHandle<()>>
}

impl AsyncPeriodicRunner
{
    pub fn new<P: Poller + 'static>(poller: P, poll_period: Duration) -> AsyncPeriodicRunner
    {
        let terminate_arc = Arc::new((Mutex::new(false), Condvar::new()));
        let result = AsyncPeriodicRunner{
            terminate: terminate_arc.clone(),
            thread: Some(thread::spawn(move || -> () {
                let &(ref l, ref cvar) = &*terminate_arc;
                let mut terminate = l.lock().unwrap();
                while !*terminate {
                    let now = Instant::now();
                    poller.poll();
                    let elapsed = now.elapsed();
                    let sleep_duration = if elapsed < poll_period {
                        poll_period - elapsed
                    } else {
                        println!("Poller is running too long: {:?}. Consider increasing poll period.",
                                 elapsed);
                        Duration::from_secs(0)
                    };
                    terminate = cvar.wait_timeout(terminate, sleep_duration).unwrap().0;
                }
            }))
        };
        return result;
    }
}

impl Drop for AsyncPeriodicRunner
{
    fn drop(&mut self)
    {
        let &(ref l, ref cvar) = &*self.terminate;
        {
            let mut terminate = l.lock().unwrap();
            *terminate = true;
            cvar.notify_one();
        }
        println!("Waiting for poller thread to exit...");
        if let Some(h) = self.thread.take() {
            h.join().unwrap();
        };
    }
}
