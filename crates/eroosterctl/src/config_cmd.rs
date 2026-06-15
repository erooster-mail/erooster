// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! `eroosterctl config validate` — validate the configuration file.

use crate::output::{print_error, print_json, print_success, OutputFormat};
use clap::Subcommand;
use color_eyre::eyre::Result;
use erooster_core::{backend::database::get_database, config::Config};
#[allow(unused_imports)]
use erooster_core::backend::database::Database;
use serde::Serialize;

#[derive(Subcommand, Debug)]
pub enum ConfigCommands {
    /// Validate the configuration file and check all required resources are accessible
    Validate,
}

#[derive(Serialize)]
struct ValidateIssue {
    field: String,
    message: String,
}

#[derive(Serialize)]
struct ValidateReport {
    ok: bool,
    issues: Vec<ValidateIssue>,
}

pub async fn run(cmd: ConfigCommands, config: &Config, format: OutputFormat, no_color: bool) -> Result<()> {
    match cmd {
        ConfigCommands::Validate => validate(config, format, no_color).await,
    }
}

async fn validate(config: &Config, format: OutputFormat, no_color: bool) -> Result<()> {
    let mut issues: Vec<ValidateIssue> = Vec::new();

    // Required string fields
    if config.mail.hostname.is_empty() {
        issues.push(ValidateIssue {
            field: "mail.hostname".to_string(),
            message: "must not be empty".to_string(),
        });
    }

    if config.mail.dkim_key_path.is_empty() {
        issues.push(ValidateIssue {
            field: "mail.dkim_key_path".to_string(),
            message: "must not be empty".to_string(),
        });
    } else if !std::path::Path::new(&config.mail.dkim_key_path).exists() {
        issues.push(ValidateIssue {
            field: "mail.dkim_key_path".to_string(),
            message: format!("file not found: {}", config.mail.dkim_key_path),
        });
    }

    if config.mail.dkim_key_selector.is_empty() {
        issues.push(ValidateIssue {
            field: "mail.dkim_key_selector".to_string(),
            message: "must not be empty".to_string(),
        });
    }

    if config.tls.cert_path.is_empty() {
        issues.push(ValidateIssue {
            field: "tls.cert_path".to_string(),
            message: "must not be empty".to_string(),
        });
    } else if !std::path::Path::new(&config.tls.cert_path).exists() {
        issues.push(ValidateIssue {
            field: "tls.cert_path".to_string(),
            message: format!("file not found: {}", config.tls.cert_path),
        });
    }

    if config.tls.key_path.is_empty() {
        issues.push(ValidateIssue {
            field: "tls.key_path".to_string(),
            message: "must not be empty".to_string(),
        });
    } else if !std::path::Path::new(&config.tls.key_path).exists() {
        issues.push(ValidateIssue {
            field: "tls.key_path".to_string(),
            message: format!("file not found: {}", config.tls.key_path),
        });
    }

    if config.database.url.is_empty() {
        issues.push(ValidateIssue {
            field: "database.url".to_string(),
            message: "must not be empty".to_string(),
        });
    }

    // Try connecting to the database
    if !config.database.url.is_empty() {
        if let Err(e) = get_database(config).await {
            issues.push(ValidateIssue {
                field: "database.url".to_string(),
                message: format!("connection failed: {e}"),
            });
        }
    }

    let ok = issues.is_empty();

    if format == OutputFormat::Json {
        print_json(&ValidateReport { ok, issues })?;
    } else if ok {
        print_success(no_color, "Configuration is valid.");
    } else {
        for issue in &issues {
            print_error(no_color, &format!("{}: {}", issue.field, issue.message));
        }
        std::process::exit(1);
    }
    Ok(())
}
