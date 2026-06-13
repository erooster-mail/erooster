// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use super::dane::{fetch_tlsa_records, validate_cert_against_tlsa};
use super::mta_sts::{fetch_mta_sts_policy, mx_allowed_by_policy, MtaStsMode};
use erooster_core::line_codec::LinesCodec;
use {
    color_eyre::{self, Result},
    futures::{SinkExt, StreamExt},
    hickory_resolver::{proto::rr::RData, TokioResolver},
    mail_auth::{
        common::{
            crypto::{RsaKey, Sha256},
            headers::HeaderWriter,
        },
        dkim::DkimSigner,
    },
    rustls::{
        self,
        pki_types::{pem::PemObject, PrivateKeyDer, PrivatePkcs1KeyDer, ServerName},
        RootCertStore,
    },
    serde::{self, Deserialize, Serialize},
    tokio::{
        io::{AsyncRead, AsyncWrite},
        net::TcpStream,
        time::timeout,
    },
    tokio_rustls::TlsConnector,
    tokio_util::codec::Framed,
    tracing::{debug, instrument, warn},
    uuid::Uuid,
    webpki_roots,
};
use std::{collections::BTreeMap, error::Error, io, net::IpAddr, path::Path, time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "self::serde")]
pub struct EmailPayload {
    pub id: Uuid,
    /// Recipients grouped by their destination domain.
    pub to: BTreeMap<String, Vec<String>>,
    pub from: String,
    pub body: String,
    pub sender_domain: String,
    pub dkim_key_path: String,
    pub dkim_key_selector: String,
    /// When true, delivery MUST use TLS (RFC 8689). Defaults to false for
    /// backwards-compatibility with serialized payloads that predate this field.
    #[serde(default)]
    pub require_tls: bool,
}

trait AsyncReadWrite: AsyncRead + AsyncWrite + Send {}
impl<T: AsyncRead + AsyncWrite + Send> AsyncReadWrite for T {}
type DynStream = Box<dyn AsyncReadWrite + Unpin>;
type DynFramed = Framed<DynStream, LinesCodec>;

fn dkim_sign(
    domain: &str,
    raw_email: &str,
    dkim_key_path: &str,
    dkim_key_selector: &str,
) -> Result<String> {
    let private_key = std::fs::read_to_string(Path::new(&dkim_key_path))?;
    let pk_rsa = RsaKey::<Sha256>::from_key_der(
        PrivateKeyDer::Pkcs1(
            PrivatePkcs1KeyDer::from_pem_slice(private_key.as_bytes())
                .map_err(|e| color_eyre::eyre::eyre!("Failed to parse DKIM PEM: {e}"))?,
        ),
    )
    .map_err(|e| color_eyre::eyre::eyre!("Failed to load DKIM private key: {e:?}"))?;
    let signature_rsa = DkimSigner::from_key(pk_rsa)
        .domain(domain)
        .selector(dkim_key_selector)
        .headers(["From", "To", "Subject"])
        .sign(raw_email.as_bytes())
        .map_err(|e| color_eyre::eyre::eyre!("Failed to sign email: {e:?}"))?;

    Ok(format!("{}{raw_email}", signature_rsa.to_header()))
}

/// Builds a `TlsConnector` backed by the system root certificates.
fn tls_connector() -> TlsConnector {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConnector::from(std::sync::Arc::new(config))
}

/// Outcome of a port-25 connection attempt.
struct Port25Connection {
    framed: DynFramed,
    /// True when STARTTLS was successfully negotiated.
    is_tls: bool,
    /// True when the remote server advertised REQUIRETLS in its post-STARTTLS
    /// EHLO (RFC 8689 §4).  Always false when `is_tls` is false.
    requiretls_advertised: bool,
    /// DER-encoded leaf certificate presented by the server during TLS handshake.
    /// None when `is_tls` is false.
    peer_cert_der: Option<Vec<u8>>,
}

