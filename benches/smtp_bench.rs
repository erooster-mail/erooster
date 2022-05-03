use criterion::{black_box, criterion_group, criterion_main, Criterion};
use erooster::config::Config;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::Arc;
use std::{thread, time::Duration};
use tracing::{error, info};

fn login() {
    let mut stream = TcpStream::connect("127.0.0.1:25").unwrap();

    //TODO write a message
    stream.write(&[1]).unwrap();
    //TODO verify and continue
    stream.read(&mut [0; 128]).unwrap();
}

pub fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("login", |b| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.spawn(async {
            info!("Starting ERooster Server");
            let config = if Path::new("./config.yml").exists() {
                Arc::new(Config::load("./config.yml").await.unwrap())
            } else if Path::new("/etc/erooster/config.yml").exists() {
                Arc::new(Config::load("/etc/erooster/config.yml").await.unwrap())
            } else if Path::new("/etc/erooster/config.yaml").exists() {
                Arc::new(Config::load("/etc/erooster/config.yaml").await.unwrap())
            } else {
                error!("No config file found. Please follow the readme.");
                return;
            };
            if let Err(e) = erooster::smtp_servers::unencrypted::Unencrypted::run(config).await {
                panic!("Unable to start server: {:?}", e);
            }
            thread::sleep(Duration::from_millis(500));
        });
        b.iter(|| login())
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
