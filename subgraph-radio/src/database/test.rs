#[cfg(test)]
mod tests {
    use graphcast_sdk::{graphcast_agent::message_typing::GraphcastMessage, networks::NetworkName};
    use sqlx::SqlitePool;

    use crate::{
        database::{
            get_comparison_results, get_comparison_results_by_deployment, get_local_attestations,
            get_local_attestations_by_identifier, get_remote_ppoi_messages,
            get_upgrade_intent_messages, insert_local_attestation, insert_remote_ppoi_message,
            recent_upgrade, save_comparison_result, save_upgrade_intent_message,
        },
        messages::{poi::PublicPoiMessage, upgrade::UpgradeIntentMessage},
        operator::attestation::{
            handle_comparison_result, Attestation, ComparisonResult, ComparisonResultType,
        },
    };

    #[sqlx::test(migrations = "../migrations")]
    async fn test_db_basic(pool: SqlitePool) {
        let attestation1 = Attestation {
            identifier: "0xa1".to_string(),
            block_number: 0,
            ppoi: "ppoi-x".to_string(),
            stake_weight: 1,
            senders: vec![],
            sender_group_hash: "hash".to_string(),
            timestamp: vec![1],
        };

        let attestation2 = Attestation {
            identifier: "0xa1".to_string(),
            block_number: 0,
            ppoi: "ppoi-y".to_string(),
            stake_weight: 1,
            senders: vec![],
            sender_group_hash: "hash".to_string(),
            timestamp: vec![1],
        };

        let attestation3 = Attestation {
            identifier: "0xa2".to_string(),
            block_number: 0,
            ppoi: "ppoi-z".to_string(),
            stake_weight: 1,
            senders: vec![],
            sender_group_hash: "hash".to_string(),
            timestamp: vec![1],
        };

        let _ = insert_local_attestation(&pool, attestation1).await;
        let _ = insert_local_attestation(&pool, attestation2).await;
        let _ = insert_local_attestation(&pool, attestation3).await;

        let new_comparison_result = ComparisonResult {
            deployment: "test_deployment".to_string(),
            block_number: 42,
            result_type: ComparisonResultType::Match,
            local_attestation: None,
            attestations: vec![],
        };

        let _ = save_comparison_result(&pool, &new_comparison_result).await;

        let hash: String = "QmWECgZdP2YMcV9RtKU41GxcdW8EGYqMNoG98ubu5RGN6U".to_string();
        let content: String =
            "0xa6008cea5905b8b7811a68132feea7959b623188e2d6ee3c87ead7ae56dd0eae".to_string();
        let nonce: u64 = 123321;
        let block_number: u64 = 0;
        let block_hash = "0xblahh".to_string();
        let ppoi_msg = PublicPoiMessage::build(
            hash.clone(),
            content,
            nonce,
            NetworkName::Goerli,
            block_number,
            block_hash,
            String::from("0xe9a1cabd57700b17945fd81feefba82340d9568f"),
        );
        let sig: String = "4be6a6b7f27c4086f22e8be364cbdaeddc19c1992a42b08cbe506196b0aafb0a68c8c48a730b0e3155f4388d7cc84a24b193d091c4a6a4e8cd6f1b305870fae61b".to_string();

        let msg = GraphcastMessage::new(
            hash.clone(),
            nonce,
            String::from("0xe9a1cabd57700b17945fd81feefba82340d9568f"),
            ppoi_msg,
            sig,
        )
        .expect("Shouldn't get here since the message is purposefully constructed for testing");

        let _ = insert_remote_ppoi_message(&pool, &msg).await;

        let sig: String = "4be6a6b7f27c4086f22e8be364cbdaeddc19c1992a42b0 8cbe506196b0aafb0a68c8c48a730b0e3155f4388d7cc84a24b193d091c4a6a4e8cd6f1b305870fae61b".to_string();
        let deployment = "QmacQnSgia4iDPWHpeY6aWxesRFdb8o5DKZUx96zZqEWrB".to_string();

        let ui_msg = UpgradeIntentMessage {
            deployment,
            subgraph_id: String::from("CnJMdCkW3pr619gsJVtUPAWxspALPdCMw6o7obzYBNp3"),
            new_hash: String::from("QmacQnSgia4iDPWHpeY6aWxesRFdb8o5DKZUx96zZqEWAA"),
            nonce,
            graph_account: String::from("0xe9a1cabd57700b17945fd81feefba82340d9568f"),
        };
        let msg = GraphcastMessage::new(
            "QmacQnSgia4iDPWHpeY6aWxesRFdb8o5DKZUx96zZqEWrB".to_string(),
            nonce,
            String::from("0xe9a1cabd57700b17945fd81feefba82340d9568f"),
            ui_msg,
            sig,
        )
        .expect("Shouldn't get here since the message is purposefully constructed for testing");

        let _ = save_upgrade_intent_message(&pool, msg).await;

        let local_attestations = get_local_attestations(&pool).await.unwrap();
        let remote_ppoi_messages = get_remote_ppoi_messages(&pool).await.unwrap();
        let upgrade_intent_messages = get_upgrade_intent_messages(&pool).await.unwrap();
        let comparison_results = get_comparison_results(&pool).await.unwrap();

        assert_eq!(
            local_attestations.len(),
            3,
            "The number of local attestations should be 3"
        );
        assert_eq!(
            remote_ppoi_messages.len(),
            1,
            "There should be one remote PPOI message"
        );
        assert_eq!(
            upgrade_intent_messages.len(),
            1,
            "There should be one upgrade intent message"
        );

        let local_attestations_0xa1 = get_local_attestations_by_identifier(&pool, "0xa1")
            .await
            .unwrap();

        assert_eq!(
            local_attestations_0xa1.len(),
            2,
            "There should be two local attestations for identifier 0xa1"
        );

        let local_attestations_0xa2 = get_local_attestations_by_identifier(&pool, "0xa2")
            .await
            .unwrap();

        assert_eq!(
            local_attestations_0xa2.len(),
            1,
            "There should be one local attestation for identifier 0xa2"
        );

        let poi_0xa1 = &local_attestations_0xa1.first().unwrap().ppoi;
        assert_eq!(poi_0xa1, "ppoi-x");

        assert_eq!(comparison_results.len(), 1);

        let comparison_result_for_test_deployemnt =
            get_comparison_results_by_deployment(&pool, "test_deployment")
                .await
                .unwrap()
                .unwrap();

        assert_eq!(comparison_result_for_test_deployemnt.block_number, 42);

        assert_eq!(
            comparison_result_for_test_deployemnt.result_type,
            ComparisonResultType::Match
        );
    }

