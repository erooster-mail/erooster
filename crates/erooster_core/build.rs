// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! Build script for `erooster_core`: emits vergen git metadata and migration rerun triggers.

use vergen_gix::{Emitter, Gix};

fn main() -> std::io::Result<()> {
    let gix = Gix::builder().sha(true).build();
    Emitter::default()
        .add_instructions(&gix)
        .map_err(std::io::Error::other)?
        .emit()
        .map_err(std::io::Error::other)?;

    // For migrations
    println!("cargo:rerun-if-changed=migrations");

    Ok(())
}
