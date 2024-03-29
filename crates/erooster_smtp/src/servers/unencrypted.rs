// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::{
    commands::{Data, Response},
    servers::{
        encrypted::{get_tls_acceptor, listen_tls},
        send_capabilities,
        state::Connection,
    },
};
use erooster_core::{
    backend::{database::DB, storage::Storage},
    config::Config,
    line_codec::LinesCodec,
    LINE_LIMIT,
};
use erooster_deps::{
    color_eyre::{self, eyre::Context, Result},
    futures::{SinkExt, StreamExt},
    tokio::{self, net::TcpListener, task::JoinHandle},
    tokio_stream::wrappers::TcpListenerStream,
    tokio_util::{codec::Framed, sync::CancellationToken},
    tracing::{self, debug, error, info, instrument},
};
use std::net::SocketAddr;
/// An unencrypted smtp Server
pub struct Unencrypted;

impl Unencrypted {
    // TODO: make this only pub for benches and tests
    #[allow(missing_docs)]
    #[allow(clippy::missing_errors_doc)]
    #[instrument(skip(config, database, storage, shutdown_flag))]
    pub async fn run(
        config: Config,
        database: &DB,
        storage: &Storage,
        shutdown_flag: CancellationToken,
    ) -> color_eyre::eyre::Result<()> {
        let addrs: Vec<SocketAddr> = if let Some(listen_ips) = &config.listen_ips {
            listen_ips
                .iter()
                .map(|ip| format!("{ip}:587").parse())
                .chain(listen_ips.iter().map(|ip| format!("{ip}:25").parse()))
                .filter_map(Result::ok)
                .collect()
        } else {
            vec!["0.0.0.0:587".parse()?, "0.0.0.0:25".parse()?]
        };
        for addr in addrs {
            info!("[SMTP] Trying to listen on {:?}", addr);
            let listener = TcpListener::bind(addr).await?;
            info!("[SMTP] Listening on unencrypted Port");
            let stream = TcpListenerStream::new(listener);

            let database = database.clone();
            let storage = storage.clone();
            let config = config.clone();
            let shutdown_flag = shutdown_flag.clone();
            tokio::spawn(async move {
                listen(stream, &config, &database, &storage, shutdown_flag.clone()).await;
            });
        }

        Ok(())
    }
}

#[allow(clippy::too_many_lines)]
async fn listen(
    mut stream: TcpListenerStream,
    config: &Config,
    database: &DB,
    storage: &Storage,
    shutdown_flag: CancellationToken,
) {
    let shutdown_flag_clone = shutdown_flag.clone();
    while let Some(Ok(tcp_stream)) = stream.next().await {
        if shutdown_flag_clone.clone().is_cancelled() {
            break;
        }
        let peer = tcp_stream.peer_addr().expect("[SMTP] peer addr to exist");
        debug!("[SMTP] Got new peer: {}", peer);

        let database = database.clone();
        let storage = storage.clone();
        let config = config.clone();
        let shutdown_flag_clone = shutdown_flag_clone.clone();
        let connection: JoinHandle<Result<()>> = tokio::spawn(async move {
            let lines = Framed::new(tcp_stream, LinesCodec::new_with_max_length(LINE_LIMIT));
            let (mut lines_sender, mut lines_reader) = lines.split();

            let state = Connection::new(false, peer.ip().to_string());

            // Greet the client with the capabilities we provide
            send_capabilities(&config, &mut lines_sender)
                .await
                .context("Unable to send greeting to client. Closing connection.")?;

            let mut data = Data { con_state: state };

            let mut do_starttls = false;
            let shutdown_flag_clone = shutdown_flag_clone.clone();
            while let Some(Ok(line)) = lines_reader.next().await {
                if shutdown_flag_clone.clone().is_cancelled() {
                    if let Err(e) = lines_sender.send(String::from("421 Shutting down")).await {
                        error!("[SMTP] Error sending response: {:?}", e);
                    }
                    break;
                }
                debug!("[SMTP] [{}] Got Command: {}", peer, line);

                // TODO make sure to handle IDLE different as it needs us to stream lines
                // TODO pass lines and make it possible to not need new lines in responds but instead directly use `lines.send`
                let response = data
                    .parse(&mut lines_sender, &config, &database, &storage, line)
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
                                do_starttls = true;
                                lines_sender.send(String::from("220 TLS go ahead")).await?;
                                break;
                            }
                            Response::Continue => {}
                        }
                    }
                    // We try a last time to do a graceful shutdown before closing
                    Err(e) => {
                        if let Err(e) = lines_sender
                            .send(format!("500 This should not happen: {e}"))
                            .await
                        {
                            error!("[SMTP] Error sending response: {:?}", e);
                        }
                        error!("[SMTP] Failure happened: {}", e);
                        debug!("[SMTP] Closing connection");
                        break;
                    }
                }
            }
            if do_starttls {
                debug!("[SMTP] Starting to reunite");
                let framed_stream = lines_sender.reunite(lines_reader)?;
                let stream = framed_stream.into_inner();
                debug!("[SMTP] Finished to reunite");
                let acceptor = get_tls_acceptor(&config)?;
                debug!("[SMTP] Starting to listen using tls");
                if let Err(e) = listen_tls(
                    stream,
                    &config,
                    &database,
                    &storage,
                    acceptor,
                    Some(data),
                    true,
                    shutdown_flag_clone.clone(),
                ) {
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
