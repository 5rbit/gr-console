// GR 콘솔 PLC 입출력 벤치 — OPC UA 명령 경로, Task 입력, Cell/Station 입출력을 콘솔 API 로 검증한다.
//
// 사용:
//   gr-console --demo-opcua                      (가짜 GRM OPC UA 서버 + 가짜 S7 PLC, 계약 레이아웃에서 생성)
//   node tools/bench/plc-io-bench.mjs [--base http://127.0.0.1:8090] [--out bench-report.json]
//
// 실기에 붙은 콘솔이면 셀/스테이션 쓰기와 태스크 제출이 실제 PLC 를 바꾸므로 기본으로 거부한다(--real 로 해제).
// 검증 항목
//   A. OPC UA: 세션 준비, 탐색한 노드 수 = GRM 계약 레이아웃의 GR[n].CMD 멤버 수, 경로 일치
//   B. Task: PICK/DROP(셀·스테이션)/MOVE/MEASURE 제출 → 에코·수락 → GR2 RES.Task 가 원장 plc_task 와 전 필드 일치
//      → GRM CMD 헤더 소거 → 완료. 거부 코드(미등록 셀), 대기 중 취소, 강제 완료.
//   C. Cell/Station: PLC 읽기 = 레지스트리, AUTO 중 쓰기 거부, 강제 쓰기 후 바이트 검증, S7 재읽기 일치(GR2·GRM),
//      diff 없음, Excel 내보내기 → 가져오기 미리보기 라운드트립.

const args = process.argv.slice(2)
const arg = (name, dflt) => {
  const i = args.indexOf(name)
  return i >= 0 && args[i + 1] ? args[i + 1] : dflt
}
const BASE = arg('--base', 'http://127.0.0.1:8090').replace(/\/$/, '')
const OUT = arg('--out', '')
const REAL = args.includes('--real')

const results = []
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const list = (x) => (Array.isArray(x) ? x : (x?.items ?? x?.rows ?? []))

async function call(method, path, body) {
  const init = { method, headers: {} }
  if (body instanceof FormData) init.body = body
  else if (body !== undefined) {
    init.headers['content-type'] = 'application/json'
    init.body = JSON.stringify(body)
  }
  const r = await fetch(BASE + path, init)
  const text = await r.text()
  let json
  try {
    json = text ? JSON.parse(text) : null
  } catch {
    json = text
  }
  return { status: r.status, body: json }
}
const get = (p) => call('GET', p)
const post = (p, b) => call('POST', p, b)

async function waitFor(fn, ms, every = 200) {
  const end = Date.now() + ms
  let last
  while (Date.now() < end) {
    last = await fn()
    if (last) return last
    await sleep(every)
  }
  return last
}

function check(group, name, ok, detail = '') {
  results.push({ group, name, ok: !!ok, detail: typeof detail === 'string' ? detail : JSON.stringify(detail) })
  console.log(`${ok ? '  ✔' : '  ✘'} ${group} ${name}${detail ? ' — ' + (typeof detail === 'string' ? detail : JSON.stringify(detail)) : ''}`)
  return !!ok
}

/** 두 JSON 의 차이(숫자는 1e-3 허용) — 경로 목록. */
function diffJson(a, b, path = '', out = []) {
  if (typeof a === 'number' && typeof b === 'number') {
    if (Math.abs(a - b) > 1e-3) out.push(`${path}: ${a} ≠ ${b}`)
  } else if (Array.isArray(a) && Array.isArray(b)) {
    if (a.length !== b.length) out.push(`${path}: len ${a.length} ≠ ${b.length}`)
    a.forEach((v, i) => diffJson(v, b[i], `${path}[${i}]`, out))
  } else if (a && b && typeof a === 'object' && typeof b === 'object') {
    for (const k of new Set([...Object.keys(a), ...Object.keys(b)])) diffJson(a[k], b[k], path ? `${path}.${k}` : k, out)
  } else if (a !== b) out.push(`${path}: ${JSON.stringify(a)} ≠ ${JSON.stringify(b)}`)
  return out
}

const db = (plc, name, path) => get(`/api/plc/${plc}/db/${name}${path ? `?path=${encodeURIComponent(path)}` : ''}`)

