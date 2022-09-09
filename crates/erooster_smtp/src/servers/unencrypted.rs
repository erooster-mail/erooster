use crate::{
    commands::{Data, Response},
    servers::{
        encrypted::{get_tls_acceptor, listen_tls},
        send_capabilities,
        state::Connection,
    },
};
use color_eyre::{eyre::Context, Result};
use erooster_core::{
    backend::{database::DB, storage::Storage},
    config::Config,
    line_codec::LinesCodec,
    LINE_LIMIT,
};
use futures::{channel::mpsc, SinkExt, StreamExt};
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::codec::Framed;
use tracing::{debug, error, info, instrument};

/// An unencrypted smtp Server
pub struct Unencrypted;

impl Unencrypted {
    // TODO make this only pub for benches and tests
    #[allow(missing_docs)]
    #[allow(clippy::missing_errors_doc)]
    #[instrument(skip(config, database, storage))]
    pub async fn run(
        config: Arc<Config>,
        database: DB,
        storage: Arc<Storage>,
    ) -> color_eyre::eyre::Result<()> {
        let addrs: Vec<SocketAddr> = if let Some(listen_ips) = &config.listen_ips {
            listen_ips
                .iter()
                .map(|ip| format!("{}:587", ip).parse())
                .chain(listen_ips.iter().map(|ip| format!("{}:25", ip).parse()))
                .filter_map(Result::ok)
                .collect()
        } else {
            vec!["0.0.0.0:587".parse()?, "0.0.0.0:25".parse()?]
        };
        for addr in addrs {
            info!("[SMTP] Trying to listen on {:?}", addr);
            let listener = TcpListener::bind(addr).await?;
            info!("[SMTP] Listening on unecrypted Port");
            let stream = TcpListenerStream::new(listener);

            let config = Arc::clone(&config);
            let database = Arc::clone(&database);
            let storage = Arc::clone(&storage);
            tokio::spawn(async move {
                listen(
                    stream,
                    Arc::clone(&config),
                    Arc::clone(&database),
                    Arc::clone(&storage),
                )
                .await;
            });
        }

        Ok(())
    }
}

#[allow(clippy::too_many_lines)]
async fn listen(
    mut stream: TcpListenerStream,
    config: Arc<Config>,
    database: DB,
    storage: Arc<Storage>,
) {
    while let Some(Ok(tcp_stream)) = stream.next().await {
        let peer = tcp_stream.peer_addr().expect("[SMTP] peer addr to exist");
        debug!("[SMTP] Got new peer: {}", peer);

        let config = Arc::clone(&config);
        let database = Arc::clone(&database);
        let storage = Arc::clone(&storage);
        let connection: JoinHandle<Result<()>> = tokio::spawn(async move {
            let lines = Framed::new(tcp_stream, LinesCodec::new_with_max_length(LINE_LIMIT));
            let (mut lines_sender, mut lines_reader) = lines.split();

            let state = Connection::new(false, peer.ip().to_string());

            let do_starttls = Arc::new(AtomicBool::new(false));
            let (mut tx, mut rx) = mpsc::unbounded();
            let cloned_do_starttls = Arc::clone(&do_starttls);
            let sender = tokio::spawn(async move {
                while let Some(res) = rx.next().await {
                    if let Err(e) = lines_sender.send(res).await {
                        error!("[SMTP] Error sending response: {:?}", e);
                        break;
                    }
                    if Arc::clone(&cloned_do_starttls).load(Ordering::Relaxed) {
                        debug!("[SMTP] releasing the sender");
                        break;
                    }
                }
                lines_sender
            });

            // Greet the client with the capabilities we provide
            send_capabilities(Arc::clone(&config), &mut tx)
                .await
                .context("Unable to send greeting to client. Closing connection.")?;
            let do_starttls = Arc::clone(&do_starttls);

            let data = Data {
                con_state: Arc::clone(&state),
            };
            while let Some(Ok(line)) = lines_reader.next().await {
                debug!("[SMTP] [{}] Got Command: {}", peer, line);

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
                        match response {
                            Response::Exit => {
                                // Used for later session timer management
                                debug!("[SMTP] Closing connection");
                                break;
                            }
                            Response::STARTTLS => {
                                debug!("[SMTP] Switching context");
                                Arc::clone(&do_starttls).store(true, Ordering::Relaxed);
                                tx.send(String::from("220 TLS go ahead")).await?;
                                break;
                            }
                            Response::Continue => {}
                        }
                    }
                    // We try a last time to do a graceful shutdown before closing
                    Err(e) => {
                        if let Err(e) = tx.send(format!("500 This should not happen: {}", e)).await
                        {
                            error!("[SMTP] Error sending response: {:?}", e);
                        }
                        error!("[SMTP] Failure happened: {}", e);
                        debug!("[SMTP] Closing connection");
                        sender.abort();
                        break;
                    }
                }
            }
            if do_starttls.load(Ordering::Relaxed) {
                debug!("[SMTP] Waiting for sender");
                //sender.abort();
                let sender = sender.await?;
                debug!("[SMTP] Starting to reunite");
                let framed_stream = sender.reunite(lines_reader)?;
                let stream = framed_stream.into_inner();
                debug!("[SMTP] Finished to reunite");
                let acceptor = get_tls_acceptor(&config)?;
                debug!("[SMTP] Starting to listen using tls");
                if let Err(e) = listen_tls(
                    stream,
                    config,
                    database,
                    storage,
                    acceptor,
                    Some(data),
                    true,
                )
                .await
                {
                    error!("[SMTP] Error while upgrading to tls: {}", e);
                }
            }
            Ok(())
        });
        let resp = connection.await;
        if let Ok(Err(e)) = resp {
            error!("[SMTP] Error: {:?}", e);
        }
    }
}
