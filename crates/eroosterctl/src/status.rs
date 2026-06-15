// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! `eroosterctl status` — live health checks for all server components.

use crate::output::{color_disabled, print_json, OutputFormat};
use color_eyre::eyre::Result;
use erooster_core::{
    backend::{
        admin::{mailbox_count, queue_stats, user_count, QueueStats},
        database::{get_database, Database},
    },
    config::Config,
};
use owo_colors::OwoColorize;
use serde::Serialize;
use std::fmt::Write as _;
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    time::timeout,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const BANNER_TIMEOUT: Duration = Duration::from_secs(5);

/// Result of a single health check.
#[derive(Debug, Serialize)]
pub struct CheckResult {
    /// Human-readable name of the check.
    pub name: String,
    /// `OK`, `WARN`, or `FAIL`.
    pub status: String,
    /// One-line detail string shown next to the status.
    pub detail: String,
}

/// Full status report (JSON output).
#[derive(Debug, Serialize)]
pub struct StatusReport {
    /// eroosterctl version.
    pub version: String,
    /// All individual check results.
    pub checks: Vec<CheckResult>,
    /// Total registered users.
    pub users: i64,
    /// Total distinct mailbox folders.
    pub mailboxes: i64,
    /// Queue entries waiting for delivery.
    pub queue_pending: i64,
    /// Queue entries that failed delivery.
    pub queue_failed: i64,
}

pub async fn run(config: &Config, format: OutputFormat, no_color: bool) -> Result<()> {
    let no_color = color_disabled(no_color);
    let mut checks = Vec::new();
    let mut all_ok = true;

    // --- Database ---
    let db_result = get_database(config).await;
    let (db_ok, db_detail, db_pool) = match db_result {
        Ok(ref db) => match user_count(db.get_pool()).await {
            Ok(_) => (true, config.database.url.clone(), Some(db.get_pool().clone())),
            Err(e) => (false, e.to_string(), None),
        },
        Err(ref e) => (false, e.to_string(), None),
    };
    if !db_ok {
        all_ok = false;
    }
    checks.push(CheckResult {
        name: "Database".to_string(),
        status: if db_ok { "OK" } else { "FAIL" }.to_string(),
        detail: db_detail,
    });

    // --- TLS certificate ---
    let tls = check_tls(config);
    if tls.status == "FAIL" {
        all_ok = false;
    }
    checks.push(tls);

    // --- DKIM sign roundtrip ---
    let dkim = check_dkim(config);
    if dkim.status == "FAIL" {
        all_ok = false;
    }
    checks.push(dkim);

    // --- Autoconfig endpoint ---
    let autoconfig = check_autoconfig(config).await;
    if autoconfig.status == "FAIL" {
        all_ok = false;
    }
    checks.push(autoconfig);

    // --- SMTP / IMAP port checks ---
    let hostname = config.mail.hostname.clone();
    let (smtp25, smtp587, imap143, imap993) = tokio::join!(
        check_smtp_plain(&hostname, 25),
        check_smtp_plain(&hostname, 587),
        check_imap_plain(&hostname, 143),
        check_imap_tls(&hostname, 993, config),
    );
    for c in [&smtp25, &smtp587, &imap143, &imap993] {
        if c.status == "FAIL" {
            all_ok = false;
        }
    }
    checks.extend([smtp25, smtp587, imap143, imap993]);

    // --- Counts (only when DB is healthy) ---
    let (users, mailboxes, queue) = if let Some(pool) = db_pool {
        let u = user_count(&pool).await.unwrap_or(0);
        let m = mailbox_count(&pool).await.unwrap_or(0);
        let q = queue_stats(&pool).await.unwrap_or_default();
        (u, m, q)
    } else {
        (0, 0, QueueStats::default())
    };

    let version = env!("CARGO_PKG_VERSION").to_string();

    if format == OutputFormat::Json {
        print_json(&StatusReport {
            version,
            checks,
            users,
            mailboxes,
            queue_pending: queue.pending,
            queue_failed: queue.failed,
        })?;
    } else {
        render_table(&checks, &version, users, mailboxes, &queue, no_color);
    }

    if !all_ok {
        std::process::exit(1);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Port connectivity checks
// ---------------------------------------------------------------------------

/// Connect to a TCP port, read the first banner line, validate it with `check`.
async fn check_banner(
    name: &str,
    hostname: &str,
    port: u16,
    check: impl Fn(&str) -> bool,
) -> CheckResult {
    let addr = format!("{hostname}:{port}");
    let stream = match timeout(CONNECT_TIMEOUT, TcpStream::connect((hostname, port))).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            return CheckResult {
                name: name.to_string(),
                status: "FAIL".to_string(),
                detail: format!("{addr}: {e}"),
            }
        }
        Err(_) => {
            return CheckResult {
                name: name.to_string(),
                status: "FAIL".to_string(),
                detail: format!("{addr}: connection timed out"),
            }
        }
    };

    let mut reader = BufReader::new(stream);
    let mut banner = String::new();
    match timeout(BANNER_TIMEOUT, reader.read_line(&mut banner)).await {
        Ok(Ok(_)) => {
            let banner = banner.trim_end();
            if check(banner) {
                CheckResult {
                    name: name.to_string(),
                    status: "OK".to_string(),
                    detail: format!("{addr}  {banner}"),
                }
            } else {
                CheckResult {
                    name: name.to_string(),
                    status: "FAIL".to_string(),
                    detail: format!("{addr}: unexpected banner: {banner}"),
                }
            }
        }
        Ok(Err(e)) => CheckResult {
            name: name.to_string(),
            status: "FAIL".to_string(),
            detail: format!("{addr}: read error: {e}"),
        },
        Err(_) => CheckResult {
            name: name.to_string(),
            status: "FAIL".to_string(),
            detail: format!("{addr}: banner read timed out"),
        },
    }
}

