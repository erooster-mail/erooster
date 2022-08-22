use cfdkim::{DkimPrivateKey, SignerBuilder};
use color_eyre::Result;
use erooster_core::line_codec::{LinesCodec, LinesCodecError};
use futures::{Sink, SinkExt, Stream, StreamExt};
use rsa::pkcs1::DecodeRsaPrivateKey;
use rustls::OwnedTrustAnchor;
use serde::{Deserialize, Serialize};
use simdutf8::compat::from_utf8;
use sqlxmq::{job, CurrentJob};
use std::{
    collections::HashMap, error::Error, io, net::IpAddr, path::Path, sync::Arc, time::Duration,
};
use tokio::{net::TcpStream, time::timeout};
use tokio_rustls::TlsConnector;
use tokio_util::codec::Framed;
use tracing::{debug, error, instrument};
use trust_dns_resolver::TokioAsyncResolver;

#[derive(Debug, Serialize, Deserialize)]
pub struct EmailPayload {
    // Map to addresses by domain
    pub to: HashMap<String, Vec<String>>,
    pub from: String,
    pub body: String,
    pub sender_domain: String,
    pub dkim_key_path: String,
    pub dkim_key_selector: String,
}

fn dkim_sign(
    domain: &str,
    raw_email: &str,
    dkim_key_path: &str,
    dkim_key_selector: &str,
) -> Result<String> {
    let email = mailparse::parse_mail(raw_email.as_bytes())?;
    debug!("raw_bytes: {:?}", from_utf8(email.raw_bytes)?);

    let private_key = rsa::RsaPrivateKey::read_pkcs1_pem_file(Path::new(&dkim_key_path))?;
    let time = time::OffsetDateTime::now_utc();

    let signer = SignerBuilder::new()
        .with_signed_headers(&[
            "From",
            "Reply-To",
            "Subject",
            "Date",
            "To",
            "CC",
            "Resent-Date",
            "Resent-From",
            "Resent-To",
            "Resent-Cc",
            "In-Reply-To",
            "References",
            "List-Id",
            "List-Help",
            "List-Unsubscribe",
            "List-Subscribe",
            "List-Post",
            "List-Owner",
            "List-Archive",
        ])?
        .with_body_canonicalization(cfdkim::canonicalization::Type::Relaxed)
        .with_header_canonicalization(cfdkim::canonicalization::Type::Relaxed)
        .with_private_key(DkimPrivateKey::Rsa(private_key))
        .with_selector(dkim_key_selector)
        .with_signing_domain(domain)
        .with_time(time)
        .build()?;
    let header = signer.sign(&email)?;

    Ok(format!("{}\r\n{}", header, raw_email))
}

#[allow(clippy::too_many_lines)]
#[instrument(skip(con, email, current_job, to))]
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

    let signed_body = dkim_sign(
        &email.sender_domain,
        &email.body,
        &email.dkim_key_path,
        &email.dkim_key_selector,
    )?;
    ///////////////////////////////////
    lines_sender.send(signed_body).await?;
    lines_sender.send(String::from(".")).await?;
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

