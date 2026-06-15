// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! Output formatting — table and JSON — with `NO_COLOR` / TTY detection.

use comfy_table::{Cell, Table};
use owo_colors::OwoColorize;
use serde::Serialize;
use std::io::{self, IsTerminal, Write};

/// The two output formats supported across all commands.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    /// Human-readable table (default).
    #[default]
    Table,
    /// Machine-readable JSON.
    Json,
}


/// Returns true if color output should be suppressed.
///
/// Color is disabled when any of the following hold (per <https://no-color.org/>):
/// - The `NO_COLOR` environment variable is set (to any value, including empty)
/// - `TERM=dumb`
/// - stdout is not a TTY
/// - `--no-color` was passed (caller sets `force_no_color = true`)
pub fn color_disabled(force_no_color: bool) -> bool {
    if force_no_color {
        return true;
    }
    if std::env::var_os("NO_COLOR").is_some() {
        return true;
    }
    if std::env::var("TERM").as_deref() == Ok("dumb") {
        return true;
    }
    !io::stdout().is_terminal()
}

/// Print a table with the given column headers and rows to stdout.
pub fn print_table(headers: &[&str], rows: Vec<Vec<String>>) {
    let mut table = Table::new();
    table.set_header(headers.iter().map(Cell::new));
    for row in rows {
        table.add_row(row.iter().map(Cell::new));
    }
    println!("{table}");
}

/// Serialize `value` as pretty-printed JSON to stdout.
///
/// # Errors
/// Returns an error if serialization fails.
pub fn print_json<T: Serialize>(value: &T) -> color_eyre::eyre::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Print an error message to stderr.
pub fn print_error(no_color: bool, msg: &str) {
    let _ = if no_color {
        writeln!(io::stderr(), "Error: {msg}")
    } else {
        writeln!(io::stderr(), "{}", format!("Error: {msg}").red())
    };
}

/// Print a warning to stderr.
#[allow(dead_code)]
pub fn print_warning(no_color: bool, msg: &str) {
    let _ = if no_color {
        writeln!(io::stderr(), "Warning: {msg}")
    } else {
        writeln!(io::stderr(), "{}", format!("Warning: {msg}").yellow())
    };
}

/// Print a success message to stdout.
pub fn print_success(no_color: bool, msg: &str) {
    if no_color {
        println!("{msg}");
    } else {
        println!("{}", msg.green());
    }
}
