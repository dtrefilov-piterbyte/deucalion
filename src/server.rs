use hyper::header::ContentType;
use hyper::server::{Request, Response, Handler};
use hyper::mime::Mime;
use prometheus::{Encoder,TextEncoder};
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

pub struct DeucalionHandler {
}

impl DeucalionHandler {
    pub fn new() -> DeucalionHandler {
        DeucalionHandler{}
    }
}

impl Handler for DeucalionHandler {
    fn handle(&self, _: Request, mut res: Response) {
        let encoder = TextEncoder::new();
        //println!("Handling {} request from {}", req.method, req.remote_addr);

        SELF_TEST_SCRAPE_REQUESTS_COUNTER.inc();

        let metric_families = gather();
        let mut buffer = vec![];
        encoder.encode(&metric_families, &mut buffer).unwrap();
        res.headers_mut()
            .set(ContentType(encoder.format_type().parse::<Mime>().unwrap()));
        res.send(&buffer).unwrap();
    }
}


