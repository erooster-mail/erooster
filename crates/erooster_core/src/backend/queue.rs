// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! Persistent outbound mail queue backed by the SQLx database.
//!
//! ## Design goals
//!
//! - **Durability**: emails survive server restarts (unlike `tokio::spawn`).
//! - **Observability**: every entry has a `status` column and a `delivery_log`
//!   that records every attempt with a timestamp and error, so postmasters can
//!   see exactly what happened.
//! - **Manual control**: `force_retry` resets an entry so the worker picks it
//!   up again immediately; `abandon` marks an entry as permanently dead.
//! - **Retry schedule** (loosely following RFC 5321 §4.5.4):
//!   attempt 0 → immediate, 1 → 5 min, 2 → 15 min, 3 → 1 h,
//!   4–8 → 4 h each, ≥ MAX_ATTEMPTS → abandoned automatically.

use {color_eyre::eyre::Result, serde_json};

/// Maximum number of delivery attempts before an entry is abandoned.
pub const MAX_ATTEMPTS: i32 = 10;

/// Observable status of a queue entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueStatus {
    /// Waiting for the next delivery attempt.
    Pending,
    /// Currently being delivered by the worker.
    Delivering,
    /// Last delivery attempt failed; will be retried.
    Failed,
    /// Exceeded max attempts; no further delivery will be tried.
    Abandoned,
    /// Delivered directly to local maildir (no outbound relay needed).
    Delivered,
}

impl QueueStatus {
    /// Returns the status as a lowercase string for database storage.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Delivering => "delivering",
            Self::Failed => "failed",
            Self::Abandoned => "abandoned",
            Self::Delivered => "delivered",
        }
    }
}

impl std::fmt::Display for QueueStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for QueueStatus {
    fn from(s: &str) -> Self {
        match s {
            "delivering" => Self::Delivering,
            "failed" => Self::Failed,
            "abandoned" => Self::Abandoned,
            "delivered" => Self::Delivered,
            _ => Self::Pending,
        }
    }
}

/// One entry in the per-attempt delivery history.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "serde")]
pub struct DeliveryAttempt {
    /// RFC 3339-style timestamp of this attempt.
    pub at: String,
    /// Error message from this attempt.
    pub error: String,
}

/// A single row from the outbound queue.
#[derive(Debug)]
pub struct QueueEntry {
    /// UUID as a string (both postgres UUID and sqlite TEXT).
    pub id: String,
    /// Serialised JSON payload (opaque to the queue layer).
    pub payload: String,
    /// Current delivery status.
    pub status: QueueStatus,
    /// How many delivery attempts have been made so far.
    pub attempts: i32,
    /// Most recent delivery error, if any.
    pub last_error: Option<String>,
    /// Full per-attempt history (oldest first).
    pub delivery_log: Vec<DeliveryAttempt>,
    /// Sender address (for postmaster inspection).
    pub from_addr: String,
    /// Comma-separated recipient addresses.
    pub to_addrs: String,
}

/// Seconds to wait before the next retry for a given attempt number.
#[must_use]
pub const fn retry_delay_secs(attempts: i32) -> i64 {
    match attempts {
        0 => 0,
        1 => 5 * 60,
        2 => 15 * 60,
        3 => 60 * 60,
        _ => 4 * 60 * 60,
    }
}

/// Current UTC time formatted as ISO 8601 without pulling in chrono.
pub(crate) fn now_utc_iso8601() -> String {
    let total_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let sec = total_secs % 60;
    let minute = (total_secs / 60) % 60;
    let hour = (total_secs / 3600) % 24;
    let days = total_secs / 86400;
    // Julian Day Number → Gregorian calendar (algorithm from Meeus "Astronomical Algorithms")
    let jdn = days + 2_440_588;
    let jdn_a = jdn + 32_044;
    let jdn_b = (4 * jdn_a + 3) / 146_097;
    let jdn_c = jdn_a - (146_097 * jdn_b) / 4;
    let jdn_d = (4 * jdn_c + 3) / 1_461;
    let jdn_e = jdn_c - (1_461 * jdn_d) / 4;
    let jdn_m = (5 * jdn_e + 2) / 153;
    let day = jdn_e - (153 * jdn_m + 2) / 5 + 1;
    let month = jdn_m + 3 - 12 * (jdn_m / 10);
    let year = 100 * jdn_b + jdn_d - 4_800 + jdn_m / 10;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{sec:02}Z")
}

