// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! Build script for eroosterctl: emits vergen git metadata.

use vergen::EmitBuilder;

fn main() -> std::io::Result<()> {
    EmitBuilder::builder()
        .git_sha(true)
        .emit()
        .map_err(std::io::Error::other)?;
    Ok(())
}
