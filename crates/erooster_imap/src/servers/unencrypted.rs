// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::{
    commands::{Data, Response},
    servers::{
        encrypted::{get_tls_acceptor, listen_tls},
        state::{Connection, State},
    },
    Server, CAPABILITY_UNENCRYPTED_HELLO,
};
use erooster_core::{
    backend::{
        database::DB,
        storage::{MailStorage, Storage},
    },
    config::Config,
    line_codec::LinesCodec,
    LINE_LIMIT,
};
use notify;
use {
    color_eyre::{self, Result},
    futures::{SinkExt, StreamExt},
    notify::{recommended_watcher, RecursiveMode, Watcher},
    tokio::{self, net::TcpListener, sync::mpsc, task::JoinHandle},
    tokio_stream::wrappers::TcpListenerStream,
    tokio_util::codec::Framed,
    tracing::{debug, error, info, instrument},
};
use std::net::SocketAddr;

/// An unencrypted imap Server
pub struct Unencrypted;

impl Server for Unencrypted {
    #[instrument(skip(config, database, storage))]
    async fn run(config: Config, database: &DB, storage: &Storage) -> color_eyre::eyre::Result<()> {
        let addrs: Vec<SocketAddr> = if let Some(listen_ips) = &config.listen_ips {
            listen_ips
                .iter()
                .map(|ip| format!("{ip}:143").parse())
                .filter_map(Result::ok)
                .collect()
        } else {
            vec!["0.0.0.0:143".parse()?]
        };
        for addr in addrs {
            info!("[IMAP] Trying to listen on {:?}", addr);
            let listener = TcpListener::bind(addr).await?;
            info!("[IMAP] Listening on unencrypted Port");
            let stream = TcpListenerStream::new(listener);

            let database = database.clone();
            let storage = storage.clone();
            let config = config.clone();
            tokio::spawn(async move {
                listen(stream, &config, &database, &storage).await;
            });
        }
        Ok(())
    }
}

#[allow(clippy::too_many_lines)]
#[instrument(skip(stream, config, database, storage))]
async fn listen(mut stream: TcpListenerStream, config: &Config, database: &DB, storage: &Storage) {
    while let Some(Ok(tcp_stream)) = stream.next().await {
        let peer = tcp_stream.peer_addr().expect("peer addr to exist");
        debug!("[IMAP] Got new peer: {}", peer);

        let database = database.clone();
        let storage = storage.clone();
        let config = config.clone();
        let connection: JoinHandle<Result<()>> = tokio::spawn(async move {
            let lines = Framed::new(tcp_stream, LinesCodec::new_with_max_length(LINE_LIMIT));
            let (mut lines_sender, mut lines_reader) = lines.split();
            if let Err(e) = lines_sender
                .send(CAPABILITY_UNENCRYPTED_HELLO.to_string())
                .await
            {
                error!(
                    "Unable to send greeting to client. Closing connection. Error: {}",
                    e
                );
                return Ok(());
            }
            let state = Connection::new(false);

            let mut data = Data { con_state: state };
            let mut do_starttls = false;
            while let Some(Ok(line)) = lines_reader.next().await {
                debug!("[IMAP] [{}] Got Command: {}", peer, line);

                let response = data
                    .parse(&mut lines_sender, &config, &database, &storage, line)
                    .await;

                match response {
                    Ok(Response::Exit) => {
                        debug!("[IMAP] Closing connection");
                        break;
                    }
                    Ok(Response::STARTTLS(tag)) => {
                        debug!("[IMAP] Switching context");
                        do_starttls = true;
                        lines_sender
                            .send(format!("{tag} OK Begin TLS negotiation now"))
                            .await?;
                        break;
                    }
                    Ok(Response::Idle { ref tag }) => {
                        let tag = tag.clone();
                        let idle_info = if let State::Selected(f, _) = &data.con_state.state {
                            data.con_state.username.clone().map(|u| (f.replace('/', "."), u))
                        } else {
                            None
                        };
                        if let Some((folder, username)) = idle_info {
                            match storage.to_ondisk_path(folder, username) {
                                Ok(mailbox_path) => {
                                    let (tx, mut rx) = mpsc::channel::<notify::Result<notify::Event>>(16);
                                    match recommended_watcher(move |res| {
                                        let _ = tx.blocking_send(res);
                                    }) {
                                        Ok(mut watcher) => {
                                            if let Err(e) = watcher.watch(&mailbox_path, RecursiveMode::NonRecursive) {
                                                error!("[IMAP] IDLE: failed to watch mailbox: {e}");
                                            } else {
                                                loop {
                                                    tokio::select! {
                                                        Some(_) = rx.recv() => {
                                                            let count = storage.count_cur(&mailbox_path)
                                                                + storage.count_new(&mailbox_path);
                                                            if let Err(e) = lines_sender.send(format!("* {count} EXISTS")).await {
                                                                error!("[IMAP] IDLE: send EXISTS failed: {e}");
                                                                break;
                                                            }
                                                        }
                                                        line = lines_reader.next() => {
                                                            match line {
                                                                Some(Ok(l)) if l.trim().eq_ignore_ascii_case("done") => {
                                                                    if let Err(e) = lines_sender.send(format!("{tag} OK IDLE terminated")).await {
                                                                        error!("[IMAP] IDLE: send termination failed: {e}");
                                                                    }
                                                                    break;
                                                                }
                                                                None => break,
                                                                _ => {}
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            drop(watcher);
                                        }
                                        Err(e) => error!("[IMAP] IDLE: failed to create watcher: {e}"),
                                    }
                                }
                                Err(e) => error!("[IMAP] IDLE: bad mailbox path: {e}"),
                            }
                        }
                    }
                    Ok(Response::Continue) => {}
                    // We try a last time to do a graceful shutdown before closing
                    Err(e) => {
                        if let Err(e) = lines_sender
                            .send(format!("* BAD [SERVERBUG] This should not happen: {e}"))
                            .await
                        {
                            error!("Unable to send error response: {}", e);
                        }
                        error!("[IMAP] Failure happened: {}", e);
                        debug!("[IMAP] Closing connection");
                        break;
                    }
                }
            }
            if do_starttls {
                debug!("[IMAP] Starting to reunite");
                let framed_stream = lines_sender.reunite(lines_reader)?;
                let stream = framed_stream.into_inner();
                debug!("[IMAP] Finished to reunite");
                let acceptor = get_tls_acceptor(&config)?;
                debug!("[IMAP] Starting to listen using tls");
                if let Err(e) = listen_tls(
                    stream,
                    &config,
                    &database,
                    &storage,
                    acceptor,
                    Some(data),
                    true,
                ) {
                    error!("[SMTP] Error while upgrading to tls: {}", e);
                }
            }
            Ok(())
        });
        let resp = connection.await;
        if let Ok(Err(e)) = resp {
            error!("[IMAP] Error: {:?}", e);
        }
    }
}
