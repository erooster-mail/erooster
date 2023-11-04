// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use std::{process::exit, sync::Arc, time::Duration};

use crate::servers::sending::send_email_job;
use erooster_core::{
    backend::{database::DB, storage::Storage},
    config::Config,
};
use erooster_deps::{
    color_eyre,
    futures::{Sink, SinkExt},
    serde_json,
    tokio::{
        self,
        signal::unix::{signal, SignalKind},
        sync::Mutex,
        time::timeout,
    },
    tokio_util::sync::CancellationToken,
    tracing::{self, error, info, instrument, warn},
    yaque::{
        recovery::{recover, unlock_queue},
        Receiver, ReceiverBuilder, Sender,
    },
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
#[instrument(skip(config, database, storage))]
pub async fn start(
    config: Config,
    database: &DB,
    storage: &Storage,
) -> color_eyre::eyre::Result<()> {
    let shutdown_flag = CancellationToken::new();
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

    // Get SIGTERMs
    let mut sigterms = signal(SignalKind::terminate())?;

    match receiver {
        Ok(receiver) => {
            let receiver = Arc::new(Mutex::const_new(receiver));
            let receiver_clone = Arc::clone(&receiver);

            let config_clone = config.clone();
            tokio::spawn(async move {
                loop {
                    let mut receiver_lock = Arc::clone(&receiver).lock_owned().await;
                    let data = timeout(Duration::from_secs(1), receiver_lock.recv()).await;

                    if let Ok(data) = data {
                        match data {
                            Ok(data) => {
                                let email_bytes = &*data;
                                let email_json =
                                    serde_json::from_slice::<EmailPayload>(email_bytes)
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
                                tracing::error!(
                                    "Error while receiving data from receiver: {:?}",
                                    e
                                );
                            }
                        };
                    }
                }
            });

            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    cleanup(&receiver_clone, &shutdown_flag, &config_clone).await;
                }
                _ = sigterms.recv() => {
                    cleanup(&receiver_clone, &shutdown_flag, &config_clone).await;
                }
            }
            Ok(())
        }
        Err(e) => {
            error!("Unable to open receiver: {:?}. Giving up.", e);
            Ok(())
        }
    }
}

async fn cleanup(
    receiver: &Arc<Mutex<Receiver>>,
    shutdown_flag: &CancellationToken,
    config: &Config,
) {
    info!("Received ctr-c. Cleaning up");
    let mut lock = receiver.lock().await;

    info!("Gained lock. Saving queue");
    lock.save().expect("Unable to save queue");
    info!("Saved queue. Exiting");
    shutdown_flag.cancel();
    unlock_queue(&config.task_folder).expect("Failed to unlock queue");
    exit(0);
}
