use futures::SinkExt;
use tokio::net::TcpListener;
use tokio_stream::{wrappers::TcpListenerStream, StreamExt};
use tokio_util::codec::Framed;
use tracing::{debug, info};

use crate::{
    commands::{capability::get_capabilities, Data, Parser},
    line_codec::LinesCodec,
};

pub async fn run() {
    let listener = TcpListener::bind("0.0.0.0:143").await.unwrap();
    info!("Listening on unecrypted Port");
    let mut stream = TcpListenerStream::new(listener);
    while let Some(Ok(tcp_stream)) = stream.next().await {
        let peer = tcp_stream.peer_addr().expect("peer addr to exist");
        debug!("Got new peer: {}", peer);

        tokio::spawn(async move {
            let mut lines = Framed::new(tcp_stream, LinesCodec::new());
            let capabilities = get_capabilities();
            lines
                .send(format!("* OK [{}] IMAP4rev2 Service Ready", capabilities))
                .await
                .unwrap();
            let mut state = super::ConnectionState {
                state: super::State::NotAuthenticated,
                ip: peer.ip(),
                secure: false,
            };
            while let Some(Ok(line)) = lines.next().await {
                let data = &mut Data {
                    command_data: None,
                    con_state: &mut state,
                };

                debug!("[{}] Got Command: {}", peer, line);

                // TODO make sure to handle IDLE different as it needs us to stream lines
                // TODO pass lines and make it possible to not need new lines in responds but instead directly use `lines.send`
                let response = data.parse(&mut lines, line).await;

                match response {
                    Ok(response) => {
                        // Cleanup timeout managers
                        if response {
                            // Used for later session timer management
                            debug!("Closing connection");
                            break;
                        }
                    }
                    // We try a last time to do a graceful shutdown before closing
                    Err(e) => {
                        lines
                            .send(format!("* BAD [SERVERBUG] This should not happen: {}", e))
                            .await
                            .unwrap();
                        debug!("Closing connection");
                        break;
                    }
                }
            }
        });
    }
}
