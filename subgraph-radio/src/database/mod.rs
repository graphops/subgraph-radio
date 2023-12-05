#![allow(clippy::unnecessary_cast)]
mod test;

use std::str::FromStr;

use graphcast_sdk::graphcast_agent::message_typing::GraphcastMessage;
use sqlx::{Error as SqlxError, SqlitePool};

use crate::messages::poi::PublicPoiMessage;
use crate::messages::upgrade::UpgradeIntentMessage;
use crate::operator::attestation::{Attestation, ComparisonResult, ComparisonResultType};
use crate::operator::notifier::Notification;
use crate::DatabaseError;
pub async fn insert_local_attestation(
    pool: &SqlitePool,
    new_attestation: Attestation,
) -> Result<Attestation, SqlxError> {
    let insert_attestation_query = r#"
        INSERT INTO local_attestations (identifier, block_number, ppoi, stake_weight, sender_group_hash)
        VALUES (?, ?, ?, ?, ?)
    "#;
    sqlx::query(insert_attestation_query)
        .bind(&new_attestation.identifier)
        .bind(new_attestation.block_number as i64)
        .bind(&new_attestation.ppoi)
        .bind(new_attestation.stake_weight as i64)
        .bind(&new_attestation.sender_group_hash)
        .execute(pool)
        .await?;

    for sender in &new_attestation.senders {
        let insert_sender_query = r#"
            INSERT INTO local_attestation_senders (attestation_block_number, attestation_identifier, attestation_ppoi, sender)
            VALUES (?, ?, ?, ?)
        "#;
        sqlx::query(insert_sender_query)
            .bind(new_attestation.block_number as i64)
            .bind(&new_attestation.identifier)
            .bind(&new_attestation.ppoi)
            .bind(sender)
            .execute(pool)
            .await?;
    }

    for timestamp in &new_attestation.timestamp {
        let insert_timestamp_query = r#"
            INSERT INTO local_attestation_timestamps (attestation_block_number, attestation_identifier, attestation_ppoi, timestamp)
            VALUES (?, ?, ?, ?)
        "#;
        sqlx::query(insert_timestamp_query)
            .bind(new_attestation.block_number as i64)
            .bind(&new_attestation.identifier)
            .bind(&new_attestation.ppoi)
            .bind(timestamp)
            .execute(pool)
            .await?;
    }

    Ok(new_attestation)
}

pub async fn get_local_attestation(
    pool: &SqlitePool,
    identifier: &str,
    block_number: u64,
) -> Result<Option<Attestation>, SqlxError> {
    let block_number = block_number as i64;

    let basic_attestation = sqlx::query!(
        r#"
        SELECT identifier, block_number, ppoi, stake_weight, sender_group_hash
        FROM local_attestations
        WHERE identifier = ? AND block_number = ?
        "#,
        identifier,
        block_number
    )
    .fetch_optional(pool)
    .await?;

    if let Some(basic) = basic_attestation {
        let block_number = block_number as i64;
        let senders = sqlx::query!(
            r#"
            SELECT sender
            FROM local_attestation_senders
            WHERE attestation_identifier = ? AND attestation_block_number = ?
            "#,
            identifier,
            block_number
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| row.sender)
        .collect::<Vec<String>>();

        let timestamps = sqlx::query!(
            r#"
            SELECT timestamp
            FROM local_attestation_timestamps
            WHERE attestation_identifier = ? AND attestation_block_number = ?
            "#,
            identifier,
            block_number
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| row.timestamp as u32)
        .collect::<Vec<u32>>();

        let attestation = Attestation {
            identifier: basic.identifier,
            block_number: basic.block_number as u64,
            ppoi: basic.ppoi,
            stake_weight: basic.stake_weight as u64,
            senders,
            sender_group_hash: basic.sender_group_hash,
            timestamp: timestamps,
        };

        Ok(Some(attestation))
    } else {
        Ok(None)
    }
}

