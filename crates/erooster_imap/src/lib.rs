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
#![feature(string_remove_matches)]
#![feature(box_into_inner)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
#![deny(unsafe_code, clippy::unwrap_used)]
#![warn(
    clippy::cognitive_complexity,
    clippy::branches_sharing_code,
    clippy::imprecise_flops,
    clippy::missing_const_for_fn,
    clippy::mutex_integer,
    clippy::path_buf_push_overwrite,
    clippy::redundant_pub_crate,
    clippy::pedantic,
    clippy::dbg_macro,
    clippy::todo,
    clippy::fallible_impl_from,
    clippy::filetype_is_file,
    clippy::suboptimal_flops,
    clippy::fn_to_numeric_cast_any,
    clippy::if_then_some_else_none,
    clippy::imprecise_flops,
    clippy::lossy_float_literal,
    clippy::panic_in_result_fn,
    clippy::clone_on_ref_ptr
)]
#![warn(missing_docs)]
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
use erooster_deps::{
    async_trait::async_trait,
    color_eyre,
    const_format::formatcp,
    tokio,
    tracing::{self, instrument},
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
#[async_trait]
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
pub fn start(config: Config, database: &DB, storage: &Storage) -> color_eyre::eyre::Result<()> {
    std::fs::create_dir_all(&config.mail.maildir_folders)?;

    let db_clone = database.clone();
    let storage_clone = storage.clone();
    let config_clone = config.clone();
    tokio::spawn(async move {
        if let Err(e) =
            servers::unencrypted::Unencrypted::run(config_clone, &db_clone, &storage_clone).await
        {
            panic!("Unable to start server: {e:?}");
        }
    });
    let db_clone = database.clone();
    let config = config.clone();
    let storage_clone = storage.clone();
    tokio::spawn(async move {
        if let Err(e) = servers::encrypted::Encrypted::run(config, &db_clone, &storage_clone).await
        {
            panic!("Unable to start TLS server: {e:?}");
        }
    });
    Ok(())
}
