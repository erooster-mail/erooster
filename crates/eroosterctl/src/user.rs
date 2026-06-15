// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! `eroosterctl user` — user management subcommands.

use crate::output::{color_disabled, print_error, print_json, print_success, print_table, OutputFormat};
use clap::Subcommand;
use color_eyre::eyre::Result;
use erooster_core::{
    backend::database::{get_database, Database},
    config::Config,
};
use secrecy::SecretString;
use serde::Serialize;
use std::io::IsTerminal;

#[derive(Subcommand, Debug)]
pub enum UserCommands {
    /// List all users
    #[command(alias = "ls")]
    List,

    /// Add a new user
    #[command(alias = "add")]
    Add {
        /// Email address of the new user
        #[arg(short, long)]
        email: Option<String>,
        /// Password for the new user
        #[arg(short, long)]
        password: Option<SecretString>,
    },

    /// Delete a user
    #[command(alias = "del", alias = "rm")]
    Delete {
        /// Email address of the user to delete
        email: String,
        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },

    /// Change a user's password
    Passwd {
        /// Email address of the user
        email: String,
        /// Current password
        #[arg(long)]
        current_password: Option<SecretString>,
        /// New password
        #[arg(long)]
        new_password: Option<SecretString>,
    },

    /// Check if a user exists (exit 0 = exists, exit 1 = not found)
    Exists {
        /// Email address to check
        email: String,
    },

    /// Bulk import users from a CSV file (email,password per line)
    Import {
        /// Path to CSV file; reads from stdin if omitted
        #[arg(short, long)]
        file: Option<String>,
    },
}

#[derive(Serialize)]
struct UserRow {
    username: String,
}

pub async fn run(
    cmd: UserCommands,
    config: &Config,
    format: OutputFormat,
    yes: bool,
    no_color: bool,
) -> Result<()> {
    let no_color = color_disabled(no_color);
    match cmd {
        UserCommands::List => list(config, format).await,
        UserCommands::Add { email, password } => add(email, password, config, no_color).await,
        UserCommands::Delete { email, yes: local_yes } => {
            delete(&email, yes || local_yes, config, no_color).await
        }
        UserCommands::Passwd {
            email,
            current_password,
            new_password,
        } => passwd(&email, current_password, new_password, config, no_color).await,
        UserCommands::Exists { email } => exists(&email, config).await,
        UserCommands::Import { file } => import(file, config, no_color).await,
    }
}

async fn list(config: &Config, format: OutputFormat) -> Result<()> {
    let db = get_database(config).await?;
    let users = db.list_users().await?;

    if format == OutputFormat::Json {
        let rows: Vec<UserRow> = users.into_iter().map(|u| UserRow { username: u }).collect();
        print_json(&rows)?;
    } else {
        let rows = users.into_iter().map(|u| vec![u]).collect();
        print_table(&["USERNAME"], rows);
    }
    Ok(())
}

async fn add(
    email: Option<String>,
    password: Option<SecretString>,
    config: &Config,
    no_color: bool,
) -> Result<()> {
    let is_tty = std::io::stdin().is_terminal();

    let email = if let Some(e) = email {
        e
    } else {
        if !is_tty {
            print_error(no_color, "--email is required in non-interactive mode");
            std::process::exit(2);
        }
        dialoguer::Input::<String>::new()
            .with_prompt("Email address")
            .interact_text()?
    };

    let password = if let Some(p) = password {
        p
    } else {
        if !is_tty {
            print_error(no_color, "--password is required in non-interactive mode");
            std::process::exit(2);
        }
        let raw = dialoguer::Password::new()
            .with_prompt("Password")
            .with_confirmation("Confirm password", "Passwords do not match")
            .interact()?;
        SecretString::new(raw.into_boxed_str())
    };

    let db = get_database(config).await?;
    let email_lc = email.to_lowercase();
    db.add_user(&email_lc).await?;
    db.change_password(&email_lc, password).await?;
    print_success(no_color, &format!("User '{email_lc}' added."));
    Ok(())
}

