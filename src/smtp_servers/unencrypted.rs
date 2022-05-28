use crate::{
    backend::database::DB,
    config::Config,
    line_codec::LinesCodec,
    smtp_commands::Data,
    smtp_servers::{
        send_capabilities,
        state::{Connection, State},
    },
};
use futures::{channel::mpsc, SinkExt, StreamExt};
use std::{net::SocketAddr, sync::Arc};
use tokio::{net::TcpListener, sync::RwLock};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::codec::Framed;
use tracing::{debug, error, info};

/// An unencrypted smtp Server
pub struct Unencrypted;

impl Unencrypted {
    // TODO make this only pub for benches and tests
    #[allow(missing_docs)]
    #[allow(clippy::missing_errors_doc)]
    pub async fn run(config: Arc<Config>, database: DB) -> color_eyre::eyre::Result<()> {
        let addr: Vec<SocketAddr> = if let Some(listen_ips) = &config.listen_ips {
            listen_ips
                .iter()
                .map(|ip| format!("{}:25", ip).parse().unwrap())
                .collect()
        } else {
            vec!["0.0.0.0:25".parse()?]
        };
        info!("[SMTP] Trying to listen on {:?}", addr);
        let listener = TcpListener::bind(&addr[..]).await?;
        info!("[SMTP] Listening on unecrypted Port");
        let mut stream = TcpListenerStream::new(listener);
        while let Some(Ok(tcp_stream)) = stream.next().await {
            let peer = tcp_stream.peer_addr().expect("[SMTP] peer addr to exist");
            debug!("[SMTP] Got new peer: {}", peer);

            let config = Arc::clone(&config);
            let database = Arc::clone(&database);
            tokio::spawn(async move {
                let lines = Framed::new(tcp_stream, LinesCodec::new());
                let (mut lines_sender, mut lines_reader) = lines.split();

                let state = Arc::new(RwLock::new(Connection {
                    secure: false,
                    state: State::NotAuthenticated,
                    data: None,
                    receipts: None,
                    sender: None,
                }));

                let (mut tx, mut rx) = mpsc::unbounded();
                tokio::spawn(async move {
                    while let Some(res) = rx.next().await {
                        if let Err(e) = lines_sender.send(res).await {
                            error!("[SMTP] Error sending response: {:?}", e);
                        }
                    }
                });

                // Greet the client with the capabilities we provide
                send_capabilities(Arc::clone(&config), &mut tx)
                    .await
                    .unwrap();

                while let Some(Ok(line)) = lines_reader.next().await {
                    let data = Data {
                        con_state: Arc::clone(&state),
                    };

                    debug!("[SMTP] [{}] Got Command: {}", peer, line);

                    // TODO make sure to handle IDLE different as it needs us to stream lines
                    // TODO pass lines and make it possible to not need new lines in responds but instead directly use `lines.send`
                    let response = data
                        .parse(&mut tx, Arc::clone(&config), Arc::clone(&database), line)
                        .await;

                    match response {
                        Ok(response) => {
                            // Cleanup timeout managers
                            if response {
                                // Used for later session timer management
                                debug!("[SMTP] Closing connection");
                                break;
                            }
                        }
                        // We try a last time to do a graceful shutdown before closing
                        Err(e) => {
                            if let Err(e) =
                                tx.send(format!("500 This should not happen: {}", e)).await
                            {
                                error!("[SMTP] Error sending response: {:?}", e);
                            }
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
