# Build Stage
FROM rust:1-bullseye AS build-image

# Update and install necessary packages, including profiling tools
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        wget \
        curl \
        libpq-dev \
        pkg-config \
        clang \
        build-essential \
        libc6-dev \
        heaptrack \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# Ensure CA certificates are installed
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates

# Copy project files to the container
COPY . /subgraph-radio
WORKDIR /subgraph-radio

# Install Golang
RUN sh install-golang.sh
ENV PATH=$PATH:/usr/local/go/bin

# Set Rust flags to link against libresolv
ENV RUSTFLAGS="-C link-arg=-lresolv"

# Build the Rust project
RUN cargo build --release -p subgraph-radio

# Check if the binary is successfully built
RUN ls -lh /subgraph-radio/target/release/

# Runtime Stage
FROM debian:bullseye-slim as runtime

# Update and install necessary packages, including heaptrack dependencies
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        libc6-dev \
        heaptrack \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# Copy necessary files from the build stage
COPY --from=build-image /usr/share/zoneinfo /usr/share/zoneinfo
COPY --from=build-image /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=build-image /etc/passwd /etc/passwd
COPY --from=build-image /etc/group /etc/group
COPY --from=build-image /usr/bin/heaptrack /usr/bin/heaptrack
COPY --from=build-image /subgraph-radio/target/release/subgraph-radio /usr/local/bin/subgraph-radio

# Ensure the binary exists in the correct path
RUN ls -lh /usr/local/bin/subgraph-radio

# Set the entry point to run the application
ENTRYPOINT [ "/usr/local/bin/subgraph-radio" ]
