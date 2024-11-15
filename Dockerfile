FROM rust:1.82-bullseye AS build-image

# Update and install necessary packages, including libc6-dev for libresolv
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        wget \
        curl \
        libpq-dev \
        pkg-config \
        clang \
        build-essential \
        libc6-dev \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# Ensure CA certificates are installed
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates

# Copy project files to the container
COPY . /subgraph-radio
WORKDIR /subgraph-radio

# Install Golang
RUN ./scripts/install-go.sh
ENV PATH=$PATH:/usr/local/go/bin

# Set Rust flags to link against libresolv
ENV RUSTFLAGS="-C link-arg=-lresolv"

# Build the Rust project
RUN cargo build --release -p subgraph-radio

# Setup the runtime environment
FROM alpine:3.17.3 as alpine
RUN set -x \
    && apk update \
    && apk add --no-cache upx dumb-init
COPY --from=build-image /subgraph-radio/target/release/subgraph-radio /subgraph-radio/target/release/subgraph-radio
RUN upx --overlay=strip --best /subgraph-radio/target/release/subgraph-radio

FROM debian:bullseye-slim as runtime
COPY --from=build-image /usr/share/zoneinfo /usr/share/zoneinfo
COPY --from=build-image /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=build-image /etc/passwd /etc/passwd
COPY --from=build-image /etc/group /etc/group
COPY --from=alpine /usr/bin/dumb-init /usr/bin/dumb-init
COPY --from=alpine "/subgraph-radio/target/release/subgraph-radio" "/usr/local/bin/subgraph-radio"
ENTRYPOINT [ "/usr/bin/dumb-init", "--", "/usr/local/bin/subgraph-radio" ]
