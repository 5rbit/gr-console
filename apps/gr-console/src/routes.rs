use axum::Router;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;

use crate::sse::broadcast_sse;
use crate::state::AppState;

async fn events(State(st): State<AppState>) -> impl IntoResponse {
    broadcast_sse(st.events.subscribe(), "event", None)
}

pub fn router(st: AppState) -> Router {
    Router::new()
        // 콘솔 내부 알림 스트림 — `/api/events` 는 이벤트 로그(`evtlog`)가 쓴다.
        .route("/api/console/events", get(events))
        .merge(crate::evtlog::routes::router())
        .merge(crate::plc::routes::router())
        .merge(crate::backup::router())
        .merge(crate::measure::routes::router())
        .merge(crate::laser::router())
        .merge(crate::gripper::router())
        .merge(crate::para::router())
        .merge(crate::record::router())
        .merge(crate::trace::routes::router())
        .merge(crate::registry::routes::router())
        .merge(crate::registry::dims::router())
        .merge(crate::ledger::routes::router())
        .merge(crate::jobs::routes::router())
        .merge(crate::jobs::templates::router())
        .merge(crate::scenario::routes::router())
        .merge(crate::stock::routes::router())
        .merge(crate::taskgen::routes::router())
        .merge(crate::taskgen::requests::router())
        .merge(crate::mux::router())
        .merge(crate::pallet::routes::router())
        .with_state(st)
}
