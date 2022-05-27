use crate::database::DB;
use crate::line_codec::LinesCodec;
use crate::{config::Config, database::Database};
use futures::StreamExt;
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use serde::{Deserialize, Serialize};
use sqlxmq::{job, CurrentJob, JobRegistry, OwnedHandle};
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_util::codec::Framed;
use tracing::debug;
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
    let mut registry = JobRegistry::new(&[send_email_job]);
    // Here is where you can configure the registry
    registry.set_error_handler(|name: &str, error: Box<dyn Error + Send + 'static>| {
        tracing::error!("Job `{}` failed: {}", name, error);
    });

    // And add context
    registry.set_context("");

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
#[job(retries = 3, backoff_secs = 1200)]
#[allow(clippy::too_many_lines)]
pub async fn send_email_job(
    // The first argument should always be the current job.
    mut current_job: CurrentJob,
    // Additional arguments are optional, but can be used to access context
    // provided via [`JobRegistry::set_context`].
    _message: &'static str,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    debug!(
        "[{}] Starting to send email job: {}",
        current_job.id(),
        current_job.id()
    );
    // Decode a JSON payload
    let email: Option<EmailPayload> = current_job.json()?;
    if let Some(email) = email {
        debug!("[{}] Found payload", current_job.id());
        let resolver = TokioAsyncResolver::tokio_from_system_conf()?;

        debug!("[{}] Setup for tls connection done", current_job.id());
        for (target, to) in email.to {
            debug!("[{}] Looking up {}", current_job.id(), target);
            let mx_record_resp = resolver.mx_lookup(target.clone()).await?;

            let response = resolver.lookup_ip(target.clone()).await?;

            let mut address = response.iter().next().ok_or("No address found")?;
            for record in mx_record_resp {
                println!(
                    "[{}] Found MX: {} {}",
                    current_job.id(),
                    record.preference(),
                    record.exchange()
                );
                let lookup_response = resolver.lookup_ip(record.exchange().clone()).await;
                if let Ok(lookup_response) = lookup_response {
                    let mut ip_addrs = lookup_response.iter();
                    if let Some(ip_addrs) = ip_addrs.next() {
                        address = ip_addrs;
                    }
                }
            }

            debug!("[{}] Got {} for {}", current_job.id(), address, target);

            // let stream = TcpStream::connect(&(address, 465)).await?;
            let stream = TcpStream::connect(&(address, 25)).await?;
            debug!("[{}] Connected to {} via tcp", current_job.id(), target);

            let lines = Framed::new(stream, LinesCodec::new());

            // We split these as we handle the sink in a broadcast instead to be able to push non linear data over the socket
            let (mut lines_sender, mut lines_reader) = lines.split();

            debug!(
                "[{}] Fully Connected. Waiting for response",
                current_job.id()
            );
            // TODO this is totally dumb code currently.
            // We check if we get a ready status
            let first = lines_reader
                .next()
                .await
                .ok_or("Server did not send ready status")??;

            debug!("[{}] Got greeting: {}", current_job.id(), first);
            if !first.starts_with("220") {
                lines_sender.send(String::from("RSET")).await?;
                lines_sender.send(String::from("QUIT")).await?;
                return Err("Server did not send ready status".into());
            }
            // We send EHLO
            lines_sender
                .send(format!("EHLO {}", email.sender_domain))
                .await?;

            debug!("[{}] Sent EHLO", current_job.id());
            // Check if we get greeted and finished all caps
            let mut capabilities_happening = true;
            while capabilities_happening {
                let line = lines_reader
                    .next()
                    .await
                    .ok_or("Server did not respond")??;
                debug!("[{}] Got: {}", current_job.id(), line);
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
            debug!("[{}] Sent MAIL FROM", current_job.id());
            let line = lines_reader
                .next()
                .await
                .ok_or("Server did not respond")??;
            debug!("[{}] got {}", current_job.id(), line);
            if !line.starts_with("250") {
                lines_sender.send(String::from("RSET")).await?;
                lines_sender.send(String::from("QUIT")).await?;
                return Err("Server did not accept MAIL FROM command".into());
            }

            // We send RCPT TO
            // TODO actually follow spec here. This may be garbage :P
            for to in to {
                lines_sender.send(format!("RCPT TO:<{}>", to)).await?;
                debug!("[{}] Sent RCPT TO", current_job.id());
                let line = lines_reader
                    .next()
                    .await
                    .ok_or("Server did not respond")??;
                debug!("[{}] Got {}", current_job.id(), line);
                if !line.starts_with("250") && !line.starts_with("550") {
                    lines_sender.send(String::from("RSET")).await?;
                    lines_sender.send(String::from("QUIT")).await?;
                    return Err("Server did not accept RCPT TO command".into());
                }
            }

            // Send the body
            lines_sender.send(String::from("DATA")).await?;
            debug!("[{}] Sent DATA", current_job.id());

            let line = lines_reader
                .next()
                .await
                .ok_or("Server did not respond")??;
            debug!("[{}] Got {}", current_job.id(), line);
            if !line.starts_with("354") {
                lines_sender.send(String::from("RSET")).await?;
                lines_sender.send(String::from("QUIT")).await?;
                return Err("Server did not accept data command".into());
            }

            lines_sender.send(email.body.clone()).await?;
            debug!("[{}] Sent body and ending", current_job.id());

            let line = lines_reader
                .next()
                .await
                .ok_or("Server did not respond")??;
            debug!("[{}] Got {}", current_job.id(), line);
            if !line.starts_with("250") {
                lines_sender.send(String::from("RSET")).await?;
                lines_sender.send(String::from("QUIT")).await?;
                return Err("Server did not accept data command".into());
            }

            // QUIT after sending
            lines_sender.send(String::from("QUIT")).await?;
        }
        debug!(
            "[{}] Finished sending email job: {}",
            current_job.id(),
            current_job.id()
        );
        // Mark the job as complete
        current_job.complete().await?;
    } else {
        debug!("[{}] Something broken", current_job.id());
        return Err("No email payload found".into());
    }

    Ok(())
}
