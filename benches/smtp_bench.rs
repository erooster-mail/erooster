use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::net::TcpStream;
use erooster::config::Config;
use std::sync::Arc;
use std::path::Path;
use tokio::runtime::Runtime;
use std::io::{Read, Write};
use tracing::{info,error};

pub fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("login", |b| {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
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
            erooster::smtp_servers::start(config).unwrap();
        });
        b.iter(|| {
            let mut stream = TcpStream::connect("127.0.0.1:25").unwrap();

            //TODO write a message
            stream.write(&[1]).unwrap();
            //TODO verify and continue
            stream.read(&mut [0; 128]).unwrap();
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
