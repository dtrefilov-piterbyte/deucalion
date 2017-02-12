use hyper::header::ContentType;
use hyper::server::{Request, Response, Handler};
use hyper::mime::Mime;
use prometheus::Encoder;
use prometheus::{gather, Counter};

lazy_static! {
    pub static ref SELF_TEST_SCRAPE_REQUESTS_COUNTER: Counter = register_counter!(
        opts!(
            "deucalion_scrape_requests_counter",
            "Scrape requests served by this Deucalion instance.",
            labels!{"instance" => "deucalion",}
        )
    ).unwrap();
}

pub struct DeucalionHandler<E: Encoder + 'static> {
    encoder: E
}

impl<E: Encoder + 'static> DeucalionHandler<E> {
    pub fn new(encoder: E) -> DeucalionHandler<E> {
        DeucalionHandler{encoder:encoder}
    }
}

impl<E: Encoder + 'static + Send + Sync> Handler for DeucalionHandler<E> {
    fn handle(&self, _: Request, mut res: Response) {
        //println!("Handling {} request from {}", req.method, req.remote_addr);

        SELF_TEST_SCRAPE_REQUESTS_COUNTER.inc();

        let metric_families = gather();
        let mut buffer = vec![];
        self.encoder.encode(&metric_families, &mut buffer).unwrap();
        res.headers_mut()
            .set(ContentType(self.encoder.format_type().parse::<Mime>().unwrap()));
        res.send(&buffer).unwrap();
    }
}