/// Connects to the remote host on port 25, negotiates STARTTLS when available
/// (RFC 3207), and returns a stream positioned after the EHLO exchange, ready
/// for MAIL FROM.
#[allow(clippy::too_many_lines)]
#[instrument(skip(addr, email, tls_domain))]
async fn connect_with_starttls(
    addr: IpAddr,
    email: &EmailPayload,
    tls_domain: &str,
) -> Result<Port25Connection, Box<dyn Error + Send + Sync + 'static>> {
    let tcp = match timeout(
        Duration::from_secs(10),
        TcpStream::connect((addr, 25u16)),
    )
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(e.into()),
        Err(_) => return Err("Connection to port 25 timed out".into()),
    };
    debug!("[{}] Connected to {} port 25", email.id, tls_domain);

    let plain = Framed::new(tcp, LinesCodec::new());
    let (mut sender, mut reader) = plain.split();

    // Read server greeting (220)
    let greeting = reader
        .next()
        .await
        .ok_or("Server did not send a greeting")??;
    debug!("[{}] Greeting: {}", email.id, greeting);
    if !greeting.starts_with("220") {
        return Err(format!("Unexpected SMTP greeting: {greeting}").into());
    }

    // Send EHLO and collect capability lines
    sender
        .send(format!("EHLO {}", email.sender_domain))
        .await?;
    let mut starttls_available = false;
    loop {
        let line = reader
            .next()
            .await
            .ok_or("Server did not respond to EHLO")??;
        debug!("[{}] EHLO: {}", email.id, line);
        if !line.starts_with("250") {
            return Err(format!("EHLO rejected: {line}").into());
        }
        // Capability keyword is after "250-" or "250 "
        if line.len() > 4 && line[4..].eq_ignore_ascii_case("STARTTLS") {
            starttls_available = true;
        }
        // The final EHLO line uses a space instead of a hyphen after the code
        if line.len() > 3 && line.as_bytes()[3] == b' ' {
            break;
        }
    }

    if starttls_available {
        sender.send(String::from("STARTTLS")).await?;
        let starttls_resp = reader
            .next()
            .await
            .ok_or("No response to STARTTLS command")??;
        debug!("[{}] STARTTLS: {}", email.id, starttls_resp);

        if starttls_resp.starts_with("220") {
            // Extract the underlying TcpStream and wrap in TLS
            let plain_framed = sender
                .reunite(reader)
                .map_err(|e| -> Box<dyn Error + Send + Sync + 'static> {
                    e.to_string().into()
                })?;
            let tcp = plain_framed.into_inner();

            let connector = tls_connector();
            let domain = ServerName::try_from(tls_domain)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid dnsname"))?
                .to_owned();
            let tls_stream = connector.connect(domain, tcp).await?;
            debug!("[{}] STARTTLS upgrade complete for {}", email.id, tls_domain);

            // Extract leaf certificate before consuming the stream into Framed.
            let peer_cert_der = tls_stream
                .get_ref()
                .1
                .peer_certificates()
                .and_then(|certs| certs.first())
                .map(|cert| cert.to_vec());

            let tls_framed: DynFramed =
                Framed::new(Box::new(tls_stream) as DynStream, LinesCodec::new());
            let (mut tls_sender, mut tls_reader) = tls_framed.split();

            // RFC 3207 §4: client MUST re-issue EHLO after STARTTLS.
            // RFC 8689 §4: sender MUST verify that the remote advertises
            // REQUIRETLS in this post-STARTTLS EHLO before delivering a
            // message with the REQUIRETLS flag.
            tls_sender
                .send(format!("EHLO {}", email.sender_domain))
                .await?;
            let mut requiretls_advertised = false;
            loop {
                let line = tls_reader
                    .next()
                    .await
                    .ok_or("No EHLO response after STARTTLS")??;
                debug!("[{}] Post-STARTTLS EHLO: {}", email.id, line);
                if !line.starts_with("250") {
                    return Err(format!("EHLO rejected after STARTTLS: {line}").into());
                }
                if line.len() > 4 && line[4..].eq_ignore_ascii_case("REQUIRETLS") {
                    requiretls_advertised = true;
                }
                if line.len() > 3 && line.as_bytes()[3] == b' ' {
                    break;
                }
            }

            let tls_rejoined = tls_sender
                .reunite(tls_reader)
                .map_err(|e| -> Box<dyn Error + Send + Sync + 'static> {
                    e.to_string().into()
                })?;
            return Ok(Port25Connection {
                framed: tls_rejoined,
                is_tls: true,
                requiretls_advertised,
                peer_cert_der,
            });
        }
    }

    // No STARTTLS or server declined — return the plain connection.
    // REQUIRETLS cannot be satisfied on a plain connection.
    let plain_framed = sender
        .reunite(reader)
        .map_err(|e| -> Box<dyn Error + Send + Sync + 'static> { e.to_string().into() })?;
    let tcp = plain_framed.into_inner();
    let boxed: DynFramed = Framed::new(Box::new(tcp) as DynStream, LinesCodec::new());
    Ok(Port25Connection {
        framed: boxed,
        is_tls: false,
        requiretls_advertised: false,
        peer_cert_der: None,
    })
}

