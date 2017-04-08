FROM alpine:latest
MAINTAINER Dmitry Trefilov <the-alien@live.ru>

ENV LANG=en-US.UTF-8

RUN apk --update upgrade && \
    apk add curl ca-certificates && \
    update-ca-certificates && \
    rm -rf /var/cache/apk/*

RUN mkdir -p /opt/deucalion
COPY target/x86_64-unknown-linux-musl/release/deucalion /opt/deucalion
COPY config.yml /opt/deucalion
RUN chmod +x /opt/deucalion/deucalion

COPY docker-entrypoint.sh /bin/
RUN chmod +x /bin/docker-entrypoint.sh

EXPOSE 8082

WORKDIR /opt/deucalion

ENTRYPOINT ["/bin/docker-entrypoint.sh"]
