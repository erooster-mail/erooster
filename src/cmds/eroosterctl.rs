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

use clap::{Parser, Subcommand};
use color_eyre::eyre::Result;
use owo_colors::{
    colors::{BrightGreen, BrightWhite},
    DynColors, OwoColorize,
};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
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
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    let config = erooster::get_config(cli.config).await?;
    match &cli.command {
        Commands::Status => {
            status();
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
        }  else if index == 6 {
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