    #[tokio::test]
    async fn handle_comparison_result_new_deployment() {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("Failed to connect to the in-memory database");
        sqlx::migrate!("../migrations")
            .run(&pool)
            .await
            .expect("Could not run migration");

        let new_result = ComparisonResult {
            deployment: String::from("new_deployment"),
            block_number: 1,
            result_type: ComparisonResultType::Match,
            local_attestation: None,
            attestations: Vec::new(),
        };

        let _ = handle_comparison_result(&pool, &new_result).await;
        let comparison_results = get_comparison_results(&pool).await.unwrap();
        let result = comparison_results.first().unwrap();

        assert_eq!(result.deployment, "new_deployment".to_string());
    }

    #[tokio::test]
    async fn handle_comparison_result_change_result_type() {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("Failed to connect to the in-memory database");
        sqlx::migrate!("../migrations")
            .run(&pool)
            .await
            .expect("Could not run migration");

        let old_result = ComparisonResult {
            deployment: String::from("existing_deployment"),
            block_number: 1,
            result_type: ComparisonResultType::Match,
            local_attestation: None,
            attestations: Vec::new(),
        };

        let new_result = ComparisonResult {
            deployment: String::from("existing_deployment"),
            block_number: 1,
            result_type: ComparisonResultType::Divergent,
            local_attestation: None,
            attestations: Vec::new(),
        };

        let _ = save_comparison_result(&pool, &old_result).await;
        let _ = handle_comparison_result(&pool, &new_result).await;

        let comparison_results = get_comparison_results(&pool).await.unwrap();
        assert_eq!(
            comparison_results.first().unwrap().result_type,
            ComparisonResultType::Divergent
        );
    }

