// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::{
    backend::{
        database::{Database, DB},
        storage::{MailEntry, MailState, MailStorage},
    },
    config::Config,
};
use std::path::{Path, PathBuf};

use {
    color_eyre,
    futures::{StreamExt, TryStreamExt},
    maildir::{self, Maildir},
    mailparse::{self, ParsedMail},
    tokio::{
        fs::File,
        fs::OpenOptions,
        io::AsyncWriteExt,
        io::{AsyncBufReadExt, BufReader},
    },
    tokio_stream::wrappers::LinesStream,
    tracing::{debug, error, instrument},
};

/// The Storage handler for the maildir format
#[derive(Debug, Clone)]
pub struct MaildirStorage {
    db: DB,
    config: Config,
}

impl MaildirStorage {
    /// Create a new maildir storage handler
    #[must_use]
    #[instrument(skip(db))]
    pub fn new(db: DB, config: Config) -> Self {
        MaildirStorage { db, config }
    }
}

impl MailStorage<MaildirMailEntry> for MaildirStorage {
    #[instrument(skip(self, mailbox))]
    async fn get_uid_for_folder(&self, mailbox: &str) -> color_eyre::eyre::Result<u32> {
        let max_uid: Option<i32> =
            sqlx::query_scalar("SELECT MAX(uid) FROM mails WHERE mailbox = $1")
                .bind(mailbox)
                .fetch_optional(self.db.get_pool())
                .await?
                .flatten();
        Ok(max_uid.unwrap_or(0).cast_unsigned())
    }

