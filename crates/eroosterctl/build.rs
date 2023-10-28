// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use vergen::EmitBuilder;

fn main() -> std::io::Result<()> {
    EmitBuilder::builder().git_sha(true).emit().unwrap();
    Ok(())
}
