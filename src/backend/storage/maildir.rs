use crate::backend::{
    database::{Database, DB},
    storage::{MailEntry, MailStorage},
};
use futures::{StreamExt, TryStreamExt};
use maildir::Maildir;
use mailparse::ParsedMail;
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::{fs::OpenOptions, io::AsyncWriteExt};
use tokio_stream::wrappers::LinesStream;
use tracing::instrument;

/// The Storage handler for the maildir format
pub struct MaildirStorage {
    db: DB,
}

impl MaildirStorage {
    /// Create a new maildir storage handler
    #[must_use]
    #[instrument(skip(db))]
    pub fn new(db: DB) -> Self {
        MaildirStorage { db }
    }
}

#[async_trait::async_trait]
impl MailStorage<MaildirMailEntry> for MaildirStorage {
    #[instrument(skip(self, mailbox_path))]
    fn get_uid_for_folder(&self, mailbox_path: String) -> color_eyre::eyre::Result<u32> {
        let maildir = Maildir::from(mailbox_path);
        let current_last_id: u32 = maildir.count_cur().try_into()?;
        let new_last_id: u32 = maildir.count_new().try_into()?;
        Ok(current_last_id + new_last_id)
    }

    #[instrument(skip(self, path))]
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

    #[instrument(skip(self, path))]
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

    #[instrument(skip(self, path))]
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
            let line = format!("{}\n", line);
            file.write_all(line.as_bytes()).await?;
        }

        Ok(())
    }

    #[instrument(skip(self, mailbox_path))]
    fn create_dirs(&self, mailbox_path: String) -> color_eyre::eyre::Result<()> {
        let maildir = Maildir::from(mailbox_path);
        maildir.create_dirs().map_err(Into::into)
    }

    #[instrument(skip(self, path, data))]
    async fn store_new(&self, path: String, data: &[u8]) -> color_eyre::eyre::Result<String> {
        let maildir = Maildir::from(path);
        let maildir_id = maildir.store_new(data)?;
        sqlx::query("INSERT INTO mails (maildir_id) VALUES ($1)")
            .bind(maildir_id.clone())
            .execute(self.db.get_pool())
            .await?;
        Ok(maildir_id)
    }

    #[instrument(skip(self, path))]
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

    #[instrument(skip(self, path))]
    fn count_cur(&self, path: String) -> usize {
        let maildir = Maildir::from(path);
        maildir.count_cur()
    }

    #[instrument(skip(self, path))]
    fn count_new(&self, path: String) -> usize {
        let maildir = Maildir::from(path);
        maildir.count_new()
    }

    #[instrument(skip(self, path))]
    async fn list_cur(&self, path: String) -> Vec<MaildirMailEntry> {
        let maildir = Maildir::from(path);
        let mail_rows: Vec<DbMails> = sqlx::query_as::<_, DbMails>("SELECT * FROM mails")
            .fetch(self.db.get_pool())
            .filter_map(|x| async move { x.ok() })
            .collect()
            .await;
        maildir
            .list_cur()
            .filter_map(|x| match x {
                Ok(x) => {
                    let maildir_id = x.id();
                    let uid = mail_rows
                        .iter()
                        .find(|y| y.maildir_id == maildir_id)
                        .map_or(0, |y| y.id);
                    Some(MaildirMailEntry { uid, entry: x })
                }
                Err(_) => None,
            })
            .collect()
    }

    #[instrument(skip(self, path))]
    async fn list_new(&self, path: String) -> Vec<MaildirMailEntry> {
        let maildir = Maildir::from(path);
        let mail_rows: Vec<DbMails> = sqlx::query_as::<_, DbMails>("SELECT * FROM mails")
            .fetch(self.db.get_pool())
            .filter_map(|x| async move { x.ok() })
            .collect()
            .await;
        maildir
            .list_new()
            .filter_map(|x| match x {
                Ok(x) => {
                    let maildir_id = x.id();
                    let uid = mail_rows
                        .iter()
                        .find(|y| y.maildir_id == maildir_id)
                        .map_or(0, |y| y.id);
                    Some(MaildirMailEntry { uid, entry: x })
                }
                Err(_) => None,
            })
            .collect()
    }

    #[instrument(skip(self, path))]
    async fn list_all(&self, path: String) -> Vec<MaildirMailEntry> {
        let maildir = Maildir::from(path);
        let mail_rows: Vec<DbMails> = sqlx::query_as::<_, DbMails>("SELECT * FROM mails")
            .fetch(self.db.get_pool())
            .filter_map(|x| async move { x.ok() })
            .collect()
            .await;
        maildir
            .list_new()
            .chain(maildir.list_cur())
            .filter_map(|x| match x {
                Ok(x) => {
                    let maildir_id = x.id();
                    let uid = mail_rows
                        .iter()
                        .find(|y| y.maildir_id == maildir_id)
                        .map_or(0, |y| y.id);
                    Some(MaildirMailEntry { uid, entry: x })
                }
                Err(_) => None,
            })
            .collect()
    }
}

#[derive(sqlx::FromRow)]
struct DbMails {
    id: i64,
    #[allow(dead_code)]
    maildir_id: String,
}

/// Wrapper for the mailentries from the Maildir crate
pub struct MaildirMailEntry {
    entry: maildir::MailEntry,
    uid: i64,
}

#[async_trait::async_trait]
impl MailEntry for MaildirMailEntry {
    #[instrument(skip(self))]
    fn uid(&self) -> i64 {
        self.uid
    }

    #[instrument(skip(self))]
    fn id(&self) -> &str {
        self.entry.id()
    }

    #[instrument(skip(self))]
    fn parsed(&mut self) -> color_eyre::eyre::Result<ParsedMail> {
        self.entry.parsed().map_err(Into::into)
    }

    #[instrument(skip(self))]
    fn headers(&mut self) -> color_eyre::eyre::Result<Vec<mailparse::MailHeader>> {
        self.entry.headers().map_err(Into::into)
    }

    #[instrument(skip(self))]
    fn received(&mut self) -> color_eyre::eyre::Result<i64> {
        self.entry.received().map_err(Into::into)
    }

    #[instrument(skip(self))]
    fn date(&mut self) -> color_eyre::eyre::Result<i64> {
        self.entry.date().map_err(Into::into)
    }

    #[instrument(skip(self))]
    fn flags(&self) -> &str {
        self.entry.flags()
    }

    #[instrument(skip(self))]
    fn is_draft(&self) -> bool {
        self.entry.is_draft()
    }

    #[instrument(skip(self))]
    fn is_flagged(&self) -> bool {
        self.entry.is_flagged()
    }

    #[instrument(skip(self))]
    fn is_passed(&self) -> bool {
        self.entry.is_passed()
    }

    #[instrument(skip(self))]
    fn is_replied(&self) -> bool {
        self.entry.is_replied()
    }

    #[instrument(skip(self))]
    fn is_seen(&self) -> bool {
        self.entry.is_seen()
    }

    #[instrument(skip(self))]
    fn is_trashed(&self) -> bool {
        self.entry.is_trashed()
    }

    #[instrument(skip(self))]
    fn path(&self) -> &std::path::PathBuf {
        self.entry.path()
    }
}

#[instrument(skip(filename))]
async fn lines_from_file(filename: impl AsRef<Path>) -> std::io::Result<Vec<String>> {
    LinesStream::new(BufReader::new(File::open(filename).await?).lines())
        .try_collect::<Vec<String>>()
        .await
}
