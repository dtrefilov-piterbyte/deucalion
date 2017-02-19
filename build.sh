#!/bin/sh

OPENSSL_DIR=/usr/local/musl \
OPENSSL_STATIC=1 \
CC=/usr/local/musl/bin/musl-gcc \
LDFLAGS="-static -L/usr/local/musl/lib" \
LD_LIBRARY_PATH=/usr/local/musl/lib:$LD_LIBRARY_PATH \
CFLAGS="-I/usr/local/musl/include" \
PKG_CONFIG_PATH=/usr/local/musl/lib/pkgconfig \
cargo build --release --target=x86_64-unknown-linux-musl

strip ./target/x86_64-unknown-linux-musl/release/deucalion

docker build -t deucalion .
