use plc_link::header::{FrameHeader, HEADER_LEN, MAGIC, VERSION};
use plc_link::{ErrCode, Format};

#[test]
fn encode_is_big_endian_and_round_trips() {
    let h = FrameHeader::new(Format::Bin, 10, 7, 0xDEAD_BEEF, 236);
    let b = h.encode();
    assert_eq!(b, [0x47, 0x53, 1, 2, 0, 10, 0, 7, 0xDE, 0xAD, 0xBE, 0xEF, 0, 0, 0, 236]);
    assert_eq!(FrameHeader::decode(&b).unwrap(), h);
    let j = FrameHeader::new(Format::Json, 0xFFFF, 65535, 0, 65535);
    assert_eq!(FrameHeader::decode(&j.encode()).unwrap(), j);
    assert_eq!((MAGIC, VERSION, HEADER_LEN), (0x4753, 1, 16));
}

#[test]
fn decode_errors() {
    let good = FrameHeader::new(Format::Json, 1, 1, 0, 0).encode();
    let mut bad_magic = good;
    bad_magic[1] = 0x54;
    assert_eq!(FrameHeader::decode(&bad_magic).unwrap_err().code, ErrCode::BadMagic);
    // magic is checked before the buffer is complete
    assert_eq!(FrameHeader::decode(b"GE").unwrap_err().code, ErrCode::BadMagic);
    let mut bad_version = good;
    bad_version[2] = 2;
    assert_eq!(FrameHeader::decode(&bad_version).unwrap_err().code, ErrCode::BadVersion);
    assert_eq!(FrameHeader::decode(&good[..15]).unwrap_err().code, ErrCode::BadLength);
    let mut bad_format = good;
    bad_format[3] = 3;
    assert_eq!(FrameHeader::decode(&bad_format).unwrap_err().code, ErrCode::FormatNotAllowed);
}

#[test]
fn err_codes_match_spec() {
    let expected = [
        (0, "OK"),
        (1, "BAD_MAGIC"),
        (2, "BAD_VERSION"),
        (3, "UNKNOWN_TYPE"),
        (4, "SIG_MISMATCH"),
        (5, "BAD_LENGTH"),
        (6, "PARSE"),
        (7, "FORMAT_NOT_ALLOWED"),
        (8, "DIR_NOT_ALLOWED"),
        (9, "BUSY"),
        (10, "NO_PENDING"),
    ];
    for (code, name) in expected {
        let c = ErrCode::from_code(code).unwrap();
        assert_eq!((c.code(), c.name()), (code, name));
    }
    assert_eq!(ErrCode::from_code(11), None);
}
