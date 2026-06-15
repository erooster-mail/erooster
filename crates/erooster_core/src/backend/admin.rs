// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! Admin-facing queries that are too specific for the generic `Database` trait.
//! These run raw SQL against the pool and are only used by `eroosterctl`.

use color_eyre::eyre::Result;
use sqlx::Row;

#[cfg(feature = "postgres")]
use sqlx::PgPool as Pool;
#[cfg(feature = "sqlite")]
use sqlx::SqlitePool as Pool;

/// Summary of a single mailbox folder.
#[derive(Debug, Clone)]
pub struct MailboxSummary {
    /// Folder name as stored in the DB (e.g. `alice@example.com/INBOX`).
    pub name: String,
    /// Number of messages in the folder.
    pub message_count: i64,
}

/// Returns all mailboxes belonging to `username` with their message counts.
pub async fn list_mailboxes_for_user(
    pool: &Pool,
    username: &str,
) -> Result<Vec<MailboxSummary>> {
    let prefix = format!("{username}/%");
    let rows = sqlx::query(
        "SELECT mailbox, COUNT(*) AS cnt FROM mails WHERE mailbox LIKE $1 GROUP BY mailbox ORDER BY mailbox",
    )
    .bind(&prefix)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| MailboxSummary {
            name: r.get::<String, _>("mailbox"),
            message_count: r.get::<i64, _>("cnt"),
        })
        .collect())
}

/// Counts per-status totals for the outbound queue.
#[derive(Debug, Clone, Default)]
pub struct QueueStats {
    /// Entries waiting for the next delivery attempt.
    pub pending: i64,
    /// Entries currently being processed by the delivery worker.
    pub delivering: i64,
    /// Entries whose last attempt failed; will be retried.
    pub failed: i64,
    /// Entries that exceeded the maximum retry count and will not be retried.
    pub abandoned: i64,
    /// Entries delivered directly to local maildir (no outbound relay needed).
    pub delivered: i64,
}

/// Returns counts of queue entries grouped by status.
pub async fn queue_stats(pool: &Pool) -> Result<QueueStats> {
    let rows = sqlx::query("SELECT status, COUNT(*) AS cnt FROM outbound_queue GROUP BY status")
        .fetch_all(pool)
        .await?;

    let mut stats = QueueStats::default();
    for row in rows {
        let status_str: String = row.get("status");
        let cnt: i64 = row.get("cnt");
        match status_str.as_str() {
            "pending" => stats.pending = cnt,
            "delivering" => stats.delivering = cnt,
            "failed" => stats.failed = cnt,
            "abandoned" => stats.abandoned = cnt,
            "delivered" => stats.delivered = cnt,
            _ => {}
        }
    }
    Ok(stats)
}

/// Total number of users.
pub async fn user_count(pool: &Pool) -> Result<i64> {
    let row = sqlx::query("SELECT COUNT(*) AS cnt FROM users")
        .fetch_one(pool)
        .await?;
    Ok(row.get("cnt"))
}

/// Number of distinct mailbox folders across all users.
pub async fn mailbox_count(pool: &Pool) -> Result<i64> {
    let row = sqlx::query("SELECT COUNT(DISTINCT mailbox) AS cnt FROM mails")
        .fetch_one(pool)
        .await?;
    Ok(row.get("cnt"))
}
