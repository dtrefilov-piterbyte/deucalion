FROM alpine:latest
MAINTAINER Dmitry Trefilov <the-alien@live.ru>

ENV LANG=en-US.UTF-8

RUN mkdir -p /opt/deucalion
COPY target/x86_64-unknown-linux-musl/release/deucalion /opt/deucalion
COPY config.yml /opt/deucalion
RUN chmod +x /opt/deucalion/deucalion

COPY docker-entrypoint.sh /bin/
RUN chmod +x /bin/docker-entrypoint.sh

EXPOSE 9090

WORKDIR /opt/deucalion

ENTRYPOINT ["/bin/docker-entrypoint.sh"]
