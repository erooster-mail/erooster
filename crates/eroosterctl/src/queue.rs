// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! `eroosterctl queue` — outbound mail queue management subcommands.

use crate::output::{print_error, print_json, print_table, OutputFormat};
use clap::Subcommand;
use color_eyre::eyre::Result;
use erooster_core::{
    backend::{
        database::{get_database, Database},
        queue::{self, QueueEntry},
    },
    config::Config,
};
use serde::Serialize;

#[derive(Subcommand, Debug)]
pub enum QueueCommands {
    /// List queue entries
    #[command(alias = "ls")]
    List {
        /// Filter by status: pending, delivering, failed, abandoned, delivered
        #[arg(long)]
        status: Option<String>,
    },

    /// Show details for a queue entry
    Show {
        /// Queue entry ID
        id: String,
    },

    /// Force-retry a failed or abandoned entry
    Retry {
        /// Queue entry ID
        id: String,
    },

    /// Force-retry all failed entries
    Flush,

    /// Permanently abandon a queue entry
    Abandon {
        /// Queue entry ID
        id: String,
        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },
}

#[derive(Serialize)]
struct QueueRow {
    id: String,
    status: String,
    attempts: i32,
    from: String,
    to: String,
    last_error: Option<String>,
}

impl From<&QueueEntry> for QueueRow {
    fn from(e: &QueueEntry) -> Self {
        QueueRow {
            id: e.id.clone(),
            status: e.status.to_string(),
            attempts: e.attempts,
            from: e.from_addr.clone(),
            to: e.to_addrs.clone(),
            last_error: e.last_error.clone(),
        }
    }
}

pub async fn run(
    cmd: QueueCommands,
    config: &Config,
    format: OutputFormat,
    yes: bool,
    no_color: bool,
) -> Result<()> {
    match cmd {
        QueueCommands::List { status } => queue_list(status.as_deref(), config, format).await,
        QueueCommands::Show { id } => queue_show(&id, config, format).await,
        QueueCommands::Retry { id } => queue_retry(&id, config, no_color).await,
        QueueCommands::Flush => queue_flush(config, no_color).await,
        QueueCommands::Abandon { id, yes: local_yes } => {
            queue_abandon(&id, yes || local_yes, config, no_color).await
        }
    }
}

async fn queue_list(status: Option<&str>, config: &Config, format: OutputFormat) -> Result<()> {
    let db = get_database(config).await?;
    let entries = queue::list_all(db.get_pool(), status).await?;

    if format == OutputFormat::Json {
        let rows: Vec<QueueRow> = entries.iter().map(QueueRow::from).collect();
        print_json(&rows)?;
    } else {
        let rows = entries
            .iter()
            .map(|e| {
                vec![
                    e.id.clone(),
                    e.status.to_string(),
                    e.attempts.to_string(),
                    e.from_addr.clone(),
                    e.to_addrs.clone(),
                    e.last_error.clone().unwrap_or_default(),
                ]
            })
            .collect();
        print_table(&["ID", "STATUS", "ATTEMPTS", "FROM", "TO", "LAST ERROR"], rows);
    }
    Ok(())
}

async fn queue_show(id: &str, config: &Config, format: OutputFormat) -> Result<()> {
    let db = get_database(config).await?;
    let entries = queue::list_all(db.get_pool(), None).await?;
    let entry = entries.into_iter().find(|e| e.id == id);

    let Some(entry) = entry else {
        eprintln!("No queue entry with id '{id}'.");
        std::process::exit(1);
    };

    if format == OutputFormat::Json {
        #[derive(Serialize)]
        struct FullEntry {
            id: String,
            status: String,
            attempts: i32,
            from: String,
            to: String,
            last_error: Option<String>,
            delivery_log: Vec<erooster_core::backend::queue::DeliveryAttempt>,
        }
        print_json(&FullEntry {
            id: entry.id,
            status: entry.status.to_string(),
            attempts: entry.attempts,
            from: entry.from_addr,
            to: entry.to_addrs,
            last_error: entry.last_error,
            delivery_log: entry.delivery_log,
        })?;
    } else {
        println!("ID:           {}", entry.id);
        println!("Status:       {}", entry.status);
        println!("Attempts:     {}", entry.attempts);
        println!("From:         {}", entry.from_addr);
        println!("To:           {}", entry.to_addrs);
        if let Some(err) = &entry.last_error {
            println!("Last error:   {err}");
        }
        if !entry.delivery_log.is_empty() {
            println!("\nDelivery log:");
            for attempt in &entry.delivery_log {
                println!("  [{}] {}", attempt.at, attempt.error);
            }
        }
    }
    Ok(())
}

async fn queue_retry(id: &str, config: &Config, no_color: bool) -> Result<()> {
    let db = get_database(config).await?;
    if let Err(e) = queue::force_retry(db.get_pool(), id).await {
        print_error(no_color, &format!("Failed to retry '{id}': {e}"));
        std::process::exit(1);
    }
    println!("Entry '{id}' queued for immediate retry.");
    Ok(())
}

async fn queue_flush(config: &Config, no_color: bool) -> Result<()> {
    let db = get_database(config).await?;
    let entries = queue::list_all(db.get_pool(), Some("failed")).await?;
    let count = entries.len();
    let mut errors = 0usize;
    for entry in &entries {
        if let Err(e) = queue::force_retry(db.get_pool(), &entry.id).await {
            print_error(no_color, &format!("Failed to retry '{}': {e}", entry.id));
            errors += 1;
        }
    }
    println!("{} entries flushed, {errors} errors.", count - errors);
    if errors > 0 {
        std::process::exit(1);
    }
    Ok(())
}

async fn queue_abandon(id: &str, yes: bool, config: &Config, no_color: bool) -> Result<()> {
    let is_tty = std::io::IsTerminal::is_terminal(&std::io::stdin());

    if !yes {
        if !is_tty {
            print_error(
                no_color,
                "Confirmation required. Pass --yes to abandon non-interactively.",
            );
            std::process::exit(2);
        }
        let confirmed = dialoguer::Confirm::new()
            .with_prompt(format!("Abandon entry '{id}'? It will not be retried."))
            .default(false)
            .interact()?;
        if !confirmed {
            println!("Aborted.");
            return Ok(());
        }
    }

    let db = get_database(config).await?;
    queue::abandon(db.get_pool(), id).await?;
    println!("Entry '{id}' abandoned.");
    Ok(())
}
