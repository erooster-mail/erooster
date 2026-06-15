// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! `eroosterctl mailbox` — mailbox listing subcommand.

use crate::output::{print_json, print_table, OutputFormat};
use clap::Subcommand;
use color_eyre::eyre::Result;
use erooster_core::{
    backend::{admin::list_mailboxes_for_user, database::{get_database, Database}},
    config::Config,
};
use serde::Serialize;

#[derive(Subcommand, Debug)]
pub enum MailboxCommands {
    /// List mailboxes for a user
    #[command(alias = "ls")]
    List {
        /// Email address of the user
        email: String,
    },
}

#[derive(Serialize)]
struct MailboxRow {
    mailbox: String,
    messages: i64,
}

pub async fn run(cmd: MailboxCommands, config: &Config, format: OutputFormat) -> Result<()> {
    match cmd {
        MailboxCommands::List { email } => list(&email, config, format).await,
    }
}

async fn list(email: &str, config: &Config, format: OutputFormat) -> Result<()> {
    let db = get_database(config).await?;
    let mailboxes = list_mailboxes_for_user(db.get_pool(), email).await?;

    if format == OutputFormat::Json {
        let rows: Vec<MailboxRow> = mailboxes
            .into_iter()
            .map(|m| MailboxRow {
                mailbox: m.name,
                messages: m.message_count,
            })
            .collect();
        print_json(&rows)?;
    } else {
        let rows = mailboxes
            .into_iter()
            .map(|m| vec![m.name, m.message_count.to_string()])
            .collect();
        print_table(&["MAILBOX", "MESSAGES"], rows);
    }
    Ok(())
}
