//! Static frontend serving: `GR_CONSOLE_WEB_DIR` / config `paths.web_dir` → `apps/gr-web/dist` next to the
//! workspace → built-in fallback page.

use std::path::{Path, PathBuf};

use axum::Router;
use axum::response::Html;
use axum::routing::get;
use tower_http::services::{ServeDir, ServeFile};

pub fn attach(router: Router, web_dir: Option<&Path>) -> (Router, String) {
    let candidates: Vec<PathBuf> = web_dir
        .map(|p| vec![p.to_path_buf()])
        .unwrap_or_default()
        .into_iter()
        .chain([PathBuf::from("apps/gr-web/dist"), PathBuf::from("../gr-web/dist")])
        .collect();
    for dir in candidates {
        if dir.join("index.html").is_file() {
            let index = dir.join("index.html");
            let svc = ServeDir::new(&dir).not_found_service(ServeFile::new(index));
            return (router.fallback_service(svc), dir.display().to_string());
        }
    }
    (router.route("/", get(fallback)), "builtin".into())
}

async fn fallback() -> Html<&'static str> {
    Html(
        r#"<!doctype html><meta charset="utf-8"><title>gr-console</title>
<body style="font:14px system-ui;padding:24px"><h2>gr-console backend</h2>
<p>프론트 빌드가 없습니다. <code>cd apps/gr-web &amp;&amp; npm run build</code> 후 다시 열거나 <code>npm run dev</code>(5173) 를 쓰세요.</p>
<ul><li><a href="/api/console/info">/api/console/info</a></li><li><a href="/api/plcs">/api/plcs</a></li><li><a href="/api/status">/api/status</a></li><li><a href="/api/health">/api/health</a></li></ul></body>"#,
    )
}
