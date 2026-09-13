mod common;

use common::*;
use plc_link::framing::http::{CONTINUE_100, write_get};
use plc_link::framing::{
    ContentLength, FrameDecoder, Framing, HttpRequestDecoder, HttpResponseDecoder, NdjsonDecoder, RawFrame, Sniff, encode_frame, sniff, sniff_detail, status_text, write_request, write_response,
};
use plc_link::header::FrameHeader;
use plc_link::vectors::golden_bytes;
use plc_link::{Codec, ErrCode, Format, LinkError, Message, Role, Strictness};

trait Dec {
    fn push(&mut self, b: &[u8]);
    fn pull(&mut self) -> Result<Option<RawFrame>, LinkError>;
}

impl Dec for FrameDecoder {
    fn push(&mut self, b: &[u8]) {
        self.feed(b)
    }
    fn pull(&mut self) -> Result<Option<RawFrame>, LinkError> {
        self.next()
    }
}

impl Dec for NdjsonDecoder {
    fn push(&mut self, b: &[u8]) {
        self.feed(b)
    }
    fn pull(&mut self) -> Result<Option<RawFrame>, LinkError> {
        self.next()
    }
}

impl Dec for HttpRequestDecoder {
    fn push(&mut self, b: &[u8]) {
        self.feed(b)
    }
    fn pull(&mut self) -> Result<Option<RawFrame>, LinkError> {
        self.next()
    }
}

impl Dec for HttpResponseDecoder {
    fn push(&mut self, b: &[u8]) {
        self.feed(b)
    }
    fn pull(&mut self) -> Result<Option<RawFrame>, LinkError> {
        self.next()
    }
}

fn run(d: &mut dyn Dec, bytes: &[u8], sizes: &mut dyn FnMut() -> usize) -> Vec<RawFrame> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let n = sizes().clamp(1, bytes.len() - pos);
        d.push(&bytes[pos..pos + n]);
        pos += n;
        while let Some(f) = d.pull().unwrap() {
            out.push(f);
        }
    }
    out
}

struct Lcg(u64);

impl Lcg {
    fn size(&mut self, max: usize) -> usize {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as usize % max) + 1
    }
}

/// Whole buffer, 1-byte chunks and pseudo-random splits must give the same frames.
fn check_splits(make: &dyn Fn() -> Box<dyn Dec>, bytes: &[u8], count: usize) -> Vec<RawFrame> {
    let whole = run(&mut *make(), bytes, &mut || usize::MAX);
    assert_eq!(whole.len(), count);
    assert_eq!(run(&mut *make(), bytes, &mut || 1), whole, "1-byte chunks");
    for seed in 1..=25u64 {
        let mut l = Lcg(seed);
        assert_eq!(run(&mut *make(), bytes, &mut || l.size(23)), whole, "seed {seed}");
    }
    whole
}

fn messages(codec: &Codec) -> [Message; 3] {
    let fixture = Message { id: 40, seq: 7, payload: golden_bytes(codec.contract(), "LNK_Fixture").unwrap() };
    [codec.hello("GR2", 3, 1).unwrap(), fixture, codec.heartbeat(65535).unwrap()]
}

fn decode_all(codec: &Codec, frames: &[RawFrame]) -> Vec<Message> {
    frames.iter().map(|f| codec.decode_raw(f, Role::Pc, Strictness::Strict).unwrap()).collect()
}

#[test]
fn frame_stream_splits() {
    let codec = fixture_codec();
    let [hello, fixture, hb] = messages(&codec);
    let mut bytes = codec.to_frame(&hello, Format::Bin).unwrap();
    bytes.extend(codec.to_frame(&fixture, Format::Json).unwrap());
    bytes.extend(codec.to_frame(&hb, Format::Bin).unwrap());
    let frames = check_splits(&|| Box::new(FrameDecoder::new(65535)), &bytes, 3);
    assert_eq!(decode_all(&codec, &frames), vec![hello, fixture, hb]);
    assert_eq!((frames[0].format, frames[0].sig), (Format::Bin, Some(codec.registry().by_id(1).unwrap().sig)));
    assert_eq!((frames[1].format, frames[1].sig, frames[1].seq), (Format::Json, Some(0), Some(7)));
    assert_eq!((frames[2].sig, frames[2].payload.len()), (Some(0), 0));
}

