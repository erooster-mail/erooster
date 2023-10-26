use erooster_core::line_codec::{LinesCodec, LinesCodecError};
use erooster_deps::{
    color_eyre::Result,
    futures::{Sink, SinkExt, Stream, StreamExt},
    mail_auth::{
        common::{
            crypto::{RsaKey, Sha256},
            headers::HeaderWriter,
        },
        dkim::DkimSigner,
    },
    rustls::{self, OwnedTrustAnchor},
    serde::{self, Deserialize, Serialize},
    tokio::{net::TcpStream, time::timeout},
    tokio_rustls::TlsConnector,
    tokio_util::codec::Framed,
    tracing::{self, debug, error, instrument, warn},
    trust_dns_resolver::TokioAsyncResolver,
    uuid::Uuid,
    webpki_roots,
};
use std::{collections::BTreeMap, error::Error, io, net::IpAddr, path::Path, time::Duration};

#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "self::serde")]
pub struct EmailPayload {
    pub id: Uuid,
    // Map to addresses by domain
    pub to: BTreeMap<String, Vec<String>>,
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
    let private_key = std::fs::read_to_string(Path::new(&dkim_key_path))?;
    let pk_rsa =
        RsaKey::<Sha256>::from_pkcs1_pem(&private_key).expect("Failed to load private key");
    let signature_rsa = DkimSigner::from_key(pk_rsa)
        .domain(domain)
        .selector(dkim_key_selector)
        .headers(["From", "To", "Subject"])
        .sign(raw_email.as_bytes())
        .expect("Failed to sign email");

    Ok(format!("{}{raw_email}", signature_rsa.to_header()))
}

