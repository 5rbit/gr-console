//! Tokio transport of the PLC socket link (cargo feature `io`): everything that owns a `TcpStream`, so an
//! application only has to wire the channels of [`LinkSession`] to its own state.
//!
//! * [`stream`]  session framing over a byte stream ([`StreamDecoder`], [`encode`], [`ref_of`])
//! * [`session`] [`LinkSession`]: framing sniff, Hello exchange, heartbeat, automatic Ack, channels

pub mod session;
pub mod stream;

pub use session::{FrameInfo, LinkSession, PC_NAME, SendError, SessionCommand, SessionEnd, SessionEvent, SessionParams, SniffFailure, hello_formats, sniff_framing};
pub use stream::{StreamDecoder, encode, ref_of, wire_len};
