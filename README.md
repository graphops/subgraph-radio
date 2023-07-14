# Subgraph Radio

[![Docs](https://img.shields.io/badge/docs-latest-brightgreen.svg)](https://docs.graphops.xyz/graphcast/radios/subgraph-radio)
[![crates.io](https://img.shields.io/crates/v/subgraph-radio.svg)](https://crates.io/crates/subgraph-radio)

## Introduction

This is a Graphcast Radio focused on sending gossips about particular subgraphs on a P2P network. The available message types are Public Proof of Indexing (PoI) messages from an indexer, or a version upgrade annoucement message from a subgraph owner.

Reaching Public PoI consensus and ensuring data availability during subgraph upgrades is critical to the indexing service. Both messages should find value from indexers, subgraph owners, and ultimately data consumers.

[Documentation](https://docs.graphops.xyz/graphcast/radios/subgraph-radio) | [Packages](https://github.com/graphops/subgraph-radio/pkgs/container/subgraph-radio) | [Chat](https://discord.com/channels/438038660412342282/1087503343410225152) 

### Public PoI message

The key requirement for an Indexer to earn indexing rewards is to submit a valid Proof of Indexing (POI) promptly. The importance of valid POIs causes many Indexers to alert each other on subgraph health in community discussions. To alleviate the Indexer workload, this Radio utilized Graphcast SDK to exchange and aggregate POI along with a list of Indexer on-chain identities that can be used to trace reputations. With the pubsub pattern, the Indexer can get notified as soon as other indexers (with majority of stake) publish a POI different from the local POI.

### Version Upgrade message

When developers publish a new version (subgraph deployment) to their subgraph, data service instability may occur while their API queries the pre-existing version. Indexers may require some time to sync a subgraph to the chainhead after they have stopped syncing the previous deployment. To decrease the upgrade friction, developers can send a message before publishing the subgraph, including the old deployment hash, new deployment hash, matching subgraph id, the time they would like to publish the version. Indexers who is running the subgraph radio with notification methods configured should get notified; later on this radio can optionally automate the deployment process on graph node, but it is still at the subgraph developers' discretion to await for the indexers to sync upto chainhead, in which point they can publish the staged version without disrupting API usage.

## 🧪 Testing

To run unit tests for the Radio. We recommend using [nextest](https://nexte.st/) as your test runner. Once you have it installed you can run the tests using the following commands:

```
cargo nextest run
```

## Contributing

We welcome and appreciate your contributions! Please see the [Contributor Guide](/CONTRIBUTING.md), [Code Of Conduct](/CODE_OF_CONDUCT.md) and [Security Notes](/SECURITY.md) for this repository.