    #[tokio::test]
    async fn upgrade_ratelimiting() {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("Failed to connect to the in-memory database");
        sqlx::migrate!("../migrations")
            .run(&pool)
            .await
            .expect("Could not run migration");

        let upgrade_threshold = 86400;
        let test_id = "AAAMdCkW3pr619gsJVtUPAWxspALPdCMw6o7obzYBNp3".to_string();

        // Make 2 msgs
        let msg0 = UpgradeIntentMessage {
            deployment: "A0".to_string(),
            subgraph_id: test_id.clone(),
            new_hash: "QmVVfLWowm1xkqc41vcygKNwFUvpsDSMbHdHghxmDVmH9x".to_string(),
            nonce: 1692307513,
            graph_account: "0xe9a1cabd57700b17945fd81feefba82340d9568f".to_string(),
        };
        let gc_msg0 = GraphcastMessage {
            identifier: "A0".to_string(),
            nonce: 1692307513,
            graph_account: "0xe9a1cabd57700b17945fd81feefba82340d9568f".to_string(),
            payload: msg0,
            signature: "0xA".to_string(),
        };
        let msg1 = UpgradeIntentMessage {
            deployment: "B".to_string(),
            subgraph_id: "BBBMdCkW3pr619gsJVtUPAWxspALPdCMw6o7obzYBNp3".to_string(),
            new_hash: "QmVVfLWowm1xkqc41vcygKNwFUvpsDSMbHdHghxmDVmH9x".to_string(),
            nonce: 1691307513,
            graph_account: "0xe9a1cabd57700b17945fd81feefba82340d9568f".to_string(),
        };
        let gc_msg1 = GraphcastMessage {
            identifier: "B".to_string(),
            nonce: 1691307513,
            graph_account: "0xe9a1cabd57700b17945fd81feefba82340d9568f".to_string(),
            payload: msg1,
            signature: "0xB".to_string(),
        };

        let _ = save_upgrade_intent_message(&pool, gc_msg0).await;
        let _ = save_upgrade_intent_message(&pool, gc_msg1).await;

        let upgrade_intent_messages = get_upgrade_intent_messages(&pool).await.unwrap();
        assert_eq!(upgrade_intent_messages.len(), 2);

        let msg0 = UpgradeIntentMessage {
            deployment: "A2".to_string(),
            subgraph_id: test_id.clone(),
            new_hash: "QmVVfLWowm1xkqc41vcygKNwFUvpsDSMbHdHghxmDVmH9x".to_string(),
            nonce: 1692307600,
            graph_account: "0xe9a1cabd57700b17945fd81feefba82340d9568f".to_string(),
        };

        assert!(recent_upgrade(&pool, &msg0, upgrade_threshold)
            .await
            .unwrap());

        let msg0 = UpgradeIntentMessage {
            deployment: "A2".to_string(),
            subgraph_id: test_id.clone(),
            new_hash: "QmAAfLWowm1xkqc41vcygKNwFUvpsDSMbHdHghxmDVmH9x".to_string(),
            nonce: 1692407600,
            graph_account: "0xe9a1cabd57700b17945fd81feefba82340d9568f".to_string(),
        };
        assert!(!recent_upgrade(&pool, &msg0, upgrade_threshold)
            .await
            .unwrap());

        let gc_msg0 = GraphcastMessage {
            identifier: "A2".to_string(),
            nonce: 1692407600,
            graph_account: "0xe9a1cabd57700b17945fd81feefba82340d9568f".to_string(),
            payload: msg0,
            signature: "0xA".to_string(),
        };

        let _ = save_upgrade_intent_message(&pool, gc_msg0).await;

        let msgs = get_upgrade_intent_messages(&pool).await.unwrap();
        println!("{:?}", msgs);

        let msg = msgs
            .iter()
            .find(|msg| msg.payload.subgraph_id == test_id)
            .unwrap();

        assert_eq!(msg.nonce, 1692407600);
    }
}
