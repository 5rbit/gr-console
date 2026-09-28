//! 여러 스트림을 **한 연결**로 — `GET /api/stream?robots=1,2`.
//!
//! 브라우저는 한 호스트에 HTTP/1.1 연결을 6 개까지만 연다. 화면마다 SSE 를 따로 열면(상태 · 로봇별 상태 ·
//! Task · 재고 · 시나리오 · 스테이션 · 알림) 그 수를 넘겨서, 새 조회(GET)가 실패도 하지 않고 영원히 줄을
//! 선다 — 화면이 "읽는 중…" 에 멈춘다. 그래서 이름 붙은 이벤트를 하나의 연결로 모아 보낸다.
//!
//! 이벤트 이름은 낱개 스트림과 **같다**(`status` · `tasks` · `stock` · `run` · `stations` · `alert` ·
//! `alert_ack`). 로봇별 상태만 이름이 `status:<robot>` 이다 — 한 연결에 여러 로봇이 섞이기 때문이다.
//! 화면은 이 연결이 없으면(옛 콘솔) 낱개 스트림으로 돌아간다.

use std::convert::Infallible;
use std::time::Duration;

use axum::Router;
use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use futures::stream::{Stream, StreamExt};
use serde::Deserialize;
use serde::Serialize;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

use crate::state::AppState;

#[derive(Deserialize, Default)]
struct MuxQuery {
    /// 상태를 받을 로봇 id 목록(`1,2`). 비면 설정된 로봇 전부.
    robots: Option<String>,
}

/// broadcast 하나 → 이름 붙은 SSE 이벤트 스트림(첫 값 포함).
fn named<T: Serialize + Clone + Send + 'static>(rx: tokio::sync::broadcast::Receiver<T>, name: String, first: Option<T>) -> impl Stream<Item = Result<Event, Infallible>> + Send {
    let n1 = name.clone();
    let head = futures::stream::iter(first.into_iter().map(move |v| Ok(Event::default().event(n1.clone()).json_data(v).unwrap_or_default())));
    let tail = BroadcastStream::new(rx).map(move |r| match r {
        Ok(v) => Ok(Event::default().event(name.clone()).json_data(v).unwrap_or_default()),
        Err(BroadcastStreamRecvError::Lagged(n)) => Ok(Event::default().event("lag").data(n.to_string())),
    });
    head.chain(tail)
}

async fn stream(State(st): State<AppState>, Query(q): Query<MuxQuery>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let want: Vec<u8> = match q.robots.as_deref() {
        Some(s) => s.split(',').filter_map(|x| x.trim().parse().ok()).collect(),
        None => st.robots.iter().map(|r| r.id).collect(),
    };
    type Part = std::pin::Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;
    let mut parts: Vec<Part> = Vec::new();
    for r in st.robots.iter().filter(|r| want.contains(&r.id)) {
        parts.push(Box::pin(named(r.status.tx.subscribe(), format!("status:{}", r.id), r.status.latest())));
    }
    parts.push(Box::pin(named(st.task_events.subscribe(), "tasks".into(), Some(crate::ledger::LedgerEvent::Snapshot { tasks: st.robots.iter().flat_map(|r| r.ledger.list()).collect() }))));
    parts.push(Box::pin(named(st.stock.events.subscribe(), "stock".into(), st.stock.list().ok().map(|stock| crate::stock::StockEvent::Snapshot { stock }))));
    parts.push(Box::pin(named(st.scenario.events.subscribe(), "run".into(), Some(st.scenario.current()))));
    parts.push(Box::pin(named(crate::issue::station_live::subscribe(), "stations".into(), Some(crate::issue::station_live::snapshot()))));
    Sse::new(futures::stream::select_all(parts)).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("ping"))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/stream", get(stream))
}