pub async fn get_local_attestations_by_identifier(
    pool: &SqlitePool,
    identifier: &str,
) -> Result<Vec<Attestation>, SqlxError> {
    let attestations = sqlx::query!(
        r#"
        SELECT identifier, block_number, ppoi, stake_weight, sender_group_hash
        FROM local_attestations
        WHERE identifier = ?
        "#,
        identifier
    )
    .fetch_all(pool)
    .await?;

    let mut full_attestations = Vec::new();
    for att in attestations {
        let block_number = att.block_number as i64;

        let senders = sqlx::query!(
            r#"
            SELECT sender
            FROM local_attestation_senders
            WHERE attestation_identifier = ? AND attestation_block_number = ?
            "#,
            identifier,
            block_number,
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| row.sender)
        .collect::<Vec<String>>();

        let timestamps = sqlx::query!(
            r#"
            SELECT timestamp
            FROM local_attestation_timestamps
            WHERE attestation_identifier = ? AND attestation_block_number = ?
            "#,
            identifier,
            block_number
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| row.timestamp as u32)
        .collect::<Vec<u32>>();

        full_attestations.push(Attestation {
            identifier: att.identifier,
            block_number: block_number as u64,
            ppoi: att.ppoi,
            stake_weight: att.stake_weight as u64,
            senders,
            sender_group_hash: att.sender_group_hash,
            timestamp: timestamps,
        });
    }

    Ok(full_attestations)
}

pub async fn get_local_attestations(pool: &SqlitePool) -> Result<Vec<Attestation>, SqlxError> {
    let basic_attestations = sqlx::query!(
        r#"
        SELECT 
            identifier, 
            block_number, 
            ppoi, 
            stake_weight, 
            sender_group_hash
        FROM local_attestations
        "#
    )
    .fetch_all(pool)
    .await?;

    let mut full_attestations = Vec::new();

    for attestation in basic_attestations {
        let block_number = attestation.block_number as i64;

        let senders = sqlx::query!(
            r#"
            SELECT sender
            FROM local_attestation_senders
            WHERE attestation_identifier = ? AND attestation_block_number = ?
            "#,
            attestation.identifier,
            block_number
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|record| record.sender)
        .collect();

        let timestamps = sqlx::query!(
            r#"
            SELECT timestamp
            FROM local_attestation_timestamps
            WHERE attestation_identifier = ? AND attestation_block_number = ?
            "#,
            attestation.identifier,
            block_number
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|record| record.timestamp as u32)
        .collect();

        full_attestations.push(Attestation {
            identifier: attestation.identifier,
            block_number: attestation.block_number as u64,
            ppoi: attestation.ppoi,
            stake_weight: attestation.stake_weight as u64,
            sender_group_hash: attestation.sender_group_hash,
            senders,
            timestamp: timestamps,
        });
    }

    Ok(full_attestations)
}

pub async fn delete_outdated_local_attestations(
    pool: &SqlitePool,
    identifier: &str,
    block_number: u64,
) -> Result<usize, SqlxError> {
    let delete_senders = r#"
        DELETE FROM local_attestation_senders
        WHERE attestation_identifier = ? AND attestation_block_number <= ?
    "#;
    sqlx::query(delete_senders)
        .bind(identifier)
        .bind(block_number as i64)
        .execute(pool)
        .await?;

    let delete_timestamps = r#"
        DELETE FROM local_attestation_timestamps
        WHERE attestation_identifier = ? AND attestation_block_number <= ?
    "#;
    sqlx::query(delete_timestamps)
        .bind(identifier)
        .bind(block_number as i64)
        .execute(pool)
        .await?;

    let delete_attestations = r#"
        DELETE FROM local_attestations
        WHERE identifier = ? AND block_number <= ?
    "#;
    let affected_rows = sqlx::query(delete_attestations)
        .bind(identifier)
        .bind(block_number as i64)
        .execute(pool)
        .await?
        .rows_affected();

    Ok(affected_rows as usize)
}

pub async fn insert_remote_ppoi_message(
    pool: &SqlitePool,
    msg: &GraphcastMessage<PublicPoiMessage>,
) -> Result<RemotePpoiMessageRecord, SqlxError> {
    let new_message = &NewRemotePpoiMessage::from_graphcast_message(msg);

    let inserted_record = sqlx::query_as!(
        RemotePpoiMessageRecord,
        r#"
        INSERT INTO remote_ppoi_messages (identifier, nonce, graph_account, content, network, block_number, block_hash, signature)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        RETURNING identifier, nonce, graph_account, content, network, block_number, block_hash, signature
        "#,
        new_message.identifier,
        new_message.nonce,
        new_message.graph_account,
        new_message.content,
        new_message.network,
        new_message.block_number,
        new_message.block_hash,
        new_message.signature,
    )
    .fetch_one(pool)
    .await?;

    Ok(inserted_record)
}

