//! 내장 번들(cargo feature `embed`) — 웹 빌드(`apps/gr-web/dist`)와 PLC 계약(`plc/contract`)을 실행 파일
//! 안에 넣는다. 그래서 배포는 **실행 파일 하나**다: 어디에 두고 어디서 실행하든 화면과 계약이 따라온다.
//!
//! 왜 feature인가: `include_dir!`은 컴파일 시점에 디렉터리가 있어야 한다. 프런트를 빌드하지 않은
//! 개발 체크아웃(`cargo run -- --demo`)이 그것 때문에 컴파일에 실패하면 안 된다 — 개발은 디스크의
//! `dist/`·`plc/contract`를 읽고, 패키지(`tools/package.*`)만 `--features embed`로 만든다.
//!
//! 계약은 **디스크로 풀어서** 읽는다(`extract_contract`): 계약 로더(`plc_layout::Contract::load_dir`)가
//! 디렉터리 워커라 내장 FS 변형을 따로 두는 것보다 4.8 MB를 매 시작마다 `data/contract/`에 쓰는
//! 편이 단순하고, 실행 파일이 바뀌면 계약도 같이 바뀌는 것이 자동으로 보장된다.

use axum::Router;

#[cfg(feature = "embed")]
mod inner {
    use axum::Router;
    use axum::body::Body;
    use axum::http::{HeaderValue, StatusCode, Uri, header};
    use axum::response::{IntoResponse, Response};
    use axum::routing::get;
    use include_dir::{Dir, include_dir};

    pub static WEB: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../gr-web/dist");
    pub static CONTRACT: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../../plc/contract");

    /// 내장 웹 파일 하나 — 없으면 `index.html`(SPA: 라우트는 브라우저가 푼다).
    async fn serve(uri: Uri) -> Response {
        let path = uri.path().trim_start_matches('/');
        let (file, is_index) = match WEB.get_file(path) {
            Some(f) if !path.is_empty() => (f, false),
            _ => match WEB.get_file("index.html") {
                Some(f) => (f, true),
                None => return StatusCode::NOT_FOUND.into_response(),
            },
        };
        let mime = mime_guess::from_path(file.path()).first_or_octet_stream();
        // 해시가 붙은 `assets/*`는 영원히 캐시해도 된다(이름이 곧 내용). index.html은 매번 확인.
        let cache = if is_index || !path.starts_with("assets/") { "no-cache" } else { "public, max-age=31536000, immutable" };
        let mut res = Response::new(Body::from(file.contents()));
        res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_str(mime.as_ref()).unwrap_or(HeaderValue::from_static("application/octet-stream")));
        res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
        res
    }

    pub fn web_router(router: Router) -> Router {
        router.fallback(get(serve))
    }
}

/// 내장 웹 빌드가 있으면 그것으로 SPA를 서빙하는 라우터(`Ok`), 없으면 라우터를 그대로 돌려준다(`Err`).
pub fn attach_web(router: Router) -> Result<Router, Router> {
    #[cfg(feature = "embed")]
    {
        if inner::WEB.get_file("index.html").is_some() {
            return Ok(inner::web_router(router));
        }
    }
    Err(router)
}

/// 내장 계약이 있으면 `to` 아래에 풀고 `true`. 매 시작마다 다시 쓴다(실행 파일과 계약이 어긋날 수 없게).
pub fn extract_contract(to: &std::path::Path) -> std::io::Result<bool> {
    #[cfg(feature = "embed")]
    {
        if inner::CONTRACT.dirs().next().is_some() {
            std::fs::create_dir_all(to)?;
            inner::CONTRACT.extract(to)?;
            return Ok(true);
        }
    }
    let _ = to;
    Ok(false)
}

/// 번들 유무 — 시작 로그·`/api/console/info`가 "이 실행 파일이 무엇을 들고 있나"를 말할 때 쓴다.
pub fn summary() -> &'static str {
    if cfg!(feature = "embed") { "embedded web + contract" } else { "none (disk)" }
}