#[test]
fn ndjson_stream_splits() {
    let codec = fixture_codec();
    let [hello, fixture, hb] = messages(&codec);
    let mut bytes = codec.to_ndjson(&hello).unwrap();
    bytes.extend_from_slice(b"\r\n\n  \n");
    bytes.extend_from_slice(codec.json_text(&fixture).unwrap().as_bytes());
    bytes.extend_from_slice(b"\r\n");
    bytes.extend(codec.to_ndjson(&hb).unwrap());
    let frames = check_splits(&|| Box::new(NdjsonDecoder::new(65535)), &bytes, 3);
    assert!(frames.iter().all(|f| f.framing == Framing::Ndjson && !f.payload.ends_with(b"\r")));
    assert_eq!(decode_all(&codec, &frames), vec![hello, fixture, hb]);
}

#[test]
fn http_pipelined_requests_and_responses() {
    let codec = fixture_codec();
    let plc_style = codec.clone().with_content_length(ContentLength::Pad5);
    let [hello, fixture, hb] = messages(&codec);
    let mut bytes = codec.to_http_request(&hello, Format::Json, "POST", "/api/plc/GR2/hello", "10.0.0.1:8090").unwrap();
    bytes.extend(plc_style.to_http_request(&fixture, Format::Bin, "POST", "/api/plc/GR2/fixture", "10.0.0.1:8090").unwrap());
    bytes.extend(write_get("/api/plc/GR2/command", "10.0.0.1:8090", Format::Bin));
    let frames = check_splits(&|| Box::new(HttpRequestDecoder::pc()), &bytes, 3);
    assert_eq!(decode_all(&codec, &frames[..2]), vec![hello, fixture.clone()]);
    let m1 = frames[1].http.as_ref().unwrap();
    assert_eq!((m1.method.as_deref(), m1.path.as_deref(), m1.header("content-length")), (Some("POST"), Some("/api/plc/GR2/fixture"), Some("00170")));
    assert!(!m1.close && !m1.expect_continue);
    let m2 = frames[2].http.as_ref().unwrap();
    assert_eq!((m2.method.as_deref(), frames[2].format, frames[2].payload.len()), (Some("GET"), Format::Bin, 0));

    let ack = codec.ack(40, 7, 0, "OK", 2).unwrap();
    let mut resp = CONTINUE_100.to_vec();
    resp.extend(codec.to_http_response(200, Some(&ack), Format::Json).unwrap());
    resp.extend(codec.to_http_response(204, None, Format::Json).unwrap());
    resp.extend(plc_style.to_http_response(200, Some(&hb), Format::Bin).unwrap());
    let frames = check_splits(&|| Box::new(HttpResponseDecoder::pc()), &resp, 4);
    let statuses: Vec<u16> = frames.iter().map(|f| f.http.as_ref().unwrap().status.unwrap()).collect();
    assert_eq!(statuses, vec![100, 200, 204, 200]);
    assert_eq!(codec.decode_raw(&frames[1], Role::Pc, Strictness::Strict).unwrap(), ack);
    assert_eq!(codec.decode_raw(&frames[3], Role::Pc, Strictness::Strict).unwrap(), hb);
}

