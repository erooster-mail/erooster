use std::sync::Arc;

use async_trait::async_trait;
use futures::SinkExt;
use tokio::net::TcpListener;
use tokio_stream::{wrappers::TcpListenerStream, StreamExt};
use tokio_util::codec::Framed;
use tracing::{debug, info};

use crate::{
    imap_commands::{capability::get_capabilities, Data, Parser},
    config::Config,
    line_codec::LinesCodec,
    servers::state::{Connection, State},
    servers::Server,
};

/// An unencrypted imap Server
pub struct Unencrypted;

#[async_trait]
impl Server for Unencrypted {
    async fn run_imap(config: Arc<Config>) -> anyhow::Result<()> {
        let listener = TcpListener::bind("0.0.0.0:143").await?;
        info!("Listening on unecrypted Port");
        let mut stream = TcpListenerStream::new(listener);
        while let Some(Ok(tcp_stream)) = stream.next().await {
            let peer = tcp_stream.peer_addr().expect("peer addr to exist");
            debug!("Got new peer: {}", peer);

            let config = Arc::clone(&config);
            tokio::spawn(async move {
                let mut lines = Framed::new(tcp_stream, LinesCodec::new());
                let capabilities = get_capabilities();
                lines
                    .send(format!("* OK [{}] IMAP4rev2 Service Ready", capabilities))
                    .await
                    .unwrap();
                let mut state = Connection {
                    state: State::NotAuthenticated,
                    ip: peer.ip(),
                    secure: false,
                    username: None,
                };
                while let Some(Ok(line)) = lines.next().await {
                    let data = Data {
                        command_data: None,
                        con_state: &mut state,
                    };

                    debug!("[{}] Got Command: {}", peer, line);

                    // TODO make sure to handle IDLE different as it needs us to stream lines
                    // TODO pass lines and make it possible to not need new lines in responds but instead directly use `lines.send`
                    let response = data.parse(&mut lines, Arc::clone(&config), line).await;

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
                            lines
                                .send(format!("* BAD [SERVERBUG] This should not happen: {}", e))
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
