FROM alpine:latest
MAINTAINER Dmitry Trefilov <the-alien@live.ru>

RUN apk update && \
    GLIBC_VERSION="2.23-r3" && \
    apk --no-cache add ca-certificates wget && \
    update-ca-certificates  && \
    wget -q -O "/etc/apk/keys/sgerrand.rsa.pub" "https://raw.githubusercontent.com/sgerrand/alpine-pkg-glibc/master/sgerrand.rsa.pub" && \
    wget "https://github.com/sgerrand/alpine-pkg-glibc/releases/download/$GLIBC_VERSION/glibc-$GLIBC_VERSION.apk" && \
    wget "https://github.com/sgerrand/alpine-pkg-glibc/releases/download/$GLIBC_VERSION/glibc-bin-$GLIBC_VERSION.apk" && \
    wget "https://github.com/sgerrand/alpine-pkg-glibc/releases/download/$GLIBC_VERSION/glibc-i18n-$GLIBC_VERSION.apk" && \
    apk add --no-cache "glibc-$GLIBC_VERSION.apk" "glibc-bin-$GLIBC_VERSION.apk" "glibc-i18n-$GLIBC_VERSION.apk" && \
    /usr/glibc-compat/bin/localedef -i en_US -f UTF-8 en_US.UTF-8 && \
    rm *.apk

ENV LANG=en-US.UTF-8

RUN mkdir -p /opt/deucalion
COPY target/release/deucalion /opt/deucalion
RUN chmod +x /opt/deucalion/deucalion

COPY docker-entrypoint.sh /bin/
RUN chmod +x /bin/docker-entrypoint.sh

ENV DEUCALION_POLLING_PERIOD=10 \
    DEUCALION_LISTEN_ON=0.0.0.0:9090 \
    DEUCALION_READ_TIMEOUT=60 \
    DEUCALION_KEEP_ALIVE_TIMEOUT=1800

EXPOSE 9090

WORKDIR /opt/deucalion

ENTRYPOINT ["/bin/docker-entrypoint.sh"]