fn parse_delivery_log(raw: &str) -> Vec<DeliveryAttempt> {
    serde_json::from_str(raw).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Postgres implementation
// ---------------------------------------------------------------------------

/// Postgres-backed queue operations.
#[cfg(feature = "postgres")]
pub mod postgres {
    use super::{
        now_utc_iso8601, parse_delivery_log, retry_delay_secs, DeliveryAttempt, QueueEntry,
        QueueStatus, Result, MAX_ATTEMPTS,
    };
    use sqlx::PgPool;
    use {serde_json, tracing::instrument, uuid::Uuid};

    type EntryRow = (
        String,
        String,
        String,
        i32,
        Option<String>,
        String,
        String,
        String,
    );

    /// Insert a new entry into the queue.
    #[instrument(skip(pool, payload_json))]
    pub async fn push(
        pool: &PgPool,
        id: Uuid,
        payload_json: String,
        from_addr: &str,
        to_addrs: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO outbound_queue (id, payload, from_addr, to_addrs) \
             VALUES ($1, $2::jsonb, $3, $4)",
        )
        .bind(id)
        .bind(payload_json)
        .bind(from_addr)
        .bind(to_addrs)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Record a locally-delivered message in the queue for audit purposes.
    /// The entry is inserted with status `delivered` and will never be picked up by the worker.
    #[instrument(skip(pool, payload_json))]
    pub async fn push_local(
        pool: &PgPool,
        id: Uuid,
        payload_json: String,
        from_addr: &str,
        to_addrs: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO outbound_queue \
             (id, payload, status, attempts, next_retry_at, from_addr, to_addrs) \
             VALUES ($1, $2::jsonb, 'delivered', 0, '9999-12-31T00:00:00Z'::timestamptz, $3, $4)",
        )
        .bind(id)
        .bind(payload_json)
        .bind(from_addr)
        .bind(to_addrs)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Pop the next ready entry and mark it as `delivering`.
    /// Returns `None` when the queue is empty or no entry is due yet.
    #[instrument(skip(pool))]
    pub async fn pop(pool: &PgPool) -> Result<Option<QueueEntry>> {
        let row: Option<EntryRow> = sqlx::query_as(
            "UPDATE outbound_queue \
             SET status = 'delivering', updated_at = NOW() \
             WHERE id = ( \
                 SELECT id FROM outbound_queue \
                 WHERE status IN ('pending', 'failed') \
                   AND next_retry_at <= NOW() \
                   AND attempts < $1 \
                 ORDER BY next_retry_at \
                 LIMIT 1 \
                 FOR UPDATE SKIP LOCKED \
             ) \
             RETURNING id::text, payload::text, status, attempts, last_error, \
                       delivery_log::text, from_addr, to_addrs",
        )
        .bind(MAX_ATTEMPTS)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(
            |(id, payload, status, attempts, last_error, delivery_log_raw, from_addr, to_addrs)| {
                QueueEntry {
                    id,
                    payload,
                    status: QueueStatus::from(status.as_str()),
                    attempts,
                    last_error,
                    delivery_log: parse_delivery_log(&delivery_log_raw),
                    from_addr,
                    to_addrs,
                }
            },
        ))
    }

    /// Acknowledge successful delivery — removes the entry.
    #[instrument(skip(pool))]
    pub async fn ack(pool: &PgPool, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM outbound_queue WHERE id = $1::uuid")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Record a delivery failure, append to the delivery log, and schedule retry.
    #[instrument(skip(pool, error))]
    pub async fn nack(pool: &PgPool, id: &str, error: &str) -> Result<()> {
        let (old_attempts, log_raw): (i32, String) = sqlx::query_as(
            "SELECT attempts, delivery_log::text FROM outbound_queue WHERE id = $1::uuid",
        )
        .bind(id)
        .fetch_one(pool)
        .await?;

        let new_attempts = old_attempts + 1;
        let new_status = if new_attempts >= MAX_ATTEMPTS {
            "abandoned"
        } else {
            "failed"
        };
        let delay = retry_delay_secs(old_attempts);

        let mut log = parse_delivery_log(&log_raw);
        log.push(DeliveryAttempt {
            at: now_utc_iso8601(),
            error: error.to_string(),
        });
        let new_log = serde_json::to_string(&log).unwrap_or_else(|_| "[]".to_string());

        sqlx::query(
            "UPDATE outbound_queue \
             SET attempts = $1, \
                 status = $2::outbound_queue_status, \
                 last_error = $3, \
                 delivery_log = $4::jsonb, \
                 next_retry_at = NOW() + ($5 || ' seconds')::interval, \
                 updated_at = NOW() \
             WHERE id = $6::uuid",
        )
        .bind(new_attempts)
        .bind(new_status)
        .bind(error)
        .bind(new_log)
        .bind(delay.to_string())
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Reset a failed/abandoned entry for immediate redelivery (postmaster manual retry).
    #[instrument(skip(pool))]
    pub async fn force_retry(pool: &PgPool, id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE outbound_queue \
             SET status = 'pending', \
                 attempts = 0, \
                 last_error = NULL, \
                 next_retry_at = NOW(), \
                 updated_at = NOW() \
             WHERE id = $1::uuid",
        )
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Permanently abandon an entry without deleting it (preserves audit trail).
    #[instrument(skip(pool))]
    pub async fn abandon(pool: &PgPool, id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE outbound_queue \
             SET status = 'abandoned', updated_at = NOW() \
             WHERE id = $1::uuid",
        )
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// List all entries, optionally filtered by status string (e.g. `"failed"`).
    #[instrument(skip(pool))]
    pub async fn list_all(pool: &PgPool, status: Option<&str>) -> Result<Vec<QueueEntry>> {
        let rows: Vec<EntryRow> = if let Some(s) = status {
            sqlx::query_as(
                "SELECT id::text, payload::text, status, attempts, last_error, \
                         delivery_log::text, from_addr, to_addrs \
                 FROM outbound_queue WHERE status = $1::outbound_queue_status \
                 ORDER BY created_at",
            )
            .bind(s)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as(
                "SELECT id::text, payload::text, status, attempts, last_error, \
                         delivery_log::text, from_addr, to_addrs \
                 FROM outbound_queue ORDER BY created_at",
            )
            .fetch_all(pool)
            .await?
        };

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    payload,
                    status,
                    attempts,
                    last_error,
                    delivery_log_raw,
                    from_addr,
                    to_addrs,
                )| {
                    QueueEntry {
                        id,
                        payload,
                        status: QueueStatus::from(status.as_str()),
                        attempts,
                        last_error,
                        delivery_log: parse_delivery_log(&delivery_log_raw),
                        from_addr,
                        to_addrs,
                    }
                },
            )
            .collect())
    }
}

