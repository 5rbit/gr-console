//! Static frontend serving: `GR_CONSOLE_WEB_DIR` / config `paths.web_dir` → `apps/gr-web/dist` next to the
//! workspace → built-in fallback page.

use std::path::{Path, PathBuf};

use axum::Router;
use axum::response::Html;
use axum::routing::get;
use tower_http::services::{ServeDir, ServeFile};

pub fn attach(router: Router, web_dir: Option<&Path>) -> (Router, String) {
    // 명시한 web_dir > 내장 번들 > 개발 체크아웃의 dist. 배포 실행 파일은 옆에 낡은 dist가 있어도
    // 자기 번들을 쓴다(실행 파일과 화면이 어긋날 수 없게).
    if let Some(dir) = web_dir
        && dir.join("index.html").is_file()
    {
        let svc = ServeDir::new(dir).not_found_service(ServeFile::new(dir.join("index.html")));
        return (router.fallback_service(svc), dir.display().to_string());
    }
    let router = match crate::bundle::attach_web(router) {
        Ok(r) => return (r, "bundle".into()),
        Err(r) => r,
    };
    let candidates = [PathBuf::from("apps/gr-web/dist"), PathBuf::from("../gr-web/dist")];
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
