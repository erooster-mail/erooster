use crate::{
    commands::{Data, Response},
    servers::{send_capabilities, state::Connection},
};
use erooster_core::{
    backend::{database::DB, storage::Storage},
    config::Config,
    line_codec::LinesCodec,
    LINE_LIMIT,
};
use futures::{channel::mpsc, SinkExt, StreamExt};
use std::{
    fs,
    io::{self, BufReader},
    net::SocketAddr,
    path::Path,
    sync::Arc,
};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{
    rustls::{self, Certificate, PrivateKey},
    TlsAcceptor,
};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::codec::Framed;
use tracing::{debug, error, info, instrument};

/// An encrypted smtp Server
pub struct Encrypted;

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

pub fn get_tls_acceptor(config: &Arc<Config>) -> color_eyre::eyre::Result<TlsAcceptor> {
    // Load SSL Keys
    let certs = load_certs(Path::new(&config.tls.cert_path))?;
    let key = load_key(Path::new(&config.tls.key_path))?;

    // Sets up the TLS acceptor.
    let server_config = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;

    // Starts a TLS accepting thing.
    Ok(TlsAcceptor::from(Arc::new(server_config)))
}

impl Encrypted {
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
        let acceptor = get_tls_acceptor(&config)?;
        // Opens the listener
        let addrs: Vec<SocketAddr> = if let Some(listen_ips) = &config.listen_ips {
            listen_ips
                .iter()
                .map(|ip| format!("{}:465", ip).parse())
                .filter_map(Result::ok)
                .collect()
        } else {
            vec!["0.0.0.0:465".parse()?]
        };
        for addr in addrs {
            info!("[SMTP] Trying to listen on {:?}", addr);
            let listener = TcpListener::bind(addr).await?;
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
        listen_tls(
            tcp_stream,
            Arc::clone(&config),
            Arc::clone(&database),
            Arc::clone(&storage),
            acceptor.clone(),
            None,
            false,
        )
        .await;
    }
}

pub async fn listen_tls(
    tcp_stream: TcpStream,
    config: Arc<Config>,
    database: DB,
    storage: Arc<Storage>,
    acceptor: TlsAcceptor,
    upper_data: Option<Data>,
    starttls: bool,
) {
    let peer = tcp_stream.peer_addr().expect("[SMTP] peer addr to exist");
    debug!("[SMTP] Got new TLS peer: {:?}", peer);
    let peer = tcp_stream.peer_addr().expect("peer addr to exist");

    // We need to clone these as we move into a new thread
    let config = Arc::clone(&config);
    let database = Arc::clone(&database);
    let storage = Arc::clone(&storage);

    // Start talking with new peer on new thread
    tokio::spawn(async move {
        // Accept TCP connection
        let tls_stream = acceptor.accept(tcp_stream).await;

        // Continue if it worked
        match tls_stream {
            Ok(stream) => {
                debug!("[SMTP] TLS negotiation done");

                // Proceed as normal
                let lines = Framed::new(stream, LinesCodec::new_with_max_length(LINE_LIMIT));
                // We split these as we handle the sink in a broadcast instead to be able to push non linear data over the socket
                let (mut lines_sender, mut lines_reader) = lines.split();

                // Prepare our custom return path
                let (mut tx, mut rx) = mpsc::unbounded();
                let sender = tokio::spawn(async move {
                    while let Some(res) = rx.next().await {
                        if let Err(error) = lines_sender.send(res).await {
                            error!("[SMTP] Error sending response to client: {:?}", error);
                            break;
                        }
                    }
                });

                // Create our Connection
                let connection = Connection::new(true, peer.to_string());
                if !starttls {
                    // Greet the client with the capabilities we provide
                    if let Err(e) = send_capabilities(Arc::clone(&config), &mut tx).await {
                        error!(
                            "Unable to send greeting to client. Closing connection. Error: {}",
                            e
                        );
                        return;
                    }
                }

                // Read lines from the stream
                while let Some(Ok(line)) = lines_reader.next().await {
                    let data = if let Some(data) = upper_data.clone() {
                        {
                            data.con_state.write().await.secure = true;
                        };
                        data
                    } else {
                        Data {
                            con_state: Arc::clone(&connection),
                        }
                    };
                    debug!("[SMTP][TLS] [{}] Got Command: {}", peer, line);

                    {
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
                                if let Response::Exit = response {
                                    // Used for later session timer management
                                    debug!("[SMTP][TLS] Closing connection");
                                    break;
                                }
                            }
                            // We try a last time to do a graceful shutdown before closing
                            Err(e) => {
                                if let Err(e) =
                                    tx.send(format!("500 This should not happen: {}", e)).await
                                {
                                    error!("Unable to send error response: {}", e);
                                }
                                debug!("[SMTP][TLS] Closing TLS connection");
                                sender.abort();
                                break;
                            }
                        }
                    };
                }
            }
            Err(e) => error!("[SMTP][TLS] Got error while accepting TLS: {}", e),
        }
    });
}
