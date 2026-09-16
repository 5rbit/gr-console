//! Writers with injectable faults (simulator). The stream decoder and the encoders live in
//! `plc_link::io::stream` and are re-exported here.

use std::time::Duration;

use tokio::io::{AsyncWrite, AsyncWriteExt};

pub use plc_link::io::{StreamDecoder, encode, ref_of};

/// Simulator write faults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// Every message written in 1–3 byte pieces with a short pause.
    SplitWrites,
    /// Messages collected and written together.
    Coalesce,
    /// BIN signature (FRAME header / X-GR-Sig / envelope sig) wrong on MeasLog.
    BadSig,
    /// One frame with a bad magic per first connection.
    BadMagic,
}

impl std::str::FromStr for Fault {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "split-writes" => Ok(Fault::SplitWrites),
            "coalesce" => Ok(Fault::Coalesce),
            "bad-sig" => Ok(Fault::BadSig),
            "bad-magic" => Ok(Fault::BadMagic),
            _ => Err(format!("unknown fault {s:?} (split-writes|coalesce|bad-sig|bad-magic)")),
        }
    }
}

/// Writes `bytes`, split into small pieces when `split`.
pub async fn write_bytes<W: AsyncWrite + Unpin>(w: &mut W, bytes: &[u8], split: bool) -> std::io::Result<()> {
    if !split {
        return w.write_all(bytes).await;
    }
    let mut i = 0;
    let mut k = 0usize;
    while i < bytes.len() {
        let n = (1 + k % 3).min(bytes.len() - i);
        w.write_all(&bytes[i..i + n]).await?;
        w.flush().await?;
        i += n;
        k += 1;
        // flush per piece splits the TCP segments; pause rarely (a Windows sleep lasts ~15 ms)
        if k.is_multiple_of(512) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        } else if k.is_multiple_of(16) {
            tokio::task::yield_now().await;
        }
    }
    Ok(())
}
