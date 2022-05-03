use criterion::{black_box, criterion_group, criterion_main, Criterion};

pub fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("login", |b| {
        info!("Starting ERooster Server");
        let config = if Path::new("./config.yml").exists() {
            Arc::new(Config::load("./config.yml").await?)
        } else if Path::new("/etc/erooster/config.yml").exists() {
            Arc::new(Config::load("/etc/erooster/config.yml").await?)
        } else if Path::new("/etc/erooster/config.yaml").exists() {
            Arc::new(Config::load("/etc/erooster/config.yaml").await?)
        } else {
            error!("No config file found. Please follow the readme.");
            color_eyre::eyre::bail!("No config file found");
        };
        erooster::smtp_servers::start(Arc::clone(&config))?;
        b.iter(|| {
            let mut stream = TcpStream::connect("127.0.0.1:25").unwrap();

            //TODO write a message
            stream.write(&[1])?;
            //TODO verify and continue
            stream.read(&mut [0; 128])?;
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
