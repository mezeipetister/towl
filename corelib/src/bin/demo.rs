use std::{
  io::{Cursor, Write},
  path::Path,
  sync::{Arc, Mutex},
};

use chrono::Utc;
use corelib::fs::{Entry, LogFile};
use tokio::{spawn, task::spawn_blocking};

#[tokio::main]
async fn main() {
  let log = spawn_blocking(move || {
    corelib::fs::LogFile::init("data", "GZ".to_string(), "DEMO".to_string(), 0).unwrap()
  })
  .await
  .unwrap();

  let log = Arc::new(Mutex::new(log));

  let now = std::time::Instant::now();

  const SIZE: i32 = 50_000;

  // Perform write
  write(SIZE, log.clone()).await;

  println!(
    "Write done. Elapsed time: {} secs",
    now.elapsed().as_secs_f32()
  );

  let now = std::time::Instant::now();

  // Perform read
  read(SIZE, log.clone()).await;

  println!(
    "Read done. Elapsed time: {} secs",
    now.elapsed().as_secs_f32()
  );

  println!("{:?}", log.lock().unwrap().index);
}

async fn read(count: i32, log: Arc<Mutex<LogFile>>) {
  let (tx, mut rx) = tokio::sync::mpsc::channel::<Entry>(10);

  // Spawn background task to run filter
  let h = spawn_blocking(move || {
    let mut log = log.lock().unwrap();
    log
      .stream(Utc::now() - chrono::Duration::days(1), tx)
      .unwrap();
  });

  let mut counter = 0;
  while let Some(e) = rx.recv().await {
    counter += 1;
    if counter == count {
      println!("Last entry: {:?}", e);
    }
  }

  h.await.unwrap();

  println!("Counted entries: {}", counter);
}

async fn write(count: i32, log: Arc<Mutex<LogFile>>) {
  // let count = 10_000;

  println!("Data creation started");

  spawn_blocking(move || {
    let mut log = log.lock().unwrap();
    for i in 0..count {
      let entry = Entry {
        sender: i.to_string(),
        received: Utc::now(),
        log_format: 0,
        log_entry: format!("demodemodemo{}", i),
      };
      log.add_entry(entry).unwrap();
    }
  })
  .await
  .unwrap();
}
