// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use std::{sync::Arc, time::Duration};

use crate::servers::sending::send_email_job;
use erooster_core::{
    backend::{database::DB, storage::Storage},
    config::Config,
};
use erooster_deps::{
    color_eyre,
    futures::{Sink, SinkExt},
    serde_json,
    tokio::{self, sync::Mutex, time::timeout},
    tokio_util::sync::CancellationToken,
    tracing::{self, instrument},
    yaque::{Receiver, Sender},
};

use self::sending::EmailPayload;

pub(crate) mod encrypted;
pub(crate) mod sending;
pub(crate) mod state;

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
#[instrument(skip(config, database, storage, shutdown_flag, receiver))]
pub async fn start(
    config: Config,
    database: &DB,
    storage: &Storage,
    shutdown_flag: CancellationToken,
    receiver: Arc<Mutex<Receiver>>,
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
            panic!("Unable to start server: {e:?}");
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
            panic!("Unable to start TLS server: {e:?}");
        }
    });

    let shutdown_flag_clone = shutdown_flag.clone();
    tokio::spawn(async move {
        loop {
            if shutdown_flag_clone.is_cancelled() {
                break;
            }
            let mut receiver_lock = Arc::clone(&receiver).lock_owned().await;
            let data = timeout(Duration::from_secs(1), receiver_lock.recv()).await;

            if let Ok(data) = data {
                match data {
                    Ok(data) => {
                        let email_bytes = &*data;
                        let email_json = serde_json::from_slice::<EmailPayload>(email_bytes)
                            .expect("Unable to parse job json");

                        if let Err(e) = send_email_job(&email_json).await {
                            tracing::error!(
                                "Error while sending email: {:?}. Adding it to the queue again",
                                e
                            );
                            // FIXME: This can race the lock leading to an error. We should
                            //        probably handle this better.
                            let mut sender = Sender::open(config.task_folder.clone())
                                .expect("Unable to open sender");
                            let json_bytes = serde_json::to_vec(&email_json)
                                .expect("Unable to convert email job to json");
                            sender
                                .send(json_bytes)
                                .await
                                .expect("Unable to requeue emaul");
                        }
                        // Mark the job as complete
                        data.commit().expect("Unable to commit job");
                    }
                    Err(e) => {
                        tracing::error!("Error while receiving data from receiver: {:?}", e);
                    }
                };
            }
        }
    });
    Ok(())
}