// ────────────────────────────────────────────────────────────────────────────
async function main() {
  const info = (await get('/api/console/info')).body
  console.log(`bench → ${BASE} (${info?.app_name ?? '?'}, demo=${info?.demo})`)
  if (!info?.demo && !REAL) {
    console.error('실기 콘솔로 보입니다. 이 벤치는 PLC 테이블을 쓰고 태스크를 제출합니다. 정말 실행하려면 --real 을 붙이세요.')
    process.exit(2)
  }
  const robots = list((await get('/api/robots')).body)
  const root = robots[0]?.opcua_root ?? 'GR[2].CMD'

  // ── A. OPC UA ──────────────────────────────────────────────────────────────
  console.log('\nA. OPC UA 명령 경로')
  const st = await waitFor(async () => {
    const s = (await get('/api/opcua/state')).body
    return s?.ready ? s : null
  }, 30000, 500)
  check('A1', 'OPC UA 세션 준비', st?.kind === 'opcua' && st?.ready, st ? `${st.endpoint} · ${st.detail}` : 'not ready')
  const nodes = (await get('/api/opcua/nodes')).body
  const layout = (await get('/api/plc/GRM/layout/OPCUA')).body
  const leaves = (layout?.members ?? []).filter((m) => m.path.startsWith(root + '.') && typeof m.prim === 'string')
  check('A2', `탐색 노드 수 = 계약 레이아웃 ${root} 멤버 수`, nodes?.count === leaves.length, `${nodes?.count} / ${leaves.length}`)
  const paths = new Set(leaves.map((m) => m.path.slice(root.length + 1)))
  const sample = (nodes?.sample ?? []).map((s) => (Array.isArray(s) ? s[0] : s.path ?? s))
  const unknown = sample.filter((p) => !paths.has(p))
  check('A3', '노드 경로가 레이아웃 경로와 일치', sample.length > 0 && unknown.length === 0, unknown.length ? unknown.slice(0, 5) : `${sample.length} sample`)
  const bitNodes = leaves.filter((m) => m.path.includes('.Command.') && m.prim === 'Bool').length
  check('A4', 'Command 바이트가 Bool 비트 구조로 노출(실제 UDT)', bitNodes > 0, `${bitNodes} Bool members`)

  // ── B. Task ────────────────────────────────────────────────────────────────
  console.log('\nB. Task 입력')
  const cells = list((await get('/api/cells')).body)
  const stations = list((await get('/api/stations')).body)
  const items = list((await get('/api/items')).body)
  const item = items[0]?.code ?? null
  const gateFree = () => waitFor(async () => (await get('/api/tasks/gate')).body?.can_submit, 20000, 250)
  const task = async (id) => (await get(`/api/tasks/${id}`)).body
  const waitState = (id, states, ms) => waitFor(async () => {
    const t = await task(id)
    return states.includes(t?.state) ? t : null
  }, ms, 200)

  async function submit(label, req) {
    if (!(await gateFree())) {
      check('B', `${label}: 제출 게이트`, false, (await get('/api/tasks/gate')).body)
      return null
    }
    const body = { params: {}, position_override: null, note: `bench ${label}`, source: null, count: 1, item_code: item, ...req }
    const r = await post('/api/tasks', body)
    if (r.status !== 200) {
      check('B', `${label}: 제출`, false, `${r.status} ${JSON.stringify(r.body)}`)
      return null
    }
    return r.body
  }

  const accepted = []
  const cases = [
    { label: 'PICK 셀', req: { type: 'PICK', target: { kind: 'cell', id: cells[0]?.id } } },
    { label: 'DROP 셀', req: { type: 'DROP', target: { kind: 'cell', id: cells[1]?.id } } },
    { label: 'PICK 스테이션', req: { type: 'PICK', target: { kind: 'station', id: stations[0]?.id } } },
    { label: 'MOVE 셀', req: { type: 'MOVE', target: { kind: 'cell', id: cells[2]?.id }, item_code: null } },
    { label: 'MEASURE 셀', req: { type: 'MEASURE', target: { kind: 'cell', id: cells[3]?.id } } },
  ]
  for (const c of cases) {
    const e = await submit(c.label, c.req)
    if (!e) continue
    const t = await waitState(e.id, ['accepted', 'queued', 'running', 'completed', 'rejected', 'failed'], 10000)
    if (!check('B1', `${c.label}: 에코 수락`, t && t.ack?.accepted && t.ack?.code === 1, t ? `${t.state} code=${t.ack?.code} ${t.ack?.reason ?? ''}` : 'timeout')) continue
    const resTask = (await db('GR2', 'OPCUA', 'STAT.RES.Task')).body
    const resHeader = (await db('GR2', 'OPCUA', 'STAT.RES.Header')).body
    const d = diffJson(t.plc_task, resTask)
    check('B2', `${c.label}: GR2 RES.Task = 원장 plc_task (OPC UA→레이아웃 전 필드)`, d.length === 0, d.length ? d.slice(0, 6) : `${Object.keys(resTask ?? {}).length} fields`)
    check('B3', `${c.label}: RES.Header = 보낸 헤더`, diffJson(t.header, resHeader).length === 0, `CMD_ID ${resHeader?.CMD_ID} SEQ ${resHeader?.SEQ} CMD 0x${(resHeader?.CMD ?? 0).toString(16)}`)
    const cleared = await waitFor(async () => (await db('GRM', 'OPCUA', `${root}.Header`)).body?.CMD === 0, 4000, 250)
    check('B4', `${c.label}: GRM ${root}.Header 소거`, cleared)
    accepted.push({ label: c.label, id: e.id })
  }
  for (const a of accepted) {
    const t = await waitState(a.id, ['completed', 'failed', 'canceled', 'lost'], 60000)
    check('B5', `${a.label}: 완료`, t?.state === 'completed', t?.state ?? 'timeout')
  }

  // 거부 — PLC CELL 에 없는 셀
  const bogus = 998
  await post('/api/cells/bulk', [{ id: bogus, use: true, blend_use: false, section: 1, row: 9, col: 9, length: 1200, width: 1200, position: [5000, 5000, 1500] }])
  const rj = await submit('PICK 미등록 셀', { type: 'PICK', target: { kind: 'cell', id: bogus } })
  if (rj) {
    const t = await waitState(rj.id, ['rejected', 'accepted', 'queued', 'running', 'completed'], 10000)
    check('B6', 'PLC 에 없는 셀 → 거부 코드 429', t?.state === 'rejected' && t?.ack?.accepted === false && t?.ack?.code === 429, t ? `${t.state} code=${t.ack?.code} ${t.ack?.reason ?? ''}` : 'timeout')
  }
  await call('DELETE', `/api/cells/${bogus}`)

  // 대기 중 취소 + 강제 완료
  const first = await submit('PICK 선행', { type: 'PICK', target: { kind: 'cell', id: cells[4]?.id } })
  const firstT = first && (await waitState(first.id, ['running', 'queued', 'accepted'], 10000))
  const second = firstT && (await submit('PICK 취소 대상', { type: 'PICK', target: { kind: 'cell', id: cells[5]?.id } }))
  if (second && (await waitState(second.id, ['queued', 'accepted'], 10000))) {
    const r = await post(`/api/tasks/${second.id}/cancel`)
    const t = await waitState(second.id, ['canceled'], 10000)
    check('B7', '대기 중 태스크 취소(Delete → PLC Canceled 링)', r.status === 200 && t?.state === 'canceled', t?.state ?? `${r.status} ${JSON.stringify(r.body)}`)
  } else check('B7', '대기 중 태스크 취소', false, 'queued 상태를 만들지 못함')
  if (first) {
    await waitState(first.id, ['running'], 10000)
    const r = await post(`/api/tasks/${first.id}/complete`)
    const t = await waitState(first.id, ['completed'], 10000)
    check('B8', '실행 중 태스크 강제 완료(Complete → PLC Completed 링)', r.status === 200 && t?.state === 'completed', t?.state ?? `${r.status} ${JSON.stringify(r.body)}`)
  }

  // ── C. Cell / Station ──────────────────────────────────────────────────────
  console.log('\nC. Cell / Station 입출력')
  const plcCells = async (plc) => {
    const d = (await db(plc, 'CELL')).body?.data
    return (d?.Cell ?? []).slice(0, d?.Count ?? 0)
  }
  const imp = await post('/api/cells/import?plc=GR2')
  check('C1', 'GR2 CELL 읽기 → 레지스트리', imp.status === 200 && (imp.body?.errors ?? []).length === 0, imp.body)
  const reg1 = list((await get('/api/cells')).body)
  const gr2c = await plcCells('GR2')
  check('C2', '레지스트리 셀 id = GR2 CELL[1..Count] id', JSON.stringify(reg1.map((c) => c.id).sort()) === JSON.stringify(gr2c.map((c) => c.Id).sort()), `${reg1.length} / ${gr2c.length}`)

  const newId = Math.max(...reg1.map((c) => c.id)) + 1
  const newCell = { id: newId, use: true, blend_use: false, section: 2, row: 4, col: 4, length: 1200, width: 1200, position: [13300, 6900, 1500] }
  await post('/api/cells/bulk', [newCell])
  const diff1 = (await get('/api/cells/diff?plc=GR2')).body
  const row = list(diff1).find((r) => r.id === newId)
  check('C3', `로컬에만 있는 셀 ${newId} 이 diff 에 보임`, row && row.status !== 'same', row?.status)

  const refused = await post('/api/cells/push?plc=GR2')
  check('C4', 'GR2 AUTO 중 셀 쓰기 거부(force 없이)', refused.status === 409, `${refused.status} ${JSON.stringify(refused.body)}`)
  const push = await post('/api/cells/push?plc=both&force=1')
  check('C5', '셀 쓰기 GR2+GRM, 쓰기 후 재읽기 바이트 검증', push.status === 200 && push.body?.verified === true, push.body)
  for (const plc of ['GR2', 'GRM']) {
    const seen = await waitFor(async () => {
      const pc = await plcCells(plc)
      const c = pc.find((x) => x.Id === newId)
      return c ? { pc, c } : null
    }, 15000, 500)
    const ok = seen && Math.abs(seen.c.Position[0] - 13300) < 1e-3 && Math.abs(seen.c.Position[1] - 6900) < 1e-3 && seen.pc.length === reg1.length + 1
    check('C6', `${plc} S7 재읽기: Count·셀 ${newId} 위치`, ok, seen ? `Count ${seen.pc.length} · ${seen.c.Position.join('/')}` : 'timeout')
    const d = list((await get(`/api/cells/diff?plc=${plc}`)).body).filter((r) => r.status !== 'same')
    check('C7', `${plc} 셀 diff 없음`, d.length === 0, d.slice(0, 3))
  }

  const spush = await post('/api/stations/push?plc=both&force=1')
  check('C8', '스테이션 쓰기 GR2(평면)+GRM(Para 중첩), 바이트 검증', spush.status === 200 && spush.body?.verified === true, spush.body)
  for (const plc of ['GR2', 'GRM']) {
    const simp = await post(`/api/stations/import?plc=${plc}`)
    check('C9', `${plc} STATION 읽기 오류 없음`, simp.status === 200 && (simp.body?.errors ?? []).length === 0, simp.body)
    const d = list((await get(`/api/stations/diff?plc=${plc}`)).body).filter((r) => r.status !== 'same')
    check('C10', `${plc} 스테이션 diff 없음`, d.length === 0, d.slice(0, 3))
  }

  const xr = await fetch(BASE + '/api/registry/export.xlsx')
  const buf = await xr.arrayBuffer()
  const fd = new FormData()
  fd.append('file', new Blob([buf]), 'bench-registry.xlsx')
  const dry = await post('/api/registry/import-file?dry_run=1', fd)
  const regCells = list((await get('/api/cells')).body).length
  const regStations = list((await get('/api/stations')).body).length
  check('C11', 'Excel 내보내기 → 가져오기 미리보기(오류 0, 행 수 일치)', dry.status === 200 && (dry.body?.errors ?? []).length === 0 && dry.body?.counts?.cells === regCells && dry.body?.counts?.stations === regStations,
    `${buf.byteLength} B · ${JSON.stringify(dry.body?.counts)} vs ${regCells}/${regStations}`)

  // ── 요약 ──────────────────────────────────────────────────────────────────
  const failed = results.filter((r) => !r.ok)
  console.log(`\n결과: ${results.length - failed.length} / ${results.length} 통과`)
  if (OUT) {
    const fs = await import('node:fs')
    fs.writeFileSync(OUT, JSON.stringify({ base: BASE, at: new Date().toISOString(), passed: results.length - failed.length, total: results.length, results }, null, 2))
    console.log(`보고서: ${OUT}`)
  }
  process.exit(failed.length ? 1 : 0)
}

main().catch((e) => {
  console.error(e)
  process.exit(3)
})
