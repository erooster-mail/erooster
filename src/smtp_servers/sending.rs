use std::{collections::HashMap, error::Error, io, net::IpAddr, sync::Arc};

use futures::{Sink, SinkExt, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use sqlxmq::{job, CurrentJob};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_util::codec::Framed;
use tracing::{debug, error};
use trust_dns_resolver::TokioAsyncResolver;

use crate::line_codec::{LinesCodec, LinesCodecError};

#[derive(Debug, Serialize, Deserialize)]
pub struct EmailPayload {
    // Map to addresses by domain
    pub to: HashMap<String, Vec<String>>,
    pub from: String,
    pub body: String,
    pub sender_domain: String,
}

#[allow(clippy::too_many_lines)]
async fn send_email<T>(
    con: T,
    email: &EmailPayload,
    current_job: &CurrentJob,
    to: &Vec<String>,
    tls: bool,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>>
where
    T: Stream<Item = Result<String, LinesCodecError>> + Sink<String, Error = LinesCodecError>,
{
    let (mut lines_sender, mut lines_reader) = con.split();
    debug!(
        "[{}] [{}] Fully Connected. Waiting for response",
        current_job.id(),
        if tls { "TLS" } else { "Plain" }
    );
    // TODO this is totally dumb code currently.
    // We check if we get a ready status
    let first = lines_reader
        .next()
        .await
        .ok_or("Server did not send ready status")??;

    debug!(
        "[{}] [{}] Got greeting: {}",
        current_job.id(),
        if tls { "TLS" } else { "Plain" },
        first
    );
    if !first.starts_with("220") {
        lines_sender.send(String::from("RSET")).await?;
        lines_sender.send(String::from("QUIT")).await?;
        debug!(
            "[{}] [{}] Got full {:?}",
            current_job.id(),
            if tls { "TLS" } else { "Plain" },
            lines_reader
                .filter_map(|x| async move { x.ok() })
                .collect::<Vec<String>>()
                .await
        );
        return Err("Server did not send ready status".into());
    }
    // We send EHLO
    lines_sender
        .send(format!("EHLO {}", email.sender_domain))
        .await?;

    debug!(
        "[{}] [{}] Sent EHLO",
        current_job.id(),
        if tls { "TLS" } else { "Plain" },
    );
    // Check if we get greeted and finished all caps
    let mut capabilities_happening = true;
    while capabilities_happening {
        let line = lines_reader
            .next()
            .await
            .ok_or("Server did not respond")??;
        debug!(
            "[{}] [{}] Got: {}",
            current_job.id(),
            if tls { "TLS" } else { "Plain" },
            line
        );
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
    debug!(
        "[{}] [{}] Sent MAIL FROM",
        current_job.id(),
        if tls { "TLS" } else { "Plain" },
    );
    let line = lines_reader
        .next()
        .await
        .ok_or("Server did not respond")??;
    debug!(
        "[{}] [{}] got {}",
        current_job.id(),
        if tls { "TLS" } else { "Plain" },
        line
    );
    if !line.starts_with("250") {
        lines_sender.send(String::from("RSET")).await?;
        lines_sender.send(String::from("QUIT")).await?;
        debug!(
            "[{}] [{}] Got full {:?}",
            current_job.id(),
            if tls { "TLS" } else { "Plain" },
            lines_reader
                .filter_map(|x| async move { x.ok() })
                .collect::<Vec<String>>()
                .await
        );
        return Err("Server did not accept MAIL FROM command".into());
    }

    // We send RCPT TO
    // TODO actually follow spec here. This may be garbage :P
    for to in to {
        lines_sender.send(format!("RCPT TO:<{}>", to)).await?;
        debug!(
            "[{}] [{}] Sent RCPT TO",
            current_job.id(),
            if tls { "TLS" } else { "Plain" },
        );
        let line = lines_reader
            .next()
            .await
            .ok_or("Server did not respond")??;
        debug!(
            "[{}] [{}] Got {}",
            current_job.id(),
            if tls { "TLS" } else { "Plain" },
            line
        );
        if !line.starts_with("250") && !line.starts_with("550 No such user here") {
            lines_sender.send(String::from("RSET")).await?;
            lines_sender.send(String::from("QUIT")).await?;
            debug!(
                "[{}] [{}] Got full {:?}",
                current_job.id(),
                if tls { "TLS" } else { "Plain" },
                lines_reader
                    .filter_map(|x| async move { x.ok() })
                    .collect::<Vec<String>>()
                    .await
            );
            return Err("Server did not accept RCPT TO command".into());
        }
    }

    // Send the body
    lines_sender.send(String::from("DATA")).await?;
    debug!(
        "[{}] [{}] Sent DATA",
        current_job.id(),
        if tls { "TLS" } else { "Plain" },
    );

    let line = lines_reader
        .next()
        .await
        .ok_or("Server did not respond")??;
    debug!(
        "[{}] [{}] Got {}",
        current_job.id(),
        if tls { "TLS" } else { "Plain" },
        line
    );
    if !line.starts_with("354") {
        lines_sender.send(String::from("RSET")).await?;
        lines_sender.send(String::from("QUIT")).await?;

        debug!(
            "[{}] [{}] Got full {:?}",
            current_job.id(),
            if tls { "TLS" } else { "Plain" },
            lines_reader
                .filter_map(|x| async move { x.ok() })
                .collect::<Vec<String>>()
                .await
        );
        return Err("Server did not accept data start command".into());
    }

    lines_sender.send(email.body.clone()).await?;
    debug!(
        "[{}] [{}] Sent body and ending",
        current_job.id(),
        if tls { "TLS" } else { "Plain" },
    );

    let line = lines_reader
        .next()
        .await
        .ok_or("Server did not respond")??;
    debug!("[{}] Got {}", current_job.id(), line);
    if !line.starts_with("250") {
        lines_sender.send(String::from("RSET")).await?;
        lines_sender.send(String::from("QUIT")).await?;
        debug!(
            "[{}] [{}] Got full {:?}",
            current_job.id(),
            if tls { "TLS" } else { "Plain" },
            lines_reader
                .filter_map(|x| async move { x.ok() })
                .collect::<Vec<String>>()
                .await
        );
        return Err("Server did not accept data command".into());
    }

    // QUIT after sending
    lines_sender.send(String::from("QUIT")).await?;
    Ok(())
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
        for (target, to) in &email.to {
            debug!(
                "[{}] Looking up mx records for {}",
                current_job.id(),
                target
            );
            let mx_record_resp = resolver.mx_lookup(target.clone()).await;

            debug!(
                "[{}] Looking up IP records for {}",
                current_job.id(),
                target
            );
            let mut address: Option<IpAddr> = None;
            let response = resolver.ipv6_lookup(target.clone()).await;
            if let Ok(response) = response {
                address = Some(IpAddr::V6(
                    *response.iter().next().ok_or("No address found")?,
                ));
                debug!("[{}] Got {:?} for {}", current_job.id(), address, target);
            } else {
                debug!("[{}] Looking up A records for {}", current_job.id(), target);
                let response = resolver.ipv4_lookup(target.clone()).await;
                if let Ok(response) = response {
                    address = Some(IpAddr::V4(
                        *response.iter().next().ok_or("No address found")?,
                    ));
                    debug!("[{}] Got {:?} for {}", current_job.id(), address, target);
                }
            }

            debug!(
                "[{}] Checking mx record results for {}",
                current_job.id(),
                target
            );
            if let Ok(mx_record_resp) = mx_record_resp {
                for record in mx_record_resp {
                    debug!(
                        "[{}] Found MX: {} {}",
                        current_job.id(),
                        record.preference(),
                        record.exchange()
                    );
                    let response = resolver.ipv6_lookup(record.exchange().clone()).await;
                    if let Ok(response) = response {
                        address = Some(IpAddr::V6(
                            *response.iter().next().ok_or("No address found")?,
                        ));
                        debug!("[{}] Got {:?} for {}", current_job.id(), address, target);
                    } else {
                        debug!("[{}] Looking up A records for {}", current_job.id(), target);
                        let response = resolver.ipv4_lookup(record.exchange().clone()).await;
                        if let Ok(response) = response {
                            address = Some(IpAddr::V4(
                                *response.iter().next().ok_or("No address found")?,
                            ));
                            debug!("[{}] Got {:?} for {}", current_job.id(), address, target);
                        }
                    }
                }
            }

            if matches!(address, None) {
                debug!("[{}] No address found for {}", current_job.id(), target);
                continue;
            }

            if let Ok(secure_con) =
                get_secure_connection(address.unwrap(), &current_job, target).await
            {
                if let Err(e) = send_email(secure_con, &email, &current_job, to, true).await {
                    error!(
                        "[{}] Error sending email via tls on port 465: {}",
                        current_job.id(),
                        e
                    );
                    match get_unsecure_connection(address.unwrap(), &current_job, target).await {
                        Ok(unsecure_con) => {
                            if let Err(e) =
                                send_email(unsecure_con, &email, &current_job, to, false).await
                            {
                                error!(
                                    "[{}] Error sending email via tcp on port 25: {}",
                                    current_job.id(),
                                    e
                                );
                            }
                        }
                        Err(e) => {
                            error!(
                                "[{}] Error sending email via tcp on port 25: {}",
                                current_job.id(),
                                e
                            );
                        }
                    }
                }
            } else {
                let unsecure_con =
                    get_unsecure_connection(address.unwrap(), &current_job, target).await?;
                if let Err(e) = send_email(unsecure_con, &email, &current_job, to, false).await {
                    error!(
                        "[{}] Error sending email via tcp on port 25: {}",
                        current_job.id(),
                        e
                    );
                }
            }
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

async fn get_unsecure_connection(
    addr: IpAddr,
    current_job: &CurrentJob,
    target: &String,
) -> Result<
    impl Stream<Item = Result<String, LinesCodecError>> + Sink<String, Error = LinesCodecError>,
    Box<dyn Error + Send + Sync + 'static>,
> {
    let stream = TcpStream::connect(&(addr, 25)).await?;
    debug!(
        "[{}] Connected to {} via tcp as {:?}",
        current_job.id(),
        target,
        stream.local_addr()?
    );

    Ok(Framed::new(stream, LinesCodec::new()))
}

async fn get_secure_connection(
    addr: IpAddr,
    current_job: &CurrentJob,
    target: &String,
) -> Result<
    impl Stream<Item = Result<String, LinesCodecError>> + Sink<String, Error = LinesCodecError>,
    Box<dyn Error + Send + Sync + 'static>,
> {
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().expect("could not load platform certs") {
        roots.add(&rustls::Certificate(cert.0)).unwrap();
    }
    let config = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));

    let stream = TcpStream::connect(&(addr, 465)).await?;
    debug!(
        "[{}] Connected to {} via tcp {:?} waiting for tls magic",
        current_job.id(),
        target,
        stream.local_addr()?
    );

    let domain = rustls::ServerName::try_from(target.as_str())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid dnsname"))?;

    let stream = connector.connect(domain, stream).await?;
    debug!("[{}] Connected to {} via tls", current_job.id(), target);

    Ok(Framed::new(stream, LinesCodec::new()))
}
