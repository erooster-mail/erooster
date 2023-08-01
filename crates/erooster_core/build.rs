use std::fs;

use vergen::EmitBuilder;

fn main() -> std::io::Result<()> {
    let paths = fs::read_dir("./").unwrap();

    for path in paths {
        println!("Name: {}", path.unwrap().path().display())
    }

    EmitBuilder::builder().git_sha(true).emit().unwrap();

    // For migrations
    println!("cargo:rerun-if-changed=migrations");

    Ok(())
}
