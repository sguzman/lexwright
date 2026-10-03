use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process,
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

enum Command {
    Save { revision: u64, text: Arc<str> },
    Flush(Sender<io::Result<()>>),
}

#[derive(Debug)]
pub enum SaveEvent {
    Saved {
        revision: u64,
        elapsed: Duration,
    },
    Failed {
        revision: u64,
        elapsed: Duration,
        error: String,
    },
}

pub struct LedgerStore {
    path: PathBuf,
    command_tx: Sender<Command>,
    event_rx: Receiver<SaveEvent>,
}

impl Default for LedgerStore {
    fn default() -> Self {
        Self::new(default_ledger_path())
    }
}

impl LedgerStore {
    pub fn new(path: PathBuf) -> Self {
        let (command_tx, command_rx) = mpsc::channel::<Command>();
        let (event_tx, event_rx) = mpsc::channel::<SaveEvent>();
        let worker_path = path.clone();

        thread::Builder::new()
            .name("lexwright-save".to_owned())
            .spawn(move || save_worker(worker_path, command_rx, event_tx))
            .expect("failed to start ledger save worker");

        Self {
            path,
            command_tx,
            event_rx,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> io::Result<String> {
        match fs::read_to_string(&self.path) {
            Ok(text) => Ok(text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
            Err(error) => Err(error),
        }
    }

    pub fn queue_save(&self, revision: u64, text: Arc<str>) -> Result<(), String> {
        self.command_tx
            .send(Command::Save { revision, text })
            .map_err(|_| "save worker stopped".to_owned())
    }

    pub fn poll(&self) -> Option<SaveEvent> {
        self.event_rx.try_recv().ok()
    }

    pub fn flush(&self) -> io::Result<()> {
        let (reply_tx, reply_rx) = mpsc::channel();

        self.command_tx
            .send(Command::Flush(reply_tx))
            .map_err(|_| io::Error::other("save worker stopped"))?;

        reply_rx
            .recv()
            .map_err(|_| io::Error::other("save worker stopped"))?
    }
}

fn save_worker(path: PathBuf, command_rx: Receiver<Command>, event_tx: Sender<SaveEvent>) {
    let mut last_error: Option<(io::ErrorKind, String)> = None;

    while let Ok(command) = command_rx.recv() {
        match command {
            Command::Save { revision, text } => {
                let started = Instant::now();
                match atomic_write(&path, text.as_bytes()) {
                    Ok(()) => {
                        last_error = None;
                        let _ = event_tx.send(SaveEvent::Saved {
                            revision,
                            elapsed: started.elapsed(),
                        });
                    }
                    Err(error) => {
                        let elapsed = started.elapsed();
                        let message = error.to_string();
                        last_error = Some((error.kind(), message.clone()));
                        let _ = event_tx.send(SaveEvent::Failed {
                            revision,
                            elapsed,
                            error: message,
                        });
                    }
                }
            }
            Command::Flush(reply_tx) => {
                let result = match &last_error {
                    Some((kind, message)) => Err(io::Error::new(*kind, message.clone())),
                    None => Ok(()),
                };
                let _ = reply_tx.send(result);
            }
        }
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let temp_path = path.with_extension(format!("tmp-{}", process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp_path)?;

        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);

        fs::rename(&temp_path, path)?;

        #[cfg(unix)]
        if let Some(parent) = path.parent()
            && let Ok(directory) = File::open(parent)
        {
            let _ = directory.sync_all();
        }

        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }

    result
}

fn default_ledger_path() -> PathBuf {
    if let Some(path) = nonempty_env("LEXWRIGHT_LEDGER") {
        return PathBuf::from(path);
    }

    if let Some(data_home) = nonempty_env("XDG_DATA_HOME") {
        return PathBuf::from(data_home).join("lexwright/ledger.txt");
    }

    if let Some(home) = nonempty_env("HOME") {
        return PathBuf::from(home).join(".local/share/lexwright/ledger.txt");
    }

    PathBuf::from("lexwright-ledger.txt")
}

fn nonempty_env(name: &str) -> Option<std::ffi::OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::atomic_write;
    use std::{
        fs,
        path::PathBuf,
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn test_path() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before unix epoch")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "lexwright-storage-test-{}-{nonce}/ledger.txt",
            process::id()
        ))
    }

    #[test]
    fn atomic_write_replaces_previous_contents() {
        let path = test_path();

        atomic_write(&path, b"first").expect("first write failed");
        atomic_write(&path, b"second").expect("second write failed");

        let contents = fs::read_to_string(&path).expect("read failed");
        assert_eq!(contents, "second");

        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}
