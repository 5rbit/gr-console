//! In-process fake S7 server serving in-memory DBs (tests / demo mode).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use crate::cotp;
use crate::pdu;

/// Shared DB memory: DB number -> bytes. Tests and the demo generator mutate it directly.
pub type DbStore = Arc<Mutex<HashMap<u16, Vec<u8>>>>;

pub struct FakeS7Server {
    pub addr: SocketAddr,
    pub dbs: DbStore,
    pub pdu: u16,
    handle: JoinHandle<()>,
}

impl FakeS7Server {
    /// Binds 127.0.0.1:0 and serves `dbs` until dropped. `pdu` is the PDU size offered.
    pub async fn spawn(dbs: DbStore, pdu: u16) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let store = dbs.clone();
        let handle = tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else { break };
                let store = store.clone();
                tokio::spawn(async move {
                    let _ = serve_conn(sock, store, pdu).await;
                });
            }
        });
        Ok(Self { addr, dbs, pdu, handle })
    }

    pub fn store() -> DbStore {
        Arc::new(Mutex::new(HashMap::new()))
    }

    pub fn set_db(&self, db: u16, bytes: Vec<u8>) {
        self.dbs.lock().unwrap_or_else(PoisonError::into_inner).insert(db, bytes);
    }

    pub fn get_db(&self, db: u16) -> Option<Vec<u8>> {
        self.dbs.lock().unwrap_or_else(PoisonError::into_inner).get(&db).cloned()
    }
}

impl Drop for FakeS7Server {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn read_tpkt(sock: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut head = [0u8; 4];
    sock.read_exact(&mut head).await?;
    let len = cotp::tpkt_len(&head).ok_or_else(|| std::io::Error::other("bad TPKT"))?;
    let mut buf = vec![0u8; len];
    buf[..4].copy_from_slice(&head);
    sock.read_exact(&mut buf[4..]).await?;
    Ok(buf)
}

async fn serve_conn(mut sock: TcpStream, store: DbStore, pdu: u16) -> std::io::Result<()> {
    // COTP connect
    let cr = read_tpkt(&mut sock).await?;
    if cr.get(5) != Some(&0xE0) {
        return Ok(());
    }
    sock.write_all(&cotp::connect_confirm()).await?;
    loop {
        let pkt = read_tpkt(&mut sock).await?;
        let Some(s7) = cotp::unwrap_dt(&pkt) else { continue };
        let seq = pdu::seq_of(s7).unwrap_or(0);
        let reply = match pdu::function_of(s7) {
            Some(0xF0) => pdu::setup_comm_ack(seq, pdu),
            Some(0x04) => {
                let items = pdu::parse_read_var(s7).unwrap_or_default();
                let store = store.lock().unwrap_or_else(PoisonError::into_inner);
                let results: Vec<Result<Vec<u8>, u8>> = items
                    .iter()
                    .map(|it| match store.get(&it.db) {
                        None => Err(0x0A),
                        Some(mem) => {
                            let end = it.start as usize + it.len as usize;
                            if end > mem.len() { Err(0x05) } else { Ok(mem[it.start as usize..end].to_vec()) }
                        }
                    })
                    .collect();
                pdu::read_var_ack(seq, &results)
            }
            Some(0x05) => {
                let items = pdu::parse_write_var(s7).unwrap_or_default();
                let mut store = store.lock().unwrap_or_else(PoisonError::into_inner);
                let codes: Vec<u8> = items
                    .iter()
                    .map(|(it, bytes)| match store.get_mut(&it.db) {
                        None => 0x0A,
                        Some(mem) => {
                            let end = it.start as usize + bytes.len();
                            if end > mem.len() {
                                0x05
                            } else {
                                mem[it.start as usize..end].copy_from_slice(bytes);
                                0xFF
                            }
                        }
                    })
                    .collect();
                pdu::write_var_ack(seq, &codes)
            }
            _ => pdu::ack_header(seq, 0, 0, 0x81, 0x04).to_vec(),
        };
        sock.write_all(&cotp::wrap_dt(&reply)).await?;
    }
}
