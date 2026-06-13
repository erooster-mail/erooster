// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! Test utilities for creating isolated per-test storage instances.
//!
//! Each call to [`setup_test_storage`] creates a unique named in-memory SQLite
//! database so parallel tests don't interfere with each other.

use crate::{
    backend::{
        database::{get_database, DB},
        storage::{self, Storage},
    },
    config::{Config, Database, Mail, MessageSize, Tls, Webserver},
};
use {color_eyre::Result, uuid::Uuid};

/// Creates an isolated in-memory SQLite storage instance for a single test.
///
/// Each call generates a unique database name, so tests running in parallel
/// don't share any state. The maildir is also unique per call to avoid
/// filesystem collisions.
///
/// # Errors
///
/// Returns an error if the database cannot be initialised (migrations fail, etc.)
pub async fn setup_test_storage() -> Result<(Config, Storage)> {
    let id = Uuid::new_v4().simple().to_string();
    let config = Config {
        database: Database {
            url: format!("sqlite:file:{id}?mode=memory&cache=shared"),
        },
        mail: Mail {
            maildir_folders: format!("/tmp/erooster-test-{id}"),
            hostname: "localhost".to_string(),
            displayname: "Erooster Test".to_string(),
            dkim_key_path: "/tmp/dkim.private".to_string(),
            dkim_key_selector: "default".to_string(),
            max_message_size: MessageSize(25 * 1_048_576),
        },
        tls: Tls {
            key_path: "./certs/key.pem".to_string(),
            cert_path: "./certs/cert.pem".to_string(),
        },
        webserver: Webserver {
            port: 8080,
            tls: false,
        },
        rspamd: None,
        task_folder: format!("/tmp/erooster-tasks-{id}"),
        listen_ips: None,
    };
    let database: DB = get_database(&config).await?;
    let storage = storage::get_storage(database, config.clone());
    Ok((config, storage))
}
