fn main() -> std::io::Result<()> {
    let out_dir =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").ok_or(std::io::ErrorKind::NotFound)?);
    let dest_path = out_dir
        .parent()
        .ok_or(std::io::ErrorKind::NotFound)?
        .parent()
        .ok_or(std::io::ErrorKind::NotFound)?
        .parent()
        .ok_or(std::io::ErrorKind::NotFound)?;

    let cmd = clap::Command::new("erooster")
        .version("0.1.0")
        .author("MTRNord <mtrnord@nordgedanken.dev>")
        .about("An IMAP4v2 compatible mail server")
        .arg(clap::arg!(-c --config <CONFIG>).help("The config file location for the server. Defaults to config.yml or config.yaml at workspace or /etc/erooster")
                .required(false)
                .takes_value(true)
                .default_value("config.yml"));

    let man = clap_mangen::Man::new(cmd);
    let mut buffer: Vec<u8> = Default::default();
    man.render(&mut buffer)?;

    std::fs::write(dest_path.join("erooster.1"), buffer)?;

    let cmd = clap::Command::new("eroosterctl")
        .version("0.1.0")
        .author("MTRNord <mtrnord@nordgedanken.dev>")
        .about("An IMAP4v2 compatible mail server")
        .arg(clap::arg!(-c --config <CONFIG>).help("The config file location for the server. Defaults to config.yml or config.yaml at workspace or /etc/erooster")
                .required(false)
                .takes_value(true)
                .default_value("config.yml"));

    let man = clap_mangen::Man::new(cmd);
    let mut buffer: Vec<u8> = Default::default();
    man.render(&mut buffer)?;

    std::fs::write(dest_path.join("eroosterctl.1"), buffer)?;

    Ok(())
}
