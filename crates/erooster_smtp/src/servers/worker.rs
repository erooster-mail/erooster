// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! Background queue worker: pops outbound emails from the persistent queue and
//! attempts delivery via `send_email_job`. On success the entry is acked; on
//! failure it is nacked so the scheduler can retry with exponential backoff.

use crate::servers::sending::{send_email_job, EmailPayload};
use erooster_core::backend::{database::DB, database::Database, queue};
use {
    serde_json,
    tokio::{self, time::Duration},
    tokio_util::sync::CancellationToken,
    tracing::{error, info, instrument, warn},
};

/// Starts the outbound queue worker loop.
///
/// The worker polls the queue every `poll_interval` and delivers any ready
/// messages. It exits cleanly when `shutdown` is cancelled.
#[instrument(skip(database, shutdown))]
pub async fn run(database: DB, shutdown: CancellationToken) {
    let poll_interval = Duration::from_secs(30);
    info!("Outbound queue worker started");

    loop {
        tokio::select! {
            () = shutdown.cancelled() => {
                info!("Outbound queue worker shutting down");
                return;
            }
            () = tokio::time::sleep(poll_interval) => {
                process_queue(&database).await;
            }
        }
    }
}

async fn process_queue(database: &DB) {
    loop {
        match queue::pop(database.get_pool()).await {
            Err(e) => {
                error!("Failed to pop from outbound queue: {e:?}");
                return;
            }
            Ok(None) => return, // queue empty
            Ok(Some(entry)) => {
                deliver(database, entry).await;
            }
        }
    }
}

#[allow(clippy::cognitive_complexity)]
async fn deliver(database: &DB, entry: queue::QueueEntry) {
    let payload: EmailPayload = match serde_json::from_str(&entry.payload) {
        Ok(p) => p,
        Err(e) => {
            error!("Malformed queue entry {}: {e:?}", entry.id);
            // Poison pill — remove it so it doesn't block the queue forever.
            if let Err(e) = queue::ack(database.get_pool(), &entry.id).await {
                error!("Failed to ack malformed entry {}: {e:?}", entry.id);
            }
            return;
        }
    };

    match send_email_job(&payload).await {
        Ok(()) => {
            info!("Successfully delivered outbound email {}", entry.id);
            if let Err(e) = queue::ack(database.get_pool(), &entry.id).await {
                error!("Failed to ack delivered entry {}: {e:?}", entry.id);
            }
        }
        Err(e) => {
            let next_attempt = entry.attempts + 1;
            if next_attempt >= queue::MAX_ATTEMPTS {
                warn!(
                    "Outbound email {} exceeded max attempts, giving up: {e:?}",
                    entry.id
                );
                if let Err(ack_err) = queue::ack(database.get_pool(), &entry.id).await {
                    error!(
                        "Failed to remove abandoned entry {}: {ack_err:?}",
                        entry.id
                    );
                }
            } else {
                warn!(
                    "Outbound email {} delivery attempt {} failed, will retry: {e:?}",
                    entry.id, entry.attempts
                );
                if let Err(nack_err) =
                    queue::nack(database.get_pool(), &entry.id, &format!("{e:?}")).await
                {
                    error!("Failed to nack entry {}: {nack_err:?}", entry.id);
                }
            }
        }
    }
}
