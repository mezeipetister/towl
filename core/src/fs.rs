/// Only sync operations
/// as fs operations on OS's are not async
/// operations. Call these methods from a block_on
/// code block to work with async code
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::io::prelude::*;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::{
    error::Error,
    fs::{File, OpenOptions},
    io::{BufReader, Seek, SeekFrom, Write},
};

const MAGIC: [u8; 9] = *b"towlfile*";
const VERSION: i32 = 1;
const HEADER_START: u64 = 0;
const INDEX_START: u64 = 1024;
const INDEX_OFFSET: u64 = 1024;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Header {
    magic: [u8; 9],
    version: i32,
    pub org: String,
    pub title: String,
    pub id: usize,
}

impl Header {
    fn check_magic(&self) -> bool {
        self.magic == MAGIC
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Index {
    opened: DateTime<Utc>,
    closed: Option<DateTime<Utc>>,
    count: usize,
    first_date: Option<DateTime<Utc>>,
    last_date: Option<DateTime<Utc>>,
}

impl Index {
    fn add_entry(&mut self, entry: &Entry) {
        match self.first_date {
            Some(_) => self.last_date = Some(entry.date),
            None => self.first_date = Some(entry.date),
        }
        self.count += 1;
    }
    fn close(&mut self) {
        self.closed = Some(Utc::now());
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Entry {
    pub date: DateTime<Utc>,
    pub ip: String,
    pub message: String,
}

#[derive(Clone)]
pub struct LogFile {
    inner: Arc<Mutex<LogFileInner>>,
}

struct LogFileInner {
    pub header: Header,
    pub index: Index,
    file: BufReader<File>,
}

impl LogFile {
    pub async fn init(path: &Path, org: String, title: String, id: usize) -> crate::Result<Self> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || -> crate::Result<Self> {
            // Create file
            let file = File::create(path).map_err(|e| e.to_string())?;
            // Set min file size
            file.set_len(INDEX_START + INDEX_OFFSET)
                .map_err(|e| e.to_string())?;
            // Create header
            let header = Header {
                magic: MAGIC,
                version: VERSION,
                org,
                title,
                id,
            };
            // Create index
            let index = Index {
                opened: Utc::now(),
                closed: None,
                count: 0,
                first_date: None,
                last_date: None,
            };
            let inner = LogFileInner {
                header,
                index,
                file: BufReader::new(file),
            };
            let mut res = LogFile {
                inner: Arc::new(Mutex::new(inner)),
            };
            // Save header to disk
            res.save_header()?;
            // Save index to disk
            res.save_index()?;
            // Return file
            Ok(res)
        })
        .await
        .unwrap()
    }
    pub async fn open(path: &str) -> crate::Result<Self> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || -> crate::Result<Self> {
            // Open file with read write access
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(|e| e.to_string())?;

            // Seek from start and read header
            let _ = file.seek(SeekFrom::Start(HEADER_START));
            let header: Header = bincode::deserialize_from(&file).map_err(|e| e.to_string())?;

            if !header.check_magic() {
                return Err("Not a towl log file. Magic error.".into());
            }

            // Seek from index start position and read index
            let _ = file.seek(SeekFrom::Start(INDEX_START));
            let index: Index = bincode::deserialize_from(&file).map_err(|e| e.to_string())?;

            let inner = LogFileInner {
                header,
                index,
                file: BufReader::new(file),
            };
            // Return self
            Ok(Self {
                inner: Arc::new(Mutex::new(inner)),
            })
        })
        .await
        .unwrap()
    }
    pub async fn is_towl_file(path: &str) -> bool {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || -> bool {
            let mut file = match OpenOptions::new().read(true).open(path) {
                Ok(f) => f,
                Err(_) => return false,
            };

            let _ = file.seek(SeekFrom::Start(HEADER_START));
            let mut _magic = [0, 0, 0, 0, 0, 0, 0, 0, 0];
            let magic = match file.read_exact(&mut _magic) {
                Ok(_) => (),
                Err(_) => return false,
            };

            _magic == MAGIC
        })
        .await
        .unwrap()
    }
    pub fn close(&mut self) -> crate::Result<()> {
        self.inner.lock().unwrap().index.close();
        self.save_index()
    }
    fn set_header(&mut self, header: Header) -> Result<(), Box<dyn Error>> {
        // Store old header
        let old_header = self.inner.lock().unwrap().header.clone();
        // Set new header
        self.inner.lock().unwrap().header = header;
        // Save header to disk
        self.save_header()?;
        Ok(())
    }
    fn save_header(&mut self) -> crate::Result<()> {
        // Set cursor to 0 bytes
        let _ = self
            .inner
            .lock()
            .unwrap()
            .file
            .seek(SeekFrom::Start(HEADER_START));
        let header = self.inner.lock().unwrap().header.clone();
        // Serialize header
        bincode::serialize_into(self.inner.lock().unwrap().file.get_mut(), &header)
            .map_err(|e| e.to_string())?;
        // Flush file
        self.flush().map_err(|e| e.to_string())?;
        Ok(())
    }
    fn save_index(&mut self) -> crate::Result<()> {
        // Set cursor to 0 bytes
        let _ = self
            .inner
            .lock()
            .unwrap()
            .file
            .seek(SeekFrom::Start(INDEX_START));
        let index = self.inner.lock().unwrap().index.clone();
        // Serialize header
        bincode::serialize_into(self.inner.lock().unwrap().file.get_mut(), &index)
            .map_err(|e| e.to_string())?;
        // Flush file
        self.flush().map_err(|e| e.to_string())?;
        Ok(())
    }
    fn flush(&mut self) -> Result<(), Box<dyn Error>> {
        self.inner.lock().unwrap().file.get_mut().flush()?;
        Ok(())
    }
    pub async fn add_entry(&mut self, entry: Entry) -> crate::Result<()> {
        let mut file = self.clone();
        tokio::task::spawn_blocking(move || -> crate::Result<()> {
            // Set cursor to the end
            file.inner
                .lock()
                .unwrap()
                .file
                .seek(SeekFrom::End(0))
                .unwrap();
            // Serialize entry into it
            bincode::serialize_into(file.inner.lock().unwrap().file.get_mut(), &entry)
                .map_err(|e| e.to_string())?;
            // Update index
            file.inner.lock().unwrap().index.add_entry(&entry);
            // Store index to disk
            file.save_index()?;

            Ok(())
        })
        .await
        .unwrap()
    }
    pub async fn header(&self) -> Header {
        let file = self.clone();
        tokio::task::spawn_blocking(move || -> Header { file.inner.lock().unwrap().header.clone() })
            .await
            .unwrap()
    }
    pub async fn stream<'a, F>(&'a mut self, mut f: F) -> crate::Result<()>
    where
        F: FnMut(Entry) + Send + 'static,
    {
        let mut file = self.clone();
        tokio::task::spawn_blocking(move || -> crate::Result<()> {
            // Set offset to start position
            file.inner
                .lock()
                .unwrap()
                .file
                .seek(SeekFrom::Start(INDEX_START + INDEX_OFFSET))
                .map_err(|e| e.to_string())?;

            // Stream log entries
            while let Ok(entry) =
                bincode::deserialize_from(&mut file.inner.lock().unwrap().deref_mut().file)
            {
                f(entry)
            }
            Ok(())
        })
        .await
        .unwrap()
    }
}
