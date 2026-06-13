// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! Build script for `erooster_core`: emits vergen git metadata and migration rerun triggers.

use vergen::EmitBuilder;

fn main() -> std::io::Result<()> {
    EmitBuilder::builder()
        .git_sha(true)
        .emit()
        .map_err(std::io::Error::other)?;

    // For migrations
    println!("cargo:rerun-if-changed=migrations");

    Ok(())
}
