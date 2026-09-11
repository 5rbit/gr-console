//! S7comm PDU building / parsing (job + ack-data; functions 0xF0 setup, 0x04 read var, 0x05 write var).

use crate::error::S7Error;

pub const AREA_DB: u8 = 0x84;
/// Item-spec transport size BYTE.
pub const TRANSPORT_BYTE: u8 = 0x02;
/// Data-section transport size for byte/word/dword payloads (length expressed in bits).
pub const DATA_TRANSPORT_BIT_LEN: u8 = 0x04;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    pub db: u16,
    pub start: u32,
    pub len: u16,
}

pub fn job_header(seq: u16, param_len: u16, data_len: u16) -> [u8; 10] {
    [
        0x32,
        0x01,
        0x00,
        0x00,
        (seq >> 8) as u8,
        seq as u8,
        (param_len >> 8) as u8,
        param_len as u8,
        (data_len >> 8) as u8,
        data_len as u8,
    ]
}

/// Ack-data header (12 bytes incl. error class/code).
pub fn ack_header(seq: u16, param_len: u16, data_len: u16, class: u8, code: u8) -> [u8; 12] {
    [
        0x32,
        0x03,
        0x00,
        0x00,
        (seq >> 8) as u8,
        seq as u8,
        (param_len >> 8) as u8,
        param_len as u8,
        (data_len >> 8) as u8,
        data_len as u8,
        class,
        code,
    ]
}

fn item_spec(item: &Item, out: &mut Vec<u8>) {
    let bit_addr = item.start * 8;
    out.extend_from_slice(&[
        0x12,
        0x0A,
        0x10,
        TRANSPORT_BYTE,
        (item.len >> 8) as u8,
        item.len as u8,
        (item.db >> 8) as u8,
        item.db as u8,
        AREA_DB,
        (bit_addr >> 16) as u8,
        (bit_addr >> 8) as u8,
        bit_addr as u8,
    ]);
}

/// Parses one 12-byte item spec (used by the fake server).
pub fn parse_item_spec(b: &[u8]) -> Option<Item> {
    if b.len() < 12 || b[0] != 0x12 || b[1] != 0x0A || b[2] != 0x10 {
        return None;
    }
    let count = ((b[4] as u16) << 8) | b[5] as u16;
    let db = ((b[6] as u16) << 8) | b[7] as u16;
    let bit_addr = ((b[9] as u32) << 16) | ((b[10] as u32) << 8) | b[11] as u32;
    let len = match b[3] {
        0x01 => count.div_ceil(8), // BIT transport: unsupported beyond single bits, approximate
        0x02 | 0x04 => count,      // BYTE / WORD? (WORD would be count*2, not used here)
        _ => count,
    };
    Some(Item { db, start: bit_addr / 8, len })
}

/// Setup communication job: requests `pdu` bytes PDU, 1 parallel job each way.
pub fn setup_comm(seq: u16, pdu: u16) -> Vec<u8> {
    let mut v = job_header(seq, 8, 0).to_vec();
    v.extend_from_slice(&[0xF0, 0x00, 0x00, 0x01, 0x00, 0x01, (pdu >> 8) as u8, pdu as u8]);
    v
}

pub fn setup_comm_ack(seq: u16, pdu: u16) -> Vec<u8> {
    let mut v = ack_header(seq, 8, 0, 0, 0).to_vec();
    v.extend_from_slice(&[0xF0, 0x00, 0x00, 0x01, 0x00, 0x01, (pdu >> 8) as u8, pdu as u8]);
    v
}

/// Parses the setup-communication ack and returns the negotiated PDU size.
pub fn parse_setup_ack(s7: &[u8]) -> Result<u16, S7Error> {
    check_ack_header(s7)?;
    if s7.len() < 20 {
        return Err(S7Error::Pdu("setup ack too short".into()));
    }
    let pdu = ((s7[18] as u16) << 8) | s7[19] as u16;
    if pdu < 100 {
        return Err(S7Error::Pdu(format!("negotiated PDU {pdu} too small")));
    }
    Ok(pdu)
}

pub fn read_var(seq: u16, items: &[Item]) -> Vec<u8> {
    let param_len = 2 + 12 * items.len();
    let mut v = job_header(seq, param_len as u16, 0).to_vec();
    v.push(0x04);
    v.push(items.len() as u8);
    for it in items {
        item_spec(it, &mut v);
    }
    v
}

/// Parses a read-var ack. Returns one result per requested item (in order).
pub fn parse_read_ack(s7: &[u8], expected: &[Item]) -> Result<Vec<Result<Vec<u8>, S7Error>>, S7Error> {
    check_ack_header(s7)?;
    if s7.len() < 14 || s7[12] != 0x04 {
        return Err(S7Error::Pdu("read ack: bad function".into()));
    }
    let count = s7[13] as usize;
    if count != expected.len() {
        return Err(S7Error::Pdu(format!("read ack: item count {count} != {}", expected.len())));
    }
    let mut pos = 14;
    let mut out = Vec::with_capacity(count);
    for (i, it) in expected.iter().enumerate() {
        if pos + 4 > s7.len() {
            return Err(S7Error::Pdu("read ack: truncated item".into()));
        }
        let ret = s7[pos];
        let transport = s7[pos + 1];
        let mut len = ((s7[pos + 2] as usize) << 8) | s7[pos + 3] as usize;
        pos += 4;
        if ret != 0xFF {
            out.push(Err(S7Error::Item { code: ret }));
            continue; // failed items carry no data
        }
        if transport == 0x04 || transport == 0x03 {
            len /= 8; // length given in bits
        }
        if len != it.len as usize {
            return Err(S7Error::Pdu(format!("read ack: item {i} length {len} != {}", it.len)));
        }
        if pos + len > s7.len() {
            return Err(S7Error::Pdu("read ack: truncated data".into()));
        }
        out.push(Ok(s7[pos..pos + len].to_vec()));
        pos += len;
        if len % 2 == 1 && i + 1 < count {
            pos += 1; // pad byte between items
        }
    }
    Ok(out)
}

