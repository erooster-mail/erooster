use crate::servers::sending::send_email_job;
use erooster_core::{
    backend::{
        database::{Database, DB},
        storage::Storage,
    },
    config::Config,
};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use sqlxmq::{JobRegistry, OwnedHandle};
use std::error::Error;
use std::sync::Arc;
use tracing::instrument;

pub(crate) mod encrypted;
pub(crate) mod sending;
pub(crate) mod state;

// TODO make this only pub for benches and tests
#[allow(missing_docs)]
pub mod unencrypted;

pub(crate) async fn send_capabilities<S>(
    config: Arc<Config>,
    lines_sender: &mut S,
) -> color_eyre::eyre::Result<()>
where
    S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
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
    database: DB,
    storage: Arc<Storage>,
) -> color_eyre::eyre::Result<OwnedHandle> {
    let config_clone = Arc::clone(&config);
    let db_clone = Arc::clone(&database);
    let storage_clone = Arc::clone(&storage);
    tokio::spawn(async move {
        if let Err(e) = unencrypted::Unencrypted::run(
            Arc::clone(&config_clone),
            Arc::clone(&db_clone),
            Arc::clone(&storage_clone),
        )
        .await
        {
            panic!("Unable to start server: {:?}", e);
        }
    });
    let db_clone = Arc::clone(&database);
    tokio::spawn(async move {
        if let Err(e) = encrypted::Encrypted::run(
            Arc::clone(&config),
            Arc::clone(&db_clone),
            Arc::clone(&storage),
        )
        .await
        {
            panic!("Unable to start TLS server: {:?}", e);
        }
    });

    let pool = database.get_pool();

    // Construct a job registry from our job.
    let mut registry = JobRegistry::new(&[send_email_job]);
    // Here is where you can configure the registry
    registry.set_error_handler(|name: &str, error: Box<dyn Error + Send + 'static>| {
        tracing::error!("Job `{}` failed: {}", name, error);
    });

    // And add context
    registry.set_context("");

    let runner = registry
        // Create a job runner using the connection pool.
        .runner(pool)
        // Here is where you can configure the job runner
        // Aim to keep 10-20 jobs running at a time.
        .set_concurrency(10, 20)
        // Start the job runner in the background.
        .run()
        .await?;
    Ok(runner)
}
