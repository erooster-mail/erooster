use crate::database::DB;
use crate::line_codec::LinesCodec;
use crate::{config::Config, database::Database};
use futures::StreamExt;
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use rustls::OwnedTrustAnchor;
use serde::{Deserialize, Serialize};
use sqlxmq::{job, CurrentJob, JobRegistry, OwnedHandle};
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_util::codec::Framed;
use trust_dns_resolver::TokioAsyncResolver;

pub(crate) mod encrypted;
pub(crate) mod state;
// TODO make this only pub for benches and tests
#[allow(missing_docs)]
pub mod unencrypted;

pub(crate) async fn send_capabilities<S>(
    config: Arc<Config>,
    lines_sender: &mut S,
) -> color_eyre::eyre::Result<()>
where
    S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
{
    lines_sender
        .send(format!("220 {} ESMTP Erooster", config.mail.hostname))
        .await?;
    Ok(())
}

/// Starts the smtp server
///
/// # Errors
///
/// Returns an error if the server startup fails
pub async fn start(config: Arc<Config>, database: DB) -> color_eyre::eyre::Result<OwnedHandle> {
    let config_clone = Arc::clone(&config);
    let db_clone = Arc::clone(&database);
    tokio::spawn(async move {
        if let Err(e) =
            unencrypted::Unencrypted::run(Arc::clone(&config_clone), Arc::clone(&db_clone)).await
        {
            panic!("Unable to start server: {:?}", e);
        }
    });
    let db_clone = Arc::clone(&database);
    tokio::spawn(async move {
        if let Err(e) = encrypted::Encrypted::run(Arc::clone(&config), Arc::clone(&db_clone)).await
        {
            panic!("Unable to start TLS server: {:?}", e);
        }
    });

    let pool = database.get_pool();

    // Construct a job registry from our job.
    let registry = JobRegistry::new(&[send_email_job]);
    // Here is where you can configure the registry
    // registry.set_error_handler(...)

    // And add context
    //registry.set_context("Hello");

    let runner = registry
        // Create a job runner using the connection pool.
        .runner(pool)
        // Here is where you can configure the job runner
        // Aim to keep 10-20 jobs running at a time.
        .set_concurrency(10, 20)
        // Start the job runner in the background.
        .run()
        .await?;
    Ok(runner)
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct EmailPayload {
    // Map to addresses by domain
    pub to: HashMap<String, Vec<String>>,
    pub from: String,
    pub body: String,
    pub sender_domain: String,
}

// Arguments to the `#[job]` attribute allow setting default job options.
#[job]
pub async fn send_email_job(
    // The first argument should always be the current job.
    mut current_job: CurrentJob,
    // Additional arguments are optional, but can be used to access context
    // provided via [`JobRegistry::set_context`].
    _message: &'static str,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    // Decode a JSON payload
    let email: Option<EmailPayload> = current_job.json()?;
    if let Some(email) = email {
        let resolver = TokioAsyncResolver::tokio_from_system_conf()?;
        let mut root_cert_store = rustls::RootCertStore::empty();
        root_cert_store.add_server_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.0.iter().map(
            |ta| {
                OwnedTrustAnchor::from_subject_spki_name_constraints(
                    ta.subject,
                    ta.spki,
                    ta.name_constraints,
                )
            },
        ));
        let config = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_cert_store)
            .with_no_client_auth();

        for (target, to) in email.to {
            let response = resolver.lookup_ip(target.clone()).await?;

            let address = response.iter().next().ok_or("No address found")?;

            let connector = TlsConnector::from(Arc::new(config.clone()));

            let stream = TcpStream::connect(&(address, 465)).await?;

            let domain = rustls::ServerName::try_from(target.as_str()).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid dnsname")
            })?;

            let stream = connector.connect(domain, stream).await?;
            let lines = Framed::new(stream, LinesCodec::new());

            // We split these as we handle the sink in a broadcast instead to be able to push non linear data over the socket
            let (mut lines_sender, mut lines_reader) = lines.split();
            // TODO this is totally dumb code currently.
            // We check if we get a ready status
            let first = lines_reader
                .next()
                .await
                .ok_or("Server did not send ready status")??;
            if !first.starts_with("220") {
                lines_sender.send(String::from("RSET")).await?;
                lines_sender.send(String::from("QUIT")).await?;
                return Err("Server did not send ready status".into());
            }
            // We send EHLO
            lines_sender
                .send(format!("EHLO {}", email.sender_domain))
                .await?;

            // Check if we get greeted and finished all caps
            let mut capabilities_happening = true;
            while capabilities_happening {
                let line = lines_reader
                    .next()
                    .await
                    .ok_or("Server did not respond")??;
                let char_4 = line
                    .chars()
                    .nth(3)
                    .ok_or("Server did not respond as expected")?;
                if char_4 == ' ' {
                    capabilities_happening = false;
                }
            }

            // We send MAIL FROM
            lines_sender
                .send(format!("MAIL FROM:<{}>", email.from))
                .await?;
            let line = lines_reader
                .next()
                .await
                .ok_or("Server did not respond")??;
            if line != "250 OK" {
                lines_sender.send(String::from("RSET")).await?;
                lines_sender.send(String::from("QUIT")).await?;
                return Err("Server did not accept MAIL FROM command".into());
            }

            // We send RCPT TO
            // TODO actually follow spec here. This may be garbage :P
            for to in to {
                lines_sender.send(format!("RCPT TO:<{}>", to)).await?;
                let line = lines_reader
                    .next()
                    .await
                    .ok_or("Server did not respond")??;
                if line != "250 OK" && line != "550 No such user here" {
                    lines_sender.send(String::from("RSET")).await?;
                    lines_sender.send(String::from("QUIT")).await?;
                    return Err("Server did not accept RCPT TO command".into());
                }
            }

            // Send the body
            lines_sender.send(String::from("DATA")).await?;
            // send body end
            lines_sender.send(String::from(".")).await?;

            let line = lines_reader
                .next()
                .await
                .ok_or("Server did not respond")??;
            if !line.starts_with("354") {
                lines_sender.send(String::from("RSET")).await?;
                lines_sender.send(String::from("QUIT")).await?;
                return Err("Server did not accept data command".into());
            }

            // QUIT after sending
            lines_sender.send(String::from("QUIT")).await?;
        }
        // Mark the job as complete
        current_job.complete().await?;
    } else {
        return Err("No email payload found".into());
    }

    Ok(())
}