// ---------------------------------------------------------------------------
// SQLite implementation
// ---------------------------------------------------------------------------

/// SQLite-backed queue operations.
#[cfg(feature = "sqlite")]
pub mod sqlite {
    use super::{
        now_utc_iso8601, parse_delivery_log, retry_delay_secs, DeliveryAttempt, QueueEntry,
        QueueStatus, Result, MAX_ATTEMPTS,
    };
    use sqlx::SqlitePool;
    use {serde_json, tracing::instrument, uuid::Uuid};

    type EntryRow = (
        String,
        String,
        String,
        i32,
        Option<String>,
        String,
        String,
        String,
    );

    /// Insert a new entry into the queue.
    #[instrument(skip(pool, payload_json))]
    pub async fn push(
        pool: &SqlitePool,
        id: Uuid,
        payload_json: String,
        from_addr: &str,
        to_addrs: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO outbound_queue (id, payload, from_addr, to_addrs) VALUES ($1, $2, $3, $4)",
        )
        .bind(id.to_string())
        .bind(payload_json)
        .bind(from_addr)
        .bind(to_addrs)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Record a locally-delivered message in the queue for audit purposes.
    /// The entry is inserted with status `delivered` and will never be picked up by the worker.
    #[instrument(skip(pool, payload_json))]
    pub async fn push_local(
        pool: &SqlitePool,
        id: Uuid,
        payload_json: String,
        from_addr: &str,
        to_addrs: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO outbound_queue \
             (id, payload, status, attempts, next_retry_at, from_addr, to_addrs) \
             VALUES ($1, $2, 'delivered', 0, '9999-12-31T00:00:00', $3, $4)",
        )
        .bind(id.to_string())
        .bind(payload_json)
        .bind(from_addr)
        .bind(to_addrs)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Pop the next ready entry and mark it as `delivering`.
    #[instrument(skip(pool))]
    pub async fn pop(pool: &SqlitePool) -> Result<Option<QueueEntry>> {
        let row: Option<EntryRow> = sqlx::query_as(
            "SELECT id, payload, status, attempts, last_error, delivery_log, from_addr, to_addrs \
                 FROM outbound_queue \
                 WHERE status IN ('pending', 'failed') \
                   AND next_retry_at <= datetime('now') \
                   AND attempts < $1 \
                 ORDER BY next_retry_at \
                 LIMIT 1",
        )
        .bind(MAX_ATTEMPTS)
        .fetch_optional(pool)
        .await?;

        if let Some((
            id,
            payload,
            status,
            attempts,
            last_error,
            delivery_log_raw,
            from_addr,
            to_addrs,
        )) = row
        {
            sqlx::query(
                "UPDATE outbound_queue SET status = 'delivering', updated_at = datetime('now') WHERE id = $1",
            )
            .bind(&id)
            .execute(pool)
            .await?;
            Ok(Some(QueueEntry {
                id,
                payload,
                status: QueueStatus::from(status.as_str()),
                attempts,
                last_error,
                delivery_log: parse_delivery_log(&delivery_log_raw),
                from_addr,
                to_addrs,
            }))
        } else {
            Ok(None)
        }
    }

