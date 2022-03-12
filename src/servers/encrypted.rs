use crate::{
    config::Config,
    imap_commands::{capability::get_capabilities, Data, Parser},
    line_codec::LinesCodec,
    servers::state::{Connection, State},
    servers::Server,
};
use async_trait::async_trait;
use futures::SinkExt;
use futures::{
    channel::mpsc::{self, UnboundedSender},
    StreamExt,
};
use notify::Event;
use std::{
    fs::{self},
    io::{self, BufReader},
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
use tracing::{debug, error, info};

/// An encrypted imap Server
pub struct Encrypted;

impl Encrypted {
    fn load_certs(path: &Path) -> Vec<Certificate> {
        let certfile = fs::File::open(path).expect("cannot open certificate file");
        let mut reader = BufReader::new(certfile);
        rustls_pemfile::certs(&mut reader)
            .unwrap()
            .iter()
            .map(|v| rustls::Certificate(v.clone()))
            .collect()
    }

    fn load_key(path: &Path) -> PrivateKey {
        let keyfile = fs::File::open(path).expect("cannot open private key file");
        let mut reader = BufReader::new(keyfile);

        loop {
            match rustls_pemfile::read_one(&mut reader).expect("cannot parse private key .pem file")
            {
                Some(rustls_pemfile::Item::RSAKey(key) | rustls_pemfile::Item::PKCS8Key(key)) => {
                    return rustls::PrivateKey(key)
                }
                None => break,
                _ => {}
            }
        }

        panic!("no keys found in {:?} (encrypted keys not supported)", path);
    }
}

#[async_trait]
impl Server for Encrypted {
    /// Starts a TLS server
    ///
    /// # Errors
    ///
    /// Returns an error if the cert setup fails
    async fn run_imap(
        config: Arc<Config>,
        file_watcher: broadcast::Sender<Event>,
    ) -> anyhow::Result<()> {
        // Load SSL Keys
        let certs = Encrypted::load_certs(Path::new("certs/cert.pem"));
        let key = Encrypted::load_key(Path::new("certs/key.pem"));

        // Sets up the TLS acceptor.
        let server_config = rustls::ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;

        // Starts a TLS accepting thing.
        let acceptor = TlsAcceptor::from(Arc::new(server_config));

        // Opens the listener
        let listener = TcpListener::bind("0.0.0.0:993").await.unwrap();
        info!("Listening on ecrypted Port");
        let mut stream = TcpListenerStream::new(listener);

        // Looks for new peers
        let file_watcher = file_watcher.clone();
        while let Some(Ok(tcp_stream)) = stream.next().await {
            debug!("Got new TLS peer: {:?}", tcp_stream.peer_addr());
            let peer = tcp_stream.peer_addr().expect("peer addr to exist");

            // Start talking with new peer on new thread
            let acceptor = acceptor.clone();
            let config = Arc::clone(&config);
            let file_watcher = file_watcher.clone();
            tokio::spawn(async move {
                // Accept TCP connection
                let tls_stream = acceptor.accept(tcp_stream).await;

                // Continue if it worked
                match tls_stream {
                    Ok(stream) => {
                        debug!("TLS negotiation done");

                        // Proceed as normal
                        let lines = Framed::new(stream, LinesCodec::new());
                        let (mut lines_sender, mut lines_reader) = lines.split();
                        let capabilities = get_capabilities();

                        lines_sender
                            .send(format!("* OK [{}] IMAP4rev2 Service Ready", capabilities))
                            .await
                            .unwrap();
                        let state = Arc::new(RwLock::new(Connection {
                            state: State::NotAuthenticated,
                            ip: peer.ip(),
                            secure: true,
                            username: None,
                        }));

                        let mut file_watcher_subscriber = file_watcher.subscribe();
                        let (mut tx, mut rx) = mpsc::unbounded();
                        let cloned_tx = tx.clone();
                        tokio::spawn(async move {
                            while let Some(res) = rx.next().await {
                                lines_sender.send(res).await.unwrap();
                            }
                        });
                        let cloned_state = Arc::clone(&state);
                        tokio::spawn(async move {
                            let cloned_state = Arc::clone(&cloned_state);
                            while let Ok(res) = file_watcher_subscriber.recv().await {
                                check_changes(cloned_tx.clone(), Arc::clone(&cloned_state), res)
                                    .await;
                            }
                        });
                        while let Some(Ok(line)) = lines_reader.next().await {
                            let data = Data {
                                command_data: None,
                                // TODO mutex?
                                con_state: Arc::clone(&state),
                            };
                            debug!("[{}] Got Command: {}", peer, line);
                            // TODO make sure to handle IDLE different as it needs us to stream lines
                            // TODO pass lines and make it possible to not need new lines in responds but instead directly use `lines.send`

                            {
                                let response = data.parse(&mut tx, Arc::clone(&config), line).await;
                                match response {
                                    Ok(response) => {
                                        // Cleanup timeout managers
                                        if response {
                                            // Used for later session timer management
                                            debug!("Closing TLS connection");
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
                                        debug!("Closing TLS connection");
                                        break;
                                    }
                                }
                            };
                        }
                    }
                    Err(e) => error!("Got error while accepting TLS: {}", e),
                }
            });
        }
        Ok(())
    }
}

async fn check_changes(
    lines: UnboundedSender<String>,
    state: Arc<RwLock<Connection>>,
    event: Event,
) {
    let state = { state.read().await.state.clone() };
    if state == State::Authenticated || matches!(state, State::Selected(_)) {
        println!("{:?}", event);
    }
}