/// Delivers a message to all recipients using a stream that is positioned
/// immediately after the EHLO exchange (i.e. ready to accept MAIL FROM).
#[instrument(skip(framed, email, to))]
async fn smtp_deliver(
    framed: DynFramed,
    email: &EmailPayload,
    to: &[String],
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    let (mut sender, mut reader) = framed.split();

    sender
        .send(format!("MAIL FROM:<{}>", email.from))
        .await?;
    let line = reader
        .next()
        .await
        .ok_or("No response to MAIL FROM")??;
    debug!("[{}] MAIL FROM: {}", email.id, line);
    if !line.starts_with("250") {
        sender.send(String::from("QUIT")).await?;
        return Err(format!("MAIL FROM rejected: {line}").into());
    }

    for addr in to {
        sender.send(format!("RCPT TO:<{addr}>")).await?;
        let line = reader
            .next()
            .await
            .ok_or("No response to RCPT TO")??;
        debug!("[{}] RCPT TO <{}>: {}", email.id, addr, line);
        // 251 = user not local, but will forward — still acceptable
        if !line.starts_with("250") && !line.starts_with("251") {
            sender.send(String::from("QUIT")).await?;
            return Err(format!("RCPT TO rejected for {addr}: {line}").into());
        }
    }

    sender.send(String::from("DATA")).await?;
    let line = reader
        .next()
        .await
        .ok_or("No response to DATA")??;
    debug!("[{}] DATA: {}", email.id, line);
    if !line.starts_with("354") {
        sender.send(String::from("QUIT")).await?;
        return Err(format!("DATA rejected: {line}").into());
    }

    let signed = dkim_sign(
        &email.sender_domain,
        &email.body,
        &email.dkim_key_path,
        &email.dkim_key_selector,
    )?;
    sender.send(signed).await?;
    sender.send(String::from(".")).await?;

    let line = reader
        .next()
        .await
        .ok_or("No response after message body")??;
    debug!("[{}] Message accepted: {}", email.id, line);
    if !line.starts_with("250") {
        sender.send(String::from("QUIT")).await?;
        return Err(format!("Message rejected by remote server: {line}").into());
    }

    sender.send(String::from("QUIT")).await?;
    Ok(())
}