async fn check_smtp_plain(hostname: &str, port: u16) -> CheckResult {
    let name = format!("SMTP :{port}");
    let mut result = check_banner(&name, hostname, port, |b| b.starts_with("220 ")).await;
    // Send QUIT so we don't leave the server waiting
    if result.status == "OK" {
        if let Ok(Ok(mut s)) = timeout(CONNECT_TIMEOUT, TcpStream::connect((hostname, port))).await
        {
            let _ = timeout(BANNER_TIMEOUT, async {
                let mut buf = String::new();
                let mut r = BufReader::new(&mut s);
                let _ = r.read_line(&mut buf).await;
                let _ = s.write_all(b"QUIT\r\n").await;
            })
            .await;
        }
    }
    result.name = name;
    result
}

async fn check_imap_plain(hostname: &str, port: u16) -> CheckResult {
    let name = format!("IMAP :{port}");
    // IMAP greeting starts with "* OK"
    let mut result = check_banner(&name, hostname, port, |b| b.starts_with("* OK")).await;
    if result.status == "OK" {
        if let Ok(Ok(mut s)) = timeout(CONNECT_TIMEOUT, TcpStream::connect((hostname, port))).await
        {
            let _ = timeout(BANNER_TIMEOUT, async {
                let mut buf = String::new();
                let mut r = BufReader::new(&mut s);
                let _ = r.read_line(&mut buf).await;
                let _ = s.write_all(b"A001 LOGOUT\r\n").await;
            })
            .await;
        }
    }
    result.name = name;
    result
}

