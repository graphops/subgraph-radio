#!/usr/bin/env bash

# NOTE: Run from project root ./scripts/docker-run-tests.sh

docker build -t subgraph-radio-dev -f Dockerfile.dev .
docker run --rm -v .:/app subgraph-radio-dev cargo nextest run 
