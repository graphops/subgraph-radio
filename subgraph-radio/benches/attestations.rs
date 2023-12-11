#![allow(unused_must_use)]
use criterion::criterion_main;

extern crate criterion;

mod attestation {
    use criterion::{black_box, criterion_group, Criterion};
    use graphcast_sdk::graphcast_agent::message_typing::GraphcastMessage;
    use sqlx::SqlitePool;
    use std::collections::HashMap;
    use subgraph_radio::{
        database::{insert_local_attestation, insert_remote_ppoi_message},
        messages::poi::PublicPoiMessage,
        operator::attestation::{compare_attestations, local_comparison_point, Attestation},
    };

    criterion_group!(
        benches,
        update_attestations_bench,
        compare_attestations_bench,
        comparison_point_bench
    );

    fn update_attestations_bench(c: &mut Criterion) {
        let attestation = black_box(Attestation::new(
            "Qm123".to_string(),
            1,
            "awesome-ppoi".to_string(),
            0,
            vec!["0xa1".to_string()],
            vec![2],
        ));

        c.bench_function("update_attestation", |b| {
            b.iter(|| Attestation::update(&attestation, "0xa1".to_string(), 1, 1))
        });
    }

    fn compare_attestations_bench(c: &mut Criterion) {
        let mut remote_blocks: HashMap<u64, Vec<Attestation>> = black_box(HashMap::new());
        let mut local_blocks: HashMap<u64, Attestation> = black_box(HashMap::new());

        let remote = black_box(Attestation::new(
            "Qm123".to_string(),
            1,
            "awesome-ppoi".to_string(),
            0,
            vec!["0xa1".to_string()],
            vec![0],
        ));
        black_box(remote_blocks.insert(42, vec![remote]));

        let local = black_box(Attestation::new(
            "Qm123".to_string(),
            1,
            "awesome-ppoi".to_string(),
            0,
            Vec::new(),
            vec![0],
        ));
        black_box(local_blocks.insert(42, local.clone()));

        let mut remote_attestations: HashMap<String, HashMap<u64, Vec<Attestation>>> =
            black_box(HashMap::new());
        let mut local_attestations: HashMap<String, HashMap<u64, Attestation>> =
            black_box(HashMap::new());

        black_box(remote_attestations.insert("my-awesome-hash".to_string(), remote_blocks));
        black_box(local_attestations.insert("my-awesome-hash".to_string(), local_blocks));

        c.bench_function("compare_attestations", |b| {
            b.iter(|| {
                compare_attestations(
                    Some(local.clone()),
                    42,
                    black_box(&remote_attestations.clone()),
                    "my-awesome-hash",
                )
            })
        });
    }

    async fn comparison_point_bench(c: &mut Criterion) {
        let runtime = black_box(tokio::runtime::Runtime::new().unwrap());
        let pool = black_box(
            SqlitePool::connect("sqlite::memory:")
                .await
                .expect("Failed to connect to the in-memory database"),
        );
        black_box(
            sqlx::migrate!("../migrations")
                .run(&pool)
                .await
                .expect("Could not run migration"),
        );

        let attestations = vec![
            black_box(Attestation::new(
                "awesome-ppoi".to_string(),
                42,
                "ppoi1".to_string(),
                0,
                vec!["0xa1".to_string()],
                vec![2],
            )),
            black_box(Attestation::new(
                "awesome-ppoi".to_string(),
                43,
                "ppoi2".to_string(),
                0,
                vec!["0xa2".to_string()],
                vec![4],
            )),
            black_box(Attestation::new(
                "awesome-ppoi".to_string(),
                44,
                "ppoi3".to_string(),
                1,
                vec!["0xa3".to_string()],
                vec![6],
            )),
        ];

        for attestation in attestations {
            black_box(insert_local_attestation(&pool, attestation).await.unwrap());
        }

        black_box(insert_remote_ppoi_message(&pool, &GraphcastMessage {
            identifier: String::from("hash"),
            nonce: 2,
            graph_account: String::from("0x7e6528e4ce3055e829a32b5dc4450072bac28bc6"),
            payload: PublicPoiMessage {
                identifier: String::from("hash"),
                content: String::from("awesome-ppoi"),
                nonce: 2,
                network: String::from("goerli"),
                block_number: 42,
                block_hash: String::from("4dbba1ba9fb18b0034965712598be1368edcf91ae2c551d59462aab578dab9c5"),
                graph_account: String::from("0xa1"),
            },
            signature: String::from("03b197380ab9ee3a9fcaea1301224ad1ff02e9e414275fd79d6ee463b21eb6957af7670a26b0a7f8a6316d95dba8497f2bd67b32b39be07073cf81beff0b37961b"),
        } ).await.unwrap());

        c.bench_function("comparison_point", |b| {
            b.iter(|| {
                let pool = &pool;
                runtime.block_on(async {
                    let pool_clone = pool.clone();

                    local_comparison_point("hash", 120, pool_clone)
                        .await
                        .unwrap();
                })
            });
        });

        // Clean up
        std::mem::drop(pool);
    }
}
criterion_main!(attestation::benches);
