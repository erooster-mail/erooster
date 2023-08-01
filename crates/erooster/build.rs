use vergen::EmitBuilder;

fn main() -> std::io::Result<()> {
    EmitBuilder::builder().git_sha(true).emit().unwrap();
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
                .num_args(1)
                .default_value("config.yml"));

    let man = clap_mangen::Man::new(cmd);
    let mut buffer: Vec<u8> = Default::default();
    man.render(&mut buffer)?;

    std::fs::write(dest_path.join("erooster.1"), buffer)?;

    let cmd = clap::Command::new("eroosterctl")
        .version("0.1.0")
        .author("MTRNord <mtrnord@nordgedanken.dev>")
        .about("An IMAP4v2 compatible mail server")
        .propagate_version(true)
        .subcommand_required(true)
        .arg_required_else_help(false)
        .arg(clap::arg!(-c --config [CONFIG])
        .help("The config file location for the server. Defaults to config.yml or config.yaml at workspace or /etc/erooster")
                .required(false)
                .num_args(1)
                .default_value("config.yml"))
        .subcommand(
            clap::Command::new("status")
                .about("Checks the server status"),
        )
        .subcommand(
            clap::Command::new("register")
                .about("Register a new User to the server")
                .arg(clap::arg!(-e --email [EMAIL])
                .help("The email of the new user (optional, required if --password is set)")
                .required(false)
                .num_args(1))
                .arg(clap::arg!(-p --password [PASSWORD])
                .help("The password of the new user (optional, required if --username is set)")
                .required(false)
                .num_args(1)),
        )
        .subcommand(
            clap::Command::new("change-password")
                .about("Change a users password").arg(clap::arg!(-e --email [EMAIL])
                .help("The email of the user (optional, required if any option is set)")
                .required(false)
                .num_args(1)).arg(clap::arg!(-c --current_password [CURRENT_PASSWORD])
                .help("The current password of the user (optional, required if any option is set)")
                .required(false)
                .num_args(1)).arg(clap::arg!(-n --new_password [NEW_PASSWORD])
                .help("The new password of the user (optional, required if any option is set)")
                .required(false)
                .num_args(1)),
        );

    let man = clap_mangen::Man::new(cmd);
    let mut buffer: Vec<u8> = Default::default();
    man.render(&mut buffer)?;

    std::fs::write(dest_path.join("eroosterctl.1"), buffer)?;
    Ok(())
}
