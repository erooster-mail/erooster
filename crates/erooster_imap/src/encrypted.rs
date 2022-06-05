use crate::{
    commands::Data,
    state::{Connection, State},
    Server, CAPABILITY_HELLO,
};
use async_trait::async_trait;
use erooster_core::{
    backend::{database::DB, storage::Storage},
    config::Config,
    line_codec::LinesCodec,
};
use futures::SinkExt;
use futures::{
    channel::mpsc::{self, UnboundedSender},
    StreamExt,
};
use notify::Event;
use std::{
    fs::{self},
    io::{self, BufReader},
    net::SocketAddr,
    path::Path,
    sync::Arc,
};
use tokio::{
    net::TcpListener,
    sync::{broadcast, RwLock},
};
use tokio_rustls::{
    rustls::{self, Certificate, PrivateKey},
    TlsAcceptor,
};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::codec::Framed;
use tracing::{debug, error, info, instrument};

/// An encrypted imap Server
pub struct Encrypted;

impl Encrypted {
    // Loads the certfile from the filesystem
    #[instrument(skip(path))]
    fn load_certs(path: &Path) -> color_eyre::eyre::Result<Vec<Certificate>> {
        let certfile = fs::File::open(path)?;
        let mut reader = BufReader::new(certfile);
        Ok(rustls_pemfile::certs(&mut reader)?
            .iter()
            .map(|v| rustls::Certificate(v.clone()))
            .collect())
    }
    #[instrument(skip(path))]
    fn load_key(path: &Path) -> color_eyre::eyre::Result<PrivateKey> {
        let keyfile = fs::File::open(path)?;
        let mut reader = BufReader::new(keyfile);

        loop {
            match rustls_pemfile::read_one(&mut reader)? {
                Some(
                    rustls_pemfile::Item::RSAKey(key)
                    | rustls_pemfile::Item::PKCS8Key(key)
                    | rustls_pemfile::Item::ECKey(key),
                ) => return Ok(rustls::PrivateKey(key)),
                None => break,
                _ => {}
            }
        }

        Err(color_eyre::eyre::eyre!(
            "no keys found in {:?} (encrypted keys not supported)",
            path
        ))
    }
}

#[async_trait]
impl Server for Encrypted {
    /// Starts a TLS server
    ///
    /// # Errors
    ///
    /// Returns an error if the cert setup fails
    #[allow(clippy::too_many_lines)]
    #[instrument(skip(config, database, storage, file_watcher))]
    async fn run(
        config: Arc<Config>,
        database: DB,
        storage: Arc<Storage>,
        file_watcher: broadcast::Sender<Event>,
    ) -> color_eyre::eyre::Result<()> {
        // Load SSL Keys
        let certs = Encrypted::load_certs(Path::new(&config.tls.cert_path))?;
        let key = Encrypted::load_key(Path::new(&config.tls.key_path))?;

        // Sets up the TLS acceptor.
        let server_config = rustls::ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;

        // Starts a TLS accepting thing.
        let acceptor = TlsAcceptor::from(Arc::new(server_config));

        // Opens the listener
        let addrs: Vec<SocketAddr> = if let Some(listen_ips) = &config.listen_ips {
            listen_ips
                .iter()
                .map(|ip| format!("{}:993", ip).parse().unwrap())
                .collect()
        } else {
            vec!["0.0.0.0:993".parse()?]
        };
        for addr in addrs {
            info!("[IMAP] Trying to listen on {:?}", addr);
            let listener = TcpListener::bind(addr).await?;
            info!("[IMAP] Listening on ecrypted Port");
            let stream = TcpListenerStream::new(listener);

            let config = Arc::clone(&config);
            let database = Arc::clone(&database);
            let storage = Arc::clone(&storage);
            let file_watcher = file_watcher.clone();
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                listen(
                    stream,
                    Arc::clone(&config),
                    Arc::clone(&database),
                    Arc::clone(&storage),
                    file_watcher.clone(),
                    acceptor.clone(),
                )
                .await;
            });
        }

        Ok(())
    }
}

