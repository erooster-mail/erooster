use vergen::EmitBuilder;

fn main() -> std::io::Result<()> {
    EmitBuilder::builder().git_sha(true).emit().unwrap();
    Ok(())
}
