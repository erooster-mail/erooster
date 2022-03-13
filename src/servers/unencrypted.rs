use std::sync::Arc;

use async_trait::async_trait;
use futures::{channel::mpsc, SinkExt, StreamExt};
use notify::Event;
use tokio::{
    net::TcpListener,
    sync::{broadcast, RwLock},
};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::codec::Framed;
use tracing::{debug, info};

use crate::{
    config::Config,
    imap_commands::{Data, Parser},
    line_codec::LinesCodec,
    servers::Server,
    servers::{
        state::{Connection, State},
        CAPABILITY_HELLO,
    },
};

/// An unencrypted imap Server
pub struct Unencrypted;

#[async_trait]
impl Server for Unencrypted {
    async fn run_imap(
        config: Arc<Config>,
        mut _file_watcher: broadcast::Sender<Event>,
    ) -> color_eyre::eyre::Result<()> {
        let listener = TcpListener::bind("0.0.0.0:143").await?;
        info!("Listening on unecrypted Port");
        let mut stream = TcpListenerStream::new(listener);
        while let Some(Ok(tcp_stream)) = stream.next().await {
            let peer = tcp_stream.peer_addr().expect("peer addr to exist");
            debug!("Got new peer: {}", peer);

            let config = Arc::clone(&config);
            tokio::spawn(async move {
                let lines = Framed::new(tcp_stream, LinesCodec::new());
                let (mut lines_sender, mut lines_reader) = lines.split();
                lines_sender
                    .send(CAPABILITY_HELLO.to_string())
                    .await
                    .unwrap();
                let state = Arc::new(RwLock::new(Connection {
                    state: State::NotAuthenticated,
                    secure: false,
                    username: None,
                }));

                let (mut tx, mut rx) = mpsc::unbounded();
                tokio::spawn(async move {
                    while let Some(res) = rx.next().await {
                        lines_sender.send(res).await.unwrap();
                    }
                });
                while let Some(Ok(line)) = lines_reader.next().await {
                    let mut data = Data {
                        con_state: Arc::clone(&state),
                    };

                    debug!("[{}] Got Command: {}", peer, line);

                    // TODO make sure to handle IDLE different as it needs us to stream lines
                    // TODO pass lines and make it possible to not need new lines in responds but instead directly use `lines.send`
                    let response = data.parse(&mut tx, Arc::clone(&config), line).await;

                    match response {
                        Ok(response) => {
                            // Cleanup timeout managers
                            if response {
                                // Used for later session timer management
                                debug!("Closing connection");
                                break;
                            }
                        }
                        // We try a last time to do a graceful shutdown before closing
                        Err(e) => {
                            tx.send(format!("* BAD [SERVERBUG] This should not happen: {}", e))
                                .await
                                .unwrap();
                            debug!("Closing connection");
                            break;
                        }
                    }
                }
            });
        }
        Ok(())
    }
}
