//! Erooster Mail Server
//!
//! Erooster is a rust native imap server build on modern solutions.
//! The goal being easy to setup, use and maintain for smaller mail servers
//! while being also fast and efficient.
//!
#![feature(string_remove_matches)]
#![deny(unsafe_code, clippy::unwrap_used)]
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

use clap::{Parser, Subcommand};
use color_eyre::eyre::Result;
use erooster_core::{
    backend::database::{get_database, Database},
    config::Config,
    panic_handler::EroosterPanicMessage,
};
use indicatif::{ProgressBar, ProgressStyle};
use owo_colors::{
    colors::{BrightCyan, BrightGreen, BrightRed, BrightWhite},
    DynColors, OwoColorize,
};
use secrecy::SecretString;
use std::io::Write;
use std::{borrow::Cow, time::Duration};
use std::{io, process::exit, sync::Arc};
use tracing::{error, info, warn};
use tracing_error::ErrorLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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
        password: Option<SecretString>,
    },
    // Change a users password
    ChangePassword {
        /// The email of the user (optional, required if any option is set)
        #[clap(short, long)]
        email: Option<String>,
        /// The current password of the user (optional, required if any option is set)
        #[clap(short, long)]
        current_password: Option<SecretString>,
        /// The new password of the user (optional, required if any option is set)
        #[clap(short, long)]
        new_password: Option<SecretString>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let builder = color_eyre::config::HookBuilder::default().panic_message(EroosterPanicMessage);
    let (panic_hook, eyre_hook) = builder.into_hooks();
    eyre_hook.install()?;

    let cli = Cli::parse();
    let config = erooster_core::get_config(cli.config).await?;

    let mut _guard;
    if config.sentry {
        let filter_layer = tracing_subscriber::EnvFilter::try_from_default_env()
            .or_else(|_| tracing_subscriber::EnvFilter::try_new("info"))?;
        tracing_subscriber::Registry::default()
            .with(sentry::integrations::tracing::layer())
            .with(filter_layer)
            .with(tracing_subscriber::fmt::Layer::default())
            .with(ErrorLayer::default())
            .init();
        info!("Sentry logging is enabled. Change the config to disable it.");

        _guard = sentry::init((
            "https://49e511ff807e45ffa19be1c63cfda26c@o105177.ingest.sentry.io/6458648",
            sentry::ClientOptions {
                release: Some(Cow::Owned(format!(
                    "{}@{}:{}",
                    env!("CARGO_PKG_NAME"),
                    env!("CARGO_PKG_VERSION"),
                    env!("VERGEN_GIT_SHA_SHORT")
                ))),
                traces_sample_rate: 1.0,
                enable_profiling: true,
                profiles_sample_rate: 1.0,
                ..Default::default()
            },
        ));
    } else {
        info!("Sentry logging is disabled. Change the config to enable it.");
        tracing_subscriber::fmt::init();
    }

    let next = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        eprintln!("{}", panic_hook.panic_report(panic_info));
        next(panic_info);
    }));

    match cli.command {
        Commands::Status => {
            status();
        }
        Commands::Register { email, password } => {
            register(email, password, config).await;
        }
        Commands::ChangePassword {
            email,
            current_password,
            new_password,
        } => {
            change_password(email, current_password, new_password, config).await;
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

#[allow(clippy::unwrap_used)]
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
            out = format!("{out}{}", char.color(colors[current_color_index]).bold());
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
            println!("{line}");
        }
    }
}

