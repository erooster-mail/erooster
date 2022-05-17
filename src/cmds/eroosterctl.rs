//! Erooster Mail Server
//!
//! Erooster is a rust native imap server build on modern solutions.
//! The goal being easy to setup, use and maintain for smaller mail servers
//! while being also fast and efficient.
//!
#![feature(string_remove_matches)]
#![deny(unsafe_code)]
#![warn(
    clippy::cognitive_complexity,
    clippy::branches_sharing_code,
    clippy::imprecise_flops,
    clippy::missing_const_for_fn,
    clippy::mutex_integer,
    clippy::path_buf_push_overwrite,
    clippy::redundant_pub_crate,
    clippy::pedantic,
    clippy::dbg_macro,
    clippy::todo,
    clippy::fallible_impl_from,
    clippy::filetype_is_file,
    clippy::suboptimal_flops,
    clippy::fn_to_numeric_cast_any,
    clippy::if_then_some_else_none,
    clippy::imprecise_flops,
    clippy::lossy_float_literal,
    clippy::panic_in_result_fn,
    clippy::clone_on_ref_ptr
)]
#![warn(missing_docs)]
#![allow(clippy::missing_panics_doc)]

use std::{io, sync::Arc};

use clap::{Parser, Subcommand};
use color_eyre::eyre::Result;
use erooster::{
    config::Config,
    database::{get_database, Database},
};
use indicatif::{ProgressBar, ProgressStyle};
use owo_colors::{
    colors::{BrightCyan, BrightGreen, BrightRed, BrightWhite},
    DynColors, OwoColorize,
};
use tracing::error;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None, propagate_version = true)]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
    #[clap(short, long, default_value = "./config.yml")]
    config: String,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Checks the server status
    Status,
    /// Register a new User to the server
    Register {
        /// The email of the new user (optional, required if --password is set)
        #[clap(short, long)]
        email: Option<String>,
        /// The password of the new user (optional, required if --username is set)
        #[clap(short, long)]
        password: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    let config = erooster::get_config(cli.config).await?;
    match cli.command {
        Commands::Status => {
            status();
        }
        Commands::Register { email, password } => {
            register(email, password, &config).await;
        }
    }
    Ok(())
}

const ICON: &str = r#"
     __;//;
    /;.;__;.;\
    \ ;\/; /;
 ';__/    \
  \-      )
   \_____/;
_____;|;_;|;____; 
     " ""#;

const FINS: &str = "#A62A13";
const HEAD: &str = "#FFBA00";
const EYES: &str = "#232324";
const BEAK: &str = "#FF8D16";
const BODY: &str = "#8E5E4F";
const FEATHER: &str = "#2C5422";
const LINE: &str = "#000000";
const CLAWS: &str = "#FFB804";

fn status() {
    let colors: [DynColors; 17] = [
        HEAD, FINS, HEAD, EYES, HEAD, EYES, HEAD, BEAK, HEAD, FEATHER, BODY, LINE, CLAWS, LINE,
        CLAWS, LINE, CLAWS,
    ]
    .map(|color| color.parse().unwrap());

    let mut current_color_index = 0;
    let mut out = String::new();
    for char in ICON.chars() {
        if char == ';' {
            current_color_index += 1;
        } else {
            out = format!("{}{}", out, char.color(colors[current_color_index]).bold());
        }
    }

    for (index, line) in out.lines().enumerate() {
        if index == 2 {
            println!(
                "{}    {}         {}",
                line,
                "Erooster:".fg::<BrightWhite>(),
                "OK".fg::<BrightGreen>().bold()
            );
        } else if index == 3 {
            println!(
                "{}    {}   {}",
                line,
                "Outgoing Email:".fg::<BrightWhite>(),
                "OK".fg::<BrightGreen>().bold()
            );
        } else if index == 4 {
            println!(
                "{}    {}   {}",
                line,
                "Incoming Email:".fg::<BrightWhite>(),
                "OK".fg::<BrightGreen>().bold()
            );
        } else if index == 5 {
            println!(
                "{}   {}        {}",
                line,
                "Webserver:".fg::<BrightWhite>(),
                "OK".fg::<BrightGreen>().bold()
            );
        } else if index == 6 {
            println!(
                "{}    {}         {}",
                line,
                "Database:".fg::<BrightWhite>(),
                "OK".fg::<BrightGreen>().bold()
            );
        } else {
            println!("{}", line);
        }
    }
}

async fn register(username: Option<String>, password: Option<String>, config: &Config) {
    let spinner_style = ProgressStyle::default_spinner()
        .template("{spinner} {wide_msg}")
        .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ");
    if username.is_some() && password.is_none() {
        error!("Missing password");
    } else if username.is_none() && password.is_some() {
        error!("Missing username");
    } else if username.is_none() && password.is_none() {
        clearscreen::clear().expect("failed to clear screen");
        // Get users username
        let mut username = String::new();
        println!(
            "{}",
            "Please enter the email address of the new user:".fg::<BrightCyan>()
        );
        io::stdin()
            .read_line(&mut username)
            .expect("Couldn't read line");
        // We remove the newline
        username = username.replace('\n', "").replace('\r', "");

        // TODO input validation

        // Get users password (doesnt show it)
        let password = rpassword::prompt_password(
            "Please enter the email password of the new user: ".fg::<BrightCyan>(),
        )
        .expect("Couldn't read line");

        let pb = ProgressBar::new_spinner();
        pb.set_style(spinner_style);
        pb.enable_steady_tick(100);
        clearscreen::clear().expect("failed to clear screen");
        pb.set_message("Adding the new user...".fg::<BrightGreen>().to_string());
        let result = actual_register(username, password, config).await;

        clearscreen::clear().expect("failed to clear screen");
        if let Err(error) = result {
            pb.finish_with_message(format!(
                "{}\n{}",
                "There has been an error while registering the user:".fg::<BrightRed>(),
                error.fg::<BrightRed>()
            ));
        } else {
            pb.finish_with_message(
                "User was successfully added"
                    .fg::<BrightGreen>()
                    .to_string(),
            );
        }
    } else {
        clearscreen::clear().expect("failed to clear screen");
        let pb = ProgressBar::new_spinner();
        pb.set_style(spinner_style);
        pb.enable_steady_tick(100);
        pb.set_message("Adding the new user...".fg::<BrightGreen>().to_string());

        let username = username.unwrap();
        let password = password.unwrap();
        let result = actual_register(username, password, config).await;

        clearscreen::clear().expect("failed to clear screen");
        if let Err(error) = result {
            pb.finish_with_message(format!(
                "{}\n{}",
                "There has been an error while registering the user:".fg::<BrightRed>(),
                error.fg::<BrightRed>()
            ));
        } else {
            pb.finish_with_message(
                "User was successfully added"
                    .fg::<BrightGreen>()
                    .to_string(),
            );
        }
    }
}

async fn actual_register(username: String, password: String, config: &Config) -> Result<()> {
    let database = get_database(Arc::new(config.clone()));
    database.add_user(&username).await?;
    database.change_password(&username, &password).await?;
    Ok(())
}