pub async fn count_remote_ppoi_messages(
    pool: &SqlitePool,
    identifier: &str,
) -> Result<i32, SqlxError> {
    let count: i32 = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)
        FROM remote_ppoi_messages
        WHERE identifier = ?
        "#,
        identifier
    )
    .fetch_one(pool)
    .await?;

    Ok(count)
}

pub async fn get_remote_ppoi_messages(
    pool: &SqlitePool,
) -> Result<Vec<GraphcastMessage<PublicPoiMessage>>, SqlxError> {
    let records = sqlx::query_as!(
        RemotePpoiMessageRecord,
        r#"
        SELECT identifier, nonce, graph_account, content, network, block_number, block_hash, signature
        FROM remote_ppoi_messages
        "#
    )
    .fetch_all(pool)
    .await?;

    let graphcast_messages = records
        .into_iter()
        .map(|record| GraphcastMessage {
            identifier: record.identifier.clone(),
            nonce: record.nonce,
            graph_account: record.graph_account.clone(),
            signature: record.signature,
            payload: PublicPoiMessage {
                identifier: record.identifier,
                content: record.content,
                nonce: record.nonce,
                network: record.network,
                block_number: record.block_number as u64,
                block_hash: record.block_hash,
                graph_account: record.graph_account,
            },
        })
        .collect();

    Ok(graphcast_messages)
}

pub async fn get_remote_ppoi_messages_by_identifier(
    pool: &SqlitePool,
    identifier_filter: &str,
) -> Result<Vec<GraphcastMessage<PublicPoiMessage>>, SqlxError> {
    let records = sqlx::query_as!(
        RemotePpoiMessageRecord,
        r#"
        SELECT identifier, nonce, graph_account, content, network, block_number, block_hash, signature
        FROM remote_ppoi_messages
        WHERE identifier = ?
        "#,
        identifier_filter
    )
    .fetch_all(pool)
    .await?;

    let graphcast_messages = records
        .into_iter()
        .map(|record| GraphcastMessage {
            identifier: record.identifier.clone(),
            nonce: record.nonce,
            graph_account: record.graph_account.clone(),
            signature: record.signature,
            payload: PublicPoiMessage {
                identifier: record.identifier,
                content: record.content,
                nonce: record.nonce,
                network: record.network,
                block_number: record.block_number as u64,
                block_hash: record.block_hash,
                graph_account: record.graph_account,
            },
        })
        .collect();

    Ok(graphcast_messages)
}

pub async fn clean_remote_ppoi_messages(
    pool: &SqlitePool,
    identifier: &str,
    block_number: u64,
) -> Result<usize, SqlxError> {
    let block_number = block_number as i64;

    let affected_rows = sqlx::query!(
        r#"
        DELETE FROM remote_ppoi_messages
        WHERE identifier = ? AND block_number <= ?
        "#,
        identifier,
        block_number
    )
    .execute(pool)
    .await?
    .rows_affected();

    Ok(affected_rows as usize)
}

