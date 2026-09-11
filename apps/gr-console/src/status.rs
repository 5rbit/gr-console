//! `StatusEvent` stream: the GR status PLC's WEBMON snapshot, republished on every webmon tick.

use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Value as Json, json};
use tokio::sync::broadcast;

use crate::plc::{PlcHandle, Tier};

#[derive(Clone)]
pub struct StatusBus {
    pub tx: broadcast::Sender<Json>,
    last: Arc<Mutex<Option<Json>>>,
}

impl StatusBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(64);
        Self { tx, last: Arc::new(Mutex::new(None)) }
    }

    pub fn latest(&self) -> Option<Json> {
        self.last.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Follows `plc` and emits a StatusEvent whenever the `db` (WEBMON) is refreshed.
    pub fn follow(&self, plc: PlcHandle, db: String, source: &'static str) {
        let bus = self.clone();
        tokio::spawn(async move {
            let mut rx = plc.events.subscribe();
            let mut seq: u64 = 0;
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        if !(ev.tier == Tier::Webmon || (ev.tier == Tier::Fast && plc.cfg.webmon.is_empty())) || !ev.dbs.iter().any(|d| d == &db) {
                            continue;
                        }
                        let snap = plc.snap();
                        let Some(d) = snap.db(&db) else { continue };
                        seq += 1;
                        let event = json!({ "at": d.at, "source": source, "seq": seq, "plc": plc.name(), "webmon": *d.json });
                        *bus.last.lock().unwrap_or_else(PoisonError::into_inner) = Some(event.clone());
                        let _ = bus.tx.send(event);
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
    }
}

impl Default for StatusBus {
    fn default() -> Self {
        Self::new()
    }
}
