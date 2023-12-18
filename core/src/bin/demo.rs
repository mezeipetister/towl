use core::{
    fs::Entry,
    logger::{Config, Logger},
};

use chrono::Utc;

#[tokio::main]
async fn main() {
    let config = Config::builder().org("gz".into()).title("log".into());
    let mut logger = Logger::init(config).await.unwrap();
    logger.archive().await.unwrap();
    println!("Counter is: {}", core::logger::counter_value().await);

    let mut rx = logger.watch().await;
    tokio::task::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            println!("Log arrived: {:?}", msg);
        }
    });

    for i in 1..10 {
        let _ = logger
            .add_entry(Entry {
                date: Utc::now(),
                ip: "-".into(),
                message: format!("Msg {}", i),
            })
            .await;
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
