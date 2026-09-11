# M3-A (task-issue + registry) → lead requests

Nothing outside `registry/**`, `issue/**`, `components/task/**`, `lib/task/**`, `lib/gr/rejectCodes.ts` was touched.
No `PlcHandle` / `state.rs` change was needed — `read`/`write`/`snap`/`layout`/`contract.encode_path` were enough.

## Routes registered from `registry/routes.rs` (no `routes.rs` edit)
- `POST /api/issue/compose` → `issue::Composed { task, params, warnings }` (preview; same `compose()` the ledger/scenario call)
- `GET /api/registry/diff?plc=` → `{ plc, cells: DiffRow[], stations: DiffRow[] }` (plan §API)
- `POST /api/cells|stations/push?plc=GR2|GRM|both|gr2_s7[&force=1]`, `GET …/diff?plc=`, `GET …/export.xlsx`, `POST …/import-file?dry_run=1|0`,
  `GET /api/registry/export.xlsx`, `POST /api/registry/import-file`

## Shared-contract suggestions (`src/lib/api.ts` / `src/lib/types.ts`) — merge at your discretion
- `api.cellsPush/stationsPush` are typed `ImportResult` and take `PlcId`; the backend returns
  `PushResult { plc, db, written_bytes, count, verified, mismatch_at?, writes, results? }` and accepts `force`.
  The slice uses `src/lib/task/api.ts` (`taskApi`) + `src/lib/task/types.ts` (`PushResult`, `ComposePreview`, `FileImportResult`, `PlcTarget`).
  If you want them in the shared files, they lift verbatim.
- `?plc=` accepts `gr2_s7`/`grm_s7` (suffix stripped) so the existing `api.cellsImport(PlcId)` keeps working.
- `Cell` / `Station` JSON carries `plc_seen_at: string | null` (already emitted by the baseline views) — not in `types.ts`.
- `ImportResult.errors[]` rows carry an extra `sheet` field; file imports add `dry_run` and `counts {cells, stations, items}`.

## Things only the lead can fix
- `cargo clippy --workspace -- -D warnings` fails before reaching `gr-console`: `crates/plc-layout/src/parse.rs:62` (unwrap after `is_some`)
  and `:194` (manual char comparison). `gr-console` registry/issue files are clippy-clean (checked without `-D`, filtered).
- No `rustfmt.toml`; default `rustfmt --check` reflows nearly every existing file (e.g. `cmd/mod.rs`). If `just verify` runs
  `cargo fmt --check`, add `rustfmt.toml` (e.g. `max_width = 200`, `use_small_heuristics = "Max"`) — the new files follow the repo's long-line style.
- `.gitignore` has `/data/*.db*` — a per-agent data dir (`data/agent-a/gr-console.db`) is not covered. Suggest `/data/` or `/data/**/*.db*`.
- Shared `target/`: a running demo server holds `target/debug/gr-console.exe`, so a parallel `cargo build` fails with `os error 5`.
  I built/tested with `--target-dir target/agent-a` (ignored by `/target`). Consider documenting per-agent target dirs.
- Demo world pins GR2 `OPCUA.STAT.Mode.Auto = true`, so PLC push in `--demo` always needs `force=1` (the UI's confirm dialog has the checkbox).
  If you want a no-force demo path, expose a mode toggle in `DemoWorld`.

## Assumptions to confirm on real hardware (M4)
- GRM `STATION`: only `Station[i].Para` (LGR_Station_Para inside LGR_STATION) is rewritten per element + `Count` last (33 writes for 32 slots);
  assumes GRM's `Findindex_*` also scans up to `STATION.Count`.
- GR2 `CELL`/`STATION` and GRM `CELL` are written as one array range + `Count` (needs Phase-0 `S7_Optimized_Access := 'FALSE'` on GR2 STATION / GRM CELL).
- Read-back verification compares only the written byte ranges (other GRM STATION bytes are live runtime state).
