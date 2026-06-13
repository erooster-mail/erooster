// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use {
    color_eyre,
    serde::{self, Deserialize, Deserializer, Serialize, Serializer},
    serde_saphyr, tokio,
};

const fn default_webserver_port() -> u16 {
    8080
}

const fn default_webserver_tls() -> bool {
    false
}

const fn default_max_message_size() -> MessageSize {
    MessageSize(25 * 1_048_576) // 25 MB
}

/// A message size value that can be written in the config as a human-readable
/// string (`"25 MB"`, `"1 GB"`, `"500 KB"`) or as a plain number (bytes).
///
/// Accepted units (case-insensitive, space between number and unit is optional):
/// `B`, `KB` / `KiB`, `MB` / `MiB`, `GB` / `GiB`, `TB` / `TiB`.
/// All binary multiples (1 KB = 1024 bytes, 1 MB = 1 048 576 bytes, etc.).
#[derive(Debug, Clone, Copy)]
pub struct MessageSize(pub u64);

impl MessageSize {
    /// Returns the size in bytes.
    #[must_use]
    pub const fn as_bytes(self) -> u64 {
        self.0
    }
}

impl Serialize for MessageSize {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(self.0)
    }
}

impl<'de> Deserialize<'de> for MessageSize {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = MessageSize;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(
                    "a size as a string (e.g. \"25 MB\", \"1 GB\", \"500 KB\") \
                     or a plain number of bytes",
                )
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<MessageSize, E> {
                Ok(MessageSize(v))
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<MessageSize, E> {
                if v < 0 {
                    return Err(E::custom("message size cannot be negative"));
                }
                Ok(MessageSize(v.cast_unsigned()))
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<MessageSize, E> {
                parse_size(v).map_err(E::custom)
            }
        }
        d.deserialize_any(Visitor)
    }
}

fn parse_size(s: &str) -> Result<MessageSize, String> {
    let s = s.trim();
    // Split into numeric prefix and unit suffix.
    let split_pos = s
        .find(|c: char| c.is_alphabetic())
        .unwrap_or(s.len());
    let (num_str, unit_str) = s.split_at(split_pos);
    let num: u64 = num_str
        .trim()
        .parse()
        .map_err(|_| format!("cannot parse number from \"{s}\""))?;
    let multiplier: u64 = match unit_str.trim().to_uppercase().as_str() {
        "" | "B" => 1,
        "KB" | "KIB" => 1_024,
        "MB" | "MIB" => 1_048_576,
        "GB" | "GIB" => 1_073_741_824,
        "TB" | "TIB" => 1_099_511_627_776,
        other => return Err(format!("unknown size unit \"{other}\"; use B, KB, MB, GB, or TB")),
    };
    Ok(MessageSize(num * multiplier))
}

/// The complete configuration for the Erooster mail server.
///
/// Erooster reads this file on startup. After changing any value, restart the
/// server for it to take effect.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(crate = "self::serde")]
pub struct Config {
    /// TLS (encryption) settings — certificates and private keys.
    pub tls: Tls,

    /// Core mail settings — your domain name, mailbox storage, DKIM signing.
    pub mail: Mail,

    /// IP addresses the server should listen on.
    ///
    /// Leave this out (or set to null) to listen on all interfaces (`0.0.0.0`),
    /// which is the right choice for most single-server setups.
    ///
    /// Example to listen only on a specific IP:
    /// ```yaml
    /// listen_ips:
    ///   - "203.0.113.42"
    /// ```
    pub listen_ips: Option<Vec<String>>,

    /// Database connection settings.
    pub database: Database,

    /// Settings for the built-in web interface (autoconfig + admin page).
    pub webserver: Webserver,

    /// Optional spam-filter integration with Rspamd.
    ///
    /// Remove this section entirely if you are not running Rspamd.
    pub rspamd: Option<Rspamd>,

    /// Folder on disk where background task state is kept.
    ///
    /// This is used internally by the mail queue. You usually do not need to
    /// change this. Make sure the folder exists and is writable by the server.
    pub task_folder: String,
}

/// Settings for the built-in web interface.
///
/// The web interface serves the autoconfig endpoint (`/.well-known/autoconfig/`)
/// that lets mail clients like Thunderbird configure themselves automatically.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(crate = "self::serde")]
pub struct Webserver {
    /// Port the web interface listens on. Defaults to `8080`.
    ///
    /// If you put a reverse proxy (nginx, Caddy) in front of Erooster you can
    /// keep this on `8080` and expose port 80/443 through the proxy.
    #[serde(default = "default_webserver_port")]
    pub port: u16,

    /// Whether the web interface should use TLS directly.
    ///
    /// Set to `true` only if you are NOT using a reverse proxy and want
    /// Erooster to terminate HTTPS itself using the certificate in `tls`.
    /// Defaults to `false`.
    #[serde(default = "default_webserver_tls")]
    pub tls: bool,
}

/// Database connection settings.
///
/// Erooster needs a database to store user accounts and the outbound mail queue.
/// `PostgreSQL` is recommended for production; `SQLite` works well for testing and
/// small single-user setups.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(crate = "self::serde")]
pub struct Database {
    /// Database connection string.
    ///
    /// **`PostgreSQL`** (recommended):
    /// ```yaml
    /// url: "postgres://user:password@localhost/erooster"
    /// ```
    ///
    /// **`SQLite`** (simple/testing):
    /// ```yaml
    /// url: "sqlite:///var/lib/erooster/erooster.db"
    /// ```
    pub url: String,
}

