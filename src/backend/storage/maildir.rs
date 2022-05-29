use crate::backend::storage::{MailEntry, MailStorage};
use futures::TryStreamExt;
use maildir::Maildir;
use mailparse::ParsedMail;
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::{fs::OpenOptions, io::AsyncWriteExt};
use tokio_stream::wrappers::LinesStream;

/// The Storage handler for the maildir format
pub struct MaildirStorage;

impl MaildirStorage {
    /// Create a new maildir storage handler
    #[must_use]
    pub const fn new() -> Self {
        MaildirStorage
    }
}

impl Default for MaildirStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl MailStorage<MaildirMailEntry> for MaildirStorage {
    fn get_uid_for_folder(&self, mailbox_path: String) -> color_eyre::eyre::Result<u32> {
        let maildir = Maildir::from(mailbox_path);
        let current_last_id: u32 = maildir.count_cur().try_into()?;
        let new_last_id: u32 = maildir.count_new().try_into()?;
        Ok(current_last_id + new_last_id)
    }

    async fn get_flags(&self, path: &Path) -> std::io::Result<Vec<String>> {
        let flags_file = path.join(".erooster_folder_flags");
        if flags_file.exists() {
            let file = File::open(flags_file).await?;
            let buf = BufReader::new(file);
            let flags = LinesStream::new(buf.lines()).try_collect().await;
            return flags;
        }
        Ok(vec![])
    }

    async fn add_flag(&self, path: &Path, flag: &str) -> color_eyre::eyre::Result<()> {
        let flags_file = path.join(".erooster_folder_flags");
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(flags_file)
            .await?;
        file.write_all(flag.as_bytes()).await?;
        Ok(())
    }

    async fn remove_flag(&self, path: &Path, flag: &str) -> color_eyre::eyre::Result<()> {
        let flags_file = path.join(".erooster_folder_flags");
        let mut lines = lines_from_file(&flags_file).await?;

        lines.retain(|x| x != flag);

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(flags_file)
            .await?;

        for line in lines {
            file.write_all(line.as_bytes()).await?;
        }

        Ok(())
    }

    fn create_dirs(&self, mailbox_path: String) -> color_eyre::eyre::Result<()> {
        let maildir = Maildir::from(mailbox_path);
        maildir.create_dirs().map_err(Into::into)
    }

    fn store_new(&self, path: String, data: &[u8]) -> color_eyre::eyre::Result<String> {
        let maildir = Maildir::from(path);
        maildir.store_new(data).map_err(Into::into)
    }
    fn list_subdirs(&self, path: String) -> color_eyre::eyre::Result<Vec<PathBuf>> {
        let maildir = Maildir::from(path);
        Ok(maildir
            .list_subdirs()
            .filter_map(|x| match x {
                Ok(x) => Some(x.path().to_path_buf()),
                Err(_) => None,
            })
            .collect())
    }
    fn count_cur(&self, path: String) -> usize {
        let maildir = Maildir::from(path);
        maildir.count_cur()
    }
    fn count_new(&self, path: String) -> usize {
        let maildir = Maildir::from(path);
        maildir.count_new()
    }
    fn list_cur(&self, path: String) -> Vec<MaildirMailEntry> {
        let maildir = Maildir::from(path);
        maildir
            .list_cur()
            .filter_map(|x| match x {
                Ok(x) => Some(MaildirMailEntry(x)),
                Err(_) => None,
            })
            .collect()
    }
    fn list_new(&self, path: String) -> Vec<MaildirMailEntry> {
        let maildir = Maildir::from(path);
        maildir
            .list_new()
            .filter_map(|x| match x {
                Ok(x) => Some(MaildirMailEntry(x)),
                Err(_) => None,
            })
            .collect()
    }
}

/// Wrapper for the mailentries from the Maildir crate
pub struct MaildirMailEntry(maildir::MailEntry);

impl MailEntry for MaildirMailEntry {
    fn id(&self) -> &str {
        self.0.id()
    }
    fn parsed(&mut self) -> color_eyre::eyre::Result<ParsedMail> {
        self.0.parsed().map_err(Into::into)
    }

    fn headers(&mut self) -> color_eyre::eyre::Result<Vec<mailparse::MailHeader>> {
        self.0.headers().map_err(Into::into)
    }

    fn received(&mut self) -> color_eyre::eyre::Result<i64> {
        self.0.received().map_err(Into::into)
    }

    fn date(&mut self) -> color_eyre::eyre::Result<i64> {
        self.0.date().map_err(Into::into)
    }

    fn flags(&self) -> &str {
        self.0.flags()
    }

    fn is_draft(&self) -> bool {
        self.0.is_draft()
    }

    fn is_flagged(&self) -> bool {
        self.0.is_flagged()
    }

    fn is_passed(&self) -> bool {
        self.0.is_passed()
    }

    fn is_replied(&self) -> bool {
        self.0.is_replied()
    }

    fn is_seen(&self) -> bool {
        self.0.is_seen()
    }

    fn is_trashed(&self) -> bool {
        self.0.is_trashed()
    }

    fn path(&self) -> &std::path::PathBuf {
        self.0.path()
    }
}

async fn lines_from_file(filename: impl AsRef<Path>) -> std::io::Result<Vec<String>> {
    LinesStream::new(BufReader::new(File::open(filename).await?).lines())
        .try_collect::<Vec<String>>()
        .await
}
