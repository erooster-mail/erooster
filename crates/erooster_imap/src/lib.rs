// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! Erooster IMAP Mail Server
//!
//! Erooster is a rust native imap server build on modern solutions.
//! The goal being easy to setup, use and maintain for smaller mail servers
//! while being also fast and efficient.
//!
//! This crate is containing the imap logic of the erooster mail server.
//!
#![allow(
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    // This seems to be buggy with instrument macros
    clippy::panic_in_result_fn
)]

use crate::commands::capability::{get_capabilities, get_unencrypted_capabilities};
use erooster_core::{
    backend::{database::DB, storage::Storage},
    config::Config,
};
use {
    color_eyre,
    const_format::formatcp,
    tokio,
    tracing::{error, instrument},
};

pub(crate) mod commands;
pub(crate) mod servers;

/// A const variant of the Capabilities we welcome clients with
pub const CAPABILITY_HELLO: &str = formatcp!(
    "* OK [{}] IMAP4rev1/IMAP4rev2 Service Ready",
    get_capabilities()
);

/// A const variant of the Capabilities we welcome clients with
pub const CAPABILITY_UNENCRYPTED_HELLO: &str = formatcp!(
    "* OK [{}] IMAP4rev1/IMAP4rev2 Service Ready",
    get_unencrypted_capabilities()
);

/// An implementation of a imap server
#[allow(async_fn_in_trait)]
pub trait Server {
    /// Start the server
    async fn run(config: Config, database: &DB, storage: &Storage) -> color_eyre::eyre::Result<()>;
}

/// Starts the imap server
///
/// # Errors
///
/// Returns an error if the server startup fails
#[instrument(skip(config, database, storage))]
pub fn start(config: &Config, database: &DB, storage: &Storage) -> color_eyre::eyre::Result<()> {
    std::fs::create_dir_all(&config.mail.maildir_folders)?;

    let db_clone = database.clone();
    let storage_clone = storage.clone();
    let config_clone = config.clone();
    tokio::spawn(async move {
        if let Err(e) =
            servers::unencrypted::Unencrypted::run(config_clone, &db_clone, &storage_clone).await
        {
            error!("IMAP server error: {e:?}");
        }
    });
    let db_clone = database.clone();
    let config = config.clone();
    let storage_clone = storage.clone();
    tokio::spawn(async move {
        if let Err(e) = servers::encrypted::Encrypted::run(config, &db_clone, &storage_clone).await
        {
            error!("IMAP TLS server error: {e:?}");
        }
    });
    Ok(())
}
