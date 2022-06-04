use crate::{
    commands::Data,
    servers::{send_capabilities, state::Connection},
};
use erooster_core::{
    backend::{database::DB, storage::Storage},
    config::Config,
    line_codec::LinesCodec,
};
use futures::{channel::mpsc, SinkExt, StreamExt};
use std::{
    fs,
    io::{self, BufReader},
    net::SocketAddr,
    path::Path,
    sync::Arc,
};
use tokio::net::TcpListener;
use tokio_rustls::{
    rustls::{self, Certificate, PrivateKey},
    TlsAcceptor,
};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::codec::Framed;
use tracing::{debug, error, info, instrument};

/// An encrypted smtp Server
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

        color_eyre::eyre::bail!("no keys found in {:?} (encrypted keys not supported)", path)
    }

    /// Starts a TLS server
    ///
    /// # Errors
    ///
    /// Returns an error if the cert setup fails
    #[instrument(skip(config, database, storage))]
    pub(crate) async fn run(
        config: Arc<Config>,
        database: DB,
        storage: Arc<Storage>,
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
                .map(|ip| format!("{}:465", ip).parse().unwrap())
                .collect()
        } else {
            vec!["0.0.0.0:465".parse()?]
        };
        for addr in addrs {
            info!("[SMTP] Trying to listen on {:?}", addr);
            let listener = TcpListener::bind(addr).await.unwrap();
            info!("[SMTP] Listening on ecrypted Port");
            let stream = TcpListenerStream::new(listener);
            let config = Arc::clone(&config);
            let database = Arc::clone(&database);
            let storage = Arc::clone(&storage);
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                listen(
                    stream,
                    Arc::clone(&config),
                    Arc::clone(&database),
                    Arc::clone(&storage),
                    acceptor.clone(),
                )
                .await;
            });
        }

        Ok(())
    }
}

#[instrument(skip(stream, config, database, storage, acceptor))]
async fn listen(
    mut stream: TcpListenerStream,
    config: Arc<Config>,
    database: DB,
    storage: Arc<Storage>,
    acceptor: TlsAcceptor,
) {
    // Looks for new peers
    while let Some(Ok(tcp_stream)) = stream.next().await {
        debug!("[SMTP] Got new TLS peer: {:?}", tcp_stream.peer_addr());
        let peer = tcp_stream.peer_addr().expect("peer addr to exist");

        // We need to clone these as we move into a new thread
        let config = Arc::clone(&config);
        let database = Arc::clone(&database);
        let storage = Arc::clone(&storage);

        // Start talking with new peer on new thread
        let acceptor = acceptor.clone();
        tokio::spawn(async move {
            // Accept TCP connection
            let tls_stream = acceptor.accept(tcp_stream).await;

            // Continue if it worked
            match tls_stream {
                Ok(stream) => {
                    debug!("[SMTP] TLS negotiation done");

                    // Proceed as normal
                    let lines = Framed::new(stream, LinesCodec::new());
                    // We split these as we handle the sink in a broadcast instead to be able to push non linear data over the socket
                    let (mut lines_sender, mut lines_reader) = lines.split();
                    // Create our Connection
                    let connection = Connection::new(true);

                    // Prepare our custom return path
                    let (mut tx, mut rx) = mpsc::unbounded();
                    tokio::spawn(async move {
                        while let Some(res) = rx.next().await {
                            lines_sender.send(res).await.unwrap();
                        }
                    });

                    // Greet the client with the capabilities we provide
                    send_capabilities(Arc::clone(&config), &mut tx)
                        .await
                        .unwrap();

                    // Read lines from the stream
                    while let Some(Ok(line)) = lines_reader.next().await {
                        let data = Data {
                            con_state: Arc::clone(&connection),
                        };
                        debug!("[SMTP] [{}] Got Command: {}", peer, line);
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
                                        debug!("[SMTP] Closing TLS connection");
                                        break;
                                    }
                                }
                                // We try a last time to do a graceful shutdown before closing
                                Err(e) => {
                                    tx.send(format!("500 This should not happen: {}", e))
                                        .await
                                        .unwrap();
                                    debug!("[SMTP] Closing TLS connection");
                                    break;
                                }
                            }
                        };
                    }
                }
                Err(e) => error!("[SMTP] Got error while accepting TLS: {}", e),
            }
        });
    }
}
