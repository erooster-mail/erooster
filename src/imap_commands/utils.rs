use color_eyre::eyre::eyre;
use futures::TryStreamExt;
use maildir::Maildir;
use std::fs::OpenOptions;
use std::io::prelude::*;
use std::path::Path;
use tokio::fs::File;
use tokio_stream::wrappers::LinesStream;

use tokio::io::{AsyncBufReadExt, BufReader};

pub async fn get_uid_for_folder(maildir: Maildir) -> color_eyre::eyre::Result<u32> {
    let uid_file = maildir.path().join(".current_uid");
    if uid_file.exists() {
        let file = File::open(uid_file).await?;
        let buf = BufReader::new(file);
        let lines: Vec<_> = LinesStream::new(buf.lines())
            .try_collect::<Vec<String>>()
            .await?
            .iter()
            .map(|x| x.parse::<u32>())
            .collect();
        return lines[0]
            .clone()
            .map_err(|_| eyre!("failed to parse to number"));
    }

    // If we have messages use those as fallback
    let current_last_id: u32 = maildir.count_cur().try_into()?;
    let new_last_id: u32 = maildir.count_cur().try_into()?;
    Ok(current_last_id + new_last_id)
}

pub async fn get_flags(path: &Path) -> std::io::Result<Vec<String>> {
    let flags_file = path.join(".erooster_folder_flags");
    if flags_file.exists() {
        let file = File::open(flags_file).await?;
        let buf = BufReader::new(file);
        let flags = LinesStream::new(buf.lines()).try_collect().await;
        return flags;
    }
    Ok(vec![])
}

pub fn add_flag(path: &Path, flag: &str) -> color_eyre::eyre::Result<()> {
    let flags_file = path.join(".erooster_folder_flags");
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(flags_file)?;
    writeln!(file, "{}", flag)?;
    Ok(())
}

async fn lines_from_file(filename: impl AsRef<Path>) -> std::io::Result<Vec<String>> {
    LinesStream::new(BufReader::new(File::open(filename).await?).lines())
        .try_collect::<Vec<String>>()
        .await
}

pub async fn remove_flag(path: &Path, flag: &str) -> color_eyre::eyre::Result<()> {
    let flags_file = path.join(".erooster_folder_flags");
    let mut lines = lines_from_file(&flags_file).await?;

    lines.retain(|x| x != flag);

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .open(flags_file)?;

    for line in lines {
        writeln!(file, "{}", line)?;
    }

    Ok(())
}
