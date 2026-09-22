//! 포터블 백업 · 옛 폴더 이관.
//!
//! 콘솔 폴더 하나(`gr-console.exe` + `gr-console.toml` + `data/`)가 곧 설치본이다. 이 모듈은 그 폴더의
//! **옮겨 다녀야 하는 것 전부**를 한 폴더로 떠 두고(백업), 옛 폴더에서 새 폴더로 가져온다(이관).
//!
//! - 백업: `<base>/backup/gr-console-backup-<시각>/` 에 `gr-console.toml`, `data/gr-console.db`(`VACUUM INTO` —
//!   실행 중에도 WAL 까지 합친 일관 사본), `data/pki`, `data/records`, `data/traces`, `data/opcua-nodes*.json`.
//!   이 폴더의 내용을 새 콘솔 폴더에 그대로 복사하면 복원이다. 실행 중이면 `POST /api/admin/backup`.
//! - 이관(`--import-from <옛 폴더>`): 콘솔이 꺼져 있을 때만. 옛 폴더의 설정을 읽어 그 data 위치에서 DB 사본을 뜨고
//!   (옛 DB 는 건드리지 않는다 — 마이그레이션은 새 콘솔이 켜질 때 백업 뒤 한다) 나머지 파일을 복사한다.
//!
//! 계약(`data/contract`)과 로그는 옮기지 않는다 — 계약은 실행 파일이 매번 다시 풀고, 로그는 그 자리의 기록이다.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use axum::Router;
use axum::extract::State;
use axum::routing::post;
use serde_json::{Value as Json, json};

use crate::config::Config;
use crate::db::Db;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// 옮겨 다니는 data 아래 항목(파일 또는 폴더). DB 는 따로(`VACUUM INTO`).
const CARRY: [&str; 3] = ["pki", "records", "traces"];

static ORIGIN: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();

/// 시작할 때 한 번 — API 백업이 설정 파일과 기준 폴더를 알게.
pub fn init(config_path: &Path, base: &Path) {
    let _ = ORIGIN.set((config_path.to_path_buf(), base.to_path_buf()));
}

fn stamp() -> String {
    crate::util::now_str().chars().filter(char::is_ascii_digit).take(14).collect()
}

/// 폴더 통째 복사(없으면 건너뜀). 대상이 있으면 합친다(같은 이름은 덮어씀).
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<u64> {
    if !from.exists() {
        return Ok(0);
    }
    if from.is_file() {
        if let Some(p) = to.parent() {
            std::fs::create_dir_all(p)?;
        }
        return std::fs::copy(from, to);
    }
    std::fs::create_dir_all(to)?;
    let mut n = 0;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        n += copy_tree(&e.path(), &to.join(e.file_name()))?;
    }
    Ok(n)
}

/// data 아래의 노드 캐시(`opcua-nodes*.json`).
fn node_caches(data_dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(data_dir)
        .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("opcua-nodes") && n.ends_with(".json"))).collect())
        .unwrap_or_default()
}

/// 백업 한 벌 — 만든 폴더를 돌려준다.
pub fn snapshot(db: &Db, cfg: &Config, config_path: &Path, base: &Path) -> anyhow::Result<PathBuf> {
    let dest = base.join("backup").join(format!("gr-console-backup-{}", stamp()));
    if dest.exists() {
        anyhow::bail!("백업 폴더가 이미 있습니다: {}", dest.display());
    }
    let data = dest.join("data");
    std::fs::create_dir_all(&data)?;
    db.backup_into(&data.join("gr-console.db"))?;
    if config_path.is_file() {
        std::fs::copy(config_path, dest.join("gr-console.toml"))?;
    }
    for k in CARRY {
        copy_tree(&cfg.paths.data_dir.join(k), &data.join(k))?;
    }
    for p in node_caches(&cfg.paths.data_dir) {
        if let Some(n) = p.file_name() {
            std::fs::copy(&p, data.join(n))?;
        }
    }
    std::fs::write(
        dest.join("BACKUP.txt"),
        format!(
            "gr-console 백업\n시각: {}\n버전: {}\n원본 폴더: {}\n\n복원: 이 폴더의 gr-console.toml 과 data/ 를 콘솔 폴더(gr-console.exe 옆)에 복사한 뒤 실행한다.\n",
            crate::util::now_str(),
            env!("CARGO_PKG_VERSION"),
            base.display()
        ),
    )?;
    tracing::info!(to = %dest.display(), "backup written");
    Ok(dest)
}

