#!/usr/bin/env bash

# NOTE: Run from project root ./scripts/docker-cargo-publish.sh

docker build -t subgraph-radio-dev -f Dockerfile.dev .
docker run --rm -e CARGO_REGISTRY_TOKEN=$CARGO_REGISTRY_TOKEN -v .:/app subgraph-radio-dev sh -c "cargo publish --token $CARGO_REGISTRY_TOKEN -p subgraph-radio"