/// TLS (encryption) certificate settings.
///
/// Erooster needs a valid TLS certificate to encrypt connections to your mail
/// clients and other mail servers. The easiest way to get one is with
/// [Let's Encrypt](https://letsencrypt.org/) / `certbot` or `acme.sh`.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(crate = "self::serde")]
pub struct Tls {
    /// Path to the private key file (PEM format).
    ///
    /// Example: `"/etc/letsencrypt/live/mail.example.com/privkey.pem"`
    pub key_path: String,

    /// Path to the certificate file (PEM format, full chain).
    ///
    /// Example: `"/etc/letsencrypt/live/mail.example.com/fullchain.pem"`
    pub cert_path: String,
}

/// Optional Rspamd spam-filter integration.
///
/// [Rspamd](https://rspamd.com/) is a fast, open-source spam filter. When
/// configured here, Erooster will check every incoming message through Rspamd
/// and act on the result (reject, greylist, or add a header).
///
/// To disable spam filtering, remove the `rspamd:` section from your config.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(crate = "self::serde")]
pub struct Rspamd {
    /// HTTP address of the Rspamd worker process.
    ///
    /// If Rspamd runs on the same machine: `"http://127.0.0.1:11333"`
    pub address: String,
}

/// Core mail server settings.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(crate = "self::serde")]
pub struct Mail {
    /// Directory where mailboxes are stored on disk (Maildir format).
    ///
    /// Each user gets their own sub-folder here. Make sure the directory
    /// exists and is writable by the server process.
    ///
    /// Example: `"/var/mail/erooster"`
    pub maildir_folders: String,

    /// The domain name of your mail server (what appears after the `@`).
    ///
    /// This must match the domain you set up MX records for.
    ///
    /// Example: `"mail.example.com"`
    pub hostname: String,

    /// A human-readable name shown to mail clients during auto-configuration.
    ///
    /// This is what Thunderbird and other clients will display as the account
    /// name when they auto-discover your server.
    ///
    /// Example: `"Example Mail"`
    pub displayname: String,

    /// Path to your DKIM private key file (RSA, PEM format).
    ///
    /// DKIM signs outgoing emails so receiving servers can verify they
    /// genuinely came from your domain. Without it, your mail is more likely
    /// to be marked as spam.
    ///
    /// Generate a key pair with:
    /// ```bash
    /// opendkim-genkey --domain=example.com --subdomains
    /// ```
    /// This creates `default.private` (the key) and `default.txt` (the DNS
    /// record you need to add).
    ///
    /// Example: `"/etc/erooster/dkim/example.com.private"`
    pub dkim_key_path: String,

    /// The DKIM selector — a short label that links the signing key to the
    /// matching public key published in your DNS.
    ///
    /// This must match the prefix of the DNS TXT record you published
    /// (e.g. if you published `default._domainkey.example.com`, the selector
    /// is `"default"`).
    pub dkim_key_selector: String,

    /// Maximum size of a single email.
    ///
    /// Emails larger than this are rejected before they are fully received,
    /// saving bandwidth. This limit is advertised to other mail servers.
    ///
    /// You can write this as a human-readable string or as a plain number
    /// of bytes:
    ///
    /// ```yaml
    /// max_message_size: "25 MB"   # 25 megabytes (default, matches Gmail)
    /// max_message_size: "50 MB"   # 50 megabytes
    /// max_message_size: "1 GB"    # 1 gigabyte
    /// max_message_size: 26214400  # raw bytes (25 MiB)
    /// ```
    ///
    /// Accepted units: `B`, `KB`, `MB`, `GB`, `TB` (and `KiB`/`MiB`/`GiB`/`TiB`).
    #[serde(default = "default_max_message_size")]
    pub max_message_size: MessageSize,
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
        let config: Self = serde_saphyr::from_str(&contents)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_size_bytes() {
        assert_eq!(parse_size("1024").unwrap().0, 1024);
        assert_eq!(parse_size("0 B").unwrap().0, 0);
        assert_eq!(parse_size("512B").unwrap().0, 512);
    }

    #[test]
    fn parse_size_kb() {
        assert_eq!(parse_size("1 KB").unwrap().0, 1_024);
        assert_eq!(parse_size("1KiB").unwrap().0, 1_024);
        assert_eq!(parse_size("10kb").unwrap().0, 10_240);
    }

    #[test]
    fn parse_size_mb() {
        assert_eq!(parse_size("25 MB").unwrap().0, 25 * 1_048_576);
        assert_eq!(parse_size("25MiB").unwrap().0, 25 * 1_048_576);
    }

    #[test]
    fn parse_size_gb() {
        assert_eq!(parse_size("1 GB").unwrap().0, 1_073_741_824);
        assert_eq!(parse_size("2GiB").unwrap().0, 2 * 1_073_741_824);
    }

    #[test]
    fn parse_size_error() {
        assert!(parse_size("25 XB").is_err());
        assert!(parse_size("abc").is_err());
    }
}
