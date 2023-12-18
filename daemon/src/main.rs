use chrono::Utc;
use proto::towl::Entry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::spawn;
use tokio::sync::{mpsc, mpsc::Receiver};
use tokio::task::spawn_blocking;

// fn deserialize<'a>(i: &'a str) -> Value {
//   let i: Value = serde_json::from_str(i).unwrap();
//   i
// }

fn create_hash(from: &str) -> String {
  // create a Sha256 object
  let mut hasher = Sha256::new();
  // Update hasher
  hasher.update(from.as_bytes());
  // Return hash String
  format!("{:x}", hasher.finalize())
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct Config {
  remote_addr: String,
  remote_port: String,
  sender_name: String,
}

#[tokio::main]
async fn main() {
  // Read config
  // from /etc/towl/daemon.json
  let config = if let Ok(mut config_file) = tokio::fs::File::open("/etc/towl/daemon.json").await {
    let mut c = String::new();
    config_file
      .read_to_string(&mut c)
      .await
      .expect("Cannot read config file; it does exist, but error during read.");
    spawn_blocking(move || {
      let config: Config =
        serde_json::from_str(&c).expect("Wrong config format! Cannot deserialize");
      config
    })
    .await
    .unwrap()
  } else {
    // If no config found, return default config
    Config::default()
  };

  // Create channels
  let (tx, rx) = mpsc::channel(100);

  // Spawn sender as background process
  spawn(async move {
    sender(rx, config.clone()).await;
  });

  let mut child = Command::new("journalctl")
    .args(&["-f", "-o", "json", "--since", "now"])
    .stdout(Stdio::piped())
    .spawn()
    .expect("failed to execute child");

  let output = child.stdout.as_mut().unwrap();
  let outbuf = BufReader::new(output);
  let mut lines = outbuf.lines();

  while let Some(log_json) = lines
    .next_line()
    .await
    .expect("Error during getting next line from journalctl process")
  {
    let _ = tx.send(log_json).await.unwrap();
  }
}

async fn sender(mut rx: Receiver<String>, config: Config) {
  // Connect to remote
  let mut remote = proto::towl::towl_server_client::TowlServerClient::connect(format!(
    "{}:{}",
    &config.remote_addr, &config.remote_port
  ))
  .await
  .expect("Error connecting to remote");

  while let Some(log_json) = rx.recv().await {
    // Create ID
    let id = {
      let log_json_cloned = log_json.clone();
      spawn_blocking(move || create_hash(&log_json_cloned))
        .await
        .expect("Error creating hash value")
    };
    // Get sender ID
    let sender = config.sender_name.clone();
    // Calculate received date as RFC3339
    let received_rfc3339 = Utc::now().to_rfc3339();

    let _ = remote
      .add(Entry {
        id,
        sender,
        received_rfc3339,
        log_json,
      })
      .await
      .expect("Error adding log entry to remote");
  }
}
