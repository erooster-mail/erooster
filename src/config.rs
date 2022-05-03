use serde::{Deserialize, Serialize};

/// The config for the mailserver
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    /// Configurations specific to the TLS part
    pub tls: Tls,
    /// Configurations specific to the mail concept itself
    pub mail: Mail,
    /// IP the server should listen on instead of any
    pub listen_ips: Option<Vec<String>>,
    /// Configurations specific to the Database
    pub database: Database,
}

/// Configurations specific to the Database
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Database {
    /// Path to the sqlite db
    #[cfg(feature = "sqlite")]
    pub sqlite_path: String,
    /// Connection string for the postgres database
    #[cfg(feature = "postgres")]
    pub postgres_url: String,
}

/// Configurations specific to the TLS part
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Tls {
    /// Path to the key file
    pub key_path: String,
    /// Path to the certificate file
    pub cert_path: String,
}

/// Configurations specific to the mail concept itself
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Mail {
    /// Path where maildir style mailboxes are going to get created
    pub maildir_folders: String,
    /// Hostname the SMTP server lives at.
    pub hostname: String,
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
