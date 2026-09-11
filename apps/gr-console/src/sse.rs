//! broadcast channel → Server-Sent Events. A lagging client receives an `event: lag` frame.

use std::convert::Infallible;
use std::time::Duration;

use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::Stream;
use serde::Serialize;
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

pub fn broadcast_sse<T: Serialize + Clone + Send + 'static>(rx: broadcast::Receiver<T>, event_name: &'static str, first: Option<T>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let head = futures::stream::iter(first.into_iter().map(move |v| Ok(Event::default().event(event_name).json_data(v).unwrap_or_default())));
    let tail = BroadcastStream::new(rx).map(move |r| match r {
        Ok(v) => Ok(Event::default().event(event_name).json_data(v).unwrap_or_default()),
        Err(BroadcastStreamRecvError::Lagged(n)) => Ok(Event::default().event("lag").data(n.to_string())),
    });
    Sse::new(head.chain(tail)).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("ping"))
}
