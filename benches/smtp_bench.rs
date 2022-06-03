use criterion::{criterion_group, criterion_main, Criterion};
use erooster::backend::database::{get_database, Database, DB};
use erooster::backend::storage::get_storage;
use erooster::{config::Config, line_codec::LinesCodec};
use futures::{SinkExt, StreamExt};
use std::{path::Path, sync::Arc, thread, time::Duration};
use tokio::net::TcpStream;
use tokio::runtime;
use tokio_util::codec::Framed;
use tracing::{error, info};

// Warning: This seems to fail on windows but works on linux fine
async fn login() {
    let stream = TcpStream::connect("127.0.0.1:25").await.unwrap();

    let stream_codec = Framed::new(stream, LinesCodec::new());
    let (mut sender, mut reader) = stream_codec.split();
    sender.send(String::from("EHLO localhost")).await.unwrap();
    let resp = reader.next().await.unwrap().unwrap();
    assert_eq!(resp, String::from("220 localhost ESMTP Erooster"));
    let resp = reader.next().await.unwrap().unwrap();
    assert_eq!(resp, String::from("250-localhost"));
    let resp = reader.next().await.unwrap().unwrap();
    assert_eq!(resp, String::from("250 AUTH LOGIN"));
    sender.send(String::from("AUTH LOGIN")).await.unwrap();
    let resp = reader.next().await.unwrap().unwrap();
    assert_eq!(resp, String::from("334 VXNlcm5hbWU6"));
    sender
        .send(String::from("dGVzdEBsb2NhbGhvc3Q="))
        .await
        .unwrap();
    let resp = reader.next().await.unwrap().unwrap();
    assert_eq!(resp, String::from("334 UGFzc3dvcmQ6"));
    sender.send(String::from("dGVzdA==")).await.unwrap();
    let resp = reader.next().await.unwrap().unwrap();
    assert_eq!(resp, String::from("235 ok"));
}

pub fn criterion_benchmark(c: &mut Criterion) {
    //tracing_subscriber::fmt::init();
    c.bench_function("login", |b| {
        let rt = runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
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

            match get_database(Arc::clone(&config)).await {
                Ok(db) => {
                    let database: DB = Arc::new(db);
                    database.add_user("test@localhost").await.unwrap();
                    database
                        .change_password("test@localhost", "test")
                        .await
                        .unwrap();

                    let storage = Arc::new(get_storage(Arc::clone(&database)));

                    if let Err(e) = erooster::smtp_servers::unencrypted::Unencrypted::run(
                        config, database, storage,
                    )
                    .await
                    {
                        panic!("Unable to start server: {:?}", e);
                    }
                }
                Err(e) => panic!("Unable to connect to database server: {:?}", e),
            }
        });
        thread::sleep(Duration::from_millis(500));
        b.to_async(rt).iter(login)
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
