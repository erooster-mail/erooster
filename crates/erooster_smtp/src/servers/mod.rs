// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use erooster_core::{
    backend::{database::DB, storage::Storage},
    config::Config,
};
use {
    color_eyre,
    futures::{Sink, SinkExt},
    tokio,
    tokio_util::sync::CancellationToken,
    tracing::{self, instrument},
};

pub(crate) mod dane;
pub(crate) mod encrypted;
pub(crate) mod mta_sts;
pub(crate) mod sending;
pub(crate) mod state;
pub(crate) mod worker;

// TODO: make this only pub for benches and tests
#[allow(missing_docs)]
pub mod unencrypted;

pub(crate) async fn send_capabilities<S, E>(
    config: &Config,
    lines_sender: &mut S,
) -> color_eyre::eyre::Result<()>
where
    E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
    S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
{
    lines_sender
        .send(format!("220 {} ESMTP Erooster", config.mail.hostname))
        .await?;
    Ok(())
}

/// Starts the smtp server
///
/// # Errors
///
/// Returns an error if the server startup fails
#[instrument(skip(config, database, storage, shutdown_flag))]
pub async fn start(
    config: Config,
    database: &DB,
    storage: &Storage,
    shutdown_flag: CancellationToken,
) -> color_eyre::eyre::Result<()> {
    let db_clone = database.clone();
    let storage_clone = storage.clone();
    let config_clone = config.clone();
    let shutdown_flag_clone = shutdown_flag.clone();
    tokio::spawn(async move {
        if let Err(e) = unencrypted::Unencrypted::run(
            config_clone,
            &db_clone,
            &storage_clone,
            shutdown_flag_clone,
        )
        .await
        {
            tracing::error!("Unable to start SMTP server: {e:?}");
        }
    });
    let db_clone = database.clone();
    let storage_clone = storage.clone();
    let config_clone = config.clone();
    let shutdown_flag_clone = shutdown_flag.clone();
    tokio::spawn(async move {
        if let Err(e) =
            encrypted::Encrypted::run(config_clone, &db_clone, &storage_clone, shutdown_flag_clone)
                .await
        {
            tracing::error!("Unable to start TLS SMTP server: {e:?}");
        }
    });

    let db_clone = database.clone();
    let shutdown_flag_clone = shutdown_flag.clone();
    tokio::spawn(worker::run(db_clone, shutdown_flag_clone));

    Ok(())
}
