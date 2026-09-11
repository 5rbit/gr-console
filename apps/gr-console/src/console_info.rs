use axum::Router;
use axum::routing::get;
use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct ConsoleInfo {
    pub app_id: &'static str,
    pub app_name: &'static str,
    pub tabs: Vec<&'static str>,
    pub default_tab: &'static str,
    pub live_view: &'static str,
    pub demo: bool,
    pub version: &'static str,
}

pub fn info(demo: bool) -> ConsoleInfo {
    ConsoleInfo {
        app_id: "gr",
        app_name: if demo { "GR 콘솔 (DEMO)" } else { "GR 콘솔" },
        tabs: vec!["task", "taskmgr", "measure", "scenario"],
        default_tab: "task",
        live_view: "gantry",
        demo,
        version: env!("CARGO_PKG_VERSION"),
    }
}

pub fn router(demo: bool) -> Router {
    let i = info(demo);
    Router::new().route(
        "/api/console/info",
        get(move || {
            let i = i.clone();
            async move { axum::Json(i) }
        }),
    )
}
