/// Only sync operations
/// as fs operations on OS's are not async
/// operations. Call these methods from a block_on
/// code block to work with async code
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread::spawn;
use tokio::sync::mpsc::Sender;
use tokio::task::spawn_blocking;

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
      Some(_) => self.last_date = Some(entry.received),
      None => self.first_date = Some(entry.received),
    }
    self.count += 1;
  }
  fn close(&mut self) {
    self.closed = Some(Utc::now());
  }
  fn reset(&mut self) {
    self.count = 0;
    self.first_date = None;
    self.last_date = None;
  }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Entry {
  pub id: String,
  pub sender: String,
  pub received: DateTime<Utc>,
  pub log_json: String,
}

pub struct LogFile {
  pub header: Header,
  pub index: Index,
  file: BufWriter<File>,
}

impl LogFile {
  pub fn init(parent_path: &str, org: String, title: String, id: usize) -> crate::Result<Self> {
    let log_path = Path::new(parent_path).join(format!("{id}.towl"));

    if log_path.exists() {
      return Err("Log file have already exist.".to_string());
    }

    // Create file
    let file = File::create(&log_path).map_err(|e| e.to_string())?;
    // Set min file size
    file
      .set_len(INDEX_START + INDEX_OFFSET)
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
    let mut res = LogFile {
      header,
      index,
      file: BufWriter::new(file),
    };
    // Save header to disk
    res.save_header()?;
    // Save index to disk
    res.save_index()?;

    // Open file
    Self::open(Path::new(parent_path).join(format!("{id}.towl")).as_path())
  }
  pub fn open<T>(path: T) -> crate::Result<Self>
  where
    T: AsRef<Path>,
  {
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

    let mut res = LogFile {
      header,
      index,
      file: BufWriter::new(file),
    };

    // Check if we need reindex
    // If opened file is not closed, then we need to reindex it
    res.reindex()?;

    // Return self
    Ok(res)
  }
  pub fn is_towl_file(path: &str) -> bool {
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
  }
  /// Close log file
  pub fn close(&mut self) -> crate::Result<()> {
    self.index.close();
    self.save_index()
  }
  fn save_header(&mut self) -> crate::Result<()> {
    // Set cursor to 0 bytes
    let _ = self.file.seek(SeekFrom::Start(HEADER_START));
    let header = self.header.clone();
    // Serialize header
    bincode::serialize_into(self.file.get_mut(), &header).map_err(|e| e.to_string())?;
    // Flush file
    self.flush().map_err(|e| e.to_string())?;
    Ok(())
  }
  fn save_index(&mut self) -> crate::Result<()> {
    // Set cursor to 0 bytes
    let _ = self.file.seek(SeekFrom::Start(INDEX_START));
    // Serialize index
    bincode::serialize_into(self.file.get_mut(), &self.index).map_err(|e| e.to_string())?;
    // Flush file
    self.flush().map_err(|e| e.to_string())?;
    Ok(())
  }
  fn flush(&mut self) -> Result<(), Box<dyn Error>> {
    self.file.get_mut().flush()?;
    Ok(())
  }
  /// Add entry to log file
  pub fn add_entry(&mut self, entry: Entry) -> crate::Result<()> {
    // Set cursor to the end
    self.file.seek(SeekFrom::End(0)).unwrap();
    // Serialize entry into it
    bincode::serialize_into(self.file.get_mut(), &entry).map_err(|e| e.to_string())?;
    // Update index
    // We need to save index when we close this logfile
    self.index.add_entry(&entry);

    Ok(())
  }
  pub fn header(&self) -> &Header {
    &self.header
  }
  pub fn reindex(&mut self) -> crate::Result<()> {
    // Set offset to start position
    self
      .file
      .seek(SeekFrom::Start(INDEX_START + INDEX_OFFSET))
      .map_err(|e| e.to_string())?;

    // Reset index
    self.index.reset();

    // Stream log entries
    while let Ok(entry) = bincode::deserialize_from::<&mut File, Entry>(&mut self.file.get_mut()) {
      self.index.add_entry(&entry);
    }

    // Save index
    self.save_index()?;
    Ok(())
  }
  pub fn stream<'a>(&'a mut self, after_dt: DateTime<Utc>, tx: Sender<Entry>) -> crate::Result<()> {
    // Set offset to start position
    self
      .file
      .seek(SeekFrom::Start(INDEX_START + INDEX_OFFSET))
      .map_err(|e| e.to_string())?;

    // Stream log entries
    while let Ok(entry) = bincode::deserialize_from::<&mut File, Entry>(&mut self.file.get_mut()) {
      if entry.received > after_dt {
        tx.blocking_send(entry)
          .expect("Error sending entry to reader via tokio channel");
      }
    }
    Ok(())
  }
}
