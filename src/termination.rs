use std::sync::{Arc, Mutex, Condvar};
use ctrlc;

pub struct TerminationGuard {
    running: Arc<(Mutex<bool>, Condvar)>
}

impl TerminationGuard {
    pub fn new() -> TerminationGuard {
        let running_arc = Arc::new((Mutex::new(true), Condvar::new()));
        let result = TerminationGuard{
            running: running_arc.clone()
        };
        ctrlc::set_handler(move || {
            let &(ref l, ref cvar) = &*running_arc;
            {
                println!("Terminating...");
                let mut r = l.lock().unwrap();
                *r = false;
                cvar.notify_all();
            }
        });
        result
    }
}

impl Drop for TerminationGuard  {
    fn drop(&mut self) {
        let &(ref l, ref cvar) = &*self.running;
        let mut r = l.lock().unwrap();
        // protection against spurios wakes
        while *r {
            r = cvar.wait(r).unwrap()
        }
    }
}
