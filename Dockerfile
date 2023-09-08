FROM rust:1-bullseye AS build-image

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        wget \
        curl \
        libpq-dev \
        pkg-config \
        libssl-dev \
        clang \
        build-essential \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates

COPY . /subgraph-radio
WORKDIR /subgraph-radio

RUN sh install-golang.sh
ENV PATH=$PATH:/usr/local/go/bin

RUN cargo build --release -p subgraph-radio

FROM alpine:3.17.3 as alpine
RUN set -x \
    && apk update \
    && apk add --no-cache upx dumb-init
COPY --from=build-image /subgraph-radio/target/release/subgraph-radio /subgraph-radio/target/release/subgraph-radio
RUN upx --overlay=strip --best /subgraph-radio/target/release/subgraph-radio

FROM gcr.io/distroless/cc AS runtime
COPY --from=build-image /usr/share/zoneinfo /usr/share/zoneinfo
COPY --from=build-image /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=build-image /etc/passwd /etc/passwd
COPY --from=build-image /etc/group /etc/group
COPY --from=alpine /usr/bin/dumb-init /usr/bin/dumb-init
COPY --from=alpine "/subgraph-radio/target/release/subgraph-radio" "/usr/local/bin/subgraph-radio"
COPY --from=busybox:1.35.0-uclibc /bin/sh /bin/sh
ENTRYPOINT [ "/usr/bin/dumb-init", "--", "/usr/local/bin/subgraph-radio" ]