#[allow(clippy::too_many_lines)]
#[instrument(skip(con, email, to))]
async fn send_email<T>(
    con: T,
    email: &EmailPayload,
    to: &Vec<String>,
    tls: bool,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>>
where
    T: Stream<Item = Result<String, LinesCodecError>> + Sink<String, Error = LinesCodecError>,
{
    let (mut lines_sender, mut lines_reader) = con.split();
    debug!(
        "[{}] [{}] Fully Connected. Waiting for response",
        email.id,
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
        email.id,
        if tls { "TLS" } else { "Plain" },
        first
    );
    if !first.starts_with("220") {
        lines_sender.send(String::from("RSET")).await?;
        lines_sender.send(String::from("QUIT")).await?;
        debug!(
            "[{}] [{}] Got full {:?}",
            email.id,
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
        email.id,
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
            email.id,
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
        email.id,
        if tls { "TLS" } else { "Plain" },
    );
    let line = lines_reader
        .next()
        .await
        .ok_or("Server did not respond")??;
    debug!(
        "[{}] [{}] got {}",
        email.id,
        if tls { "TLS" } else { "Plain" },
        line
    );
    if !line.starts_with("250") {
        lines_sender.send(String::from("RSET")).await?;
        lines_sender.send(String::from("QUIT")).await?;
        debug!(
            "[{}] [{}] Got full {:?}",
            email.id,
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
        lines_sender.send(format!("RCPT TO:<{to}>")).await?;
        debug!(
            "[{}] [{}] Sent RCPT TO",
            email.id,
            if tls { "TLS" } else { "Plain" },
        );
        let line = lines_reader
            .next()
            .await
            .ok_or("Server did not respond")??;
        debug!(
            "[{}] [{}] Got {}",
            email.id,
            if tls { "TLS" } else { "Plain" },
            line
        );
        if !line.starts_with("250") && !line.starts_with("550 No such user here") {
            lines_sender.send(String::from("RSET")).await?;
            lines_sender.send(String::from("QUIT")).await?;
            debug!(
                "[{}] [{}] Got full {:?}",
                email.id,
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
        email.id,
        if tls { "TLS" } else { "Plain" },
    );

    let line = lines_reader
        .next()
        .await
        .ok_or("Server did not respond")??;
    debug!(
        "[{}] [{}] Got {}",
        email.id,
        if tls { "TLS" } else { "Plain" },
        line
    );
    if !line.starts_with("354") {
        lines_sender.send(String::from("RSET")).await?;
        lines_sender.send(String::from("QUIT")).await?;

        debug!(
            "[{}] [{}] Got full {:?}",
            email.id,
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
    lines_sender.send(signed_body).await?;
    lines_sender.send(String::from(".")).await?;
    debug!(
        "[{}] [{}] Sent body and ending",
        email.id,
        if tls { "TLS" } else { "Plain" },
    );

    let line = lines_reader
        .next()
        .await
        .ok_or("Server did not respond")??;
    debug!("[{}] Got {}", email.id, line);
    if !line.starts_with("250") {
        lines_sender.send(String::from("RSET")).await?;
        lines_sender.send(String::from("QUIT")).await?;
        debug!(
            "[{}] [{}] Got full {:?}",
            email.id,
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
#[allow(clippy::too_many_lines)]
#[instrument(skip(email))]
pub async fn send_email_job(
    email: &EmailPayload,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    debug!("[{}] Starting to send email job: {}", email.id, email.id);
    // Decode a JSON payload
    debug!("[{}] Found payload", email.id);
    let resolver = TokioAsyncResolver::tokio_from_system_conf()?;

    debug!("[{}] Setup for tls connection done", email.id);
    for (target, to) in &email.to {
        debug!("[{}] Looking up mx records for {}", email.id, target);
        let mx_record_resp = resolver.mx_lookup(target.clone()).await;

        debug!("[{}] Looking up IP records for {}", email.id, target);
        let mut address: Option<IpAddr> = None;
        let mut tls_domain: String = target.to_string();
        let response = resolver.ipv6_lookup(target.clone()).await;
        if let Ok(response) = response {
            address = Some(IpAddr::V6(
                response.iter().next().ok_or("No address found")?.0,
            ));
            debug!("[{}] Got {:?} for {}", email.id, address, target);
        } else {
            debug!("[{}] Looking up A records for {}", email.id, target);
            let response = resolver.ipv4_lookup(target.clone()).await;
            if let Ok(response) = response {
                address = Some(IpAddr::V4(
                    response.iter().next().ok_or("No address found")?.0,
                ));
                debug!("[{}] Got {:?} for {}", email.id, address, target);
            }
        }

        debug!("[{}] Checking mx record results for {}", email.id, target);
        if let Ok(mx_record_resp) = mx_record_resp {
            for record in mx_record_resp {
                debug!(
                    "[{}] Found MX: {} {}",
                    email.id,
                    record.preference(),
                    record.exchange()
                );
                let response = resolver.ipv6_lookup(record.exchange().clone()).await;
                if let Ok(response) = response {
                    address = Some(IpAddr::V6(
                        response.iter().next().ok_or("No address found")?.0,
                    ));
                    let exchange_record = record.exchange().to_utf8();
                    tls_domain = if let Some(record) = exchange_record.strip_suffix('.') {
                        record.to_string()
                    } else {
                        exchange_record
                    };
                    debug!("[{}] Got {:?} for {}", email.id, address, target);
                    break;
                }

                debug!("[{}] Looking up A records for {}", email.id, target);
                let response = resolver.ipv4_lookup(record.exchange().clone()).await;
                if let Ok(response) = response {
                    address = Some(IpAddr::V4(
                        response.iter().next().ok_or("No address found")?.0,
                    ));
                    let exchange_record = record.exchange().to_utf8();
                    tls_domain = if let Some(record) = exchange_record.strip_suffix('.') {
                        record.to_string()
                    } else {
                        exchange_record
                    };
                    debug!("[{}] Got {:?} for {}", email.id, address, target);
                    break;
                }
            }
        }

        if address.is_none() {
            debug!("[{}] No address found for {}", email.id, target);
            continue;
        }
        let Some(address) = address else { continue };

        match get_secure_connection(address, email, target, &tls_domain).await {
            Ok(secure_con) => {
                if let Err(e) = send_email(secure_con, email, to, true).await {
                    warn!(
                        "[{}] Error sending email via tls on port 465 to {}: {}",
                        email.id, target, e
                    );
                    // TODO try starttls first
                    match get_unsecure_connection(address, email, target).await {
                        Ok(unsecure_con) => {
                            if let Err(e) = send_email(unsecure_con, email, to, false).await {
                                return Err(From::from(format!(
                                    "[{}] Error sending email via tcp on port 25 to {}: {}",
                                    email.id, target, e
                                )));
                            }
                        }
                        Err(e) => {
                            return Err(From::from(format!(
                                "[{}] Error sending email via tcp on port 25 to {}: {}",
                                email.id, target, e
                            )));
                        }
                    }
                }
            }
            Err(e) => {
                error!(
                    "[{}] Error sending email via tls on port 465 to {}: {}",
                    email.id, target, e
                );
                // TODO try starttls first
                let unsecure_con = get_unsecure_connection(address, email, target).await?;
                if let Err(e) = send_email(unsecure_con, email, to, false).await {
                    return Err(From::from(format!(
                        "[{}] Error sending email via tcp on port 25 to {}: {}",
                        email.id, target, e
                    )));
                }
            }
        }
    }
    debug!("[{}] Finished sending email job: {}", email.id, email.id);

    Ok(())
}

#[instrument(skip(addr, email, target))]
async fn get_unsecure_connection(
    addr: IpAddr,
    email: &EmailPayload,
    target: &String,
) -> Result<
    impl Stream<Item = Result<String, LinesCodecError>> + Sink<String, Error = LinesCodecError>,
    Box<dyn Error + Send + Sync + 'static>,
> {
    match timeout(Duration::from_secs(5), TcpStream::connect(&(addr, 25))).await {
        Ok(Ok(stream)) => {
            debug!(
                "[{}] Connected to {} via tcp as {:?}",
                email.id,
                target,
                stream.local_addr()?
            );

            Ok(Framed::new(stream, LinesCodec::new()))
        }
        Ok(Err(e)) => Err(format!(
            "[{}] Error connecting to {} via tcp: {}",
            email.id, target, e
        )
        .into()),
        Err(e) => Err(format!(
            "[{}] Error connecting to {} via tcp: {}",
            email.id, target, e
        )
        .into()),
    }
}

#[instrument(skip(addr, target, email, tls_domain))]
async fn get_secure_connection(
    addr: IpAddr,
    email: &EmailPayload,
    target: &str,
    tls_domain: &str,
) -> Result<
    impl Stream<Item = Result<String, LinesCodecError>> + Sink<String, Error = LinesCodecError>,
    Box<dyn Error + Send + Sync + 'static>,
> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.iter().map(|ta| {
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
    let connector = TlsConnector::from(std::sync::Arc::new(config));
    debug!("[{}] Trying tls", email.id,);
    match timeout(Duration::from_secs(5), TcpStream::connect(&(addr, 465))).await {
        Ok(Ok(stream)) => {
            debug!(
                "[{}] Connected to {} via tcp {:?} waiting for tls magic",
                email.id,
                target,
                stream.local_addr()?
            );

            let domain = rustls::ServerName::try_from(tls_domain)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid dnsname"))?;

            let stream = connector.connect(domain, stream).await?;
            debug!("[{}] Connected to {} via tls", email.id, target);

            Ok(Framed::new(stream, LinesCodec::new()))
        }
        Ok(Err(e)) => Err(format!(
            "[{}] Error connecting to {} via tls: {}",
            email.id, target, e
        )
        .into()),
        Err(e) => Err(format!(
            "[{}] Error connecting to {} via tls after {}",
            email.id, target, e
        )
        .into()),
    }
}
