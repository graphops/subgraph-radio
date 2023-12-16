#![allow(clippy::unnecessary_cast)]
mod test;

use std::str::FromStr;

use graphcast_sdk::graphcast_agent::message_typing::GraphcastMessage;
use sqlx::{Error as SqlxError, SqlitePool};
use tracing::trace;

use crate::messages::poi::PublicPoiMessage;
use crate::messages::upgrade::UpgradeIntentMessage;
use crate::operator::attestation::{Attestation, ComparisonResult, ComparisonResultType};
use crate::operator::notifier::Notification;
use crate::DatabaseError;
use serde_json;

pub async fn insert_local_attestation(
    pool: &SqlitePool,
    new_attestation: Attestation,
) -> Result<Attestation, DatabaseError> {
    trace!("Inserting record {:?}", new_attestation);

    let senders = serde_json::to_string(&new_attestation.senders)?;
    let timestamp = serde_json::to_string(&new_attestation.timestamp)?;
    let block = new_attestation.block_number.to_string();
    let stake = new_attestation.stake_weight.to_string();

    let insert_attestation_query = r#"
        INSERT INTO local_attestations (identifier, block_number, ppoi, stake_weight, sender_group_hash, senders, timestamp)
        VALUES (?, ?, ?, ?, ?, ?, ?)
    "#;
    sqlx::query(insert_attestation_query)
        .bind(&new_attestation.identifier)
        .bind(block)
        .bind(&new_attestation.ppoi)
        .bind(stake)
        .bind(&new_attestation.sender_group_hash)
        .bind(&senders)
        .bind(&timestamp)
        .execute(pool)
        .await?;

    Ok(new_attestation)
}

