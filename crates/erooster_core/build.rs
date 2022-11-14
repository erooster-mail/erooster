use std::fs;

use vergen::{vergen, Config, ShaKind};

fn main() -> std::io::Result<()> {
    let paths = fs::read_dir("./").unwrap();

    for path in paths {
        println!("Name: {}", path.unwrap().path().display())
    }

    let mut config = Config::default();
    *config.git_mut().sha_kind_mut() = ShaKind::Short;
    *config.git_mut().skip_if_error_mut() = true;
    vergen(config).unwrap();
    Ok(())
}
