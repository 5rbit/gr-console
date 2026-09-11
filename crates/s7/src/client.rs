use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::cotp;
use crate::error::S7Error;
use crate::pdu::{self, Item};

#[derive(Clone, Debug)]
pub struct S7Config {
    pub host: String,
    pub port: u16,
    pub rack: u8,
    pub slot: u8,
    /// 1 = PG (default), 2 = OP, 3 = S7 basic.
    pub connection_type: u8,
    pub timeout: Duration,
    /// Requested PDU size (the PLC may negotiate down). 960 is the S7-1500 maximum.
    pub pdu_request: u16,
}

impl Default for S7Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 102,
            rack: 0,
            slot: 1,
            connection_type: 1,
            timeout: Duration::from_millis(3000),
            pdu_request: 960,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DbRead {
    pub db: u16,
    pub start: u32,
    pub len: usize,
}

#[derive(Clone, Debug)]
pub struct DbWrite {
    pub db: u16,
    pub start: u32,
    pub data: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeResult {
    /// The DB is exactly `expected` bytes long.
    Exact,
    /// The DB is longer than expected; `actual` is its size if it could be determined (< 64 KiB).
    Larger { actual: Option<u32> },
    /// The DB is shorter than expected; `actual` is the determined size.
    Smaller { actual: u32 },
    /// The DB does not exist (item code 0x0A).
    Missing,
}

pub struct S7Client {
    cfg: S7Config,
    stream: TcpStream,
    seq: u16,
    pdu: u16,
}

impl S7Client {
    pub async fn connect(cfg: &S7Config) -> Result<Self, S7Error> {
        let addr = format!("{}:{}", cfg.host, cfg.port);
        let stream = tokio::time::timeout(cfg.timeout, TcpStream::connect(&addr))
            .await
            .map_err(|_| S7Error::ConnectTimeout(addr.clone()))??;
        stream.set_nodelay(true)?;
        let mut c = Self { cfg: cfg.clone(), stream, seq: 1, pdu: 240 };
        c.send_raw(&cotp::connect_request(cfg.connection_type, cfg.rack, cfg.slot)).await?;
        let cc = c.recv_tpkt().await?;
        if cc.len() < 6 || cc[5] != 0xD0 {
            return Err(S7Error::Cotp(cc.get(5).copied().unwrap_or(0)));
        }
        let req = pdu::setup_comm(c.next_seq(), cfg.pdu_request);
        let ack = c.exchange(&req).await?;
        c.pdu = pdu::parse_setup_ack(&ack)?;
        tracing::debug!(host = %cfg.host, pdu = c.pdu, "S7 connected");
        Ok(c)
    }

    pub fn pdu_size(&self) -> u16 {
        self.pdu
    }

    pub fn config(&self) -> &S7Config {
        &self.cfg
    }

    /// Max payload bytes for one read item response.
    pub fn max_read_chunk(&self) -> usize {
        (self.pdu as usize).saturating_sub(18).max(1)
    }

    /// Max payload bytes for one write item request.
    pub fn max_write_chunk(&self) -> usize {
        (self.pdu as usize).saturating_sub(28).max(1)
    }

    /// Reads `len` bytes of DB `db` starting at byte `start`, chunking as needed.
    pub async fn read_db(&mut self, db: u16, start: u32, len: usize) -> Result<Vec<u8>, S7Error> {
        let mut out = Vec::with_capacity(len);
        let chunk = self.max_read_chunk();
        let mut done = 0usize;
        while done < len {
            let n = chunk.min(len - done);
            let item = Item { db, start: start + done as u32, len: n as u16 };
            let req = pdu::read_var(self.next_seq(), &[item]);
            let ack = self.exchange(&req).await?;
            let mut res = pdu::parse_read_ack(&ack, &[item])?;
            out.extend_from_slice(&res.remove(0)?);
            done += n;
        }
        Ok(out)
    }

    /// Reads several ranges. Small ranges are packed into multi-item requests (≤ 20 items per PDU);
    /// large ones are chunked individually. One result per input range, in order.
    pub async fn read_many(&mut self, reads: &[DbRead]) -> Result<Vec<Result<Vec<u8>, S7Error>>, S7Error> {
        let mut out: Vec<Option<Result<Vec<u8>, S7Error>>> = (0..reads.len()).map(|_| None).collect();
        let chunk = self.max_read_chunk();
        let mut batch: Vec<(usize, Item)> = Vec::new();
        let mut batch_bytes = 0usize;
        for (idx, r) in reads.iter().enumerate() {
            if r.len > chunk {
                out[idx] = Some(self.read_db(r.db, r.start, r.len).await);
                continue;
            }
            let item_cost = r.len + 4 + (r.len % 2);
            let projected = 14 + 12 * (batch.len() + 1) + batch_bytes + item_cost;
            if batch.len() >= 20 || projected > self.pdu as usize {
                self.flush_batch(&mut batch, &mut out).await?;
                batch_bytes = 0;
            }
            batch.push((idx, Item { db: r.db, start: r.start, len: r.len as u16 }));
            batch_bytes += item_cost;
        }
        self.flush_batch(&mut batch, &mut out).await?;
        Ok(out
            .into_iter()
            .map(|o| o.unwrap_or_else(|| Err(S7Error::Pdu("unfilled read slot".into()))))
            .collect())
    }