#[test]
fn writers_produce_plc_header_shapes() {
    assert_eq!(
        write_request("POST", "/api/plc/GR2/hello", "192.168.0.10:8090", &[("Content-Type", "application/json")], Some(b"{}"), ContentLength::Pad5),
        b"POST /api/plc/GR2/hello HTTP/1.1\r\nHost: 192.168.0.10:8090\r\nContent-Type: application/json\r\nContent-Length: 00002\r\n\r\n{}".to_vec()
    );
    assert_eq!(write_get("/api/status", "192.168.0.1:80", Format::Json), b"GET /api/status HTTP/1.1\r\nHost: 192.168.0.1:80\r\nAccept: application/json\r\n\r\n".to_vec());
    assert_eq!(
        write_response(200, &[("Content-Type", "application/octet-stream"), ("X-GR-Type", "Ack")], Some(&[1, 2]), ContentLength::Pad5),
        b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nX-GR-Type: Ack\r\nContent-Length: 00002\r\n\r\n\x01\x02".to_vec()
    );
    assert_eq!(write_response(204, &[], None, ContentLength::Pad5), b"HTTP/1.1 204 No Content\r\n\r\n".to_vec());
    assert_eq!(write_response(404, &[], None, ContentLength::Plain), b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec());

    let codec = fixture_codec().with_content_length(ContentLength::Pad5);
    let [_, fixture, _] = messages(&codec);
    let sig = codec.registry().by_id(40).unwrap().sig;
    let req = codec.to_http_request(&fixture, Format::Bin, "POST", "/api/plc/GR2/fixture", "h:1").unwrap();
    let head = format!(
        "POST /api/plc/GR2/fixture HTTP/1.1\r\nHost: h:1\r\nContent-Type: application/octet-stream\r\nX-GR-Type: Fixture\r\nX-GR-Seq: 7\r\nX-GR-Sig: 0x{sig:08X}\r\nContent-Length: 00170\r\n\r\n"
    );
    assert_eq!(&req[..head.len()], head.as_bytes());
    assert_eq!(req.len(), head.len() + 170);

    for (code, text) in [
        (200, "OK"),
        (204, "No Content"),
        (400, "Bad Request"),
        (404, "Not Found"),
        (405, "Method Not Allowed"),
        (409, "Conflict"),
        (411, "Length Required"),
        (413, "Payload Too Large"),
        (431, "Request Header Fields Too Large"),
        (501, "Not Implemented"),
        (503, "Service Unavailable"),
    ] {
        assert_eq!(status_text(code), text);
    }
}

