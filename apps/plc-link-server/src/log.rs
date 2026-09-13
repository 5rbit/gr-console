//! Message log entries and the JSONL file writer (`data/plc-link/log-YYYYMMDD.jsonl`).

use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use plc_link::{Format, Framing};
use serde::Serialize;
use serde_json::value::RawValue;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    Rx,
    Tx,
}

impl Dir {
    pub fn parse(s: &str) -> Option<Dir> {
        match s {
            "rx" => Some(Dir::Rx),
            "tx" => Some(Dir::Tx),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct LogEntry {
    pub id: u64,
    /// RFC 3339 local time.
    pub ts: String,
    pub plc: String,
    pub dir: Dir,
    pub framing: Option<Framing>,
    pub format: Option<Format>,
    pub msg_type: Option<u16>,
    pub name: Option<String>,
    pub seq: Option<u16>,
    pub sig: Option<u32>,
    /// Bytes on the wire (frame / line / HTTP body).
    pub len: usize,
    pub ok: bool,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    pub peer: Option<String>,
    /// Message data as JSON (declaration order), `null` without payload.
    pub data: Option<Box<RawValue>>,
    /// Wire JSON text of the data, or the (truncated) received text of a message that failed to decode.
    pub data_text: Option<String>,
    /// UDT bytes (canonical payload) as hex.
    pub hex: Option<String>,
}

impl LogEntry {
    pub fn new(plc: &str, dir: Dir) -> Self {
        LogEntry {
            id: 0,
            ts: String::new(),
            plc: plc.to_string(),
            dir,
            framing: None,
            format: None,
            msg_type: None,
            name: None,
            seq: None,
            sig: None,
            len: 0,
            ok: true,
            error: None,
            warnings: Vec::new(),
            peer: None,
            data: None,
            data_text: None,
            hex: None,
        }
    }
}

/// Lossy text of received bytes for the log (max 2000 characters).
pub fn preview_text(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    s.chars().take(2000).collect()
}

/// Starts the JSONL writer thread; entries are appended to `<dir>/log-YYYYMMDD.jsonl` and flushed every second.
pub fn spawn_file_writer(dir: PathBuf) -> std::io::Result<mpsc::Sender<std::sync::Arc<LogEntry>>> {
    std::fs::create_dir_all(&dir)?;
    let (tx, rx) = mpsc::channel::<std::sync::Arc<LogEntry>>();
    std::thread::Builder::new().name("plc-link-log".into()).spawn(move || {
        let mut current: Option<(String, std::io::BufWriter<std::fs::File>)> = None;
        loop {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(e) => {
                    let stamp = crate::util::date_stamp();
                    if current.as_ref().is_none_or(|(s, _)| *s != stamp) {
                        if let Some((_, mut w)) = current.take() {
                            let _ = w.flush();
                        }
                        let path = dir.join(format!("log-{stamp}.jsonl"));
                        match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                            Ok(f) => current = Some((stamp, std::io::BufWriter::new(f))),
                            Err(err) => tracing::warn!(path = %path.display(), %err, "log file"),
                        }
                    }
                    if let Some((_, w)) = current.as_mut()
                        && let Ok(line) = serde_json::to_string(&*e)
                    {
                        let _ = w.write_all(line.as_bytes());
                        let _ = w.write_all(b"\n");
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some((_, w)) = current.as_mut() {
                        let _ = w.flush();
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if let Some((_, mut w)) = current.take() {
                        let _ = w.flush();
                    }
                    return;
                }
            }
        }
    })?;
    Ok(tx)
}
