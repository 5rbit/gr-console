//! TPKT (RFC 1006) + COTP (ISO 8073 class 0) framing.

/// COTP connection request. Source TSAP is fixed 0x0100; destination TSAP is
/// `(connection_type, rack<<5 | slot)` where connection_type 1 = PG, 2 = OP, 3 = S7 basic.
pub fn connect_request(connection_type: u8, rack: u8, slot: u8) -> Vec<u8> {
    let dst_lo = (rack << 5) | (slot & 0x1F);
    vec![
        0x03,
        0x00,
        0x00,
        0x16, // TPKT, length 22
        0x11,
        0xE0,
        0x00,
        0x00,
        0x00,
        0x01,
        0x00, // COTP CR
        0xC0,
        0x01,
        0x0A, // TPDU size 1024
        0xC1,
        0x02,
        0x01,
        0x00, // src TSAP
        0xC2,
        0x02,
        connection_type,
        dst_lo, // dst TSAP
    ]
}

/// COTP connection confirm (used by the fake server).
pub fn connect_confirm() -> Vec<u8> {
    vec![0x03, 0x00, 0x00, 0x16, 0x11, 0xD0, 0x00, 0x01, 0x00, 0x01, 0x00, 0xC0, 0x01, 0x0A, 0xC1, 0x02, 0x01, 0x00, 0xC2, 0x02, 0x01, 0x01]
}

/// Wraps an S7 PDU into TPKT + COTP DT.
pub fn wrap_dt(s7: &[u8]) -> Vec<u8> {
    let total = 7 + s7.len();
    let mut pkt = Vec::with_capacity(total);
    pkt.extend_from_slice(&[0x03, 0x00, (total >> 8) as u8, total as u8, 0x02, 0xF0, 0x80]);
    pkt.extend_from_slice(s7);
    pkt
}

/// Length of the TPKT packet given its 4-byte header, or None if malformed.
pub fn tpkt_len(head: &[u8; 4]) -> Option<usize> {
    if head[0] != 0x03 {
        return None;
    }
    let len = ((head[2] as usize) << 8) | head[3] as usize;
    (len >= 4).then_some(len)
}

/// Strips TPKT + COTP DT header (7 bytes) and returns the S7 payload slice.
pub fn unwrap_dt(pkt: &[u8]) -> Option<&[u8]> {
    if pkt.len() < 7 || pkt[4] != 0x02 || pkt[5] != 0xF0 {
        return None;
    }
    Some(&pkt[7..])
}