pub async fn save_upgrade_intent_message(
    pool: &SqlitePool,
    msg: GraphcastMessage<UpgradeIntentMessage>,
) -> Result<(), SqlxError> {
    let new_message = UpgradeIntentMessage {
        deployment: msg.identifier,
        nonce: msg.nonce,
        graph_account: msg.graph_account,
        subgraph_id: msg.payload.subgraph_id,
        new_hash: msg.payload.new_hash,
        signature: msg.signature,
    };

    sqlx::query!(
        r#"
        INSERT INTO upgrade_intent_messages (deployment, nonce, graph_account, subgraph_id, new_hash, signature)
        VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(subgraph_id)
        DO UPDATE SET
            nonce = excluded.nonce,
            graph_account = excluded.graph_account,
            deployment = excluded.deployment,
            new_hash = excluded.new_hash,
            signature = excluded.signature
        "#,
        new_message.deployment,
        new_message.nonce,
        new_message.graph_account,
        new_message.subgraph_id,
        new_message.new_hash,
        new_message.signature,
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_upgrade_intent_message(
    pool: &SqlitePool,
    subgraph_id: &str,
) -> Result<Option<UpgradeIntentMessage>, SqlxError> {
    let message = sqlx::query_as!(
        UpgradeIntentMessage,
        r#"
        SELECT deployment, nonce, graph_account, subgraph_id, new_hash, signature
        FROM upgrade_intent_messages
        WHERE subgraph_id = ?
        "#,
        subgraph_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(message)
}

pub async fn get_upgrade_intent_messages(
    pool: &SqlitePool,
) -> Result<Vec<UpgradeIntentMessage>, SqlxError> {
    let messages = sqlx::query_as!(
        UpgradeIntentMessage,
        r#"
        SELECT deployment, nonce, graph_account, subgraph_id, new_hash, signature
        FROM upgrade_intent_messages
        "#
    )
    .fetch_all(pool)
    .await?;

    Ok(messages)
}

pub async fn recent_upgrade(
    pool: &SqlitePool,
    msg: &UpgradeIntentMessage,
    upgrade_threshold: i64,
) -> Result<bool, SqlxError> {
    let nonce_threshold = msg.nonce - upgrade_threshold;

    let record = sqlx::query!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM upgrade_intent_messages
            WHERE subgraph_id = ?
            AND nonce > ?
        ) AS "exists_!"
        "#,
        msg.subgraph_id,
        nonce_threshold
    )
    .fetch_one(pool)
    .await?;

    Ok(record.exists_ != 0)
}

pub async fn create_notification(
    pool: &SqlitePool,
    new_notification: Notification,
) -> Result<Notification, SqlxError> {
    let inserted_record = sqlx::query_as!(
        Notification,
        r#"
        INSERT INTO notifications (deployment, message)
        VALUES (?, ?)
        RETURNING deployment, message
        "#,
        new_notification.deployment,
        new_notification.message,
    )
    .fetch_one(pool)
    .await?;

    Ok(inserted_record)
}

pub async fn get_notification_by_deployment(
    pool: &SqlitePool,
    deployment: &str,
) -> Result<Notification, SqlxError> {
    let notification = sqlx::query_as!(
        Notification,
        r#"
        SELECT deployment, message
        FROM notifications
        WHERE deployment = ?
        "#,
        deployment
    )
    .fetch_one(pool)
    .await?;

    Ok(notification)
}

pub async fn get_notifications(pool: &SqlitePool) -> Result<Vec<Notification>, SqlxError> {
    let notifications = sqlx::query_as!(
        Notification,
        r#"
        SELECT deployment, message
        FROM notifications
        "#
    )
    .fetch_all(pool)
    .await?;

    Ok(notifications)
}

pub async fn clear_all_notifications(pool: &SqlitePool) -> Result<(), SqlxError> {
    sqlx::query!(
        r#"
        DELETE FROM notifications
        "#
    )
    .execute(pool)
    .await?;

    Ok(())
}

#[derive(sqlx::FromRow)]
#[allow(dead_code)]
pub struct RemotePpoiMessageRecord {
    identifier: String,
    nonce: i64,
    graph_account: String,
    content: String,
    network: String,
    block_number: i64,
    block_hash: String,
    signature: String,
}

impl From<RemotePpoiMessageRecord> for PublicPoiMessage {
    fn from(record: RemotePpoiMessageRecord) -> Self {
        PublicPoiMessage {
            identifier: record.identifier,
            content: record.content,
            nonce: record.nonce,
            network: record.network,
            block_number: record.block_number as u64,
            block_hash: record.block_hash,
            graph_account: record.graph_account,
        }
    }
}

pub async fn save_comparison_result(
    pool: &SqlitePool,
    comparison_result: &ComparisonResult,
) -> Result<u64, DatabaseError> {
    let local_attestation_json =
        serde_json::to_string(&comparison_result.local_attestation.as_ref())
            .map_err(DatabaseError::SerializationError)?;
    let attestations_json = serde_json::to_string(&comparison_result.attestations)
        .map_err(DatabaseError::SerializationError)?;

    let result_type = comparison_result.result_type.to_string();
    let block = comparison_result.block_number as i64;
    let deployment = comparison_result.deployment_hash();

    let rows_affected = sqlx::query!(
        "INSERT INTO comparison_results (deployment, block_number, result_type, local_attestation_json, attestations_json)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(deployment)
         DO UPDATE SET
         block_number = excluded.block_number,
         result_type = excluded.result_type,
         local_attestation_json = excluded.local_attestation_json,
         attestations_json = excluded.attestations_json",
        deployment,
        block,
        result_type,
        local_attestation_json,
        attestations_json
    )
    .execute(pool)
    .await
    .map_err(DatabaseError::CoreSqlx)?
    .rows_affected();

    Ok(rows_affected)
}

pub async fn get_comparison_results(
    pool: &SqlitePool,
) -> Result<Vec<ComparisonResult>, DatabaseError> {
    let rows = sqlx::query!(
        r#"
        SELECT deployment, block_number, result_type, local_attestation_json, attestations_json
        FROM comparison_results
        "#
    )
    .fetch_all(pool)
    .await?;

    let mut results = Vec::new();
    for row in rows {
        let local_attestation =
            serde_json::from_str::<Option<Attestation>>(&row.local_attestation_json)
                .unwrap_or(None);
        let attestations = serde_json::from_str::<Vec<Attestation>>(&row.attestations_json)
            .unwrap_or_else(|_| Vec::new());

        results.push(ComparisonResult {
            deployment: row.deployment,
            block_number: row.block_number as u64,
            result_type: ComparisonResultType::from_str(&row.result_type)?,
            local_attestation,
            attestations,
        });
    }

    Ok(results)
}

pub async fn get_comparison_results_by_type(
    pool: &SqlitePool,
    result_type: &str,
) -> Result<Vec<ComparisonResult>, DatabaseError> {
    let rows = sqlx::query!(
        r#"
        SELECT deployment, block_number, result_type, local_attestation_json, attestations_json
        FROM comparison_results
        WHERE result_type = ?
        "#,
        result_type
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let local_attestation =
                serde_json::from_str::<Option<Attestation>>(&row.local_attestation_json)
                    .unwrap_or(None);
            let attestations = serde_json::from_str::<Vec<Attestation>>(&row.attestations_json)
                .unwrap_or_else(|_| Vec::new());

            ComparisonResultType::from_str(&row.result_type)
                .map_err(DatabaseError::ParseError)
                .map(|result_type| ComparisonResult {
                    deployment: row.deployment,
                    block_number: row.block_number as u64,
                    result_type,
                    local_attestation,
                    attestations,
                })
        })
        .collect::<Result<Vec<ComparisonResult>, DatabaseError>>()
}