    async fn flush_batch(
        &mut self,
        batch: &mut Vec<(usize, Item)>,
        out: &mut [Option<Result<Vec<u8>, S7Error>>],
    ) -> Result<(), S7Error> {
        if batch.is_empty() {
            return Ok(());
        }
        let items: Vec<Item> = batch.iter().map(|(_, it)| *it).collect();
        let req = pdu::read_var(self.next_seq(), &items);
        let ack = self.exchange(&req).await?;
        let res = pdu::parse_read_ack(&ack, &items)?;
        for ((idx, _), r) in batch.drain(..).zip(res) {
            out[idx] = Some(r);
        }
        Ok(())
    }

    /// Writes bytes to DB `db` at byte `start`, chunking as needed. Fails on the first bad item.
    pub async fn write_db(&mut self, db: u16, start: u32, data: &[u8]) -> Result<(), S7Error> {
        let chunk = self.max_write_chunk();
        let mut done = 0usize;
        while done < data.len() {
            let n = chunk.min(data.len() - done);
            let item = Item { db, start: start + done as u32, len: n as u16 };
            let req = pdu::write_var(self.next_seq(), &[(item, &data[done..done + n])]);
            let ack = self.exchange(&req).await?;
            pdu::parse_write_ack(&ack, 1)?.remove(0)?;
            done += n;
        }
        Ok(())
    }

    pub async fn write_many(&mut self, writes: &[DbWrite]) -> Result<(), S7Error> {
        for w in writes {
            self.write_db(w.db, w.start, &w.data).await?;
        }
        Ok(())
    }

    /// Determines whether DB `db` is exactly `expected` bytes long using 1-byte reads:
    /// byte `expected-1` must be readable and byte `expected` must be out of range.
    /// On mismatch the actual size is located by binary search (≤ 17 extra reads).
    pub async fn probe_db_size(&mut self, db: u16, expected: u32) -> Result<ProbeResult, S7Error> {
        let last_ok = match self.read_byte(db, expected.saturating_sub(1)).await {
            Ok(()) => true,
            Err(S7Error::Item { code: 0x0A }) => return Ok(ProbeResult::Missing),
            Err(S7Error::Item { code: 0x05 }) => false,
            Err(e) => return Err(e),
        };
        let next_fails = match self.read_byte(db, expected).await {
            Ok(()) => false,
            Err(S7Error::Item { code: 0x05 }) => true,
            Err(e) => return Err(e),
        };
        if last_ok && next_fails {
            return Ok(ProbeResult::Exact);
        }
        let (mut lo, mut hi) = if last_ok { (expected + 1, 65536u32) } else { (0u32, expected.max(1)) };
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match self.read_byte(db, mid).await {
                Ok(()) => lo = mid + 1,
                Err(S7Error::Item { code: 0x05 }) => hi = mid,
                Err(e) => return Err(e),
            }
        }
        if last_ok {
            Ok(ProbeResult::Larger { actual: (lo < 65536).then_some(lo) })
        } else {
            Ok(ProbeResult::Smaller { actual: lo })
        }
    }

    async fn read_byte(&mut self, db: u16, at: u32) -> Result<(), S7Error> {
        let item = Item { db, start: at, len: 1 };
        let req = pdu::read_var(self.next_seq(), &[item]);
        let ack = self.exchange(&req).await?;
        pdu::parse_read_ack(&ack, &[item])?.remove(0).map(|_| ())
    }

    fn next_seq(&mut self) -> u16 {
        self.seq = self.seq.wrapping_add(1);
        if self.seq == 0 {
            self.seq = 1;
        }
        self.seq
    }

    async fn exchange(&mut self, s7: &[u8]) -> Result<Vec<u8>, S7Error> {
        let pkt = cotp::wrap_dt(s7);
        let want = pdu::seq_of(s7);
        self.send_raw(&pkt).await?;
        let reply = self.recv_tpkt().await?;
        let body = cotp::unwrap_dt(&reply).ok_or_else(|| S7Error::Pdu("bad COTP DT".into()))?;
        if let (Some(w), Some(g)) = (want, pdu::seq_of(body))
            && w != g
        {
            return Err(S7Error::Pdu(format!("sequence mismatch: sent {w}, got {g}")));
        }
        Ok(body.to_vec())
    }

    async fn send_raw(&mut self, pkt: &[u8]) -> Result<(), S7Error> {
        tokio::time::timeout(self.cfg.timeout, self.stream.write_all(pkt)).await.map_err(|_| S7Error::Timeout)??;
        Ok(())
    }

    async fn recv_tpkt(&mut self) -> Result<Vec<u8>, S7Error> {
        let fut = async {
            let mut head = [0u8; 4];
            self.stream.read_exact(&mut head).await?;
            let len = cotp::tpkt_len(&head).ok_or_else(|| S7Error::Pdu("bad TPKT".into()))?;
            let mut buf = vec![0u8; len];
            buf[..4].copy_from_slice(&head);
            self.stream.read_exact(&mut buf[4..]).await?;
            Ok::<_, S7Error>(buf)
        };
        tokio::time::timeout(self.cfg.timeout, fut).await.map_err(|_| S7Error::Timeout)?
    }
}
