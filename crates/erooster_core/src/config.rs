// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use erooster_deps::{
    color_eyre,
    serde::{self, Deserialize, Serialize},
    serde_yaml, tokio,
};

const fn default_webserver_port() -> u16 {
    8080
}

/// The config for the mailserver
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(crate = "self::serde")]
pub struct Config {
    /// Configurations specific to the TLS part
    pub tls: Tls,
    /// Configurations specific to the mail concept itself
    pub mail: Mail,
    /// IP the server should listen on instead of any
    pub listen_ips: Option<Vec<String>>,
    /// Configurations specific to the Database
    pub database: Database,
    /// The config of the webserver
    pub webserver: Webserver,
    /// The config related to the optional rspamd integration
    pub rspamd: Option<Rspamd>,
    /// Folder where the pending tasks are stored
    pub task_folder: String,
}

/// The config for the webserver
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(crate = "self::serde")]
pub struct Webserver {
    /// The port of the webserver
    #[serde(default = "default_webserver_port")]
    pub port: u16,
    /// If enabled the webserver will use TLS
    #[serde(default)]
    pub tls: bool,
}

/// Configurations specific to the Database
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(crate = "self::serde")]
pub struct Database {
    /// Connection string for the database
    pub url: String,
}

/// Configurations specific to the TLS part
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(crate = "self::serde")]
pub struct Tls {
    /// Path to the key file
    pub key_path: String,
    /// Path to the certificate file
    pub cert_path: String,
}

/// Configuration specific to rspamd
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(crate = "self::serde")]
pub struct Rspamd {
    /// Address where the rspamd worker is at
    pub address: String,
}

/// Configurations specific to the mail concept itself
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(crate = "self::serde")]
pub struct Mail {
    /// Path where maildir style mailboxes are going to get created
    pub maildir_folders: String,
    /// Hostname the SMTP server lives at.
    pub hostname: String,
    /// The Displayname to be used in software like thunderbird
    pub displayname: String,
    /// The private dkim key in rsa format
    ///
    /// Use this to generate the key:
    ///
    /// ```bash
    /// opendkim-genkey \
    /// --domain=<hostname> \
    /// --subdomains
    /// ```
    pub dkim_key_path: String,
    /// The selector to be used in the dkim header
    pub dkim_key_selector: String,
}

impl Config {
    /// Loads the config file to the struct
    ///
    /// # Errors
    ///
    /// Does return io errors if something goes wrong
    pub async fn load<P: AsRef<std::path::Path> + std::fmt::Debug>(
        path: P,
    ) -> color_eyre::eyre::Result<Self> {
        let contents = tokio::fs::read_to_string(path).await?;
        let config: Self = serde_yaml::from_str(&contents)?;
        Ok(config)
    }
}