#[allow(clippy::too_many_lines)]
async fn check_imap_tls(hostname: &str, port: u16, config: &Config) -> CheckResult {
    use rustls::pki_types::{pem::PemObject, CertificateDer, ServerName};
    use std::sync::Arc;
    use tokio_rustls::TlsConnector;

    let name = format!("IMAP :{port}");
    let addr = format!("{hostname}:{port}");

    // Load the configured certificate as the trust anchor so we verify the
    // exact cert the server is expected to present (no system root store needed).
    let cert_pem = match std::fs::read(&config.tls.cert_path) {
        Ok(b) => b,
        Err(e) => {
            return CheckResult {
                name,
                status: "FAIL".to_string(),
                detail: format!("cannot read cert {}: {e}", config.tls.cert_path),
            }
        }
    };

    let cert = match CertificateDer::from_pem_slice(&cert_pem) {
        Ok(c) => c,
        Err(e) => {
            return CheckResult {
                name,
                status: "FAIL".to_string(),
                detail: format!("cannot parse cert {}: {e}", config.tls.cert_path),
            }
        }
    };

    let mut root_store = rustls::RootCertStore::empty();
    if let Err(e) = root_store.add(cert) {
        return CheckResult {
            name,
            status: "FAIL".to_string(),
            detail: format!("cannot use cert as trust anchor: {e}"),
        };
    }

    let tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(tls_config));
    let server_name = match ServerName::try_from(hostname.to_string()) {
        Ok(n) => n,
        Err(e) => {
            return CheckResult {
                name,
                status: "FAIL".to_string(),
                detail: format!("invalid server name '{hostname}': {e}"),
            }
        }
    };

    // TCP connect
    let tcp = match timeout(CONNECT_TIMEOUT, TcpStream::connect((hostname, port))).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            return CheckResult {
                name,
                status: "FAIL".to_string(),
                detail: format!("{addr}: {e}"),
            }
        }
        Err(_) => {
            return CheckResult {
                name,
                status: "FAIL".to_string(),
                detail: format!("{addr}: connection timed out"),
            }
        }
    };

    // TLS handshake
    let tls_stream = match timeout(CONNECT_TIMEOUT, connector.connect(server_name, tcp)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            return CheckResult {
                name,
                status: "FAIL".to_string(),
                detail: format!("{addr}: TLS handshake failed: {e}"),
            }
        }
        Err(_) => {
            return CheckResult {
                name,
                status: "FAIL".to_string(),
                detail: format!("{addr}: TLS handshake timed out"),
            }
        }
    };

    // Read IMAP banner over TLS
    let mut reader = BufReader::new(tls_stream);
    let mut banner = String::new();
    match timeout(BANNER_TIMEOUT, reader.read_line(&mut banner)).await {
        Ok(Ok(_)) => {
            let banner = banner.trim_end();
            if banner.starts_with("* OK") {
                CheckResult {
                    name,
                    status: "OK".to_string(),
                    detail: format!("{addr}  {banner}"),
                }
            } else {
                CheckResult {
                    name,
                    status: "FAIL".to_string(),
                    detail: format!("{addr}: unexpected banner: {banner}"),
                }
            }
        }
        Ok(Err(e)) => CheckResult {
            name,
            status: "FAIL".to_string(),
            detail: format!("{addr}: read error: {e}"),
        },
        Err(_) => CheckResult {
            name,
            status: "FAIL".to_string(),
            detail: format!("{addr}: banner read timed out"),
        },
    }
}

// ---------------------------------------------------------------------------
// TLS certificate check
// ---------------------------------------------------------------------------

