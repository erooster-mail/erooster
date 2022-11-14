use vergen::{vergen, Config, ShaKind};

fn main() -> std::io::Result<()> {
    let mut config = Config::default();
    *config.git_mut().sha_kind_mut() = ShaKind::Short;
    *config.git_mut().skip_if_error_mut() = true;
    vergen(config).unwrap();
    Ok(())
}
