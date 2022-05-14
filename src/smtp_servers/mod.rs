use crate::config::Config;
use crate::database::DB;
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::sync::Arc;

pub(crate) mod encrypted;
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
pub fn start(config: Arc<Config>, database: DB) -> color_eyre::eyre::Result<()> {
    let config_clone = Arc::clone(&config);
    let db_clone = Arc::clone(&database);
    tokio::spawn(async move {
        if let Err(e) =
            unencrypted::Unencrypted::run(Arc::clone(&config_clone), Arc::clone(&db_clone)).await
        {
            panic!("Unable to start server: {:?}", e);
        }
    });
    tokio::spawn(async move {
        if let Err(e) = encrypted::Encrypted::run(Arc::clone(&config), Arc::clone(&database)).await
        {
            panic!("Unable to start TLS server: {:?}", e);
        }
    });
    Ok(())
}
