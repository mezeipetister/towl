use crate::fs::{Entry, LogFile};
use chrono::{Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Arc};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  sync::{broadcast::Receiver, Mutex},
  task::spawn_blocking,
};

#[derive(Serialize, Deserialize, Clone)]
struct Settings {
  file_max_count: i32,
  working_id: i32,
}

impl Settings {
  async fn load(file_max_count: i32) -> crate::Result<Self> {
    let path = Path::new("settings");

    if path.exists() {
      let mut buf = Vec::new();

      tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .await
        .expect("Cannot open settings file, but it exists")
        .read_to_end(&mut buf);

      let settings: Settings = spawn_blocking(move || {
        bincode::deserialize(&buf).expect("Error deser. settings from bytes")
      })
      .await
      .unwrap();

      return Ok(settings);
    } else {
      let mut settings = Settings {
        file_max_count,
        working_id: 0,
      };

      settings.save().await?;

      Ok(settings)
    }
  }
  async fn save(&mut self) -> crate::Result<()> {
    let s = self.clone();

    let bytes = spawn_blocking(move || {
      bincode::serialize(&s).expect("Error serializing settings to bincode bytes")
    })
    .await
    .unwrap();

    if !Path::new("settings").exists() {
      tokio::fs::File::create("settings")
        .await
        .expect("Error creating settings file");
    }

    tokio::fs::OpenOptions::new()
      .write(true)
      .open("settings")
      .await
      .expect("Cannot open settings file, but it exists")
      .write_all(&bytes)
      .await
      .expect("Error writing settings to disk.");

    Ok(())
  }
}

pub struct Logger {
  settings: Settings,
  working: LogFile,
  broadcast_tx: tokio::sync::broadcast::Sender<Entry>,
}

impl Logger {
  /// Init Logger
  pub async fn init(file_max_count: i32) -> crate::Result<Logger> {
    // Load settings
    let settings = Settings::load(file_max_count).await?;

    // Init broadcast
    let (broadcast_tx, _) = tokio::sync::broadcast::channel(16);

    // Init working
    let working = if Path::new(WORKING_PATH).exists() {
      LogFile::open(WORKING_PATH).await?
    } else {
      LogFile::init(
        Path::new(WORKING_PATH),
        config.org.clone(),
        config.title.clone(),
        InternalData::read()?.counter,
      )
      .await?
    };

    let res = Logger {
      config: Arc::new(Mutex::new(config)),
      working,
      broadcast_tx,
    };

    let mut _logger = res.clone();

    // Spawn background archive checking process
    tokio::task::spawn(async move {
      let now = chrono::Utc::now();

      // Start archive at 23:59:59 each day
      let start = now
        .with_hour(23)
        .unwrap()
        .with_minute(59)
        .unwrap()
        .with_second(59)
        .unwrap();

      let duration = start.signed_duration_since(now).to_std().unwrap();

      let period = chrono::Duration::days(1).to_std().unwrap();

      let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + duration, period);

      loop {
        // Wait till tick time
        interval.tick().await;

        _logger.archive().await.unwrap();
      }
    });

    Ok(res)
  }
  /// Archive current working log
  /// and create a new one
  pub async fn archive(&mut self) -> crate::Result<()> {
    // Get working header
    let working = self.working.clone();

    // Get working header
    let working_header = working.header().await;

    // Spawn fs tasks on blocking thread
    tokio::task::spawn_blocking(move || -> crate::Result<()> {
      // Create date string
      let date = {
        let now = Utc::now();
        let (_, year) = now.year_ce();
        let date = format!("{}_{}_{}", year, now.month(), now.day());
        date
      };
      let working_path = Path::new(WORKING_PATH);

      let new_path = Path::new(ARCHIVE_PATH).join(format!(
        "{}_{}_{}_{}.twl",
        working_header.org, working_header.title, date, working_header.id
      ));
      // Only archive when working file exist
      if working_path.exists() {
        // Move working file to the archive folder
        std::fs::rename(working_path, new_path).map_err(|e| e.to_string())?;
        // Increment counter
        InternalData::increment_counter()?;
      }
      Ok(())
    })
    .await
    .expect("Error during spawn blocking when archiving")
  }
  /// Add log entry
  pub async fn add_entry(&mut self, entry: crate::fs::Entry) -> crate::Result<()> {
    // Cloning working
    self.working.add_entry(entry.clone()).await?;
    self
      .broadcast_tx
      .send(entry)
      .expect("Error sending log entry to broadcast subscribers");
    Ok(())
  }
  /// Subscribe for events
  pub async fn watch(&mut self) -> Receiver<Entry> {
    self.broadcast_tx.subscribe()
  }
}