// Note this is a hack to get max retries. Please fix this
#[job(retries = 4294967295, backoff_secs = 1200)]
#[allow(clippy::too_many_lines)]
#[instrument(skip(current_job, _message))]
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
            let mut tls_domain: String = target.to_string();
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
                        let exchange_record = record.exchange().to_utf8();
                        tls_domain = if let Some(record) = exchange_record.strip_suffix('.') {
                            record.to_string()
                        } else {
                            exchange_record
                        };
                        debug!("[{}] Got {:?} for {}", current_job.id(), address, target);
                        break;
                    }

                    debug!("[{}] Looking up A records for {}", current_job.id(), target);
                    let response = resolver.ipv4_lookup(record.exchange().clone()).await;
                    if let Ok(response) = response {
                        address = Some(IpAddr::V4(
                            *response.iter().next().ok_or("No address found")?,
                        ));
                        let exchange_record = record.exchange().to_utf8();
                        tls_domain = if let Some(record) = exchange_record.strip_suffix('.') {
                            record.to_string()
                        } else {
                            exchange_record
                        };
                        debug!("[{}] Got {:?} for {}", current_job.id(), address, target);
                        break;
                    }
                }
            }

            if matches!(address, None) {
                debug!("[{}] No address found for {}", current_job.id(), target);
                continue;
            }

            match get_secure_connection(address.unwrap(), &current_job, target, &tls_domain).await {
                Ok(secure_con) => {
                    if let Err(e) = send_email(secure_con, &email, &current_job, to, true).await {
                        error!(
                            "[{}] Error sending email via tls on port 465: {}",
                            current_job.id(),
                            e
                        );
                        match get_unsecure_connection(address.unwrap(), &current_job, target).await
                        {
                            Ok(unsecure_con) => {
                                if let Err(e) =
                                    send_email(unsecure_con, &email, &current_job, to, false).await
                                {
                                    return Err(From::from(format!(
                                        "[{}] Error sending email via tcp on port 25: {}",
                                        current_job.id(),
                                        e
                                    )));
                                }
                            }
                            Err(e) => {
                                return Err(From::from(format!(
                                    "[{}] Error sending email via tcp on port 25: {}",
                                    current_job.id(),
                                    e
                                )));
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(
                        "[{}] Error sending email via tls on port 465: {}",
                        current_job.id(),
                        e
                    );
                    let unsecure_con =
                        get_unsecure_connection(address.unwrap(), &current_job, target).await?;
                    if let Err(e) = send_email(unsecure_con, &email, &current_job, to, false).await
                    {
                        return Err(From::from(format!(
                            "[{}] Error sending email via tcp on port 25: {}",
                            current_job.id(),
                            e
                        )));
                    }
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

#[instrument(skip(addr, current_job, target))]
async fn get_unsecure_connection(
    addr: IpAddr,
    current_job: &CurrentJob,
    target: &String,
) -> Result<
    impl Stream<Item = Result<String, LinesCodecError>> + Sink<String, Error = LinesCodecError>,
    Box<dyn Error + Send + Sync + 'static>,
> {
    match timeout(Duration::from_secs(5), TcpStream::connect(&(addr, 25))).await {
        Ok(Ok(stream)) => {
            debug!(
                "[{}] Connected to {} via tcp as {:?}",
                current_job.id(),
                target,
                stream.local_addr()?
            );

            Ok(Framed::new(stream, LinesCodec::new()))
        }
        Ok(Err(e)) => Err(format!(
            "[{}] Error connecting to {} via tcp: {}",
            current_job.id(),
            target,
            e
        )
        .into()),
        Err(e) => Err(format!(
            "[{}] Error connecting to {} via tcp: {}",
            current_job.id(),
            target,
            e
        )
        .into()),
    }
}

#[instrument(skip(addr, target, current_job, tls_domain))]
async fn get_secure_connection(
    addr: IpAddr,
    current_job: &CurrentJob,
    target: &str,
    tls_domain: &str,
) -> Result<
    impl Stream<Item = Result<String, LinesCodecError>> + Sink<String, Error = LinesCodecError>,
    Box<dyn Error + Send + Sync + 'static>,
> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add_server_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| {
        OwnedTrustAnchor::from_subject_spki_name_constraints(
            ta.subject,
            ta.spki,
            ta.name_constraints,
        )
    }));
    let config = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));
    debug!("[{}] Trying tls", current_job.id(),);
    match timeout(Duration::from_secs(5), TcpStream::connect(&(addr, 465))).await {
        Ok(Ok(stream)) => {
            debug!(
                "[{}] Connected to {} via tcp {:?} waiting for tls magic",
                current_job.id(),
                target,
                stream.local_addr()?
            );

            let domain = rustls::ServerName::try_from(tls_domain)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid dnsname"))?;

            let stream = connector.connect(domain, stream).await?;
            debug!("[{}] Connected to {} via tls", current_job.id(), target);

            Ok(Framed::new(stream, LinesCodec::new()))
        }
        Ok(Err(e)) => Err(format!(
            "[{}] Error connecting to {} via tls: {}",
            current_job.id(),
            target,
            e
        )
        .into()),
        Err(e) => Err(format!(
            "[{}] Error connecting to {} via tls after {}",
            current_job.id(),
            target,
            e
        )
        .into()),
    }
}
