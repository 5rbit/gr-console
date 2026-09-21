//! 로그 출력 — **런타임 스레드는 절대 콘솔 쓰기에서 멈추지 않는다.**
//!
//! 왜: 2026-09-21 15:10 쯤 8090 콘솔이 CPU 0 %·정적 파일까지 무응답으로 굳었다. `tracing_subscriber::fmt()` 기본
//! 출력은 이벤트를 낸 스레드가 stdout 에 **동기로** 쓴다. Windows 콘솔 창에서 누가 클릭하면(빠른 편집 "선택" 모드)
//! 콘솔 쓰기가 시간 제한 없이 막히고, 첫 워커가 stdout 잠금을 쥔 채 멈추면 로그를 내는 나머지 워커도 전부 그
//! 잠금에서 선다 — 웹·PLC 폴링·OPC UA keep-alive 가 모두 서고, GRM 이 세션을 끊어 소켓만 CLOSE_WAIT 로 남았다.
//!
//! 그래서: 이벤트는 한 줄씩 제한된 채널에 **넣기만** 한다(가득 차면 버리고 센다). 전용 스레드가 꺼내 stdout 과
//! (설정을 읽은 뒤 열리는) `data/logs/gr-console-YYYY-MM-DD.log` 에 쓴다. 콘솔이 막혀도 그 스레드만 선다.
//! (빠른 편집을 코드로 끄려면 Win32 호출이 필요한데 워크스페이스가 `unsafe_code = "forbid"` 라 넣지 않았다 — 창이
//! 선택 모드에 들어가도 서는 것은 이 쓰기 스레드뿐이고, 파일 로그는 이어서 쌓인다.)

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

/// 채널에 쌓아 둘 수 있는 줄 수 — 콘솔이 막힌 동안 이만큼은 파일로 이어서 쓴다.
const QUEUE_LINES: usize = 8192;

enum Msg {
    Line(Vec<u8>),
    Flush(SyncSender<()>),
}

struct Shared {
    tx: SyncSender<Msg>,
    dropped: AtomicU64,
    /// 파일 출력 디렉터리 — `open_file_dir` 가 설정 뒤에 정한다(없으면 stdout 만).
    dir: Mutex<Option<PathBuf>>,
}

static SINK: OnceLock<Arc<Shared>> = OnceLock::new();

/// `tracing_subscriber::fmt().with_writer(logsink::make_writer())` 에 넘긴다. 처음 부를 때 쓰기 스레드를 띄운다.
pub fn make_writer() -> MakeLine {
    let shared = SINK
        .get_or_init(|| {
            let (tx, rx) = sync_channel(QUEUE_LINES);
            let shared = Arc::new(Shared { tx, dropped: AtomicU64::new(0), dir: Mutex::new(None) });
            let s2 = shared.clone();
            // 쓰기 실패(콘솔 없음 등)는 무시한다 — 로그 때문에 콘솔이 죽지 않게.
            let _ = std::thread::Builder::new().name("log-writer".into()).spawn(move || writer_loop(rx, s2));
            shared
        })
        .clone();
    MakeLine { shared }
}

/// 설정을 읽은 뒤 부른다 — 이후 줄은 `<dir>/gr-console-YYYY-MM-DD.log` 에도 쌓인다(날짜가 바뀌면 새 파일).
pub fn open_file_dir(dir: &Path) {
    if let Some(s) = SINK.get() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            tracing::warn!(dir = %dir.display(), "log dir: {e}");
            return;
        }
        *s.dir.lock().unwrap_or_else(PoisonError::into_inner) = Some(dir.to_path_buf());
    }
}

/// 서식 없는 안내 한 줄(시작 주소·종료 안내) — `println!` 대신. 런타임 스레드에서 콘솔을 직접 쓰지 않는다.
pub fn print(line: &str) {
    let mut w = make_writer_line();
    let _ = w.write_all(line.as_bytes());
    let _ = w.write_all(b"\n");
}

fn make_writer_line() -> LineWriter {
    LineWriter { buf: Vec::new(), shared: make_writer().shared }
}