fn check_tls(config: &Config) -> CheckResult {
    use x509_parser::prelude::*;

    let path = &config.tls.cert_path;
    let pem_data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            return CheckResult {
                name: "TLS cert".to_string(),
                status: "FAIL".to_string(),
                detail: format!("{path}: {e}"),
            }
        }
    };

    let (_, pem) = match parse_x509_pem(&pem_data) {
        Ok(r) => r,
        Err(e) => {
            return CheckResult {
                name: "TLS cert".to_string(),
                status: "FAIL".to_string(),
                detail: format!("{path}: failed to parse PEM: {e}"),
            }
        }
    };

    let cert = match pem.parse_x509() {
        Ok(c) => c,
        Err(e) => {
            return CheckResult {
                name: "TLS cert".to_string(),
                status: "FAIL".to_string(),
                detail: format!("{path}: failed to parse certificate: {e}"),
            }
        }
    };

    let validity = cert.validity();
    let now = x509_parser::time::ASN1Time::now();

    if now > validity.not_after {
        return CheckResult {
            name: "TLS cert".to_string(),
            status: "FAIL".to_string(),
            detail: format!("{path}: expired at {}", validity.not_after),
        };
    }

    let hostname = &config.mail.hostname;
    let covered = match cert.subject_alternative_name() {
        Ok(Some(san_ext)) => san_ext.value.general_names.iter().any(|gn| {
            if let GeneralName::DNSName(dns) = gn {
                *dns == hostname.as_str()
                    || (dns.starts_with("*.") && {
                        let suffix = &dns[2..];
                        hostname.ends_with(suffix)
                            && !hostname[..hostname.len() - suffix.len() - 1].contains('.')
                    })
            } else {
                false
            }
        }),
        _ => cert
            .subject()
            .iter_common_name()
            .any(|cn| cn.as_str().ok() == Some(hostname.as_str())),
    };

    if !covered {
        return CheckResult {
            name: "TLS cert".to_string(),
            status: "FAIL".to_string(),
            detail: format!("{path}: hostname '{hostname}' not covered by certificate"),
        };
    }

    let days_left = (validity.not_after.timestamp() - now.timestamp()) / 86_400;
    CheckResult {
        name: "TLS cert".to_string(),
        status: if days_left < 30 { "WARN" } else { "OK" }.to_string(),
        detail: format!("{path} (expires {}, {days_left}d left)", validity.not_after),
    }
}

// ---------------------------------------------------------------------------
// DKIM sign roundtrip
// ---------------------------------------------------------------------------

fn check_dkim(config: &Config) -> CheckResult {
    use mail_auth::{
        common::crypto::{RsaKey, Sha256},
        dkim::DkimSigner,
    };
    use rustls::pki_types::{pem::PemObject, PrivateKeyDer, PrivatePkcs1KeyDer, PrivatePkcs8KeyDer};

    let key_path = &config.mail.dkim_key_path;
    let selector = &config.mail.dkim_key_selector;
    let hostname = &config.mail.hostname;

    let pem = match std::fs::read_to_string(key_path) {
        Ok(s) => s,
        Err(e) => {
            return CheckResult {
                name: "DKIM key".to_string(),
                status: "FAIL".to_string(),
                detail: format!("{key_path}: {e}"),
            }
        }
    };

    let key_der = if pem.contains("BEGIN RSA PRIVATE KEY") {
        match PrivatePkcs1KeyDer::from_pem_slice(pem.as_bytes()) {
            Ok(k) => PrivateKeyDer::Pkcs1(k),
            Err(e) => {
                return CheckResult {
                    name: "DKIM key".to_string(),
                    status: "FAIL".to_string(),
                    detail: format!("{key_path}: failed to parse PKCS#1 key: {e}"),
                }
            }
        }
    } else {
        match PrivatePkcs8KeyDer::from_pem_slice(pem.as_bytes()) {
            Ok(k) => PrivateKeyDer::Pkcs8(k),
            Err(e) => {
                return CheckResult {
                    name: "DKIM key".to_string(),
                    status: "FAIL".to_string(),
                    detail: format!("{key_path}: failed to parse PKCS#8 key: {e}"),
                }
            }
        }
    };

    let rsa_key = match RsaKey::<Sha256>::from_key_der(key_der) {
        Ok(k) => k,
        Err(e) => {
            return CheckResult {
                name: "DKIM key".to_string(),
                status: "FAIL".to_string(),
                detail: format!("{key_path}: failed to load RSA key: {e:?}"),
            }
        }
    };

    let test_msg = format!(
        "From: eroosterctl@{hostname}\r\nTo: test@{hostname}\r\nSubject: DKIM health check\r\n\r\nDKIM sign test.\r\n"
    );

    let signed = DkimSigner::from_key(rsa_key)
        .domain(hostname.as_str())
        .selector(selector.as_str())
        .headers(["From", "To", "Subject"])
        .sign(test_msg.as_bytes());

    match signed {
        Ok(_) => CheckResult {
            name: "DKIM key".to_string(),
            status: "OK".to_string(),
            detail: format!("{key_path} (selector: {selector}, sign test passed)"),
        },
        Err(e) => CheckResult {
            name: "DKIM key".to_string(),
            status: "FAIL".to_string(),
            detail: format!("{key_path}: signing failed: {e:?}"),
        },
    }
}

