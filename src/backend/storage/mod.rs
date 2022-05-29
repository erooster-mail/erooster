use crate::backend::{
    database::DB,
    storage::maildir::{MaildirMailEntry, MaildirStorage},
};
use mailparse::{MailHeader, ParsedMail};
use std::path::{Path, PathBuf};

/// The maildir format
#[cfg(feature = "maildir")]
pub mod maildir;

/// The current storage type
#[cfg(feature = "maildir")]
pub type Storage = MaildirStorage;

/// The current `MailEntry` type
#[cfg(feature = "maildir")]
pub type MailEntryType = MaildirMailEntry;

/// Representation of a Mail entry
#[async_trait::async_trait]
pub trait MailEntry {
    /// The uid of the mail entry
    async fn uid(&self) -> color_eyre::eyre::Result<i64>;
    /// The id of the email
    fn id(&self) -> &str;
    /// The parsed form of the email
    fn parsed(&mut self) -> color_eyre::eyre::Result<ParsedMail>;
    /// The parsed headers of the email
    fn headers(&mut self) -> color_eyre::eyre::Result<Vec<MailHeader>>;
    /// The received time of the email
    fn received(&mut self) -> color_eyre::eyre::Result<i64>;
    /// The date of the email
    fn date(&mut self) -> color_eyre::eyre::Result<i64>;
    /// The flags of the email
    fn flags(&self) -> &str;
    /// Whether the email is a draft
    fn is_draft(&self) -> bool;
    /// Whether the email is flagged
    fn is_flagged(&self) -> bool;
    /// Whether the email is passed
    fn is_passed(&self) -> bool;
    /// Whether the email was replied to
    fn is_replied(&self) -> bool;
    /// Whether the email was seen
    fn is_seen(&self) -> bool;
    /// Whether the email was trashed
    fn is_trashed(&self) -> bool;
    /// The path of the email
    fn path(&self) -> &PathBuf;
}

/// Abstract Storage definition
//
// Note for future readers:
// These are methods as other storage types may need to store some state in the struct
#[async_trait::async_trait]
pub trait MailStorage<M: MailEntry> {
    /// Get the current UID for the folder
    fn get_uid_for_folder(&self, path: String) -> color_eyre::eyre::Result<u32>;
    /// Get the current flags for the folder
    async fn get_flags(&self, path: &Path) -> std::io::Result<Vec<String>>;
    /// Set a new flag for the folder
    async fn add_flag(&self, path: &Path, flag: &str) -> color_eyre::eyre::Result<()>;
    /// Remove a flag from the folder
    async fn remove_flag(&self, path: &Path, flag: &str) -> color_eyre::eyre::Result<()>;
    /// Creates the required folder structure
    fn create_dirs(&self, path: String) -> color_eyre::eyre::Result<()>;
    /// Store new message
    async fn store_new(&self, path: String, data: &[u8]) -> color_eyre::eyre::Result<String>;
    /// List the subfolders
    fn list_subdirs(&self, path: String) -> color_eyre::eyre::Result<Vec<PathBuf>>;
    /// Count of current messages
    fn count_cur(&self, path: String) -> usize;
    /// Count of new messages
    fn count_new(&self, path: String) -> usize;
    /// Get the current messages
    fn list_cur(&self, path: String) -> Vec<M>;
    /// Get the new messages
    fn list_new(&self, path: String) -> Vec<M>;
    /// Get the all messages
    fn list_all(&self, path: String) -> Vec<M>;
}

/// Get the struct of the current storage implementation
#[cfg(feature = "maildir")]
#[must_use]
pub const fn get_storage(db: DB) -> Storage {
    MaildirStorage::new(db)
}