/// 버퍼에 남은 줄을 내보낸다(종료 직전). 최대 `timeout` 기다리고, 콘솔이 막혀 있으면 그냥 돌아온다.
pub fn flush(timeout: std::time::Duration) {
    if let Some(s) = SINK.get() {
        let (tx, rx) = sync_channel(1);
        if s.tx.try_send(Msg::Flush(tx)).is_ok() {
            let _ = rx.recv_timeout(timeout);
        }
    }
}

fn writer_loop(rx: Receiver<Msg>, shared: Arc<Shared>) {
    let mut file: Option<(String, File)> = None;
    let mut reported = 0u64;
    while let Ok(msg) = rx.recv() {
        let line = match msg {
            Msg::Line(l) => l,
            Msg::Flush(done) => {
                let _ = std::io::stdout().flush();
                if let Some((_, f)) = file.as_mut() {
                    let _ = f.flush();
                }
                let _ = done.try_send(());
                continue;
            }
        };
        let dropped = shared.dropped.load(Ordering::Relaxed);
        let note = (dropped > reported).then(|| {
            let n = dropped - reported;
            reported = dropped;
            format!("[log] 출력이 밀려 {n} 줄을 버렸습니다\n").into_bytes()
        });
        let dir = shared.dir.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if let Some(dir) = dir {
            let day = chrono_day();
            if file.as_ref().is_none_or(|(d, _)| *d != day) {
                let path = dir.join(format!("gr-console-{day}.log"));
                file = OpenOptions::new().create(true).append(true).open(&path).ok().map(|f| (day, f));
            }
        }
        // 파일 먼저 — 콘솔이 막혀도 파일에는 막히기 직전까지 남는다.
        if let Some((_, f)) = file.as_mut() {
            if let Some(n) = &note {
                let _ = f.write_all(n);
            }
            let _ = f.write_all(&strip_ansi(&line));
        }
        let mut out = std::io::stdout().lock();
        if let Some(n) = &note {
            let _ = out.write_all(n);
        }
        let _ = out.write_all(&line);
    }
}

/// 콘솔 색(ANSI `ESC [ … 문자`)을 뺀다 — 파일은 메모장·grep 으로 읽는다.
fn strip_ansi(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == 0x1b && b.get(i + 1) == Some(&b'[') {
            i += 2;
            while i < b.len() && !(0x40..=0x7e).contains(&b[i]) {
                i += 1;
            }
            i += 1;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// 로컬 날짜 `YYYY-MM-DD` — 파일 이름용.
fn chrono_day() -> String {
    crate::util::now_str().get(..10).unwrap_or("unknown").to_string()
}

/// 이벤트 하나 = 버퍼 하나. 떨어질 때(drop) 채널에 넣는다 — 가득 차면 버리고 센다(기다리지 않는다).
pub struct LineWriter {
    buf: Vec<u8>,
    shared: Arc<Shared>,
}

impl Write for LineWriter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for LineWriter {
    fn drop(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        match self.shared.tx.try_send(Msg::Line(std::mem::take(&mut self.buf))) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.shared.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

#[derive(Clone)]
pub struct MakeLine {
    shared: Arc<Shared>,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for MakeLine {
    type Writer = LineWriter;
    fn make_writer(&'a self) -> Self::Writer {
        LineWriter { buf: Vec::with_capacity(256), shared: self.shared.clone() }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    #[test]
    fn strips_console_colors_for_the_file() {
        assert_eq!(super::strip_ansi(b"\x1b[2m2026\x1b[0m \x1b[32m INFO\x1b[0m x"), b"2026  INFO x");
    }

    #[test]
    fn full_queue_drops_instead_of_blocking() {
        // 받는 쪽이 없는(막힌) 채널: 가득 차면 쓰는 쪽은 기다리지 않고 센다.
        let (tx, _rx) = std::sync::mpsc::sync_channel(2);
        let shared = std::sync::Arc::new(super::Shared { tx, dropped: Default::default(), dir: Default::default() });
        for _ in 0..5 {
            let mut w = super::LineWriter { buf: Vec::new(), shared: shared.clone() };
            w.write_all(b"x\n").unwrap();
        }
        assert_eq!(shared.dropped.load(std::sync::atomic::Ordering::Relaxed), 3);
    }
}
