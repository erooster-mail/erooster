use crate::{
    backend::{
        database::{Database, DB},
        storage::{MailEntry, MailState, MailStorage},
    },
    config::Config,
};
use futures::{StreamExt, TryStreamExt};
use maildir::Maildir;
use mailparse::ParsedMail;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::{fs::OpenOptions, io::AsyncWriteExt};
use tokio_stream::wrappers::LinesStream;
use tracing::{debug, instrument};

/// The Storage handler for the maildir format
#[derive(Debug, Clone)]
pub struct MaildirStorage {
    // This has an Arc deep down
    db: DB,
    config: Arc<Config>,
}

impl MaildirStorage {
    /// Create a new maildir storage handler
    #[must_use]
    #[instrument(skip(db))]
    pub fn new(db: DB, config: Arc<Config>) -> Self {
        MaildirStorage { db, config }
    }
}

#[async_trait::async_trait]
impl MailStorage<MaildirMailEntry> for MaildirStorage {
    #[instrument(skip(self, mailbox_path))]
    fn get_uid_for_folder(&self, mailbox_path: &Path) -> color_eyre::eyre::Result<u32> {
        let maildir = Maildir::from(mailbox_path.to_path_buf());
        let current_last_id: u32 = maildir.count_cur().try_into()?;
        let new_last_id: u32 = maildir.count_new().try_into()?;
        Ok(current_last_id + new_last_id)
    }

    async fn find(&self, path: &Path, id: &str) -> Option<MaildirMailEntry> {
        let maildir = Maildir::from(path.to_path_buf());
        let mail_rows: Vec<DbMails> = sqlx::query_as::<_, DbMails>("SELECT * FROM mails")
            .fetch(self.db.get_pool())
            .filter_map(|x| async move { x.ok() })
            .collect()
            .await;
        maildir.find(id).map(|entry| {
            let db_item = mail_rows.iter().find(|y: &&DbMails| y.maildir_id == id);
            let uid: i32 = db_item.map_or(0, |y| y.id);
            let modseq = db_item.map_or(0, |y| y.modseq);

            let mail_state = if entry.is_seen() {
                MailState::Read
            } else {
                MailState::New
            };
            MaildirMailEntry {
                uid: uid.try_into().expect("Invalid UID"),
                modseq: modseq.try_into().expect("Invalid UID"),
                entry,
                mail_state,
                sequence_number: None,
            }
        })
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
            let line = format!("{line}\n");
            file.write_all(line.as_bytes()).await?;
        }

