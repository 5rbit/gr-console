//! Named saved filters of the Events tab (`/api/events/filters`). The query is the page's own URL form, so a saved
//! filter and a copied link reopen the same view.

use axum::Router;
use axum::extract::{Path, State};
use axum::routing::{get, put};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use super::routes::log;
use super::store::Store;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

const MAX_QUERY: usize = 4000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SavedFilter {
    pub id: i64,
    pub name: String,
    pub query: String,
    pub updated_at: String,
}

fn of(r: &rusqlite::Row) -> rusqlite::Result<SavedFilter> {
    Ok(SavedFilter { id: r.get(0)?, name: r.get(1)?, query: r.get(2)?, updated_at: r.get(3)? })
}

pub fn all(store: &Store) -> rusqlite::Result<Vec<SavedFilter>> {
    store.db().with(|c| {
        let mut st = c.prepare("SELECT id, name, query, updated_at FROM saved_filters ORDER BY name COLLATE NOCASE")?;
        let it = st.query_map([], of)?;
        it.collect()
    })
}

/// By name: saving under an existing name replaces its query. `id` renames that filter instead.
pub fn save(store: &Store, id: Option<i64>, name: &str, query: &str) -> rusqlite::Result<Option<SavedFilter>> {
    let now = crate::util::now_str();
    store.db().with(|c| {
        let id = match id {
            Some(id) => {
                if c.execute("UPDATE saved_filters SET name = ?2, query = ?3, updated_at = ?4 WHERE id = ?1", rusqlite::params![id, name, query, now])? == 0 {
                    return Ok(None);
                }
                id
            }
            None => c.query_row(
                "INSERT INTO saved_filters (name, query, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(name) DO UPDATE SET query = excluded.query, updated_at = excluded.updated_at RETURNING id",
                rusqlite::params![name, query, now],
                |r| r.get(0),
            )?,
        };
        c.query_row("SELECT id, name, query, updated_at FROM saved_filters WHERE id = ?1", [id], of).map(Some)
    })
}

pub fn delete(store: &Store, id: i64) -> rusqlite::Result<bool> {
    store.db().with(|c| Ok(c.execute("DELETE FROM saved_filters WHERE id = ?1", [id])? > 0))
}

#[derive(Deserialize)]
struct Body {
    name: String,
    query: String,
}

fn check(b: &Body) -> Result<(String, String), ApiError> {
    let name = b.name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(ApiError::BadRequest("name 은 1~60 자입니다".into()));
    }
    let query = b.query.trim().trim_start_matches('?');
    if query.len() > MAX_QUERY {
        return Err(ApiError::BadRequest(format!("query 가 {MAX_QUERY} 자를 넘습니다")));
    }
    Ok((name.to_string(), query.to_string()))
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, ApiError> + Send + 'static) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| ApiError::Internal(e.to_string()))?
}

async fn list_get(State(st): State<AppState>) -> ApiResult<Vec<SavedFilter>> {
    let log = log(&st)?.clone();
    Ok(axum::Json(blocking(move || Ok(all(&log.store)?)).await?))
}

async fn create(State(st): State<AppState>, axum::Json(b): axum::Json<Body>) -> ApiResult<SavedFilter> {
    let log = log(&st)?.clone();
    let (name, query) = check(&b)?;
    let f = blocking(move || save(&log.store, None, &name, &query)?.ok_or_else(|| ApiError::Internal("saved filter".into()))).await?;
    Ok(axum::Json(f))
}

async fn update(State(st): State<AppState>, Path(id): Path<i64>, axum::Json(b): axum::Json<Body>) -> ApiResult<SavedFilter> {
    let log = log(&st)?.clone();
    let (name, query) = check(&b)?;
    let f = blocking(move || match save(&log.store, Some(id), &name, &query) {
        Ok(Some(f)) => Ok(f),
        Ok(None) => Err(ApiError::NotFound(format!("filter {id}"))),
        Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::ConstraintViolation => Err(ApiError::Conflict(format!("같은 이름의 필터가 있습니다: {name}"))),
        Err(e) => Err(e.into()),
    })
    .await?;
    Ok(axum::Json(f))
}

async fn remove(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json> {
    let log = log(&st)?.clone();
    if !blocking(move || Ok(delete(&log.store, id)?)).await? {
        return Err(ApiError::NotFound(format!("filter {id}")));
    }
    Ok(axum::Json(json!({ "deleted": id })))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/events/filters", get(list_get).post(create)).route("/api/events/filters/{id}", put(update).delete(remove))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_by_name_replaces_and_renames_by_id() {
        let s = Store::memory();
        let a = save(&s, None, "GR2 오류", "e.plc=GR2&e.lvl=4").unwrap().unwrap();
        let again = save(&s, None, "GR2 오류", "e.plc=GR2&e.lvl=3").unwrap().unwrap();
        assert_eq!((again.id, again.query.as_str()), (a.id, "e.plc=GR2&e.lvl=3"), "same name = same filter");
        let b = save(&s, None, "alarms", "e.view=alarms").unwrap().unwrap();
        assert_eq!(all(&s).unwrap().iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["alarms", "GR2 오류"]);
        assert!(save(&s, Some(b.id), "GR2 오류", "x").is_err(), "renaming onto another name conflicts");
        assert_eq!(save(&s, Some(b.id), "알람", "e.view=alarms").unwrap().unwrap().name, "알람");
        assert!(save(&s, Some(999), "x", "y").unwrap().is_none());
        assert!(delete(&s, a.id).unwrap());
        assert_eq!(all(&s).unwrap().len(), 1);
        assert!(check(&Body { name: " ".into(), query: String::new() }).is_err());
        assert_eq!(check(&Body { name: " a ".into(), query: "?e.view=steps".into() }).unwrap(), ("a".to_string(), "e.view=steps".to_string()));
    }
}