/// Looks up MX records for each destination domain (sorted by priority),
/// negotiates STARTTLS, and delivers the message.  Tries each MX host in
/// priority order before giving up on a domain.
#[allow(clippy::too_many_lines)]
#[instrument(skip(email))]
pub async fn send_email_job(
    email: &EmailPayload,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    debug!("[{}] Starting email delivery", email.id);
    let resolver = TokioResolver::builder_tokio()?.build()?;

    for (target, to) in &email.to {
        debug!("[{}] Resolving delivery path for {}", email.id, target);

        // Collect MX records and sort by preference (lower value = higher priority)
        let mut mx_hosts: Vec<(u16, String)> = Vec::new();
        if let Ok(mx_resp) = resolver.mx_lookup(target.as_str()).await {
            let mut records: Vec<(u16, String)> = mx_resp
                .answers()
                .iter()
                .filter_map(|record| {
                    if let RData::MX(mx) = &record.data {
                        let exchange = mx.exchange.to_string();
                        let host =
                            exchange.strip_suffix('.').unwrap_or(&exchange).to_string();
                        Some((mx.preference, host))
                    } else {
                        None
                    }
                })
                .collect();
            records.sort_by_key(|(pref, _)| *pref);
            mx_hosts = records;
        }

        // No MX records — fall back to treating the domain as the mail server
        if mx_hosts.is_empty() {
            debug!(
                "[{}] No MX records for {}, attempting A/AAAA fallback",
                email.id, target
            );
            mx_hosts.push((0, target.clone()));
        }

        // Fetch MTA-STS policy for this destination domain (RFC 8461).
        let sts_policy = fetch_mta_sts_policy(target, &resolver).await;
        if let Some(ref policy) = sts_policy {
            debug!(
                "[{}] MTA-STS policy for {}: mode={:?}",
                email.id, target, policy.mode
            );
        }

        let mut delivered = false;
        'mx: for (_, host) in &mx_hosts {
            debug!("[{}] Trying MX host {}", email.id, host);
            let ip = match resolver.lookup_ip(host.as_str()).await {
                Ok(r) => {
                    if let Some(ip) = r.iter().next() {
                        ip
                    } else {
                        warn!("[{}] No IP address for MX host {}", email.id, host);
                        continue 'mx;
                    }
                }
                Err(e) => {
                    warn!(
                        "[{}] IP lookup failed for MX host {}: {}",
                        email.id, host, e
                    );
                    continue 'mx;
                }
            };

            // MTA-STS: in enforce mode the MX host must match the policy's mx list.
            if let Some(ref policy) = sts_policy {
                if policy.mode == MtaStsMode::Enforce
                    && !mx_allowed_by_policy(host, policy)
                {
                    warn!(
                        "[{}] MTA-STS enforce: MX host {} is not in the policy mx \
                         list for {}; skipping",
                        email.id, host, target
                    );
                    continue 'mx;
                }
            }

            // Port 25 is the correct relay port for MTA-to-MTA delivery (RFC 5321).
            // Ports 465/587 are submission ports for mail clients and MUST NOT be
            // used for server-to-server relay (RFC 8314).
            // Try STARTTLS first; fall back to plain when STARTTLS is not offered.
            let conn = match connect_with_starttls(ip, email, host).await {
                Ok(c) => c,
                Err(e) => {
                    warn!(
                        "[{}] Port 25 connection to {} ({}) failed: {}",
                        email.id, host, ip, e
                    );
                    continue 'mx;
                }
            };

            // MTA-STS enforce: TLS is mandatory (RFC 8461 §4.2).
            if let Some(ref policy) = sts_policy {
                if policy.mode == MtaStsMode::Enforce && !conn.is_tls {
                    warn!(
                        "[{}] MTA-STS enforce: {} did not offer STARTTLS; \
                         refusing plain delivery to {}",
                        email.id, host, target
                    );
                    continue 'mx;
                }
            }

            // RFC 8689 §4: when REQUIRETLS is set, the remote MUST support TLS
            // AND advertise REQUIRETLS in its post-STARTTLS EHLO.
            if email.require_tls {
                if !conn.is_tls {
                    warn!(
                        "[{}] REQUIRETLS: {} does not support STARTTLS; skipping",
                        email.id, host
                    );
                    continue 'mx;
                }
                if !conn.requiretls_advertised {
                    warn!(
                        "[{}] REQUIRETLS: {} did not advertise REQUIRETLS after \
                         STARTTLS (RFC 8689 §4); skipping",
                        email.id, host
                    );
                    continue 'mx;
                }
            }

            // DANE TLSA validation (RFC 7672): only when TLS was negotiated.
            // If no TLSA records exist, validation is a no-op (returns true).
            // If records exist but the cert does not match, skip this MX host.
            if conn.is_tls {
                let tlsa_records = fetch_tlsa_records(host, &resolver).await;
                if !tlsa_records.is_empty() {
                    let valid = conn
                        .peer_cert_der
                        .as_deref()
                        .is_some_and(|cert| validate_cert_against_tlsa(cert, &tlsa_records));
                    if valid {
                        debug!("[{}] DANE: cert for {} passed TLSA validation", email.id, host);
                    } else {
                        warn!(
                            "[{}] DANE: cert for {} failed TLSA validation; skipping MX host",
                            email.id, host
                        );
                        continue 'mx;
                    }
                }
            }

            let tls_label = if conn.is_tls { "STARTTLS" } else { "plain" };
            match smtp_deliver(conn.framed, email, to).await {
                Ok(()) => {
                    debug!(
                        "[{}] Delivered to {} via {} ({}) on port 25",
                        email.id, target, host, tls_label
                    );
                    delivered = true;
                    break 'mx;
                }
                Err(e) => {
                    warn!(
                        "[{}] Delivery to {} via {} ({}) on port 25 failed: {}",
                        email.id, target, host, tls_label, e
                    );
                }
            }
        }

        if !delivered {
            return Err(
                format!("Failed to deliver message to {target}: all MX hosts exhausted")
                    .into(),
            );
        }
    }

    debug!("[{}] Email delivery complete", email.id);
    Ok(())
}