/// 옛 콘솔 폴더에서 설정·데이터를 가져온다(이 콘솔이 꺼져 있을 때). `force` 면 이미 있는 DB 를 data/backup 으로 비키고 덮는다.
pub fn import_from(old_base: &Path, cfg: &Config, config_path: &Path, force: bool) -> anyhow::Result<()> {
    let old_cfg_path = old_base.join("gr-console.toml");
    let mut old = Config::load(&old_cfg_path)?;
    old.anchor(old_base);
    if !old.paths.sqlite.is_file() {
        anyhow::bail!("옛 폴더에 DB 가 없습니다: {}", old.paths.sqlite.display());
    }
    let new_db = &cfg.paths.sqlite;
    if new_db.exists() {
        if !force {
            anyhow::bail!("이 폴더에 이미 DB 가 있습니다: {} — 덮으려면 --force (기존 DB 는 data/backup 으로 옮겨 둔다)", new_db.display());
        }
        let aside = cfg.paths.data_dir.join("backup").join(format!("gr-console-{}-replaced.db", stamp()));
        std::fs::create_dir_all(aside.parent().unwrap_or(&cfg.paths.data_dir))?;
        // 켜져 있지 않으므로 WAL 은 종료 때 합쳐졌다 — 본 파일만 옮기고 -wal/-shm 은 지운다.
        std::fs::rename(new_db, &aside)?;
        for ext in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(PathBuf::from(format!("{}{ext}", new_db.display())));
        }
        println!("기존 DB → {}", aside.display());
    }
    std::fs::create_dir_all(&cfg.paths.data_dir)?;
    // 옛 DB 는 읽기 전용으로 열어 사본만 뜬다(마이그레이션은 새 콘솔이 켜질 때 백업 뒤).
    {
        let conn = rusqlite::Connection::open_with_flags(&old.paths.sqlite, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let p = new_db.to_string_lossy().replace('\'', "''");
        conn.execute_batch(&format!("VACUUM INTO '{p}'"))?;
    }
    if old_cfg_path.is_file() {
        if config_path.is_file() {
            let keep = config_path.with_extension(format!("toml.bak-{}", stamp()));
            std::fs::copy(config_path, &keep)?;
            println!("기존 설정 → {}", keep.display());
        }
        std::fs::copy(&old_cfg_path, config_path)?;
    }
    for k in CARRY {
        copy_tree(&old.paths.data_dir.join(k), &cfg.paths.data_dir.join(k))?;
    }
    for p in node_caches(&old.paths.data_dir) {
        if let Some(n) = p.file_name() {
            std::fs::copy(&p, cfg.paths.data_dir.join(n))?;
        }
    }
    println!("가져옴: {} → {}", old_base.display(), cfg.paths.data_dir.display());
    println!("콘솔을 켜면 DB 를 먼저 백업(data/backup)한 뒤 필요한 마이그레이션을 적용합니다.");
    Ok(())
}

/// `POST /api/admin/backup` — 실행 중인 콘솔의 백업(같은 PC 의 `--backup` 도 이것을 부른다).
async fn backup_now(State(st): State<AppState>) -> ApiResult<Json> {
    let (config_path, base) = ORIGIN.get().cloned().ok_or_else(|| ApiError::Internal("backup origin not set".into()))?;
    let db = st.db.clone();
    let cfg = st.cfg.clone();
    let dest = tokio::task::spawn_blocking(move || snapshot(&db, &cfg, &config_path, &base)).await.map_err(|e| ApiError::Internal(e.to_string()))?.map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(axum::Json(json!({ "path": dest.display().to_string() })))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/admin/backup", post(backup_now))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 백업 한 벌을 새 폴더로 이관하면 설정·DB·인증서·기록이 같이 온다.
    #[test]
    fn snapshot_then_import_carries_everything() {
        let root = std::env::temp_dir().join(format!("grc-port-{}", uuid::Uuid::new_v4()));
        let old = root.join("old");
        std::fs::create_dir_all(old.join("data/pki/own")).unwrap();
        std::fs::write(old.join("gr-console.toml"), "[server]\nbind = \"127.0.0.1:8099\"\n").unwrap();
        std::fs::write(old.join("data/pki/own/cert.der"), b"cert").unwrap();
        std::fs::create_dir_all(old.join("data/records/r1")).unwrap();
        std::fs::write(old.join("data/records/r1/meta.json"), b"{}").unwrap();
        std::fs::write(old.join("data/opcua-nodes.gr2.json"), b"{}").unwrap();
        let mut ocfg = Config::load(&old.join("gr-console.toml")).unwrap();
        ocfg.anchor(&old);
        let db = Db::open(&ocfg.paths.sqlite).unwrap();
        db.set_setting("k", "v").unwrap();

        let bk = snapshot(&db, &ocfg, &old.join("gr-console.toml"), &old).unwrap();
        assert!(bk.join("data/gr-console.db").is_file() && bk.join("data/pki/own/cert.der").is_file());
        assert!(bk.join("data/records/r1/meta.json").is_file() && bk.join("data/opcua-nodes.gr2.json").is_file());
        drop(db);

        // 새 폴더(설정 없음) 로 이관
        let new = root.join("new");
        std::fs::create_dir_all(&new).unwrap();
        let mut ncfg = Config::default();
        ncfg.anchor(&new);
        import_from(&old, &ncfg, &new.join("gr-console.toml"), false).unwrap();
        assert!(std::fs::read_to_string(new.join("gr-console.toml")).unwrap().contains("8099"));
        assert_eq!(Db::open(&ncfg.paths.sqlite).unwrap().setting("k").unwrap().as_deref(), Some("v"));
        assert!(new.join("data/pki/own/cert.der").is_file() && new.join("data/records/r1/meta.json").is_file());
        // 이미 DB 가 있으면 --force 없이는 거부
        assert!(import_from(&old, &ncfg, &new.join("gr-console.toml"), false).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
