use crate::{
    backend::{
        database::DB,
        storage::maildir::{MaildirMailEntry, MaildirStorage},
    },
    config::Config,
};
use mailparse::{MailHeader, ParsedMail};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::instrument;

/// The maildir format
#[cfg(feature = "maildir")]
pub mod maildir;

/// The current storage type
#[cfg(feature = "maildir")]
pub type Storage = MaildirStorage;

/// The current `MailEntry` type
#[cfg(feature = "maildir")]
pub type MailEntryType = MaildirMailEntry;

/// The state of a message
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailState {
    /// Messages that have been marked as read
    Read,
    /// Messaages that have been marked as unread
    New,
}

/// Representation of a Mail entry
#[async_trait::async_trait]
pub trait MailEntry {
    /// The uid of the mail entry
    fn uid(&self) -> u32;
    /// The metadata modification sequence of the mail entry
    fn modseq(&self) -> u64;
    /// The state (new or read) of the mail entry
    fn mail_state(&self) -> MailState;
    /// The sequence number of the mail entry
    fn sequence_number(&self) -> Option<u32>;
    /// The id of the email
    fn id(&self) -> &str;
    /// The parsed form of the email
    fn parsed(&mut self) -> color_eyre::eyre::Result<ParsedMail>;
    /// The parsed headers of the email
    fn headers(&mut self) -> color_eyre::eyre::Result<Vec<MailHeader>>;
    /// The received time of the email
    fn received(&mut self) -> color_eyre::eyre::Result<i64>;
    /// The sent time of the email
    fn sent(&mut self) -> color_eyre::eyre::Result<i64>;
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
    /// If the whole message contains a certain string (case sensitive)
    fn text_contains(&mut self, string: &str) -> bool;
    /// If the body contains a certain string (case sensitive)
    fn body_contains(&mut self, string: &str) -> bool;
    /// The size of the raw body in octets
    fn body_size(&mut self) -> u64;
}

/// Abstract Storage definition
//
// Note for future readers:
// These are methods as other storage types may need to store some state in the struct
#[async_trait::async_trait]
pub trait MailStorage<M: MailEntry> {
    /// Get the current UID for the folder
    fn get_uid_for_folder(&self, path: &Path) -> color_eyre::eyre::Result<u32>;
    /// Get the current flags for the folder
    async fn get_flags(&self, path: &Path) -> std::io::Result<Vec<String>>;
    /// Set a new flag for the folder
    async fn add_flag(&self, path: &Path, flag: &str) -> color_eyre::eyre::Result<()>;
    /// Remove a flag from the folder
    async fn remove_flag(&self, path: &Path, flag: &str) -> color_eyre::eyre::Result<()>;
    /// Creates the required folder structure
    fn create_dirs(&self, path: &Path) -> color_eyre::eyre::Result<()>;
    /// Store new message
    async fn store_new(
        &self,
        mailbox: String,
        path: &Path,
        data: &[u8],
    ) -> color_eyre::eyre::Result<String>;
    /// Store a message
    async fn store_cur_with_flags(
        &self,
        mailbox: String,
        path: &Path,
        data: &[u8],
        flags: Vec<String>,
    ) -> color_eyre::eyre::Result<String>;
    /// List the subfolders
    fn list_subdirs(&self, path: &Path) -> color_eyre::eyre::Result<Vec<PathBuf>>;
    /// Count of current messages
    fn count_cur(&self, path: &Path) -> usize;
    /// Count of new messages
    fn count_new(&self, path: &Path) -> usize;
    /// Get the current messages
    async fn list_cur(&self, mailbox: String, path: &Path) -> Vec<M>;
    /// Get the new messages
    async fn list_new(&self, mailbox: String, path: &Path) -> Vec<M>;
    /// Get the all messages
    async fn list_all(&self, mailbox: String, path: &Path) -> Vec<M>;
    /// Get message by non unique id
    async fn find(&self, path: &Path, id: &str) -> Option<M>;
    /// Move mail to current folder and set flags
    fn move_new_to_cur_with_flags(
        &self,
        path: &Path,
        id: &str,
        imap_flags: &[&str],
    ) -> color_eyre::eyre::Result<()>;
    /// Add flags to email
    fn add_flags(&self, path: &Path, id: &str, imap_flags: &[&str])
        -> color_eyre::eyre::Result<()>;
    /// Set flags of an email
    fn set_flags(&self, path: &Path, id: &str, imap_flags: &[&str])
        -> color_eyre::eyre::Result<()>;
    /// Remove flags of an email
    fn remove_flags(
        &self,
        path: &Path,
        id: &str,
        imap_flags: &[&str],
    ) -> color_eyre::eyre::Result<()>;
    /// Converts the imap path to a local path
    fn to_ondisk_path(&self, path: String, username: String) -> color_eyre::eyre::Result<PathBuf>;
    /// Converts the imap path to a local path name
    fn to_ondisk_path_name(&self, path: String) -> color_eyre::eyre::Result<String>;
}

/// Get the struct of the current storage implementation
#[cfg(feature = "maildir")]
#[must_use]
#[instrument(skip(db))]
pub fn get_storage(db: DB, config: Arc<Config>) -> Storage {
    MaildirStorage::new(db, config)
}
