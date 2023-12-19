/// Only sync operations
/// as fs operations on OS's are not async
/// operations. Call these methods from a block_on
/// code block to work with async code
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::path::Path;
use std::sync::Arc;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufReader, SeekFrom};
use tokio::sync::Mutex;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Entry {
  pub id: String,
  pub sender: String,
  pub received: DateTime<Utc>,
  pub log_json: String,
}

#[derive(Clone)]
pub struct LogFile {
  file: Arc<Mutex<BufReader<File>>>,
}

impl LogFile {
  pub async fn init(path: &Path) -> crate::Result<Self> {
    let path = path.to_owned();
    // Create file
    let file = File::create(path).await.map_err(|e| e.to_string())?;
    let res = LogFile {
      file: Arc::new(Mutex::new(BufReader::new(file))),
    };
    Ok(res)
  }
  pub async fn open(path: &str) -> crate::Result<Self> {
    let path = path.to_owned();
    // Open file with read write access
    let mut file = OpenOptions::new()
      .read(true)
      .write(true)
      .open(path)
      .await
      .map_err(|e| e.to_string())?;

    // Return self
    Ok(Self {
      file: Arc::new(Mutex::new(BufReader::new(file))),
    })
  }
  async fn flush(&mut self) -> Result<(), Box<dyn Error>> {
    self.file.lock().await.get_mut().flush().await?;
    Ok(())
  }
  pub async fn add_entry(&mut self, entry: Entry) -> crate::Result<()> {
    // Serialize in spawn_blocking
    // bincode is not async
    let serialized = tokio::task::spawn_blocking(move || -> crate::Result<Vec<u8>> {
      bincode::serialize(&entry).map_err(|e| e.to_string())
    })
    .await
    .unwrap()
    .unwrap();

    // Set cursor to the end
    self.file.lock().await.seek(SeekFrom::End(0)).await.unwrap();
    // Save content to file
    self.file.lock().await.write_all(&serialized).await.unwrap();
    // Flush file
    self.flush().await.unwrap();
    // Return ok
    Ok(())
  }
}