// ---------------------------------------------------------------------------
// Autoconfig endpoint
// ---------------------------------------------------------------------------

async fn check_autoconfig(config: &Config) -> CheckResult {
    let port = config.webserver.port;
    let hostname = &config.mail.hostname;
    let url = format!("http://{hostname}:{port}/mail/config-v1.1.xml");

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return CheckResult {
                name: "Autoconfig".to_string(),
                status: "FAIL".to_string(),
                detail: format!("Could not build HTTP client: {e}"),
            }
        }
    };

    match client.get(&url).send().await {
        Ok(resp) => {
            let code = resp.status();
            if code.is_success() {
                let body = resp.text().await.unwrap_or_default();
                if body.contains("clientConfig") {
                    CheckResult {
                        name: "Autoconfig".to_string(),
                        status: "OK".to_string(),
                        detail: url,
                    }
                } else {
                    CheckResult {
                        name: "Autoconfig".to_string(),
                        status: "FAIL".to_string(),
                        detail: format!("{url}: response missing <clientConfig> element"),
                    }
                }
            } else {
                CheckResult {
                    name: "Autoconfig".to_string(),
                    status: "FAIL".to_string(),
                    detail: format!("{url}: HTTP {code}"),
                }
            }
        }
        Err(e) => CheckResult {
            name: "Autoconfig".to_string(),
            status: "FAIL".to_string(),
            detail: format!("{url}: {e}"),
        },
    }
}

// ---------------------------------------------------------------------------
// Table rendering
// ---------------------------------------------------------------------------

fn colorize_status(status: &str, no_color: bool) -> String {
    if no_color {
        return status.to_string();
    }
    match status {
        "OK" => status.green().to_string(),
        "WARN" => status.yellow().to_string(),
        _ => status.red().to_string(),
    }
}

fn render_table(
    checks: &[CheckResult],
    version: &str,
    users: i64,
    mailboxes: i64,
    queue: &QueueStats,
    no_color: bool,
) {
    let rooster = [
        r"    ____",
        r"   / __ \",
        r"  |  ___/",
        r"  |  \__",
        r"   \____/",
    ];

    let mut out = String::new();

    for (i, line) in rooster.iter().enumerate() {
        if i == 0 {
            let _ = writeln!(out, "{line}   Erooster v{version}");
        } else if let Some(check) = checks.get(i - 1) {
            let _ = writeln!(
                out,
                "{line:<12}  {:<16} {}  {}",
                check.name,
                colorize_status(&check.status, no_color),
                check.detail
            );
        } else {
            let _ = writeln!(out, "{line}");
        }
    }

    for check in checks.iter().skip(rooster.len() - 1) {
        let _ = writeln!(
            out,
            "{:<12}  {:<16} {}  {}",
            "",
            check.name,
            colorize_status(&check.status, no_color),
            check.detail
        );
    }

    print!("{out}");
    println!();
    println!("  Users       {users}");
    println!("  Mailboxes   {mailboxes}");
    println!(
        "  Queue       {} pending · {} failed",
        queue.pending, queue.failed
    );
}