pub async fn get_comparison_results_by_deployment(
    pool: &SqlitePool,
    deployment_hash: &str,
) -> Result<Option<ComparisonResult>, DatabaseError> {
    let row = sqlx::query!(
        r#"
        SELECT deployment, block_number, result_type, local_attestation_json, attestations_json
        FROM comparison_results
        WHERE deployment = ?
        "#,
        deployment_hash
    )
    .fetch_optional(pool)
    .await?;

    row.map(|row| {
        let local_attestation =
            serde_json::from_str::<Option<Attestation>>(&row.local_attestation_json)
                .unwrap_or(None);
        let attestations = serde_json::from_str::<Vec<Attestation>>(&row.attestations_json)
            .unwrap_or_else(|_| Vec::new());

        Ok(ComparisonResult {
            deployment: row.deployment,
            block_number: row.block_number as u64,
            result_type: ComparisonResultType::from_str(&row.result_type)?,
            local_attestation,
            attestations,
        })
    })
    .transpose()
}

pub struct NewRemotePpoiMessage {
    pub identifier: String,
    pub nonce: i64,
    pub graph_account: String,
    pub content: String,
    pub network: String,
    pub block_number: i64,
    pub block_hash: String,
    pub signature: String,
}

impl NewRemotePpoiMessage {
    pub fn from_graphcast_message(msg: &GraphcastMessage<PublicPoiMessage>) -> Self {
        NewRemotePpoiMessage {
            identifier: msg.payload.identifier.clone(),
            nonce: msg.nonce,
            graph_account: msg.graph_account.clone(),
            content: msg.payload.content.clone(),
            network: msg.payload.network.clone(),
            block_number: msg.payload.block_number as i64,
            block_hash: msg.payload.block_hash.clone(),
            signature: msg.signature.clone(),
        }
    }
}
