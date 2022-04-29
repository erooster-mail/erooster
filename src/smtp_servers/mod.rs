use std::sync::Arc;

use crate::config::Config;

pub(crate) mod encrypted;
pub(crate) mod state;
pub(crate) mod unencrypted;

/// Starts the smtp server
///
/// # Errors
///
/// Returns an error if the server startup fails
pub fn start(config: Arc<Config>) -> color_eyre::eyre::Result<()> {
    let config_clone = Arc::clone(&config);
    tokio::spawn(async move {
        if let Err(e) = unencrypted::Unencrypted::run(Arc::clone(&config_clone)).await {
            panic!("Unable to start server: {:?}", e);
        }
    });
    tokio::spawn(async move {
        if let Err(e) = encrypted::Encrypted::run(Arc::clone(&config)).await {
            panic!("Unable to start TLS server: {:?}", e);
        }
    });
    Ok(())
}