pub async fn get_local_attestation(
    pool: &SqlitePool,
    identifier: &str,
    block_number: u64,
) -> Result<Option<Attestation>, DatabaseError> {
    let block = block_number.to_string();

    let attestation_data = sqlx::query!(
        r#"
        SELECT 
            identifier, 
            block_number, 
            ppoi, 
            stake_weight, 
            sender_group_hash,
            senders, 
            timestamp
        FROM local_attestations
        WHERE identifier = ? AND block_number = ?
        "#,
        identifier,
        block
    )
    .fetch_optional(pool)
    .await?;

    if let Some(data) = attestation_data {
        let senders: Vec<String> =
            serde_json::from_str(&data.senders).map_err(DatabaseError::SerializationError)?;
        let timestamps: Vec<u64> =
            serde_json::from_str(&data.timestamp).map_err(DatabaseError::SerializationError)?;

        let attestation = Attestation {
            identifier: data.identifier,
            block_number: data.block_number as u64,
            ppoi: data.ppoi,
            stake_weight: data.stake_weight as u64,
            senders,
            sender_group_hash: data.sender_group_hash,
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
) -> Result<Vec<Attestation>, DatabaseError> {
    let attestations = sqlx::query!(
        r#"
        SELECT 
            identifier, 
            block_number, 
            ppoi, 
            stake_weight, 
            sender_group_hash, 
            senders, 
            timestamp
        FROM local_attestations
        WHERE identifier = ?
        "#,
        identifier
    )
    .fetch_all(pool)
    .await?;

    let mut full_attestations = Vec::new();
    for att in attestations {
        let senders: Vec<String> =
            serde_json::from_str(&att.senders).map_err(DatabaseError::SerializationError)?;
        let timestamps: Vec<u64> =
            serde_json::from_str(&att.timestamp).map_err(DatabaseError::SerializationError)?;

        full_attestations.push(Attestation {
            identifier: att.identifier,
            block_number: att.block_number as u64,
            ppoi: att.ppoi,
            stake_weight: att.stake_weight as u64,
            senders,
            sender_group_hash: att.sender_group_hash,
            timestamp: timestamps,
        });
    }

    Ok(full_attestations)
}

pub async fn get_local_attestations(pool: &SqlitePool) -> Result<Vec<Attestation>, DatabaseError> {
    let attestation_records = sqlx::query!(
        r#"
        SELECT 
            identifier, 
            block_number, 
            ppoi, 
            stake_weight, 
            sender_group_hash,
            senders,
            timestamp
        FROM local_attestations
        "#
    )
    .fetch_all(pool)
    .await?;

    let mut full_attestations = Vec::new();

    for record in attestation_records {
        let senders: Vec<String> =
            serde_json::from_str(&record.senders).map_err(DatabaseError::SerializationError)?;
        let timestamps: Vec<u64> =
            serde_json::from_str(&record.timestamp).map_err(DatabaseError::SerializationError)?;

        full_attestations.push(Attestation {
            identifier: record.identifier,
            block_number: record.block_number as u64,
            ppoi: record.ppoi,
            stake_weight: record.stake_weight as u64,
            sender_group_hash: record.sender_group_hash,
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
    let delete_attestations = r#"
        DELETE FROM local_attestations
        WHERE identifier = ? AND block_number <= ?
    "#;

    let block = block_number.to_string();
    let affected_rows = sqlx::query(delete_attestations)
        .bind(identifier)
        .bind(block)
        .execute(pool)
        .await?
        .rows_affected();

    Ok(affected_rows as usize)
}

pub async fn insert_remote_ppoi_message(
    pool: &SqlitePool,
    msg: &GraphcastMessage<PublicPoiMessage>,
) -> Result<GraphcastMessage<PublicPoiMessage>, SqlxError> {
    let block = msg.payload.block_number.to_string();
    let nonce = msg.nonce.to_string();
    sqlx::query!(
        r#"
        INSERT INTO remote_ppoi_messages (identifier, nonce, graph_account, content, network, block_number, block_hash, signature)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        msg.identifier,
        nonce,
        msg.graph_account,
        msg.payload.content,
        msg.payload.network,
        block,
        msg.payload.block_hash,
        msg.signature,
    )
    .execute(pool)
    .await?;

    Ok(msg.clone())
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
    let rows = sqlx::query!(
        r#"
        SELECT identifier, nonce, graph_account, content, network, block_number, block_hash, signature
        FROM remote_ppoi_messages
        "#
    )
    .fetch_all(pool)
    .await?;

    let graphcast_messages = rows
        .into_iter()
        .map(|row| GraphcastMessage {
            identifier: row.identifier.clone(),
            nonce: row.nonce as u64,
            graph_account: row.graph_account.clone(),
            signature: row.signature,
            payload: PublicPoiMessage {
                identifier: row.identifier,
                content: row.content,
                nonce: row.nonce as u64,
                network: row.network,
                block_number: row.block_number as u64,
                block_hash: row.block_hash,
                graph_account: row.graph_account,
            },
        })
        .collect();

    Ok(graphcast_messages)
}

pub async fn get_remote_ppoi_messages_by_identifier(
    pool: &SqlitePool,
    identifier_filter: &str,
) -> Result<Vec<GraphcastMessage<PublicPoiMessage>>, DatabaseError> {
    let rows = sqlx::query!(
        r#"
        SELECT identifier, nonce, graph_account, content, network, block_number, block_hash, signature
        FROM remote_ppoi_messages
        WHERE identifier = ?
        "#,
        identifier_filter
    )
    .fetch_all(pool)
    .await?;

    let graphcast_messages = rows
        .into_iter()
        .map(|row| GraphcastMessage {
            identifier: row.identifier.clone(),
            nonce: row.nonce as u64,
            graph_account: row.graph_account.clone(),
            signature: row.signature,
            payload: PublicPoiMessage {
                identifier: row.identifier,
                content: row.content,
                nonce: row.nonce as u64,
                network: row.network,
                block_number: row.block_number as u64,
                block_hash: row.block_hash,
                graph_account: row.graph_account,
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
    let block = block_number.to_string();

    let affected_rows = sqlx::query!(
        r#"
        DELETE FROM remote_ppoi_messages
        WHERE identifier = ? AND block_number <= ?
        "#,
        identifier,
        block
    )
    .execute(pool)
    .await?
    .rows_affected();

    Ok(affected_rows as usize)
}

pub async fn save_upgrade_intent_message(
    pool: &SqlitePool,
    msg: GraphcastMessage<UpgradeIntentMessage>,
) -> Result<(), DatabaseError> {
    let nonce = msg.nonce.to_string();

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
        msg.identifier,
        nonce,
        msg.graph_account,
        msg.payload.subgraph_id,
        msg.payload.new_hash,
        msg.signature,
    )
    .execute(pool)
    .await
    .map_err(DatabaseError::from)?;

    Ok(())
}

pub async fn get_upgrade_intent_message(
    pool: &SqlitePool,
    subgraph_id: &str,
) -> Result<Option<GraphcastMessage<UpgradeIntentMessage>>, DatabaseError> {
    let message = sqlx::query!(
        r#"
        SELECT deployment, nonce, graph_account, subgraph_id, new_hash, signature
        FROM upgrade_intent_messages
        WHERE subgraph_id = ?
        "#,
        subgraph_id
    )
    .fetch_optional(pool)
    .await?;

    match message {
        Some(msg) => {
            let payload = UpgradeIntentMessage {
                deployment: msg.deployment.clone(),
                subgraph_id: msg.subgraph_id,
                new_hash: msg.new_hash,
                nonce: msg.nonce as u64,
                graph_account: msg.graph_account.clone(),
            };

            let graphcast_message = GraphcastMessage::new(
                msg.deployment,
                msg.nonce as u64,
                msg.graph_account,
                payload,
                msg.signature,
            )?;

            Ok(Some(graphcast_message))
        }
        None => Ok(None),
    }
}

pub async fn get_upgrade_intent_message_by_id(
    pool: &SqlitePool,
    subgraph_id: &str,
) -> Result<Option<GraphcastMessage<UpgradeIntentMessage>>, DatabaseError> {
    let message = sqlx::query!(
        r#"
        SELECT deployment, nonce, graph_account, subgraph_id, new_hash, signature
        FROM upgrade_intent_messages
        WHERE subgraph_id = ?
        "#,
        subgraph_id
    )
    .fetch_optional(pool)
    .await?;

    match message {
        Some(msg) => {
            let payload = UpgradeIntentMessage {
                deployment: msg.deployment.clone(),
                subgraph_id: msg.subgraph_id,
                new_hash: msg.new_hash,
                nonce: msg.nonce as u64,
                graph_account: msg.graph_account.clone(),
            };

            let graphcast_message = GraphcastMessage::new(
                msg.deployment,
                msg.nonce as u64,
                msg.graph_account,
                payload,
                msg.signature,
            )?;

            Ok(Some(graphcast_message))
        }
        None => Ok(None),
    }
}

pub async fn get_upgrade_intent_messages(
    pool: &SqlitePool,
) -> Result<Vec<GraphcastMessage<UpgradeIntentMessage>>, DatabaseError> {
    let raw_messages = sqlx::query!(
        r#"
        SELECT deployment, nonce, graph_account, subgraph_id, new_hash, signature
        FROM upgrade_intent_messages
        "#
    )
    .fetch_all(pool)
    .await?;

    let mut messages = Vec::new();
    for msg in raw_messages {
        let payload = UpgradeIntentMessage {
            deployment: msg.deployment.clone(),
            subgraph_id: msg.subgraph_id,
            new_hash: msg.new_hash,
            nonce: msg.nonce as u64,
            graph_account: msg.graph_account.clone(),
        };

        let graphcast_message = GraphcastMessage::new(
            msg.deployment,
            msg.nonce as u64,
            msg.graph_account,
            payload,
            msg.signature,
        )?;

        messages.push(graphcast_message);
    }

    Ok(messages)
}

pub async fn recent_upgrade(
    pool: &SqlitePool,
    msg: &UpgradeIntentMessage,
    upgrade_threshold: u64,
) -> Result<bool, SqlxError> {
    let nonce_threshold = (msg.nonce - upgrade_threshold).to_string();

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
    let block = comparison_result.block_number.to_string();
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
