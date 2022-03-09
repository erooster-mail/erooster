use async_trait::async_trait;

pub(crate) mod encrypted;
pub(crate) mod unencrypted;

/// An implementation of a imap server
#[async_trait]
pub trait Server {
    /// Start the server
    async fn run() -> anyhow::Result<()>;
}

pub use encrypted::Encrypted;
pub use unencrypted::Unencrypted;