    async fn find(&self, path: &Path, id: &str) -> Option<MaildirMailEntry> {
        let maildir = Maildir::from(path.to_path_buf());
        let mail: Vec<_> =
            sqlx::query_as::<_, DbMails>("SELECT * FROM mails WHERE maildir_id = $1")
                .bind(id)
                .fetch(self.db.get_pool())
                .filter_map(|x| async move {
                    match x {
                        Err(e) => {
                            error!("Failed to fetch row: {e}");
                            None
                        }
                        Ok(x) => Some(x),
                    }
                })
                .collect()
                .await;

        let db_item = mail.iter().find(|y: &&DbMails| y.maildir_id == id);
        maildir.find(id).map(|entry| {
            let uid: i32 = db_item.map_or(0, |y| y.uid);
            let mailbox: String = db_item.map_or(String::from("unknown"), |y| y.mailbox.clone());
            let modseq = db_item.map_or(0, |y| y.modseq);

            let mail_state = if entry.is_seen() {
                MailState::Read
            } else {
                MailState::New
            };
            MaildirMailEntry {
                uid: uid.try_into().expect("Invalid UID"),
                mailbox,
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
        let existing = self.get_flags(path).await.unwrap_or_default();
        if existing.iter().any(|f| f == flag) {
            return Ok(());
        }
        let flags_file = path.join(".erooster_folder_flags");
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(flags_file)
            .await?;
        let line = format!("{flag}\n");
        file.write_all(line.as_bytes()).await?;
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
            .truncate(true)
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

    #[instrument(skip(self, mailbox, path, data))]
    async fn store_cur_with_flags(
        &self,
        mailbox: String,
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
        let next_uid: i32 =
            sqlx::query_scalar("SELECT COALESCE(MAX(uid), 0) + 1 FROM mails WHERE mailbox = $1")
                .bind(mailbox.as_str())
                .fetch_one(self.db.get_pool())
                .await?;
        sqlx::query("INSERT INTO mails (maildir_id, modseq, mailbox, uid) VALUES ($1, $2, $3, $4)")
            .bind(maildir_id.clone())
            .bind(1i64)
            .bind(mailbox)
            .bind(next_uid)
            .execute(self.db.get_pool())
            .await?;
        Ok(maildir_id)
    }

    #[instrument(skip(self, mailbox, path, data))]
    async fn store_new(
        &self,
        mailbox: String,
        path: &Path,
        data: &[u8],
        dkim_status: Option<String>,
    ) -> color_eyre::eyre::Result<String> {
        let maildir = Maildir::from(path.to_path_buf());
        let maildir_id = maildir.store_new(data)?;
        let next_uid: i32 =
            sqlx::query_scalar("SELECT COALESCE(MAX(uid), 0) + 1 FROM mails WHERE mailbox = $1")
                .bind(mailbox.as_str())
                .fetch_one(self.db.get_pool())
                .await?;
        sqlx::query(
            "INSERT INTO mails (maildir_id, modseq, mailbox, uid, dkim_status) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(maildir_id.clone())
        .bind(1i64)
        .bind(mailbox)
        .bind(next_uid)
        .bind(dkim_status)
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

    #[instrument(skip(self, mailbox, path))]
    async fn list_cur(&self, mailbox: String, path: &Path) -> Vec<MaildirMailEntry> {
        let maildir = Maildir::from(path.to_path_buf());
        let mail_rows: Vec<DbMails> =
            sqlx::query_as::<_, DbMails>("SELECT * FROM mails WHERE mailbox = $1")
                .bind(mailbox.as_str())
                .fetch(self.db.get_pool())
                .filter_map(|x| async move {
                    match x {
                        Err(e) => {
                            error!("Failed to fetch row: {e}");
                            None
                        }
                        Ok(x) => Some(x),
                    }
                })
                .collect()
                .await;
        let mails: Vec<_> = maildir
            .list_cur()
            .filter_map(|x| match x {
                Ok(entry) => {
                    let maildir_id = entry.id();

                    let db_item = mail_rows.iter().find(|y| y.maildir_id == maildir_id);
                    let uid: i32 = db_item.map_or(0, |y| y.uid);
                    let mailbox: String =
                        db_item.map_or(String::from("unknown"), |y| y.mailbox.clone());
                    let modseq = db_item.map_or(0, |y| y.modseq);

                    Some(MaildirMailEntry {
                        uid: uid.try_into().expect("Invalid UID"),
                        modseq: modseq.try_into().expect("Invalid UID"),
                        entry,
                        mailbox,
                        mail_state: MailState::Read,
                        sequence_number: None,
                    })
                }
                Err(_) => None,
            })
            .collect();
        for mail in &mails {
            if mail.mailbox == "unknown" {
                sqlx::query("UPDATE mails SET mailbox = $1 WHERE maildir_id = $2")
                    .bind(mailbox.clone())
                    .bind(mail.id())
                    .execute(self.db.get_pool())
                    .await
                    .expect("Failed to update row");
            }
        }
        mails
    }

    #[instrument(skip(self, mailbox, path))]
    async fn list_new(&self, mailbox: String, path: &Path) -> Vec<MaildirMailEntry> {
        let maildir = Maildir::from(path.to_path_buf());
        let mail_rows: Vec<DbMails> =
            sqlx::query_as::<_, DbMails>("SELECT * FROM mails WHERE mailbox = $1")
                .bind(mailbox.as_str())
                .fetch(self.db.get_pool())
                .filter_map(|x| async move {
                    match x {
                        Err(e) => {
                            error!("Failed to fetch row: {e}");
                            None
                        }
                        Ok(x) => Some(x),
                    }
                })
                .collect()
                .await;

        let mails: Vec<_> = maildir
            .list_new()
            .filter_map(|x| match x {
                Ok(entry) => {
                    let maildir_id = entry.id();

                    let db_item = mail_rows.iter().find(|y| y.maildir_id == maildir_id);
                    let uid: i32 = db_item.map_or(0, |y| y.uid);
                    let mailbox: String =
                        db_item.map_or(String::from("unknown"), |y| y.mailbox.clone());
                    let modseq = db_item.map_or(0, |y| y.modseq);

                    Some(MaildirMailEntry {
                        uid: uid.try_into().expect("Invalid UID"),
                        modseq: modseq.try_into().expect("Invalid UID"),
                        entry,
                        mailbox,
                        mail_state: MailState::New,
                        sequence_number: None,
                    })
                }
                Err(_) => None,
            })
            .collect();
        for mail in &mails {
            if mail.mailbox == "unknown" {
                sqlx::query("UPDATE mails SET mailbox = $1 WHERE maildir_id = $2")
                    .bind(mailbox.clone())
                    .bind(mail.id())
                    .execute(self.db.get_pool())
                    .await
                    .expect("Failed to update row");
            }
        }
        mails
    }

    #[instrument(skip(self, path, mailbox))]
    async fn list_all(&self, mailbox: String, path: &Path) -> Vec<MaildirMailEntry> {
        let pool = self.db.get_pool();
        let maildir = Maildir::from(path.to_path_buf());
        let mail_rows: Vec<DbMails> =
            sqlx::query_as::<_, DbMails>("SELECT * FROM mails WHERE mailbox = $1")
                .bind(mailbox.as_str())
                .fetch(pool)
                .filter_map(|x| async move {
                    match x {
                        Err(e) => {
                            error!("Failed to fetch row: {e}");
                            None
                        }
                        Ok(x) => Some(x),
                    }
                })
                .collect()
                .await;

        let mails: Vec<MaildirMailEntry> = maildir
            .list_new()
            .chain(maildir.list_cur())
            .filter_map(|x| match x {
                Ok(entry) => {
                    let maildir_id = entry.id();
                    let db_item = mail_rows.iter().find(|y| y.maildir_id == maildir_id);
                    let uid: i32 = db_item.map_or(0, |y| y.uid);
                    let mailbox: String =
                        db_item.map_or(String::from("unknown"), |y| y.mailbox.clone());
                    let modseq = db_item.map_or(0, |y| y.modseq);
                    let state = if entry.is_seen() {
                        MailState::Read
                    } else {
                        MailState::New
                    };
                    Some(MaildirMailEntry {
                        uid: uid.try_into().expect("Invalid UID"),
                        modseq: modseq.try_into().expect("Invalid Modseq"),
                        entry,
                        mailbox,
                        mail_state: state,
                        sequence_number: None,
                    })
                }
                Err(_) => None,
            })
            .collect();
        for mail in &mails {
            if mail.mailbox == "unknown" {
                if let Err(e) = sqlx::query("UPDATE mails SET mailbox = $1 WHERE maildir_id = $2")
                    .bind(mailbox.clone())
                    .bind(mail.id())
                    .execute(pool)
                    .await
                {
                    error!("Failed to update the mailbox in the database: {e}");
                }
            }
        }
        mails
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
        folder.retain(|c| c != '"');
        folder = folder.replace(".INBOX", "INBOX");
        Ok(folder)
    }
}

#[derive(sqlx::FromRow, Debug)]
struct DbMails {
    #[allow(dead_code)]
    id: i32,
    maildir_id: String,
    modseq: i64,
    uid: i32,
    mailbox: String,
}

/// Wrapper for the mailentries from the Maildir crate
pub struct MaildirMailEntry {
    entry: maildir::MailEntry,
    uid: u32,
    /// The mailbox folder it is in
    pub mailbox: String,
    modseq: u64,
    /// The sequence number. It is None until used
    pub sequence_number: Option<u32>,
    mail_state: MailState,
}

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
    fn parsed(&mut self) -> color_eyre::eyre::Result<ParsedMail<'_>> {
        self.entry.parsed().map_err(Into::into)
    }

    #[instrument(skip(self))]
    fn headers(&mut self) -> color_eyre::eyre::Result<Vec<mailparse::MailHeader<'_>>> {
        self.entry.headers().map_err(Into::into)
    }

    #[instrument(skip(self))]
    fn received(&mut self) -> color_eyre::eyre::Result<i64> {
        self.entry.received().map_err(Into::into)
    }

    #[instrument(skip(self))]
    fn sent(&mut self) -> color_eyre::eyre::Result<i64> {
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
    fn path(&self) -> &PathBuf {
        self.entry.path()
    }

    #[instrument(skip(self))]
    fn body_contains(&mut self, string: &str) -> bool {
        if let Ok(mail) = self.entry.parsed() {
            if let Ok(body) = mail.get_body() {
                body.contains(string)
            } else {
                false
            }
        } else {
            false
        }
    }

    #[instrument(skip(self))]
    fn text_contains(&mut self, string: &str) -> bool {
        let contained_in_headers = if let Ok(headers) = self.entry.headers() {
            headers.iter().any(|header| {
                header.get_value().contains(string) || header.get_key_ref().contains(string)
            })
        } else {
            false
        };
        self.body_contains(string) || contained_in_headers
    }

    #[instrument(skip(self))]
    fn body_size(&mut self) -> u64 {
        if let Ok(mail) = self.entry.parsed() {
            if let Ok(body) = mail.get_body_raw() {
                body.len() as u64
            } else {
                0
            }
        } else {
            0
        }
    }
}

#[instrument(skip(filename))]
async fn lines_from_file(filename: impl AsRef<Path>) -> std::io::Result<Vec<String>> {
    LinesStream::new(BufReader::new(File::open(filename).await?).lines())
        .try_collect::<Vec<String>>()
        .await
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use crate::{
        backend::storage::{MailEntry, MailStorage},
        test_helpers::setup_test_storage,
    };

    const MSG: &[u8] = b"From: a@localhost\r\nTo: b@localhost\r\nSubject: test\r\n\r\nHello\r\n";

    #[tokio::test]
    async fn uid_increments_per_message() {
        let (config, storage) = setup_test_storage().await.unwrap();
        let mailbox = "test@localhost/INBOX";
        let path = std::path::Path::new(&config.mail.maildir_folders)
            .join("test@localhost")
            .join("INBOX");
        storage.create_dirs(&path).unwrap();

        storage
            .store_new(mailbox.to_string(), &path, MSG, None)
            .await
            .unwrap();
        let uid1 = storage.get_uid_for_folder(mailbox).await.unwrap();
        assert_eq!(uid1, 1, "first message should get UID 1");

        storage
            .store_new(mailbox.to_string(), &path, MSG, None)
            .await
            .unwrap();
        let uid2 = storage.get_uid_for_folder(mailbox).await.unwrap();
        assert_eq!(uid2, 2, "second message should get UID 2");
    }

    #[tokio::test]
    async fn uid_is_per_mailbox() {
        let (config, storage) = setup_test_storage().await.unwrap();
        let inbox = "test@localhost/INBOX";
        let sent = "test@localhost/.Sent";
        let inbox_path = std::path::Path::new(&config.mail.maildir_folders)
            .join("test@localhost")
            .join("INBOX");
        let sent_path = std::path::Path::new(&config.mail.maildir_folders)
            .join("test@localhost")
            .join(".Sent");
        storage.create_dirs(&inbox_path).unwrap();
        storage.create_dirs(&sent_path).unwrap();

        storage
            .store_new(inbox.to_string(), &inbox_path, MSG, None)
            .await
            .unwrap();
        storage
            .store_new(inbox.to_string(), &inbox_path, MSG, None)
            .await
            .unwrap();
        // INBOX has 2 messages, so max uid = 2
        assert_eq!(storage.get_uid_for_folder(inbox).await.unwrap(), 2);

        // Sent is independent — first message gets UID 1, not 3
        storage
            .store_new(sent.to_string(), &sent_path, MSG, None)
            .await
            .unwrap();
        assert_eq!(storage.get_uid_for_folder(sent).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn list_all_only_returns_own_mailbox() {
        let (config, storage) = setup_test_storage().await.unwrap();
        let inbox = "test@localhost/INBOX";
        let sent = "test@localhost/.Sent";
        let inbox_path = std::path::Path::new(&config.mail.maildir_folders)
            .join("test@localhost")
            .join("INBOX");
        let sent_path = std::path::Path::new(&config.mail.maildir_folders)
            .join("test@localhost")
            .join(".Sent");
        storage.create_dirs(&inbox_path).unwrap();
        storage.create_dirs(&sent_path).unwrap();

        storage
            .store_new(inbox.to_string(), &inbox_path, MSG, None)
            .await
            .unwrap();
        storage
            .store_new(sent.to_string(), &sent_path, MSG, None)
            .await
            .unwrap();

        let inbox_mails = storage.list_all(inbox.to_string(), &inbox_path).await;
        assert_eq!(inbox_mails.len(), 1, "INBOX should have exactly 1 message");
        assert_eq!(inbox_mails[0].uid(), 1);

        let sent_mails = storage.list_all(sent.to_string(), &sent_path).await;
        assert_eq!(sent_mails.len(), 1, "Sent should have exactly 1 message");
        assert_eq!(sent_mails[0].uid(), 1);
    }

    #[tokio::test]
    async fn uid_fetch_range_finds_messages() {
        let (config, storage) = setup_test_storage().await.unwrap();
        let mailbox = "test@localhost/INBOX";
        let path = std::path::Path::new(&config.mail.maildir_folders)
            .join("test@localhost")
            .join("INBOX");
        storage.create_dirs(&path).unwrap();

        for _ in 0..3 {
            storage
                .store_new(mailbox.to_string(), &path, MSG, None)
                .await
                .unwrap();
        }

        let mails = storage.list_all(mailbox.to_string(), &path).await;
        // All three messages must have non-zero UIDs
        for mail in &mails {
            assert!(
                mail.uid() > 0,
                "every stored message must have a non-zero UID"
            );
        }
        // UIDs must be distinct
        let uids: std::collections::HashSet<u32> = mails.iter().map(|m| m.uid()).collect();
        assert_eq!(uids.len(), 3, "UIDs must be unique");
    }

    #[tokio::test]
    async fn add_flag_is_idempotent() {
        let (config, storage) = setup_test_storage().await.unwrap();
        let path = std::path::Path::new(&config.mail.maildir_folders)
            .join("test@localhost")
            .join("INBOX");
        storage.create_dirs(&path).unwrap();

        storage.add_flag(&path, "\\Subscribed").await.unwrap();
        storage.add_flag(&path, "\\Subscribed").await.unwrap();
        storage.add_flag(&path, "\\Subscribed").await.unwrap();

        let flags = storage.get_flags(&path).await.unwrap();
        let subscribed_count = flags
            .iter()
            .filter(|f| f.as_str() == "\\Subscribed")
            .count();
        assert_eq!(
            subscribed_count, 1,
            "duplicate add_flag calls must not duplicate flags"
        );
    }

    #[tokio::test]
    async fn store_cur_with_flags_creates_dirs_if_missing() {
        let (config, storage) = setup_test_storage().await.unwrap();
        let mailbox = "test@localhost/.Sent";
        let path = std::path::Path::new(&config.mail.maildir_folders)
            .join("test@localhost")
            .join(".Sent");

        // create_dirs must be called before store_cur_with_flags
        storage.create_dirs(&path).unwrap();

        let id = storage
            .store_cur_with_flags(mailbox.to_string(), &path, MSG, vec!["\\Seen".to_string()])
            .await
            .unwrap();
        assert!(!id.is_empty(), "stored message should get an id");
        let uid = storage.get_uid_for_folder(mailbox).await.unwrap();
        assert_eq!(uid, 1, "first message in .Sent should get UID 1");
    }
}
