use crate::servers::sending::send_email_job;
use erooster_core::{
    backend::{database::DB, storage::Storage},
    config::Config,
};
use futures::{Sink, SinkExt};
use std::sync::Arc;
use tracing::instrument;
use yaque::Receiver;

use self::sending::EmailPayload;

pub(crate) mod encrypted;
pub(crate) mod sending;
pub(crate) mod state;

// TODO make this only pub for benches and tests
#[allow(missing_docs)]
pub mod unencrypted;

pub(crate) async fn send_capabilities<S, E>(
    config: Arc<Config>,
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
    config: Arc<Config>,
    database: &DB,
    storage: &Storage,
) -> color_eyre::eyre::Result<()> {
    let config_clone = Arc::clone(&config);
    let db_clone = database.clone();
    let storage_clone = storage.clone();
    tokio::spawn(async move {
        if let Err(e) =
            unencrypted::Unencrypted::run(Arc::clone(&config_clone), &db_clone, &storage_clone)
                .await
        {
            panic!("Unable to start server: {e:?}");
        }
    });
    let db_clone = database.clone();
    let storage_clone = storage.clone();
    let config_clone = Arc::clone(&config);
    tokio::spawn(async move {
        if let Err(e) =
            encrypted::Encrypted::run(Arc::clone(&config_clone), &db_clone, &storage_clone).await
        {
            panic!("Unable to start TLS server: {e:?}");
        }
    });

    // Start listening for tasks
    let mut receiver = Receiver::open(config.task_folder.clone())?;

    loop {
        let data = receiver.recv().await;

        match data {
            Ok(data) => {
                let email_bytes = &*data;
                let email_json = serde_json::from_slice::<EmailPayload>(email_bytes)?;

                if let Err(e) = send_email_job(data, email_json).await {
                    tracing::error!("Error while sending email: {:?}", e);
                }
            }
            Err(e) => {
                tracing::error!("Error while receiving data from receiver: {:?}", e);
            }
        }
    }
}
