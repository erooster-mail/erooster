use crate::{
    commands::{Data, Response},
    servers::{send_capabilities, state::Connection},
};
use color_eyre::eyre::Context;
use erooster_core::{
    backend::{database::DB, storage::Storage},
    config::Config,
    line_codec::LinesCodec,
    LINE_LIMIT,
};
use futures::{SinkExt, StreamExt};
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

pub fn get_tls_acceptor(config: &Config) -> color_eyre::eyre::Result<TlsAcceptor> {
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
        config: Config,
        database: &DB,
        storage: &Storage,
    ) -> color_eyre::eyre::Result<()> {
        let acceptor = get_tls_acceptor(&config)?;
        // Opens the listener
        let addrs: Vec<SocketAddr> = if let Some(listen_ips) = &config.listen_ips {
            listen_ips
                .iter()
                .map(|ip| format!("{ip}:465").parse())
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
            error!("[SMTP][ENCRYPTED] Error while listening: {}", e);
        }
    }
}

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
        .context("[SMTP] peer addr to exist")?;
    debug!("[SMTP] Got new TLS peer: {:?}", peer);

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
                debug!("[SMTP] TLS negotiation done");

                // Proceed as normal
                let lines = Framed::new(stream, LinesCodec::new_with_max_length(LINE_LIMIT));
                // We split these as we handle the sink in a broadcast instead to be able to push non linear data over the socket
                let (mut lines_sender, mut lines_reader) = lines.split();

                // Create our Connection
                let connection = Connection::new(true, peer.ip().to_string());
                if !starttls {
                    // Greet the client with the capabilities we provide
                    if let Err(e) = send_capabilities(&config, &mut lines_sender).await {
                        error!(
                            "Unable to send greeting to client. Closing connection. Error: {}",
                            e
                        );
                        return;
                    }
                }

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
                    debug!("[SMTP][TLS] [{}] Got Command: {}", peer, line);

                    {
                        let response = data
                            .parse(&mut lines_sender, &config, &database, &storage, line)
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
                                if let Err(e) = lines_sender
                                    .send(format!("500 This should not happen: {e}"))
                                    .await
                                {
                                    error!("Unable to send error response: {}", e);
                                }
                                error!("[SMTP][TLS] Failure happened: {}", e);
                                debug!("[SMTP][TLS] Closing TLS connection");

                                break;
                            }
                        }
                    };
                }
            }
            Err(e) => error!("[SMTP][TLS] Got error while accepting TLS: {}", e),
        }
    });
    Ok(())
}
