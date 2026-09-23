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
        .route("/api/events", get(events))
        .merge(crate::plc::routes::router())
        .merge(crate::backup::router())
        .merge(crate::measure::routes::router())
        .merge(crate::laser::router())
        .merge(crate::para::router())
        .merge(crate::record::router())
        .merge(crate::trace::routes::router())
        .merge(crate::registry::routes::router())
        .merge(crate::registry::dims::router())
        .merge(crate::ledger::routes::router())
        .merge(crate::scenario::routes::router())
        .merge(crate::stock::routes::router())
        .merge(crate::taskgen::routes::router())
        .merge(crate::pallet::routes::router())
        .with_state(st)
}
