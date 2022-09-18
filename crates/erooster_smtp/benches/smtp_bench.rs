use criterion::{criterion_group, criterion_main, Criterion};
use erooster_core::backend::database::{get_database, Database, DB};
use erooster_core::backend::storage::get_storage;
use erooster_core::{config::Config, line_codec::LinesCodec};
use futures::{SinkExt, StreamExt};
use secrecy::SecretString;
use sqlx::migrate::MigrateDatabase;
use std::str::FromStr;
use std::{path::Path, sync::Arc, thread, time::Duration};
use tokio::net::TcpStream;
use tokio::runtime;
use tokio_util::codec::Framed;
use tracing::{error, info};

// Warning: This seems to fail on windows but works on linux fine
async fn receive_message() {
    let stream = TcpStream::connect("127.0.0.1:25").await.unwrap();

    let stream_codec = Framed::new(stream, LinesCodec::new());
    let (mut sender, mut reader) = stream_codec.split();

    // Send EHLO
    sender.send(String::from("EHLO localhost")).await.unwrap();
    let resp = reader.next().await.unwrap().unwrap();
    assert_eq!(resp, String::from("220 localhost ESMTP Erooster"));
    let resp = reader.next().await.unwrap().unwrap();
    assert_eq!(resp, String::from("250-localhost"));
    let resp = reader.next().await.unwrap().unwrap();
    assert_eq!(resp, String::from("250-ENHANCEDSTATUSCODES"));
    let resp = reader.next().await.unwrap().unwrap();
    assert_eq!(resp, String::from("250-STARTTLS"));
    let resp = reader.next().await.unwrap().unwrap();
    assert_eq!(resp, String::from("250 SMTPUTF8"));

    // Login to SMTP server (not needed here)
    // TODO we need to make sure to do tls here.
    /*
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
     */

    // Set FROM envelope address
    sender
        .send(String::from("MAIL FROM:<test@remote>"))
        .await
        .unwrap();
    let resp = reader.next().await.unwrap().unwrap();
    assert_eq!(resp, String::from("250 OK"));

    // Set TO envelope address
    sender
        .send(String::from("RCPT TO:<test@localhost>"))
        .await
        .unwrap();
    let resp = reader.next().await.unwrap().unwrap();
    assert_eq!(resp, String::from("250 OK"));

    // Announce that we want to send some DATA
    sender.send(String::from("DATA")).await.unwrap();
    let resp = reader.next().await.unwrap().unwrap();
    assert_eq!(
        resp,
        String::from("354 Start mail input; end with <CRLF>.<CRLF>")
    );

    // Send data and terminate with a <CRLF>.<CRLF>
    let email = include_str!("example_mail.txt");
    sender.send(format!("{}\r\n.", email)).await.unwrap();
    let resp = reader.next().await.unwrap().unwrap();
    assert_eq!(resp, String::from("250 OK"));

    // QUIT the connection
    sender.send(String::from("QUIT")).await.unwrap();
}

pub fn criterion_benchmark(c: &mut Criterion) {
    //tracing_subscriber::fmt::init();
    let mut group = c.benchmark_group("high_sample_count");

    let rt = runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    info!("Spawn setup");
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
        sqlx::postgres::Postgres::drop_database(&config.database.postgres_url)
            .await
            .unwrap();
        sqlx::postgres::Postgres::create_database(&config.database.postgres_url)
            .await
            .unwrap();
        match get_database(Arc::clone(&config)).await {
            Ok(db) => {
                info!("Connected to database");
                let database: DB = Arc::new(db);
                info!("Adding user");
                database.add_user("test@localhost").await.unwrap();
                info!("Setting user password");
                database
                    .change_password("test@localhost", SecretString::from_str("test").unwrap())
                    .await
                    .unwrap();
                info!("Created users");

                let storage = Arc::new(get_storage(Arc::clone(&database), Arc::clone(&config)));

                info!("Starting SMTP Server");
                if let Err(e) =
                    erooster_smtp::servers::unencrypted::Unencrypted::run(config, database, storage)
                        .await
                {
                    panic!("Unable to start server: {:?}", e);
                }
            }
            Err(e) => panic!("Unable to connect to database server: {:?}", e),
        }
    });
    thread::sleep(Duration::from_millis(5000));

    group.significance_level(0.05).sample_size(10000);
    group.bench_function("receive_message", |b| {
        let rt = runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        b.to_async(rt).iter(receive_message);
    });
    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
