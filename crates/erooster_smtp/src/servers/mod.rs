// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::servers::sending::send_email_job;
use erooster_core::{
    backend::{database::DB, storage::Storage},
    config::Config,
};
use erooster_deps::{
    color_eyre,
    futures::{Sink, SinkExt},
    serde_json, tokio,
    tracing::{self, error, info, instrument, warn},
    yaque::{recovery::recover, ReceiverBuilder, Sender},
};

use self::sending::EmailPayload;

pub(crate) mod encrypted;
pub(crate) mod sending;
pub(crate) mod state;

// TODO make this only pub for benches and tests
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
#[instrument(skip(config, database, storage))]
pub async fn start(
    config: Config,
    database: &DB,
    storage: &Storage,
) -> color_eyre::eyre::Result<()> {
    let db_clone = database.clone();
    let storage_clone = storage.clone();
    let config_clone = config.clone();
    tokio::spawn(async move {
        if let Err(e) = unencrypted::Unencrypted::run(config_clone, &db_clone, &storage_clone).await
        {
            panic!("Unable to start server: {e:?}");
        }
    });
    let db_clone = database.clone();
    let storage_clone = storage.clone();
    let config_clone = config.clone();
    tokio::spawn(async move {
        if let Err(e) = encrypted::Encrypted::run(config_clone, &db_clone, &storage_clone).await {
            panic!("Unable to start TLS server: {e:?}");
        }
    });

    // Start listening for tasks
    let mut receiver = ReceiverBuilder::default()
        .save_every_nth(None)
        .open(config.task_folder.clone());
    if let Err(e) = receiver {
        warn!("Unable to open receiver: {:?}. Trying to recover.", e);
        recover(&config.task_folder)?;
        receiver = ReceiverBuilder::default()
            .save_every_nth(None)
            .open(config.task_folder.clone());
        info!("Recovered queue successfully");
    }

    match receiver {
        Ok(mut receiver) => loop {
            let data = receiver.recv().await;

            match data {
                Ok(data) => {
                    let email_bytes = &*data;
                    let email_json = serde_json::from_slice::<EmailPayload>(email_bytes)?;

                    if let Err(e) = send_email_job(&email_json).await {
                        tracing::error!(
                            "Error while sending email: {:?}. Adding it to the queue again",
                            e
                        );
                        // FIXME: This can race the lock leading to an error. We should
                        //        probably handle this better.
                        let mut sender = Sender::open(config.task_folder.clone())?;
                        let json_bytes = serde_json::to_vec(&email_json)?;
                        sender.send(json_bytes).await?;
                    }
                    // Mark the job as complete
                    data.commit()?;
                }
                Err(e) => {
                    tracing::error!("Error while receiving data from receiver: {:?}", e);
                }
            }
        },
        Err(e) => {
            error!("Unable to open receiver: {:?}. Giving up.", e);
            Ok(())
        }
    }
}