        Ok(())
    }

    #[instrument(skip(self, mailbox_path))]
    fn create_dirs(&self, mailbox_path: &Path) -> color_eyre::eyre::Result<()> {
        let maildir = Maildir::from(mailbox_path.to_path_buf());
        maildir.create_dirs().map_err(Into::into)
    }

    #[instrument(skip(self, path, data))]
    async fn store_cur_with_flags(
        &self,
        path: &Path,
        data: &[u8],
        imap_flags: Vec<String>,
    ) -> color_eyre::eyre::Result<String> {
        let maildir = Maildir::from(path.to_path_buf());
        let maildir_flags = imap_flags
            .iter()
            .filter_map(|flag| {
                let normalized_flag = flag.to_lowercase().replace(['(', ')'], "");
                if normalized_flag == "\\seen" {
                    Some("S")
                } else if normalized_flag == "\\deleted" {
                    Some("T")
                } else if normalized_flag == "\\flagged" {
                    Some("F")
                } else if normalized_flag == "\\draft" {
                    Some("D")
                } else if normalized_flag == "\\answered" {
                    Some("R")
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("");
        let maildir_id = maildir.store_cur_with_flags(data, &maildir_flags)?;
        sqlx::query("INSERT INTO mails (maildir_id, modseq) VALUES ($1, $2)")
            .bind(maildir_id.clone())
            .bind(1i64)
            .execute(self.db.get_pool())
            .await?;
        Ok(maildir_id)
    }

    #[instrument(skip(self, path, data))]
    async fn store_new(&self, path: &Path, data: &[u8]) -> color_eyre::eyre::Result<String> {
        let maildir = Maildir::from(path.to_path_buf());
        let maildir_id = maildir.store_new(data)?;
        sqlx::query("INSERT INTO mails (maildir_id, modseq) VALUES ($1, $2)")
            .bind(maildir_id.clone())
            .bind(1i64)
            .execute(self.db.get_pool())
            .await?;
        Ok(maildir_id)
    }

    #[instrument(skip(self, path))]
    fn list_subdirs(&self, path: &Path) -> color_eyre::eyre::Result<Vec<PathBuf>> {
        let maildir = Maildir::from(path.to_path_buf());
        Ok(maildir
            .list_subdirs()
            .filter_map(|x| match x {
                Ok(x) => Some(x.path().to_path_buf()),
                Err(_) => None,
            })
            .collect())
    }

    #[instrument(skip(self, path))]
    fn count_cur(&self, path: &Path) -> usize {
        let maildir = Maildir::from(path.to_path_buf());
        maildir.count_cur()
    }

    #[instrument(skip(self, path))]
    fn count_new(&self, path: &Path) -> usize {
        let maildir = Maildir::from(path.to_path_buf());
        maildir.count_new()
    }

    #[instrument(skip(self, path))]
    async fn list_cur(&self, path: &Path) -> Vec<MaildirMailEntry> {
        let maildir = Maildir::from(path.to_path_buf());
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

                    let db_item = mail_rows.iter().find(|y| y.maildir_id == maildir_id);
                    let uid: i32 = db_item.map_or(0, |y| y.id);
                    let modseq = db_item.map_or(0, |y| y.modseq);

                    Some(MaildirMailEntry {
                        uid: uid.try_into().expect("Invalid UID"),
                        modseq: modseq.try_into().expect("Invalid UID"),
                        entry: x,
                        mail_state: MailState::Read,
                        sequence_number: None,
                    })
                }
                Err(_) => None,
            })
            .collect()
    }

    #[instrument(skip(self, path))]
    async fn list_new(&self, path: &Path) -> Vec<MaildirMailEntry> {
        let maildir = Maildir::from(path.to_path_buf());
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

                    let db_item = mail_rows.iter().find(|y| y.maildir_id == maildir_id);
                    let uid: i32 = db_item.map_or(0, |y| y.id);
                    let modseq = db_item.map_or(0, |y| y.modseq);

                    Some(MaildirMailEntry {
                        uid: uid.try_into().expect("Invalid UID"),
                        modseq: modseq.try_into().expect("Invalid UID"),
                        entry: x,
                        mail_state: MailState::New,
                        sequence_number: None,
                    })
                }
                Err(_) => None,
            })
            .collect()
    }

    #[instrument(skip(self, path))]
    async fn list_all(&self, path: &Path) -> Vec<MaildirMailEntry> {
        let maildir = Maildir::from(path.to_path_buf());
        let mail_rows: Vec<DbMails> = sqlx::query_as::<_, DbMails>("SELECT * FROM mails")
            .fetch(self.db.get_pool())
            .filter_map(|x| async move { x.ok() })
            .collect()
            .await;

        maildir
            .list_new()
            .chain(maildir.list_cur())
            .filter(std::result::Result::is_ok)
            .filter_map(|x| match x {
                Ok(x) => {
                    let maildir_id = x.id();
                    let db_item = mail_rows.iter().find(|y| y.maildir_id == maildir_id);
                    let uid: i32 = db_item.map_or(0, |y| y.id);
                    let modseq = db_item.map_or(0, |y| y.modseq);
                    let state = if x.is_seen() {
                        MailState::Read
                    } else {
                        MailState::New
                    };
                    Some(MaildirMailEntry {
                        uid: uid.try_into().expect("Invalid UID"),
                        modseq: modseq.try_into().expect("Invalid Modseq"),
                        entry: x,
                        mail_state: state,
                        sequence_number: None,
                    })
                }
                Err(_) => None,
            })
            .collect()
    }

    #[instrument(skip(self, path))]
    fn move_new_to_cur_with_flags(
        &self,
        path: &Path,
        id: &str,
        imap_flags: &[&str],
    ) -> color_eyre::eyre::Result<()> {
        let maildir = Maildir::from(path.to_path_buf());
        let maildir_flags = imap_flags
            .iter()
            .filter_map(|flag| {
                let normalized_flag = flag.to_lowercase().replace(['(', ')'], "");
                if normalized_flag == "\\seen" {
                    Some("S")
                } else if normalized_flag == "\\deleted" {
                    Some("T")
                } else if normalized_flag == "\\flagged" {
                    Some("F")
                } else if normalized_flag == "\\draft" {
                    Some("D")
                } else if normalized_flag == "\\answered" {
                    Some("R")
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("");
        maildir.move_new_to_cur_with_flags(id, &maildir_flags)?;
        Ok(())
    }

    #[instrument(skip(self, path))]
    fn add_flags(
        &self,
        path: &Path,
        id: &str,
        imap_flags: &[&str],
    ) -> color_eyre::eyre::Result<()> {
        let maildir = Maildir::from(path.to_path_buf());
        let maildir_flags = imap_flags
            .iter()
            .filter_map(|flag| {
                let normalized_flag = flag.to_lowercase().replace(['(', ')'], "");
                if normalized_flag == "\\seen" {
                    Some("S")
                } else if normalized_flag == "\\deleted" {
                    Some("T")
                } else if normalized_flag == "\\flagged" {
                    Some("F")
                } else if normalized_flag == "\\draft" {
                    Some("D")
                } else if normalized_flag == "\\answered" {
                    Some("R")
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("");
        debug!("flags: {:?}", maildir_flags);
        maildir.add_flags(id, &maildir_flags)?;
        Ok(())
    }

    #[instrument(skip(self, path))]
    fn set_flags(
        &self,
        path: &Path,
        id: &str,
        imap_flags: &[&str],
    ) -> color_eyre::eyre::Result<()> {
        let maildir = Maildir::from(path.to_path_buf());
        let maildir_flags = imap_flags
            .iter()
            .filter_map(|flag| {
                let normalized_flag = flag.to_lowercase().replace(['(', ')'], "");
                if normalized_flag == "\\seen" {
                    Some("S")
                } else if normalized_flag == "\\deleted" {
                    Some("T")
                } else if normalized_flag == "\\flagged" {
                    Some("F")
                } else if normalized_flag == "\\draft" {
                    Some("D")
                } else if normalized_flag == "\\answered" {
                    Some("R")
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("");
        maildir.set_flags(id, &maildir_flags)?;
        Ok(())
    }

    #[instrument(skip(self, path))]
    fn remove_flags(
        &self,
        path: &Path,
        id: &str,
        imap_flags: &[&str],
    ) -> color_eyre::eyre::Result<()> {
        let maildir = Maildir::from(path.to_path_buf());
        let maildir_flags = imap_flags
            .iter()
            .filter_map(|flag| {
                let normalized_flag = flag.to_lowercase().replace(['(', ')'], "");
                if normalized_flag == "\\seen" {
                    Some("S")
                } else if normalized_flag == "\\deleted" {
                    Some("T")
                } else if normalized_flag == "\\flagged" {
                    Some("F")
                } else if normalized_flag == "\\draft" {
                    Some("D")
                } else if normalized_flag == "\\answered" {
                    Some("R")
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("");
        maildir.set_flags(id, &maildir_flags)?;
        Ok(())
    }

    fn to_ondisk_path(&self, path: String, username: String) -> color_eyre::eyre::Result<PathBuf> {
        let folder = self.to_ondisk_path_name(path)?;
        let mailbox_path = Path::new(&self.config.mail.maildir_folders)
            .join(username)
            .join(folder);
        Ok(mailbox_path)
    }

    fn to_ondisk_path_name(&self, path: String) -> color_eyre::eyre::Result<String> {
        let mut folder = path.replace('/', ".");
        folder.insert(0, '.');
        folder.remove_matches('"');
        folder = folder.replace(".INBOX", "INBOX");
        Ok(folder)
    }
}

#[derive(sqlx::FromRow)]
struct DbMails {
    id: i32,
    maildir_id: String,
    modseq: i64,
}

/// Wrapper for the mailentries from the Maildir crate
pub struct MaildirMailEntry {
    entry: maildir::MailEntry,
    uid: u32,
    modseq: u64,
    /// The sequence number. It is None until used
    pub sequence_number: Option<u32>,
    mail_state: MailState,
}

#[async_trait::async_trait]
impl MailEntry for MaildirMailEntry {
    #[instrument(skip(self))]
    fn uid(&self) -> u32 {
        self.uid
    }

    #[instrument(skip(self))]
    fn modseq(&self) -> u64 {
        self.modseq
    }

    #[instrument(skip(self))]
    fn mail_state(&self) -> MailState {
        self.mail_state
    }

    #[instrument(skip(self))]
    fn sequence_number(&self) -> Option<u32> {
        self.sequence_number
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
    fn path(&self) -> &PathBuf {
        self.entry.path()
    }
}

#[instrument(skip(filename))]
async fn lines_from_file(filename: impl AsRef<Path>) -> std::io::Result<Vec<String>> {
    LinesStream::new(BufReader::new(File::open(filename).await?).lines())
        .try_collect::<Vec<String>>()
        .await
}