    /// Acknowledge successful delivery — removes the entry.
    #[instrument(skip(pool))]
    pub async fn ack(pool: &SqlitePool, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM outbound_queue WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Record a delivery failure, append to the delivery log, and schedule retry.
    #[instrument(skip(pool, error))]
    pub async fn nack(pool: &SqlitePool, id: &str, error: &str) -> Result<()> {
        let current: Option<(i32, String)> =
            sqlx::query_as("SELECT attempts, delivery_log FROM outbound_queue WHERE id = $1")
                .bind(id)
                .fetch_optional(pool)
                .await?;
        let (old_attempts, log_raw) = current.unwrap_or((0, "[]".to_string()));
        let new_attempts = old_attempts + 1;
        let new_status = if new_attempts >= MAX_ATTEMPTS {
            "abandoned"
        } else {
            "failed"
        };
        let delay = retry_delay_secs(old_attempts);

        let mut log = parse_delivery_log(&log_raw);
        log.push(DeliveryAttempt {
            at: now_utc_iso8601(),
            error: error.to_string(),
        });
        let new_log = serde_json::to_string(&log).unwrap_or_else(|_| "[]".to_string());

        sqlx::query(
            "UPDATE outbound_queue \
             SET attempts = $1, status = $2, last_error = $3, delivery_log = $4, \
                 next_retry_at = datetime('now', $5), updated_at = datetime('now') \
             WHERE id = $6",
        )
        .bind(new_attempts)
        .bind(new_status)
        .bind(error)
        .bind(new_log)
        .bind(format!("+{delay} seconds"))
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Reset a failed/abandoned entry for immediate redelivery.
    #[instrument(skip(pool))]
    pub async fn force_retry(pool: &SqlitePool, id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE outbound_queue \
             SET status = 'pending', attempts = 0, last_error = NULL, \
                 next_retry_at = datetime('now'), updated_at = datetime('now') \
             WHERE id = $1",
        )
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Permanently abandon an entry without deleting it.
    #[instrument(skip(pool))]
    pub async fn abandon(pool: &SqlitePool, id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE outbound_queue SET status = 'abandoned', updated_at = datetime('now') WHERE id = $1",
        )
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// List all entries, optionally filtered by status string.
    #[instrument(skip(pool))]
    pub async fn list_all(pool: &SqlitePool, status: Option<&str>) -> Result<Vec<QueueEntry>> {
        let rows: Vec<EntryRow> = if let Some(s) = status {
            sqlx::query_as(
                    "SELECT id, payload, status, attempts, last_error, delivery_log, from_addr, to_addrs \
                     FROM outbound_queue WHERE status = $1 ORDER BY created_at",
                )
                .bind(s)
                .fetch_all(pool)
                .await?
        } else {
            sqlx::query_as(
                    "SELECT id, payload, status, attempts, last_error, delivery_log, from_addr, to_addrs \
                     FROM outbound_queue ORDER BY created_at",
                )
                .fetch_all(pool)
                .await?
        };

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    payload,
                    status,
                    attempts,
                    last_error,
                    delivery_log_raw,
                    from_addr,
                    to_addrs,
                )| {
                    QueueEntry {
                        id,
                        payload,
                        status: QueueStatus::from(status.as_str()),
                        attempts,
                        last_error,
                        delivery_log: parse_delivery_log(&delivery_log_raw),
                        from_addr,
                        to_addrs,
                    }
                },
            )
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Feature-gated convenience re-exports
// ---------------------------------------------------------------------------

#[cfg(feature = "postgres")]
pub use postgres::{abandon, ack, force_retry, list_all, nack, pop, push, push_local};

#[cfg(all(feature = "sqlite", not(feature = "postgres")))]
pub use sqlite::{abandon, ack, force_retry, list_all, nack, pop, push, push_local};
