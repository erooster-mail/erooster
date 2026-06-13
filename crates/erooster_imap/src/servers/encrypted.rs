// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::{
    commands::{Data, Response},
    servers::state::{Connection, State},
    Server, CAPABILITY_HELLO,
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
    color_eyre::{self, eyre::Context},
    futures::{SinkExt, StreamExt},
    notify::{recommended_watcher, RecursiveMode, Watcher},
    rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer},
    tokio::{
        self,
        net::{TcpListener, TcpStream},
        sync::mpsc,
    },
    tokio_rustls::{rustls, TlsAcceptor},
    tokio_stream::wrappers::TcpListenerStream,
    tokio_util::codec::Framed,
    tracing::{debug, error, info, instrument},
};
use std::{
    io,
    net::SocketAddr,
    path::Path,
    sync::Arc,
};

/// An encrypted imap Server
pub struct Encrypted;

pub fn get_tls_acceptor(config: &Config) -> color_eyre::eyre::Result<TlsAcceptor> {
    // Load SSL Keys
    let certs = load_certs(Path::new(&config.tls.cert_path))?;
    let key = load_key(Path::new(&config.tls.key_path))?;

    // Sets up the TLS acceptor.
    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;

    // Starts a TLS accepting thing.
    Ok(TlsAcceptor::from(Arc::new(server_config)))
}

// Loads the certfile from the filesystem
#[instrument(skip(path))]
fn load_certs(path: &Path) -> color_eyre::eyre::Result<Vec<CertificateDer<'static>>> {
    CertificateDer::pem_file_iter(path)
        .map_err(|e| color_eyre::eyre::eyre!("{e:?}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| color_eyre::eyre::eyre!("{e:?}"))
}

#[instrument(skip(path))]
fn load_key(path: &Path) -> color_eyre::eyre::Result<PrivateKeyDer<'static>> {
    PrivateKeyDer::from_pem_file(path)
        .map_err(|e| color_eyre::eyre::eyre!("no keys found in {:?} (encrypted keys not supported): {e:?}", path))
}

impl Server for Encrypted {
    /// Starts a TLS server
    ///
    /// # Errors
    ///
    /// Returns an error if the cert setup fails
    #[allow(clippy::too_many_lines)]
    #[instrument(skip(config, database, storage))]
    async fn run(config: Config, database: &DB, storage: &Storage) -> color_eyre::eyre::Result<()> {
        // Load SSL Keys
        let acceptor = get_tls_acceptor(&config)?;

        // Opens the listener
        let addrs: Vec<SocketAddr> = if let Some(listen_ips) = &config.listen_ips {
            listen_ips
                .iter()
                .map(|ip| format!("{ip}:993").parse())
                .filter_map(Result::ok)
                .collect()
        } else {
            vec!["0.0.0.0:993".parse()?]
        };
        for addr in addrs {
            info!("[IMAP] Trying to listen on {:?}", addr);
            let listener = TcpListener::bind(addr).await?;
            info!("[IMAP] Listening on ecrypted Port");
            let stream = TcpListenerStream::new(listener);

            let database = database.clone();
            let storage = storage.clone();
            let acceptor = acceptor.clone();
            let config = config.clone();
            tokio::spawn(async move {
                listen(stream, &config, &database, &storage, acceptor.clone()).await;
            });
        }

        Ok(())
    }
}

#[instrument(skip(stream, config, database, storage, acceptor))]
async fn listen(
    mut stream: TcpListenerStream,
    config: &Config,
    database: &DB,
    storage: &Storage,
    acceptor: TlsAcceptor,
) {
    // Looks for new peers
    while let Some(Ok(tcp_stream)) = stream.next().await {
        if let Err(e) = listen_tls(
            tcp_stream,
            config,
            database,
            storage,
            acceptor.clone(),
            None,
            false,
        ) {
            error!("[IMAP][ENCRYPTED] Error while listening: {}", e);
        }
    }
}

#[allow(clippy::too_many_lines)]
pub fn listen_tls(
    tcp_stream: TcpStream,
    config: &Config,
    database: &DB,
    storage: &Storage,
    acceptor: TlsAcceptor,
    upper_data: Option<Data>,
    starttls: bool,
) -> color_eyre::eyre::Result<()> {
    let peer = tcp_stream
        .peer_addr()
        .context("[IMAP] peer addr to exist")?;
    debug!("[IMAP] Got new TLS peer: {:?}", peer);

    // We need to clone these as we move into a new thread
    let database = database.clone();
    let storage = storage.clone();
    let config = config.clone();

    // Start talking with new peer on new thread
    tokio::spawn(async move {
        // Accept TCP connection
        let tls_stream = acceptor.accept(tcp_stream).await;

        // Continue if it worked
        match tls_stream {
            Ok(stream) => {
                debug!("[IMAP] TLS negotiation done");

                // Proceed as normal
                let lines = Framed::new(stream, LinesCodec::new_with_max_length(LINE_LIMIT));
                // We split these as we handle the sink in a broadcast instead to be able to push non linear data over the socket
                let (mut lines_sender, mut lines_reader) = lines.split();

                // Greet the client with the capabilities we provide
                if !starttls {
                    if let Err(e) = lines_sender.send(CAPABILITY_HELLO.to_string()).await {
                        error!(
                            "Unable to send greeting to client. Closing connection. Error: {}",
                            e
                        );
                        return;
                    }
                }
                // Create our Connection
                let connection = Connection::new(true);

                let mut data = if let Some(mut data) = upper_data.clone() {
                    {
                        data.con_state.secure = true;
                    };
                    data
                } else {
                    Data {
                        con_state: connection,
                    }
                };
                // Read lines from the stream
                while let Some(Ok(line)) = lines_reader.next().await {
                    debug!("[IMAP] [{}] Got Command: {}", peer, line);

                    let response = data
                        .parse(&mut lines_sender, &config, &database, &storage, line)
                        .await;
                    match response {
                        Ok(Response::Exit) => {
                            debug!("[IMAP] Closing TLS connection");
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
                        Ok(_) => {}
                        // We try a last time to do a graceful shutdown before closing
                        Err(e) => {
                            if let Err(e) = lines_sender
                                .send(format!("* BAD [SERVERBUG] This should not happen: {e}"))
                                .await
                            {
                                error!("Unable to send error response: {}", e);
                            }
                            error!("[IMAP] Failure happened: {}", e);
                            debug!("[IMAP] Closing TLS connection");
                            break;
                        }
                    }
                }
            }
            Err(e) => error!("[IMAP] Got error while accepting TLS: {}", e),
        }
    });
    Ok(())
}
