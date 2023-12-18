use crate::fs::{Entry, LogFile};
use chrono::{Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::{error::Error, fs::File, io::Write, ops::Deref, path::Path, sync::Arc};
use tokio::sync::{broadcast::Receiver, Mutex};

const ARCHIVE_PATH: &'static str = "data/archive";
const WORKING_PATH: &'static str = "data/working.twl";
const INTERNAL_DATA_PATH: &'static str = "data/internal_data.db";

#[derive(Serialize, Deserialize)]
struct InternalData {
    counter: usize,
}

impl InternalData {
    fn read() -> crate::Result<Self> {
        let file = File::options()
            .read(true)
            .open(INTERNAL_DATA_PATH)
            .map_err(|e| e.to_string())?;
        let res = bincode::deserialize_from(&file).map_err(|e| e.to_string())?;
        Ok(res)
    }
    fn init() -> crate::Result<()> {
        // Check if file exist
        // create new one of no file found
        if !Path::new(INTERNAL_DATA_PATH).exists() {
            // Create empty file
            let mut file = File::create(INTERNAL_DATA_PATH).map_err(|e| e.to_string())?;
            let init_data = InternalData { counter: 1 };
            // Write init data into it
            let _ = bincode::serialize_into(&mut file, &init_data).map_err(|e| e.to_string())?;
            file.flush().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    fn save(&self) -> crate::Result<()> {
        let mut file = File::options()
            .read(true)
            .write(true)
            .open(INTERNAL_DATA_PATH)
            .map_err(|e| e.to_string())?;
        let _ = bincode::serialize_into(&mut file, self);
        file.flush().map_err(|e| e.to_string())?;
        Ok(())
    }
    fn increment_counter() -> crate::Result<()> {
        let mut logger_data = InternalData::read()?;
        logger_data.counter += 1;
        logger_data.save()?;
        Ok(())
    }
}

pub async fn counter_value() -> usize {
    tokio::task::spawn_blocking(move || InternalData::read().unwrap().counter)
        .await
        .unwrap()
}

pub enum Archive {
    Daily,
    Weekly,
}

pub struct Config {
    org: String,
    title: String,
    strategy: Archive,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            strategy: Archive::Daily,
            org: "Unknown".into(),
            title: "Unknown".into(),
        }
    }
}

impl Config {
    pub fn builder() -> Self {
        Self::default()
    }
    pub fn archive_strategy(mut self, v: Archive) -> Self {
        self.strategy = v;
        self
    }
    pub fn org(mut self, v: String) -> Self {
        self.org = v;
        self
    }
    pub fn title(mut self, v: String) -> Self {
        self.title = v;
        self
    }
}

#[derive(Clone)]
pub struct Logger {
    config: Arc<Mutex<Config>>,
    working: LogFile,
    broadcast_tx: tokio::sync::broadcast::Sender<Entry>,
}

impl Logger {
    /// Init Logger
    pub async fn init(config: Config) -> crate::Result<Logger> {
        // Spawn sync tasks
        let _ = tokio::task::spawn_blocking(move || -> crate::Result<()> {
            // Init internal data
            InternalData::init()?;

            // Init archive folder
            if !Path::new(ARCHIVE_PATH).exists() {
                std::fs::create_dir(ARCHIVE_PATH).map_err(|e| e.to_string())?;
            }

            Ok(())
        })
        .await
        .expect("Error during spawn_blocking when initing");

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

            let mut interval =
                tokio::time::interval_at(tokio::time::Instant::now() + duration, period);

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
        self.broadcast_tx
            .send(entry)
            .expect("Error sending log entry to broadcast subscribers");
        Ok(())
    }
    /// Subscribe for events
    pub async fn watch(&mut self) -> Receiver<Entry> {
        self.broadcast_tx.subscribe()
    }
}