async fn register(username: Option<String>, password: Option<SecretString>, config: Arc<Config>) {
    let spinner_style = ProgressStyle::default_spinner()
        .template("{spinner} {wide_msg}")
        .expect("template working")
        .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ");
    if username.is_some() && password.is_none() {
        error!("Missing password");
    } else if username.is_none() && password.is_some() {
        error!("Missing username");
    } else if username.is_none() && password.is_none() {
        clearscreen::clear().expect("failed to clear screen");
        // Get users username
        let mut username = String::new();
        print!(
            "{}",
            "Please enter the email address of the new user: ".fg::<BrightCyan>()
        );
        io::stdout().flush().expect("Couldn't flush stdout");
        io::stdin()
            .read_line(&mut username)
            .expect("Couldn't read line");
        // We remove the newline
        username = username.replace(['\n', '\r'], "");

        // TODO input validation

        // Get users password (doesnt show it)
        let password = SecretString::new(
            rpassword::prompt_password(
                "Please enter the email password of the new user: ".fg::<BrightCyan>(),
            )
            .expect("Couldn't read line"),
        );

        let pb = ProgressBar::new_spinner();
        pb.set_style(spinner_style);
        pb.enable_steady_tick(Duration::from_millis(100));
        clearscreen::clear().expect("failed to clear screen");
        pb.set_message("Adding the new user...".fg::<BrightGreen>().to_string());
        let result = actual_register(username, password, Arc::clone(&config)).await;

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
        pb.enable_steady_tick(Duration::from_millis(100));
        pb.set_message("Adding the new user...".fg::<BrightGreen>().to_string());

        let username = username.expect("Username is not empty");
        let password = password.expect("Password is not empty");
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

async fn actual_register(
    username: String,
    password: SecretString,
    config: Arc<Config>,
) -> Result<()> {
    let database = get_database(config).await?;
    database.add_user(&username.to_lowercase()).await?;
    database
        .change_password(&username.to_lowercase(), password)
        .await?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
async fn change_password(
    username: Option<String>,
    current_password: Option<SecretString>,
    new_password: Option<SecretString>,
    config: Arc<Config>,
) {
    let spinner_style = ProgressStyle::default_spinner()
        .template("{spinner} {wide_msg}")
        .expect("template working")
        .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ");
    if username.is_some() && new_password.is_none() && current_password.is_none() {
        error!("Missing new password and current password");
    } else if username.is_none() && new_password.is_some() && current_password.is_none() {
        error!("Missing username and current password");
    } else if username.is_some() && new_password.is_some() && current_password.is_none() {
        error!("Missing current password");
    } else if username.is_some() && new_password.is_none() && current_password.is_some() {
        error!("Missing new password");
    } else if username.is_none() && new_password.is_some() && current_password.is_some() {
        error!("Missing username");
    } else if username.is_none() && new_password.is_none() && current_password.is_none() {
        clearscreen::clear().expect("failed to clear screen");
        // Get users username
        let mut username = String::new();
        print!(
            "{}",
            "Please enter the email address of the user: ".fg::<BrightCyan>()
        );
        io::stdout().flush().expect("Couldn't flush stdout");
        io::stdin()
            .read_line(&mut username)
            .expect("Couldn't read line");
        // We remove the newline
        username = username.replace(['\n', '\r'], "");

        // TODO input validation

        // Get users current password (doesnt show it)
        let current_password = SecretString::new(
            rpassword::prompt_password(
                "Please enter the current password of the user: ".fg::<BrightCyan>(),
            )
            .expect("Couldn't read line"),
        );

        // TODO repromt as needed
        if !verify_password(
            username.to_lowercase(),
            current_password,
            Arc::clone(&config),
        )
        .await
        {
            error!(
                "{}",
                "The password was incorrect. Please try again".fg::<BrightRed>()
            );
            exit(1);
        }

        let new_password = SecretString::new(
            rpassword::prompt_password(
                "Please enter the new password of the user: ".fg::<BrightCyan>(),
            )
            .expect("Couldn't read line"),
        );

        let pb = ProgressBar::new_spinner();
        pb.set_style(spinner_style);
        pb.enable_steady_tick(Duration::from_millis(100));
        clearscreen::clear().expect("failed to clear screen");
        pb.set_message(
            "Changing the users password..."
                .fg::<BrightGreen>()
                .to_string(),
        );
        let result = actual_change_password(username, new_password, Arc::clone(&config)).await;

        clearscreen::clear().expect("failed to clear screen");
        if let Err(error) = result {
            pb.finish_with_message(format!(
                "{}\n{}",
                "There has been an error while changing the users password:".fg::<BrightRed>(),
                error.fg::<BrightRed>()
            ));
        } else {
            pb.finish_with_message(
                "User's password was successfully changed"
                    .fg::<BrightGreen>()
                    .to_string(),
            );
        }
    } else {
        clearscreen::clear().expect("failed to clear screen");
        let pb = ProgressBar::new_spinner();
        pb.set_style(spinner_style);
        pb.enable_steady_tick(Duration::from_millis(100));
        pb.set_message(
            "Changing the users password..."
                .fg::<BrightGreen>()
                .to_string(),
        );

        let username = username.expect("Username is not empty");
        let current_password = current_password.expect("Password is not empty");
        if !verify_password(username.clone(), current_password, Arc::clone(&config)).await {
            error!(
                "{}",
                "The password was incorrect. Please try again".fg::<BrightRed>()
            );
            exit(1);
        }
        let new_password = new_password.expect("New Password is not empty");
        let result = actual_register(username, new_password, config).await;

        clearscreen::clear().expect("failed to clear screen");
        if let Err(error) = result {
            pb.finish_with_message(format!(
                "{}\n{}",
                "There has been an error while changing the users password:".fg::<BrightRed>(),
                error.fg::<BrightRed>()
            ));
        } else {
            pb.finish_with_message(
                "User's password was successfully changed"
                    .fg::<BrightGreen>()
                    .to_string(),
            );
        }
    }
}

async fn verify_password(
    username: String,
    current_password: SecretString,
    config: Arc<Config>,
) -> bool {
    match get_database(config).await {
        Ok(database) => database.verify_user(&username, current_password).await,
        Err(e) => {
            error!("Failed to verify password: {}", e);
            false
        }
    }
}

async fn actual_change_password(
    username: String,
    new_password: SecretString,
    config: Arc<Config>,
) -> Result<()> {
    let database = get_database(config).await?;
    database
        .change_password(&username.to_lowercase(), new_password)
        .await?;
    Ok(())
}