#[test]
fn frame_negative_cases() {
    let mut d = FrameDecoder::new(65535);
    d.feed(b"GET / HTTP/1.1\r\n");
    assert_eq!(d.next().unwrap_err().code, ErrCode::BadMagic);
    assert_eq!(d.next().unwrap_err().code, ErrCode::BadMagic, "sticky");

    let mut h = FrameHeader::new(Format::Json, 1, 1, 0, 2).encode();
    h[2] = 9;
    let mut d = FrameDecoder::new(65535);
    d.feed(&h[..3]);
    assert_eq!(d.next().unwrap_err().code, ErrCode::BadVersion);

    let mut d = FrameDecoder::new(65535);
    d.feed(&FrameHeader::new(Format::Bin, 40, 1, 0, 65536).encode());
    assert_eq!(d.next().unwrap_err().code, ErrCode::BadLength);
    let mut plc = FrameDecoder::new(8176);
    plc.feed(&FrameHeader::new(Format::Bin, 40, 1, 0, 8177).encode());
    assert_eq!(plc.next().unwrap_err().code, ErrCode::BadLength);
    let mut plc = FrameDecoder::new(8176);
    plc.feed(&FrameHeader::new(Format::Bin, 40, 1, 0, 8176).encode());
    assert_eq!(plc.next().unwrap(), None);

    let frame = encode_frame(Format::Json, 2, 1, 0, br#"{"type":"Heartbeat","seq":1,"sig":0,"data":null}"#);
    let mut d = FrameDecoder::new(65535);
    d.feed(&frame[..20]);
    assert_eq!(d.next().unwrap(), None);
    assert_eq!(d.finish().unwrap_err().code, ErrCode::BadLength, "truncated");
    d.feed(&frame[20..]);
    assert!(d.next().unwrap().is_some());
    assert!(d.finish().is_ok());
}

#[test]
fn ndjson_negative_cases() {
    let mut d = NdjsonDecoder::new(10);
    d.feed(br#"{"a":1234}"#);
    d.feed(b"\r");
    assert_eq!(d.next().unwrap(), None, "10 bytes + pending CR is still within the limit");
    d.feed(b"\n");
    assert_eq!(d.next().unwrap().unwrap().payload, br#"{"a":1234}"#.to_vec());
    d.feed(br#"{"a":12345"#);
    assert_eq!(d.next().unwrap(), None, "10 bytes without terminator still fit");
    d.feed(b"6");
    assert_eq!(d.next().unwrap_err().code, ErrCode::BadLength);
    assert_eq!(d.next().unwrap_err().code, ErrCode::BadLength, "sticky");

    let mut d = NdjsonDecoder::new(10);
    d.feed(b"{\"a\":123456}\n");
    assert_eq!(d.next().unwrap_err().code, ErrCode::BadLength);

    let mut d = NdjsonDecoder::new(100);
    d.feed(b"{\"type\":2");
    assert_eq!(d.next().unwrap(), None);
    assert_eq!(d.finish().unwrap_err().code, ErrCode::BadLength);

    // BIN cannot travel over NDJSON
    let codec = fixture_codec();
    let [_, fixture, _] = messages(&codec);
    let raw = RawFrame { framing: Framing::Ndjson, format: Format::Bin, msg_type: Some(40), seq: Some(7), sig: Some(codec.registry().by_id(40).unwrap().sig), payload: fixture.payload, http: None };
    assert_eq!(codec.decode_raw(&raw, Role::Pc, Strictness::Lenient).unwrap_err().code, ErrCode::FormatNotAllowed);
}

fn req_err(dec: HttpRequestDecoder, text: &[u8]) -> LinkError {
    let mut d = dec;
    d.feed(text);
    let e = d.next().unwrap_err();
    assert_eq!(d.next().unwrap_err(), e, "sticky");
    e
}

#[test]
fn http_negative_cases() {
    let e = req_err(HttpRequestDecoder::pc(), b"POST /api/command HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n");
    assert_eq!(e.http_status, Some(501));
    let e = req_err(HttpRequestDecoder::pc(), b"POST /api/command HTTP/1.1\r\nHost: a\r\n\r\n{}");
    assert_eq!((e.http_status, e.code), (Some(411), ErrCode::BadLength));
    let mut big = b"GET /api/status HTTP/1.1\r\nX-Pad: ".to_vec();
    big.extend(std::iter::repeat_n(b'a', 1100));
    assert_eq!(req_err(HttpRequestDecoder::plc(), &big).http_status, Some(431), "incomplete header section over the limit");
    big.extend_from_slice(b"\r\n\r\n");
    assert_eq!(req_err(HttpRequestDecoder::plc(), &big).http_status, Some(431));
    let mut ok = HttpRequestDecoder::pc();
    ok.feed(&big);
    assert!(ok.next().unwrap().is_some(), "the same header fits the PC limit");
    assert_eq!(req_err(HttpRequestDecoder::pc(), b"POST / HTTP/1.1\r\nContent-Length: 70000\r\n\r\n").http_status, Some(413));
    assert_eq!(req_err(HttpRequestDecoder::pc(), b"POST / HTTP/1.1\r\nContent-Length: 12a\r\n\r\n").http_status, Some(400));
    assert_eq!(req_err(HttpRequestDecoder::pc(), b"POST / HTTP/1.1\r\nContent-Length: 2\r\nContent-Length: 3\r\n\r\n").http_status, Some(400));
    assert_eq!(req_err(HttpRequestDecoder::pc(), b"POST / HTTP/1.1\r\nContent-Length: 0\r\nX-GR-Sig: 0xZZ\r\n\r\n").http_status, Some(400));
    assert_eq!(req_err(HttpRequestDecoder::pc(), b"HELLO\r\n\r\n").http_status, Some(400));
    assert_eq!(req_err(HttpRequestDecoder::pc(), b"GET / HTTP/2\r\n\r\n").http_status, Some(400));

    // PLC-shaped 204 carries `Content-Length: 00000`; PC-shaped 204 omits it; both are empty
    let mut r = HttpResponseDecoder::plc();
    r.feed(b"HTTP/1.1 204 No Content\r\nContent-Length: 00000\r\n\r\nHTTP/1.1 204 No Content\r\n\r\nHTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 00002\r\n\r\n{}");
    for (status, body) in [(204, &b""[..]), (204, &b""[..]), (200, &b"{}"[..])] {
        let f = r.next().unwrap().unwrap();
        assert_eq!((f.http.unwrap().status, f.payload.as_slice()), (Some(status), body));
    }
    assert_eq!(r.next().unwrap(), None);
    let mut r = HttpResponseDecoder::pc();
    r.feed(b"HTTP/1.1 204 No Content\r\nContent-Length: 00005\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
    assert_eq!(r.next().unwrap().unwrap().payload.len(), 0, "204 never reads a body");
    assert_eq!(r.next().unwrap().unwrap().http.unwrap().status, Some(200));
    let mut r = HttpResponseDecoder::pc();
    r.feed(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{}");
    assert_eq!(r.next().unwrap_err().http_status, Some(411));
}

#[test]
fn http_expect_close_and_leading_zeros() {
    let mut d = HttpRequestDecoder::pc();
    d.feed(b"POST /api/command HTTP/1.1\r\nHost: a\r\nexpect: 100-Continue\r\ncontent-length: 00002\r\nX-GR-Seq: 00007\r\nX-GR-Sig: 0x0000ABCD\r\n\r\n");
    assert_eq!(d.next().unwrap(), None);
    assert!(d.take_continue());
    assert!(!d.take_continue(), "only once");
    d.feed(b"{}GET /api/status HTTP/1.0\r\n\r\n");
    let f = d.next().unwrap().unwrap();
    let m = f.http.as_ref().unwrap();
    assert!(m.expect_continue && !m.close);
    assert_eq!((f.payload.as_slice(), f.seq, f.sig), (&b"{}"[..], Some(7), Some(0xABCD)));
    let g = d.next().unwrap().unwrap();
    assert!(g.http.unwrap().close, "HTTP/1.0 closes by default");
    assert!(!d.take_continue());

    let mut d = HttpRequestDecoder::pc();
    d.feed(b"POST /x HTTP/1.1\nConnection: Keep-Alive, close\nContent-Length: 2\n\n[]");
    let f = d.next().unwrap().unwrap();
    assert!(f.http.unwrap().close);
    assert_eq!(f.payload, b"[]");
}

#[test]
fn sniffing() {
    let frame = FrameHeader::new(Format::Bin, 1, 1, 0, 0).encode();
    assert_eq!(sniff(&frame), Some(Framing::Frame));
    assert_eq!(sniff(b"{\"type\""), Some(Framing::Ndjson));
    assert_eq!(sniff(b"GET /api/status"), Some(Framing::Http));
    assert_eq!(sniff(b"POST /api/plc"), Some(Framing::Http));
    assert_eq!(sniff(b"HTTP/1.1 200 OK"), Some(Framing::Http));
    assert_eq!(sniff_detail(b"G"), Sniff::NeedMore);
    assert_eq!(sniff_detail(b"GE"), Sniff::NeedMore);
    assert_eq!(sniff_detail(b""), Sniff::NeedMore);
    assert_eq!(sniff_detail(b"XYZ"), Sniff::Unknown);
    assert_eq!(sniff(b"GS"), Some(Framing::Frame));
    assert_eq!(sniff(b"PUT / HTTP/1.1"), None);
}
