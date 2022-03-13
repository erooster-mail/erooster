use std::io::prelude::*;
use std::path::Path;
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader},
};

pub fn get_flags(path: &Path) -> color_eyre::eyre::Result<Vec<String>> {
    let flags_file = path.join(".erooster_folder_lags");
    if flags_file.exists() {
        let file = File::open(flags_file)?;
        let buf = BufReader::new(file);
        let flags: Vec<String> = buf.lines().flatten().collect();
        return Ok(flags);
    }
    Ok(vec![])
}

pub fn add_flag(path: &Path, flag: &str) -> color_eyre::eyre::Result<()> {
    let flags_file = path.join(".erooster_folder_lags");
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(flags_file)?;
    writeln!(file, "{}", flag)?;
    Ok(())
}
