# POI-Crosschecker Radio

The key requirement for an Indexer to earn indexing rewards is to submit a valid Proof of Indexing (POI) promptly. The importance of valid POIs causes many Indexers to alert each other on subgraph health in community discussions. To alleviate the Indexer workload, this Radio utilized Graphcast SDK to exchange and aggregate POI along with a list of Indexer on-chain identities that can be used to trace reputations. With the pubsub pattern, the Indexer can get notified as soon as other indexers (with majority of stake) publish a POI different from the local POI.

## 🏃 Quickstart

📝 **As prerequisites to running the POI cross-checker, make sure that**:

1. You have registered a Graphcast operator address. You can connect a operator address to your indexer address (with a 1:1 relationship) using our very own [Registry contract](https://goerli.etherscan.io/address/0x1e408c2cf66fd3afcea0f49dc44c9f4db5575e79) (on Goerli).
2. You have **graph-node** syncing your indexer's onchain allocations.
3. You have exported the environment variables including `GRAPH_NODE_STATUS_ENDPOINT`, `PRIVATE_KEY` for operator wallet, `ETH_NODE` block provider. 
4. Optionally, provide `SLACK_TOKEN` and `SLACK_WEBHOOK` for POI divergence notifications, and `WAKU_HOST` and `WAKU_PORT` for radio's gossip node instance.
5. Have [Rust](https://www.rust-lang.org/tools/install) installed globally.

If there's no other peer subscribed to a topic you are interested in, start up a boot node
```
cargo run boot
```

With different `WAKU_PORT`s, you can start other instances with
```
cargo run
```

## 🧪 Testing

To run unit tests for the Radio. We recommend using [nextest](https://nexte.st/) as your test runner. Once you have it installed you can run the tests using the following commands:

```
cargo nextest run
```

#### 🔃 Workflow

When an Indexer runs the POI cross-checker, they immediately start listening for new blocks on Ethereum. On a certain interval the Radio fetches all the allocations of that Indexer and saves a list of the IPFS hashes of the subgraphs that the Indexer is allocating to. Right after that we loop through the list and send a request for a normalised POI for each subgraph (using the metadata of the block that we're on) and save those POIs in an in-memopry map, below we will refer to these POIs as _local_ POIs since they are the ones that we've generated.

At the same time, other Indexers running the Radio will start doing the same, which means that messages start propagating through the network. We handle each message and add the POI from it in another in-memory map, we can refer to these POIs as _remote_ POIs since these are the ones that we've received from other network participants. The messages don't come only with the POI and subgraph hash, they also include a nonce (UNIX timestamp), block number and signature. The signature is then used to derive the sender's on-chain Indexer address. It's important to note that before saving an entry to the map, we send a request for the sender's on-chain stake, which will be used later for sorting the entries.

After another interval we compare our _local_ POIs with the _remote_ ones. We sort the remote ones so that for each subgraph (on each block) we can take the POI that is backed by the most on-chain stake (❗ This does not mean the one that is sent by the Indexer with the highest stake, but rather the one that has the most **combined** stake of all the Indexers that attested to it). After we have that top POI, we compare it with our _local_ POI for that subgraph at that block. Voilà! We now know whether our POI matches with the current consensus on the network.

## Contributing

We welcome and appreciate your contributions! Please see the [Contributor Guide](/CONTRIBUTING.md), [Code Of Conduct](/CODE_OF_CONDUCT.md) and [Security Notes](/SECURITY.md) for this repository.
