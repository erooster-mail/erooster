use crate::{commands::Data, servers::state::Connection, Server, CAPABILITY_HELLO};
use async_trait::async_trait;
use erooster_core::{
    backend::{database::DB, storage::Storage},
    config::Config,
    line_codec::LinesCodec,
    LINE_LIMIT,
};
use futures::{channel::mpsc, SinkExt, StreamExt};
use notify::Event;
use std::{net::SocketAddr, sync::Arc};
use tokio::{net::TcpListener, sync::broadcast};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::codec::Framed;
use tracing::{debug, error, info, instrument};

/// An unencrypted imap Server
pub struct Unencrypted;

#[async_trait]
impl Server for Unencrypted {
    #[instrument(skip(config, database, storage, file_watcher))]
    async fn run(
        config: Arc<Config>,
        database: DB,
        storage: Arc<Storage>,
        file_watcher: broadcast::Sender<Event>,
    ) -> color_eyre::eyre::Result<()> {
        let addrs: Vec<SocketAddr> = if let Some(listen_ips) = &config.listen_ips {
            listen_ips
                .iter()
                .map(|ip| format!("{}:143", ip).parse())
                .filter_map(Result::ok)
                .collect()
        } else {
            vec!["0.0.0.0:143".parse()?]
        };
        for addr in addrs {
            info!("[IMAP] Trying to listen on {:?}", addr);
            let listener = TcpListener::bind(addr).await?;
            info!("[IMAP] Listening on unecrypted Port");
            let stream = TcpListenerStream::new(listener);

            let config = Arc::clone(&config);
            let database = Arc::clone(&database);
            let storage = Arc::clone(&storage);
            let file_watcher = file_watcher.clone();
            tokio::spawn(async move {
                listen(
                    stream,
                    Arc::clone(&config),
                    Arc::clone(&database),
                    Arc::clone(&storage),
                    file_watcher.clone(),
                )
                .await;
            });
        }
        Ok(())
    }
}

#[instrument(skip(stream, config, database, storage, _file_watcher))]
async fn listen(
    mut stream: TcpListenerStream,
    config: Arc<Config>,
    database: DB,
    storage: Arc<Storage>,
    mut _file_watcher: broadcast::Sender<Event>,
) {
    while let Some(Ok(tcp_stream)) = stream.next().await {
        let peer = tcp_stream.peer_addr().expect("peer addr to exist");
        debug!("[IMAP] Got new peer: {}", peer);

        let config = Arc::clone(&config);
        let database = Arc::clone(&database);
        let storage = Arc::clone(&storage);
        tokio::spawn(async move {
            let lines = Framed::new(tcp_stream, LinesCodec::new_with_max_length(LINE_LIMIT));
            let (mut lines_sender, mut lines_reader) = lines.split();
            if let Err(e) = lines_sender.send(CAPABILITY_HELLO.to_string()).await {
                error!(
                    "Unable to send greeting to client. Closing connection. Error: {}",
                    e
                );
                return;
            }
            let state = Connection::new(false);

            let (mut tx, mut rx) = mpsc::unbounded();
            let sender = tokio::spawn(async move {
                while let Some(res) = rx.next().await {
                    if let Err(error) = lines_sender.send(res).await {
                        error!("[IMAP] Error sending response to client: {:?}", error);
                        break;
                    }
                }
            });
            while let Some(Ok(line)) = lines_reader.next().await {
                let data = Data {
                    con_state: Arc::clone(&state),
                };

                debug!("[IMAP] [{}] Got Command: {}", peer, line);

                // TODO make sure to handle IDLE different as it needs us to stream lines
                // TODO pass lines and make it possible to not need new lines in responds but instead directly use `lines.send`
                let response = data
                    .parse(
                        &mut tx,
                        Arc::clone(&config),
                        Arc::clone(&database),
                        Arc::clone(&storage),
                        line,
                    )
                    .await;

                match response {
                    Ok(response) => {
                        // Cleanup timeout managers
                        if response {
                            // Used for later session timer management
                            debug!("[IMAP] Closing connection");
                            break;
                        }
                    }
                    // We try a last time to do a graceful shutdown before closing
                    Err(e) => {
                        if let Err(e) = tx
                            .send(format!("* BAD [SERVERBUG] This should not happen: {}", e))
                            .await
                        {
                            error!("Unable to send error response: {}", e);
                        }
                        error!("[IMAP] Failure happened: {}", e);
                        debug!("[IMAP] Closing connection");
                        sender.abort();
                        break;
                    }
                }
            }
        });
    }
}
