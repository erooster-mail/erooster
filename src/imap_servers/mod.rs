use crate::config::Config;
use crate::imap_commands::capability::get_capabilities;
use async_trait::async_trait;
use const_format::formatcp;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::{path::Path, sync::Arc};
use tokio::sync::broadcast;

#[cfg(feature = "postgres")]
use crate::database::postgres::Postgres;
#[cfg(feature = "sqlite")]
use crate::database::sqlite::Sqlite;

pub(crate) mod encrypted;
pub(crate) mod state;
pub(crate) mod unencrypted;

/// A const variant of the Capabilities we welcome clients with
pub const CAPABILITY_HELLO: &str =
    formatcp!("* OK [{}] IMAP4rev2 Service Ready", get_capabilities());

/// An implementation of a imap server
#[async_trait]
pub trait Server {
    /// Start the server
    async fn run(
        config: Arc<Config>,
        #[cfg(feature = "postgres")] database: Arc<Postgres>,
        #[cfg(feature = "sqlite")] database: Arc<Sqlite>,
        file_watcher: broadcast::Sender<Event>,
    ) -> color_eyre::eyre::Result<()>;
}

/// Starts the imap server
///
/// # Errors
///
/// Returns an error if the server startup fails
pub fn start(
    config: Arc<Config>,
    #[cfg(feature = "postgres")] database: Arc<Postgres>,
    #[cfg(feature = "sqlite")] database: Arc<Sqlite>,
) -> color_eyre::eyre::Result<()> {
    let (tx, _rx) = broadcast::channel(1);
    let tx_clone = tx.clone();
    let mut watcher = RecommendedWatcher::new(move |res: notify::Result<Event>| {
        if let Ok(event) = res {
            futures::executor::block_on(async {
                tx.send(event.clone())
                    .expect("failed to send filechange event");
            });
        }
    })?;

    std::fs::create_dir_all(&config.mail.maildir_folders)?;

    watcher.watch(
        Path::new(&config.mail.maildir_folders),
        RecursiveMode::Recursive,
    )?;

    let config_clone = Arc::clone(&config);
    let db_clone = Arc::clone(&database);
    let tx_clone2 = tx_clone.clone();
    tokio::spawn(async move {
        if let Err(e) = unencrypted::Unencrypted::run(
            Arc::clone(&config_clone),
            Arc::clone(&db_clone),
            tx_clone,
        )
        .await
        {
            panic!("Unable to start server: {:?}", e);
        }
    });
    tokio::spawn(async move {
        if let Err(e) =
            encrypted::Encrypted::run(Arc::clone(&config), Arc::clone(&database), tx_clone2).await
        {
            panic!("Unable to start TLS server: {:?}", e);
        }
    });
    Ok(())
}