#[instrument(skip(stream, config, database, storage, file_watcher, acceptor))]
async fn listen(
    mut stream: TcpListenerStream,
    config: Arc<Config>,
    database: DB,
    storage: Arc<Storage>,
    file_watcher: broadcast::Sender<Event>,
    acceptor: TlsAcceptor,
) {
    // This clone is needed but as it is an arc it is "cheap"
    let file_watcher = file_watcher.clone();
    // Looks for new peers
    while let Some(Ok(tcp_stream)) = stream.next().await {
        debug!("[IMAP] Got new TLS peer: {:?}", tcp_stream.peer_addr());
        let peer = tcp_stream.peer_addr().expect("peer addr to exist");

        // We need to clone these as we move into a new thread
        let config = Arc::clone(&config);
        let database = Arc::clone(&database);
        let storage = Arc::clone(&storage);
        let file_watcher = file_watcher.clone();

        // Start talking with new peer on new thread
        let acceptor = acceptor.clone();
        tokio::spawn(async move {
            // Accept TCP connection
            let tls_stream = acceptor.accept(tcp_stream).await;

            // Continue if it worked
            match tls_stream {
                Ok(stream) => {
                    debug!("[IMAP] TLS negotiation done");

                    // Proceed as normal
                    let lines = Framed::new(stream, LinesCodec::new());
                    // We split these as we handle the sink in a broadcast instead to be able to push non linear data over the socket
                    let (mut lines_sender, mut lines_reader) = lines.split();

                    // Greet the client with the capabilities we provide
                    lines_sender
                        .send(CAPABILITY_HELLO.to_string())
                        .await
                        .unwrap();
                    // Create our Connection
                    let connection = Connection::new(true);

                    // Prepare our custom return path
                    let (mut tx, mut rx) = mpsc::unbounded();
                    let cloned_tx = tx.clone();
                    tokio::spawn(async move {
                        while let Some(res) = rx.next().await {
                            lines_sender.send(res).await.unwrap();
                        }
                    });
                    // Connection needed for the file watcher
                    let cloned_connection = Arc::clone(&connection);

                    // Start listening to file changes for this session
                    let mut file_watcher_subscriber = file_watcher.subscribe();

                    // Listen to file changes on another thread
                    tokio::spawn(async move {
                        while let Ok(res) = file_watcher_subscriber.recv().await {
                            check_changes(cloned_tx.clone(), Arc::clone(&cloned_connection), res)
                                .await;
                        }
                    });

                    // Read lines from the stream
                    while let Some(Ok(line)) = lines_reader.next().await {
                        let data = Data {
                            con_state: Arc::clone(&connection),
                        };
                        debug!("[IMAP] [{}] Got Command: {}", peer, line);
                        // TODO make sure to handle IDLE different as it needs us to stream lines

                        {
                            let close = data
                                .parse(
                                    &mut tx,
                                    Arc::clone(&config),
                                    Arc::clone(&database),
                                    Arc::clone(&storage),
                                    line,
                                )
                                .await;
                            match close {
                                Ok(close) => {
                                    // Cleanup timeout managers
                                    if close {
                                        // Used for later session timer management
                                        debug!("[IMAP] Closing TLS connection");
                                        break;
                                    }
                                }
                                // We try a last time to do a graceful shutdown before closing
                                Err(e) => {
                                    tx.send(format!(
                                        "* BAD [SERVERBUG] This should not happen: {}",
                                        e
                                    ))
                                    .await
                                    .unwrap();
                                    debug!("[IMAP] Closing TLS connection");
                                    break;
                                }
                            }
                        };
                    }
                }
                Err(e) => error!("[IMAP] Got error while accepting TLS: {}", e),
            }
        });
    }
}

async fn check_changes(
    _lines: UnboundedSender<String>,
    state: Arc<RwLock<Connection>>,
    event: Event,
) {
    let state = { &state.read().await.state };
    if matches!(state, State::Authenticated) || matches!(state, State::Selected(_, _)) {
        println!("{:?}", event);
    }
}
