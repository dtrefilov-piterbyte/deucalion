use hyper::header::ContentType;
use hyper::server::{Request, Response, Handler};
use hyper::mime::Mime;
use prometheus::Encoder;
use prometheus::{Registry};

pub struct DeucalionHandler<E: Encoder + 'static> {
    encoder: E,
    registry: Registry
}

impl<E: Encoder + 'static> DeucalionHandler<E> {
    pub fn new(encoder: E, registry: Registry) -> DeucalionHandler<E> {
        DeucalionHandler{
            encoder:encoder,
            registry: registry
        }
    }
}

impl<E: Encoder + 'static + Send + Sync> Handler for DeucalionHandler<E> {
    fn handle(&self, _: Request, mut res: Response) {
        let metric_families = self.registry.gather();
        let mut buffer = vec![];
        self.encoder.encode(&metric_families, &mut buffer).unwrap();
        res.headers_mut()
            .set(ContentType(self.encoder.format_type().parse::<Mime>().unwrap()));
        res.send(&buffer).unwrap();
    }
}


