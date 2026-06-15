// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! `eroosterctl domain check` — DNS health check (MX, SPF, DKIM, DMARC).

use crate::output::{color_disabled, print_json, OutputFormat};
use clap::Subcommand;
use color_eyre::eyre::Result;
use erooster_core::config::Config;
use hickory_resolver::{
    proto::rr::rdata::{MX, TXT},
    TokioResolver,
};
use owo_colors::OwoColorize;
use serde::Serialize;

#[derive(Subcommand, Debug)]
pub enum DomainCommands {
    /// Check DNS records for the mail domain
    Check {
        /// Hostname to check (defaults to mail.hostname from config)
        hostname: Option<String>,
    },
}

#[derive(Debug, Serialize)]
pub struct DnsCheck {
    pub check: String,
    pub status: String,
    pub detail: String,
}

pub async fn run(
    cmd: DomainCommands,
    config: &Config,
    format: OutputFormat,
    no_color: bool,
) -> Result<()> {
    let no_color = color_disabled(no_color);
    match cmd {
        DomainCommands::Check { hostname } => {
            let host = hostname
                .as_deref()
                .unwrap_or(&config.mail.hostname)
                .to_string();
            check_dns(&host, &config.mail.dkim_key_selector, format, no_color).await
        }
    }
}

async fn check_dns(
    hostname: &str,
    selector: &str,
    format: OutputFormat,
    no_color: bool,
) -> Result<()> {
    let resolver = TokioResolver::builder_tokio()
        .map_err(|e| color_eyre::eyre::eyre!("DNS resolver init error: {e}"))?
        .build()
        .map_err(|e| color_eyre::eyre::eyre!("DNS resolver build error: {e}"))?;

    let mut checks: Vec<DnsCheck> = Vec::new();
    let mut all_pass = true;

    checks.push(check_mx(&resolver, hostname).await);
    checks.push(check_spf(&resolver, hostname).await);
    checks.push(check_dkim(&resolver, hostname, selector).await);
    checks.push(check_dmarc(&resolver, hostname).await);

    for c in &checks {
        if c.status != "PASS" {
            all_pass = false;
        }
    }

    if format == OutputFormat::Json {
        print_json(&checks)?;
    } else {
        println!("DNS checks for {hostname}:");
        println!();
        for c in &checks {
            let status_colored = if no_color {
                c.status.clone()
            } else {
                match c.status.as_str() {
                    "PASS" => c.status.green().to_string(),
                    _ => c.status.red().to_string(),
                }
            };
            println!("  {:<12}  {:<6}  {}", c.check, status_colored, c.detail);
        }
    }

    if !all_pass {
        std::process::exit(1);
    }
    Ok(())
}

async fn check_mx(resolver: &TokioResolver, hostname: &str) -> DnsCheck {
    match resolver.mx_lookup(hostname).await {
        Ok(lookup) => {
            let records: Vec<String> = lookup
                .answers()
                .iter()
                .filter_map(|r| {
                    r.try_borrow::<MX>().map(|m| {
                        format!("{} {}", m.data().preference, m.data().exchange)
                    })
                })
                .collect();
            if records.is_empty() {
                DnsCheck {
                    check: "MX".to_string(),
                    status: "FAIL".to_string(),
                    detail: "No MX records found".to_string(),
                }
            } else {
                DnsCheck {
                    check: "MX".to_string(),
                    status: "PASS".to_string(),
                    detail: records.join(", "),
                }
            }
        }
        Err(e) => DnsCheck {
            check: "MX".to_string(),
            status: "FAIL".to_string(),
            detail: format!("DNS lookup failed: {e}"),
        },
    }
}

fn txt_record_to_bytes(record: &hickory_resolver::proto::rr::RecordRef<'_, TXT>) -> Vec<u8> {
    record.data().txt_data.iter().flatten().copied().collect()
}

async fn check_spf(resolver: &TokioResolver, hostname: &str) -> DnsCheck {
    match resolver.txt_lookup(hostname).await {
        Ok(lookup) => {
            let spf = lookup.answers().iter().find_map(|r| {
                r.try_borrow::<TXT>().and_then(|t| {
                    let bytes = txt_record_to_bytes(&t);
                    bytes
                        .starts_with(b"v=spf1")
                        .then(|| String::from_utf8_lossy(&bytes).into_owned())
                })
            });
            match spf {
                Some(record) => DnsCheck {
                    check: "SPF".to_string(),
                    status: "PASS".to_string(),
                    detail: record,
                },
                None => DnsCheck {
                    check: "SPF".to_string(),
                    status: "FAIL".to_string(),
                    detail: "No SPF TXT record found".to_string(),
                },
            }
        }
        Err(e) => DnsCheck {
            check: "SPF".to_string(),
            status: "FAIL".to_string(),
            detail: format!("DNS lookup failed: {e}"),
        },
    }
}

async fn check_dkim(resolver: &TokioResolver, hostname: &str, selector: &str) -> DnsCheck {
    let dkim_name = format!("{selector}._domainkey.{hostname}");
    match resolver.txt_lookup(dkim_name.as_str()).await {
        Ok(lookup) => {
            let dkim = lookup.answers().iter().find_map(|r| {
                r.try_borrow::<TXT>().and_then(|t| {
                    let bytes = txt_record_to_bytes(&t);
                    // A valid DKIM record contains "p=" (public key field)
                    bytes
                        .windows(2)
                        .any(|w| w == b"p=")
                        .then(|| String::from_utf8_lossy(&bytes).into_owned())
                })
            });
            match dkim {
                Some(record) => DnsCheck {
                    check: "DKIM".to_string(),
                    status: "PASS".to_string(),
                    detail: format!("{dkim_name}: {record}"),
                },
                None => DnsCheck {
                    check: "DKIM".to_string(),
                    status: "FAIL".to_string(),
                    detail: format!("{dkim_name}: no p= key found in TXT record"),
                },
            }
        }
        Err(e) => DnsCheck {
            check: "DKIM".to_string(),
            status: "FAIL".to_string(),
            detail: format!("{dkim_name}: DNS lookup failed: {e}"),
        },
    }
}

async fn check_dmarc(resolver: &TokioResolver, hostname: &str) -> DnsCheck {
    let dmarc_name = format!("_dmarc.{hostname}");
    match resolver.txt_lookup(dmarc_name.as_str()).await {
        Ok(lookup) => {
            let dmarc = lookup.answers().iter().find_map(|r| {
                r.try_borrow::<TXT>().and_then(|t| {
                    let bytes = txt_record_to_bytes(&t);
                    bytes
                        .starts_with(b"v=DMARC1")
                        .then(|| String::from_utf8_lossy(&bytes).into_owned())
                })
            });
            match dmarc {
                Some(record) => DnsCheck {
                    check: "DMARC".to_string(),
                    status: "PASS".to_string(),
                    detail: record,
                },
                None => DnsCheck {
                    check: "DMARC".to_string(),
                    status: "FAIL".to_string(),
                    detail: format!("{dmarc_name}: no DMARC1 record found"),
                },
            }
        }
        Err(e) => DnsCheck {
            check: "DMARC".to_string(),
            status: "FAIL".to_string(),
            detail: format!("{dmarc_name}: DNS lookup failed: {e}"),
        },
    }
}
