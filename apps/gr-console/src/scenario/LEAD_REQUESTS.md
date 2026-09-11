# Scenario slice (M3-C) — requests for the lead

Things outside `scenario/**` / `components/scenario/**` / `lib/scenario/**` that would help, with the
workaround currently in place. None of these block the slice.

1. **`ledger::Target` — derive `PartialEq` (and ideally `Eq`)** (`apps/gr-console/src/ledger/mod.rs`).
   `scenario::Step` embeds `Option<Target>`; without it `Step` cannot derive `PartialEq`.
   *Workaround:* manual `impl PartialEq for Step` in `scenario/mod.rs` — delete it once `Target` derives.

2. **`api.ts` scenario helpers** (`apps/gr-web/src/lib/api.ts`), optional consolidation:
   - `scenarioExportUrl(id, format)` accepts `'json' | 'xlsx'`; the backend serves `json | csv` (no xlsx).
   - `scenarioRun(id, { repeat })` — the backend also takes `start_step`.
   - Missing: `POST /api/scenarios/{id}/validate` (JSON body = draft; `_` as id validates an unsaved draft),
     `GET /api/scenarios/runs?limit=` (history).
   *Workaround:* slice-local `src/lib/scenario/api.ts` built on the exported `getJson/postJson/postForm`.

3. **`types.ts`** (`apps/gr-web/src/lib/types.ts`), optional: `ScenarioRun` gained a transient `note: string | null`
   (why the submission gate is closed) and `StepResult` gained `error: string | null` + `attempt: number`.
   *Workaround:* `ScenarioRunView` / `StepResultView` in `src/lib/scenario/model.ts` widen the shared types.

4. **Shared `target/debug` collisions.** Another agent's build (from a different checkout) overwrote
   `target/debug/gr-console.exe` with a binary that did not contain this slice, which cost a debugging round.
   This slice now builds/runs with `CARGO_TARGET_DIR=target/agent-c` (agent-a does the same). Suggest each agent
   pins its own `CARGO_TARGET_DIR` in the task brief.

5. **Not mine, seen while verifying** (no action taken):
   - `cargo test -p gr-console`: `ledger::sync::tests::{remove_and_stats_and_filters, external_tasks_are_created_from_arrays_and_rings}` fail.
   - `cargo clippy -D warnings` fails in `crates/plc-layout/src/parse.rs` (unnecessary_unwrap, manual_pattern_char_comparison)
     before reaching `gr-console`; with `--no-deps` the remaining findings are in `cmd/`, `config.rs`, `demo.rs`,
     `db/`, `ledger/`, `measure/`, `plc/`, `registry/`, `state.rs`, `util.rs` — none in `scenario/`.
   - `npm run check`: `src/lib/task/compose.ts` (TS2352) and `src/components/task/ComposeCard.tsx` (TS6133) — slice A.