/// Builds a read-var ack (fake server). `results`: Ok(bytes) or Err(item code).
pub fn read_var_ack(seq: u16, results: &[Result<Vec<u8>, u8>]) -> Vec<u8> {
    let mut data = Vec::new();
    for (i, r) in results.iter().enumerate() {
        match r {
            Ok(bytes) => {
                let bits = (bytes.len() * 8) as u16;
                data.extend_from_slice(&[0xFF, DATA_TRANSPORT_BIT_LEN, (bits >> 8) as u8, bits as u8]);
                data.extend_from_slice(bytes);
                if bytes.len() % 2 == 1 && i + 1 < results.len() {
                    data.push(0);
                }
            }
            Err(code) => data.extend_from_slice(&[*code, 0x00, 0x00, 0x00]),
        }
    }
    let mut v = ack_header(seq, 2, data.len() as u16, 0, 0).to_vec();
    v.push(0x04);
    v.push(results.len() as u8);
    v.extend_from_slice(&data);
    v
}

pub fn write_var(seq: u16, items: &[(Item, &[u8])]) -> Vec<u8> {
    let param_len = 2 + 12 * items.len();
    let mut data = Vec::new();
    for (i, (it, bytes)) in items.iter().enumerate() {
        debug_assert_eq!(bytes.len(), it.len as usize);
        let bits = (bytes.len() * 8) as u16;
        data.extend_from_slice(&[0x00, DATA_TRANSPORT_BIT_LEN, (bits >> 8) as u8, bits as u8]);
        data.extend_from_slice(bytes);
        if bytes.len() % 2 == 1 && i + 1 < items.len() {
            data.push(0x00);
        }
    }
    let mut v = job_header(seq, param_len as u16, data.len() as u16).to_vec();
    v.push(0x05);
    v.push(items.len() as u8);
    for (it, _) in items {
        item_spec(it, &mut v);
    }
    v.extend_from_slice(&data);
    v
}

/// Parses a write-var job (fake server): returns items with their payloads.
pub fn parse_write_var(s7: &[u8]) -> Option<Vec<(Item, Vec<u8>)>> {
    if s7.len() < 12 || s7[0] != 0x32 || s7[1] != 0x01 || s7[10] != 0x05 {
        return None;
    }
    let count = s7[11] as usize;
    let mut pos = 12;
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        items.push(parse_item_spec(s7.get(pos..pos + 12)?)?);
        pos += 12;
    }
    let mut out = Vec::with_capacity(count);
    for (i, it) in items.into_iter().enumerate() {
        let hdr = s7.get(pos..pos + 4)?;
        let mut len = ((hdr[2] as usize) << 8) | hdr[3] as usize;
        if hdr[1] == 0x04 || hdr[1] == 0x03 {
            len /= 8;
        }
        pos += 4;
        let bytes = s7.get(pos..pos + len)?.to_vec();
        pos += len;
        if len % 2 == 1 && i + 1 < count {
            pos += 1;
        }
        out.push((it, bytes));
    }
    Some(out)
}

/// Parses a read-var job (fake server).
pub fn parse_read_var(s7: &[u8]) -> Option<Vec<Item>> {
    if s7.len() < 12 || s7[0] != 0x32 || s7[1] != 0x01 || s7[10] != 0x04 {
        return None;
    }
    let count = s7[11] as usize;
    (0..count).map(|i| parse_item_spec(s7.get(12 + 12 * i..24 + 12 * i)?)).collect()
}

pub fn write_var_ack(seq: u16, codes: &[u8]) -> Vec<u8> {
    let mut v = ack_header(seq, 2, codes.len() as u16, 0, 0).to_vec();
    v.push(0x05);
    v.push(codes.len() as u8);
    v.extend_from_slice(codes);
    v
}

/// Parses a write-var ack. Returns one result per item.
pub fn parse_write_ack(s7: &[u8], count: usize) -> Result<Vec<Result<(), S7Error>>, S7Error> {
    check_ack_header(s7)?;
    if s7.len() < 14 + count || s7[12] != 0x05 {
        return Err(S7Error::Pdu("write ack: bad function / short".into()));
    }
    if s7[13] as usize != count {
        return Err(S7Error::Pdu(format!("write ack: item count {} != {count}", s7[13])));
    }
    Ok((0..count)
        .map(|i| match s7[14 + i] {
            0xFF => Ok(()),
            c => Err(S7Error::Item { code: c }),
        })
        .collect())
}

fn check_ack_header(s7: &[u8]) -> Result<(), S7Error> {
    if s7.len() < 12 || s7[0] != 0x32 {
        return Err(S7Error::Pdu("not an S7 PDU".into()));
    }
    if s7[1] != 0x03 {
        return Err(S7Error::Pdu(format!("unexpected PDU type 0x{:02X}", s7[1])));
    }
    if s7[10] != 0 || s7[11] != 0 {
        return Err(S7Error::S7 { class: s7[10], code: s7[11] });
    }
    Ok(())
}

pub fn seq_of(s7: &[u8]) -> Option<u16> {
    (s7.len() >= 6).then(|| ((s7[4] as u16) << 8) | s7[5] as u16)
}

pub fn function_of(s7: &[u8]) -> Option<u8> {
    let param_start = if s7.get(1) == Some(&0x03) { 12 } else { 10 };
    s7.get(param_start).copied()
}