async fn delete(email: &str, yes: bool, config: &Config, no_color: bool) -> Result<()> {
    let is_tty = std::io::stdin().is_terminal();

    if !yes {
        if !is_tty {
            print_error(
                no_color,
                "Confirmation required. Pass --yes to delete non-interactively.",
            );
            std::process::exit(2);
        }
        let confirmed = dialoguer::Confirm::new()
            .with_prompt(format!("Delete '{email}'? This cannot be undone."))
            .default(false)
            .interact()?;
        if !confirmed {
            println!("Aborted.");
            return Ok(());
        }
    }

    let db = get_database(config).await?;
    db.delete_user(email).await?;
    print_success(no_color, &format!("User '{email}' deleted."));
    Ok(())
}

async fn passwd(
    email: &str,
    current_password: Option<SecretString>,
    new_password: Option<SecretString>,
    config: &Config,
    no_color: bool,
) -> Result<()> {
    let is_tty = std::io::stdin().is_terminal();

    let current = if let Some(p) = current_password {
        p
    } else {
        if !is_tty {
            print_error(no_color, "--current-password is required in non-interactive mode");
            std::process::exit(2);
        }
        let raw = dialoguer::Password::new()
            .with_prompt("Current password")
            .interact()?;
        SecretString::new(raw.into_boxed_str())
    };

    let db = get_database(config).await?;
    if !db.verify_user(email, current).await {
        print_error(no_color, "Current password is incorrect.");
        std::process::exit(1);
    }

    let new_pass = if let Some(p) = new_password {
        p
    } else {
        if !is_tty {
            print_error(no_color, "--new-password is required in non-interactive mode");
            std::process::exit(2);
        }
        let raw = dialoguer::Password::new()
            .with_prompt("New password")
            .with_confirmation("Confirm new password", "Passwords do not match")
            .interact()?;
        SecretString::new(raw.into_boxed_str())
    };

    db.change_password(email, new_pass).await?;
    print_success(no_color, &format!("Password for '{email}' updated."));
    Ok(())
}

async fn exists(email: &str, config: &Config) -> Result<()> {
    let db = get_database(config).await?;
    if db.user_exists(email).await {
        std::process::exit(0);
    } else {
        std::process::exit(1);
    }
}

async fn import(file: Option<String>, config: &Config, no_color: bool) -> Result<()> {
    use secrecy::zeroize::Zeroize;
    use std::io::BufRead;

    let input: Box<dyn std::io::Read> = match file {
        Some(path) => Box::new(std::fs::File::open(&path)?),
        None => Box::new(std::io::stdin()),
    };
    let reader = std::io::BufReader::new(input);

    let db = get_database(config).await?;
    let mut imported = 0u32;
    let mut failed = 0u32;

    for line in reader.lines() {
        // `mut` so we can zeroize before drop — the line buffer holds the password in plaintext
        let mut line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            line.zeroize();
            continue;
        }
        let Some(comma_pos) = trimmed.find(',') else {
            eprintln!("Skipping malformed line (expected email,password)");
            line.zeroize();
            failed += 1;
            continue;
        };
        let email = trimmed[..comma_pos].trim().to_lowercase();
        // Extract and immediately wrap the password, avoiding any intermediate String allocation
        let password: SecretString = trimmed[comma_pos + 1..].trim().to_string().into();
        // Zeroize the source line now — its content is no longer needed
        line.zeroize();

        if let Err(e) = db.add_user(&email).await {
            print_error(no_color, &format!("Failed to add '{email}': {e}"));
            failed += 1;
            continue;
        }
        if let Err(e) = db.change_password(&email, password).await {
            print_error(no_color, &format!("Failed to set password for '{email}': {e}"));
            failed += 1;
        } else {
            imported += 1;
        }
    }

    println!("{imported} imported, {failed} failed");
    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}
