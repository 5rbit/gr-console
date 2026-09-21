// Generate apps/gr-console/src/pallet/spec_r4.json from the shapes extracted out of
// "[GR] HKT TP2 팔렛타이징 사양서 R4 251213.pptx" (tools/pallet/shapes/*.csv, made by pptx_shapes.ps1)
// plus the hand-transcribed order tables below.
//
//   node tools/pallet/gen_spec.mjs          (write json + print report)
//   node tools/pallet/gen_spec.mjs --check  (exit 1 if the json on disk differs)
//
// Geometry: every pattern box is a 70.87 pt rounded rectangle = 1600 mm pallet.
// Tire centers are converted to machine axes (per-slide screen->machine sign), scaled to mm,
// then normalized by the minimum center-to-center distance (u = mm / minDist).
// Drag dir digits 1..8 are machine-axis codes (PLC FC DragDelta).

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const OUT = path.resolve(here, '../../apps/gr-console/src/pallet/spec_r4.json');
const PALLET_PT = 70.87;
const PALLET_MM = 1600;
const MM_PER_PT = PALLET_MM / PALLET_PT;
const CENTER_SNAP_MM = 10;

// Pattern size classes. Order tables print P5 as "601 ~ 622"; the size chart prints "601 ~ 662".
// 662 is used so that 623..662 is not a hole between P5 and P4 (663~).
const SIZE = [
  { pattern: 2, od_min: 814, od_max: 937 },
  { pattern: 3, od_min: 756, od_max: 813 },
  { pattern: 4, od_min: 663, od_max: 755 },
  { pattern: 5, od_min: 601, od_max: 662 },
  { pattern: 6, od_min: 546, od_max: 600 },
  { pattern: 8, od_min: 521, od_max: 545 },
  { pattern: 9, od_min: 0, od_max: 520 },
];
const ROW_PATTERNS = [2, 3, 4, 5, 6, 8, 9];

// dir code -> unit machine displacement sign (DragDelta: X then Y)
const DIR_VEC = { 1: [1, -1], 2: [1, 0], 3: [1, 1], 4: [0, -1], 5: [0, 1], 6: [-1, -1], 7: [-1, 0], 8: [-1, 1] };

// Order tables: "slot" or "slot p dir", in task order (seq 1 first).
// Notes record every place where the slide text is ambiguous or was corrected.
const FLOWS = [
  {
    id: 'HP_IN', name: 'Palletizing Pattern (HP 입고)', slide: 2, also_slides: [4], column: 471.04,
    drag_kind: 'in', robot: 'GR1(HP)', zone: '입고1 (Stack 422) / 입고2 (Stack 391)', sign: [-1, -1],
    orders: {
      2: '1 2p3', 3: '1 2p3 3p1', 4: '1 2p5 3p2 4p3', 5: '1 2 3p2 4p1 5p3', 6: '1 2p1 3p3 4p2 5p1 6p3',
      8: '1 2 3p2 4p1 5p3 6p2 7p1 8p3', 9: '1 2p5 3p5 4p2 5p3 6p3 7p2 8p3 9p3',
    },
    notes: {},
  },
  {
    id: 'OP_IN', name: 'Palletizing Pattern (OP 입고)', slide: 2, also_slides: [4], column: 733.63,
    drag_kind: 'in', robot: 'GR2(OP)', zone: '입고1 (Stack 422) / 입고2 (Stack 391)', sign: [-1, -1],
    orders: {
      2: '2 1p6', 3: '3 2p8 1p6', 4: '4 3p4 2p7 1p6', 5: '5 4 3p7 2p8 1p6', 6: '6 5 4p7 3p8 2p6 1p7',
      8: '8 7 6p7 5p8 4p6 3p7 2p8 1p6', 9: '9 8p4 7p4 6p7 5p6 4p6 3p7 2p6 1p6',
    },
    notes: {
      3: ['slide2 has a second row "2(p8) → 1(p6)" (2 tires, no C#3); slide4 keeps only "3 → 2(p8) → 1(p6)" (red box). The 3-tire row is used.'],
      4: ['slide2 table prints "3 (45)"; slide4 prints "3 (p4)" and circle C#3 is "4↑" → dir 4.'],
      6: ['table prints "1(7)" (missing p); circle C#1 "←7" → dir 7.'],
    },
  },
  {
    id: 'OP_IN_S5', name: 'Palletizing Pattern (OP 입고) — slide5 흐린 참고본', slide: 5, also_slides: [], column: 471.09,
    drag_kind: 'in', robot: 'GR1(OP)?', zone: '출하1,2,3 slide 참고 (흐린 표시)', sign: [-1, -1], reference: true,
    orders: {
      2: '2 1p6', 3: '3 2p8 1p6', 4: '3 4p5 1p7 2p8', 5: '4 5 3p7 1p6 2p8', 6: '5 6 4p7 2p6 3p8 1p7',
      8: '7 8 6p7 4p6 5p8 3p7 1p6 2p8', 9: '7 8p5 9p5 4p7 5p8 6p8 1p7 2p8 3p8',
    },
    notes: {
      0: ['Faded reference column on slide5. Same slot positions as OP_IN but different orders/dirs (row-by-row from the X+/Y− corner). P4 equals IN1_ALT_OP (slide3) in machine axes. Which OP 입고 version is current is an open question.'],
      6: ['table prints "1(7)" (missing p); circle C#1 "←7" → dir 7.'],
    },
  },
  {
    id: 'OP_OUT', name: 'Palletizing Pattern (OP 출하)', slide: 5, also_slides: [], column: 733.36,
    drag_kind: 'out', robot: 'GR1(OP)', zone: '출하1 (115) / 출하2 (116) / 출하3 (116)', sign: [-1, -1],
    orders: {
      2: '2p6 1', 3: '3p7 2p6 1', 4: '4p6 3p7 2p4 1', 5: '5p6 4p7 3p7 2p4 1', 6: '6p6 5p7 4p7 3p4 2p7 1',
      8: '8p6 7p7 6p7 5p7 4p7 3p7 2 1', 9: '9p6 8p6 7p7 6p6 5p6 4p7 3p4 2p4 1',
    },
    notes: {
      3: ['slide5 red box revises the order to "3(p7) → 2(p6) → 1" (used); the original row "2(p6) → 3(p7) → 1" is still printed above it and on slide6.'],
      5: ['table prints "… 3(p7) → 2 → 1" (no dir on 2) but circle C#2 is "4↓" and OP_EXT_IN P5 has "2(p4)" → dir 4.'],
      8: ['slide6 faded copy swaps C#4/C#5 positions and prints "6(p7) → 4(p7) → 5(p7)"; slide5 (used) has C#4 above C#5 and "5(p7) → 4(p7)".'],
    },
  },
  {
    id: 'OP_EXT_IN', name: 'Palletizing Pattern (OP 외부입고)', slide: 6, also_slides: [], column: 733.36,
    drag_kind: 'in', robot: 'GR1(OP)', zone: '외부입고 (117)', sign: [-1, -1],
    orders: {
      2: '1 2p6', 3: '1 3p7 2p6', 4: '1 2p4 3p7 4p6', 5: '1 2p4 3p7 4p7 5p6', 6: '1 2p7 3p4 4p7 5p7 6p6',
      8: '1 2 3p7 4p7 5p7 6p7 7p7 8p6', 9: '1 2p4 3p4 4p7 5p6 6p6 7p7 8p6 9p6',
    },
    notes: {
      3: ['"1 → 3(p7) → 2(p6)" is the reverse of the UNREVISED OP 출하 P3 order; the revised OP 출하 would reverse to "1 → 2(p6) → 3(p7)". Not changed — open question.'],
    },
  },
  {
    id: 'IN1_ALT_HP', name: '입고1 대안 레이아웃 (HP 입고)', slide: 3, also_slides: [], column: 177.79, single: 4,
    drag_kind: 'in', robot: 'GR1(HP)', zone: '입고1 (Stack 422) 대안 셀 레이아웃', sign: [1, 1],
    orders: { 4: '1 2p5 3p2 4p3' },
    notes: { 0: ['Slide3 draws the cell with X+ right / Y+ down. Only the 4-tire pattern is given. In machine axes it is identical to HP_IN P4.'] },
  },
  {
    id: 'IN1_ALT_OP', name: '입고1 대안 레이아웃 (OP 입고)', slide: 3, also_slides: [], column: 276.63, single: 4,
    drag_kind: 'in', robot: 'GR2(OP)', zone: '입고1 (Stack 422) 대안 셀 레이아웃', sign: [1, 1],
    orders: { 4: '3 4p5 1p7 2p8' },
    notes: { 0: ['Slide3 draws the cell with X+ right / Y+ down. Only the 4-tire pattern is given. Differs from OP_IN P4; equals OP_IN_S5 P4 in machine axes.'] },
  },
];

// ---------- csv ----------
function readCsv(slide) {
  const raw = fs.readFileSync(path.join(here, 'shapes', `shapes_slide${slide}.csv`), 'utf8').replace(/^﻿/, '');
  const lines = raw.split(/\r?\n/).filter(Boolean);
  const head = parseLine(lines[0]);
  return lines.slice(1).map((l) => {
    const cells = parseLine(l);
    const o = {};
    head.forEach((h, i) => (o[h] = cells[i]));
    for (const k of ['cx_pt', 'cy_pt', 'w_pt', 'h_pt']) o[k] = Number(o[k]);
    return o;
  });
}
function parseLine(line) {
  const out = [];
  let cur = '';
  let q = false;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (q) {
      if (c === '"' && line[i + 1] === '"') { cur += '"'; i++; }
      else if (c === '"') q = false;
      else cur += c;
    } else if (c === '"') q = true;
    else if (c === ',') { out.push(cur); cur = ''; }
    else cur += c;
  }
  out.push(cur);
  return out;
}

// ---------- geometry ----------
function boxesOf(rows, column, single) {
  const boxes = rows
    .filter((r) => r.geom === 'roundRect' && Math.abs(r.w_pt - PALLET_PT) < 0.5 && Math.abs(r.cx_pt - column) < 3)
    .sort((a, b) => a.cy_pt - b.cy_pt);
  if (single) {
    if (boxes.length !== 1) throw new Error(`expected 1 box at x=${column}, got ${boxes.length}`);
    return [{ pattern: single, box: boxes[0] }];
  }
  if (boxes.length !== ROW_PATTERNS.length) throw new Error(`expected 7 boxes at x=${column}, got ${boxes.length}`);
  return boxes.map((box, i) => ({ pattern: ROW_PATTERNS[i], box }));
}

function tiresIn(rows, box) {
  const half = PALLET_PT / 2;
  return rows
    .filter((r) => r.geom === 'ellipse' && Math.abs(r.cx_pt - box.cx_pt) < half && Math.abs(r.cy_pt - box.cy_pt) < half)
    .map((r) => {
      const digits = r.text.match(/\d/g) || [];
      return { slot: Number(digits[0]), label_dir: digits[1] ? Number(digits[1]) : 0, text: r.text, r };
    });
}

function parseOrder(s) {
  return s.trim().split(/\s+/).map((tok) => {
    const m = tok.match(/^(\d)(?:p(\d))?$/);
    if (!m) throw new Error(`bad order token ${tok}`);
    return { slot: Number(m[1]), dir: m[2] ? Number(m[2]) : 0 };
  });
}

const r4 = (v) => Math.round(v * 10000) / 10000;
const r1 = (v) => Math.round(v * 10) / 10;

function buildPattern(flow, pattern, box, tires, report) {
  const n = tires.length;
  if (n !== pattern) throw new Error(`${flow.id} P${pattern}: ${n} tires drawn`);
  const order = parseOrder(flow.orders[pattern]);
  const notes = [...(flow.notes[pattern] || [])];
  const [sx, sy] = flow.sign;
  const pts = tires.map((t) => ({
    ...t,
    mm: [sx * (t.r.cx_pt - box.cx_pt) * MM_PER_PT, sy * (t.r.cy_pt - box.cy_pt) * MM_PER_PT],
  }));
  let minDist = Infinity;
  for (let i = 0; i < n; i++) for (let j = i + 1; j < n; j++) {
    minDist = Math.min(minDist, Math.hypot(pts[i].mm[0] - pts[j].mm[0], pts[i].mm[1] - pts[j].mm[1]));
  }
  const d = (pts.reduce((a, p) => a + p.r.w_pt, 0) / n) * MM_PER_PT;
  const cen = [pts.reduce((a, p) => a + p.mm[0], 0) / n, pts.reduce((a, p) => a + p.mm[1], 0) / n];
  const centered = Math.hypot(cen[0], cen[1]) <= CENTER_SNAP_MM;
  const shift = centered ? cen : [0, 0];

  const slots = order.map((o, i) => {
    const p = pts.find((t) => t.slot === o.slot);
    if (!p) throw new Error(`${flow.id} P${pattern}: slot C#${o.slot} not drawn`);
    let dir = o.dir;
    if (p.label_dir !== o.dir) {
      notes.push(`C#${o.slot}: order table dir ${o.dir || '-'} vs circle label dir ${p.label_dir || '-'} → circle label used.`);
      report.mismatch.push(`${flow.id} P${pattern} C#${o.slot}: table ${o.dir} / label ${p.label_dir}`);
      dir = p.label_dir;
    }
    return {
      slot: `C#${o.slot}`,
      seq: i + 1,
      u: [r4((p.mm[0] - shift[0]) / minDist), r4((p.mm[1] - shift[1]) / minDist)],
      drag_dir: dir,
    };
  });
  if (new Set(order.map((o) => o.slot)).size !== n) throw new Error(`${flow.id} P${pattern}: duplicate slot in order`);

  // outward check: drag displacement should not point toward the pattern centroid
  for (const s of slots) {
    if (!s.drag_dir) continue;
    const p = pts.find((t) => `C#${t.slot}` === s.slot);
    const v = DIR_VEC[s.drag_dir];
    const dot = v[0] * (p.mm[0] - cen[0]) + v[1] * (p.mm[1] - cen[1]);
    if (dot < -1) report.inward.push(`${flow.id} P${pattern} ${s.slot} dir ${s.drag_dir} (dot ${r1(dot)})`);
  }
  const size = SIZE.find((c) => c.pattern === pattern);
  report.geom.push({
    flow: flow.id, pattern, d_mm: r1(d), min_dist_mm: r1(minDist), gap_mm: r1(minDist - d),
    centroid_mm: [r1(cen[0]), r1(cen[1])], centered,
  });
  return {
    pattern,
    od_min: size.od_min,
    od_max: size.od_max,
    order_text: flow.orders[pattern],
    drawn: {
      d_mm: r1(d), min_dist_mm: r1(minDist), gap_mm: r1(minDist - d),
      centroid_mm: [r1(cen[0]), r1(cen[1])], centered,
    },
    slots,
    notes,
  };
}

// ---------- dihedral comparison (report only; the Rust test asserts it) ----------
const TRANSFORMS = [];
for (const mirror of [false, true]) for (const rot of [0, 90, 180, 270]) TRANSFORMS.push({ mirror, rot });
function apply(t, [x, y]) {
  if (t.mirror) x = -x;
  switch (t.rot) {
    case 90: return [-y, x];
    case 180: return [-x, -y];
    case 270: return [y, -x];
    default: return [x, y];
  }
}
function setResidual(a, b) {
  let worst = 0;
  const used = new Set();
  for (const p of a) {
    let best = Infinity, bi = -1;
    b.forEach((q, i) => {
      if (used.has(i)) return;
      const dd = Math.hypot(p[0] - q[0], p[1] - q[1]);
      if (dd < best) { best = dd; bi = i; }
    });
    used.add(bi);
    worst = Math.max(worst, best);
  }
  return worst;
}

function compareColumns(slideA, colA, slideB, colB, label, report) {
  const ra = readCsv(slideA), rb = readCsv(slideB);
  const ba = boxesOf(ra, colA), bb = boxesOf(rb, colB);
  ba.forEach(({ pattern, box }, i) => {
    const ta = tiresIn(ra, box), tb = tiresIn(rb, bb[i].box);
    for (const t of ta) {
      const u = tb.find((x) => x.slot === t.slot);
      const dxa = t.r.cx_pt - box.cx_pt, dya = t.r.cy_pt - box.cy_pt;
      const dxb = u ? u.r.cx_pt - bb[i].box.cx_pt : NaN, dyb = u ? u.r.cy_pt - bb[i].box.cy_pt : NaN;
      if (!u || Math.hypot(dxa - dxb, dya - dyb) > 0.5 || u.label_dir !== t.label_dir) {
        report.columns.push(`${label} P${pattern} C#${t.slot}: slide${slideA}(${r1(dxa)},${r1(dya)} dir ${t.label_dir}) vs slide${slideB}(${r1(dxb)},${r1(dyb)} dir ${u?.label_dir})`);
      }
    }
  });
}

// ---------- main ----------
const report = { mismatch: [], inward: [], geom: [], columns: [], dihedral: [] };
const flows = FLOWS.map((f) => {
  const rows = readCsv(f.slide);
  const patterns = boxesOf(rows, f.column, f.single).map(({ pattern, box }) =>
    buildPattern(f, pattern, box, tiresIn(rows, box), report));
  return {
    id: f.id,
    name: f.name,
    source_slide: f.slide,
    also_slides: f.also_slides,
    drag_kind: f.drag_kind,
    reference: !!f.reference,
    robot: f.robot,
    zone: f.zone,
    screen_axes: f.sign[0] < 0 ? { right: 'X-', down: 'Y-' } : { right: 'X+', down: 'Y+' },
    notes: f.notes[0] || [],
    patterns,
  };
});

const hp = flows.find((f) => f.id === 'HP_IN');
for (const f of flows) {
  for (const p of f.patterns) {
    const ref = hp.patterns.find((q) => q.pattern === p.pattern);
    const a = ref.slots.map((s) => s.u);
    let best = { res: Infinity };
    for (const t of TRANSFORMS) {
      const res = setResidual(p.slots.map((s) => apply(t, s.u)), a);
      if (res < best.res) best = { res, t };
    }
    report.dihedral.push(`${f.id} P${p.pattern} ~ HP_IN: ${best.t.mirror ? 'mirrorX+' : ''}rot${best.t.rot} residual ${r4(best.res)}`);
  }
}
compareColumns(2, 471.04, 4, 471.04, 'HP_IN slide2 vs slide4', report);
compareColumns(2, 733.63, 4, 733.63, 'OP_IN slide2 vs slide4', report);
compareColumns(5, 733.36, 6, 470.81, 'OP_OUT slide5 vs slide6 faded', report);
compareColumns(5, 733.36, 6, 733.36, 'OP_OUT slide5 vs OP_EXT_IN slide6 (geometry only, dir glyphs differ)', report);

const spec = {
  version: 'R4-251213',
  source: '[GR] HKT TP2 팔렛타이징 사양서 R4 251213.pptx',
  generator: 'tools/pallet/gen_spec.mjs',
  pallet_size_mm: PALLET_MM,
  normalization: 'u = drawn machine-axis offset from pallet box center / min center distance; offset_mm = u * (OuterDiameter + Gap). Centroid snapped to 0 only when within 10 mm.',
  size_classes: SIZE,
  size_class_notes: ['Order tables print Pattern 5 as "601 ~ 622"; the size chart prints "601 ~ 662". 662 is used (no hole before Pattern 4 at 663).'],
  flows,
};
const json = JSON.stringify(spec, null, 2) + '\n';

if (process.argv.includes('--check')) {
  const cur = fs.existsSync(OUT) ? fs.readFileSync(OUT, 'utf8') : '';
  if (cur !== json) { console.error('spec_r4.json is stale; run node tools/pallet/gen_spec.mjs'); process.exit(1); }
  console.log('spec_r4.json up to date');
} else {
  fs.writeFileSync(OUT, json);
  console.log(`wrote ${OUT}`);
}
console.log('\n# label vs table mismatches'); report.mismatch.forEach((l) => console.log(' ', l));
console.log('\n# inward drag dirs'); report.inward.forEach((l) => console.log(' ', l));
console.log('\n# geometry (flow P d_mm min_dist gap centroid centered)');
report.geom.forEach((g) => console.log(`  ${g.flow} P${g.pattern}: D ${g.d_mm} min ${g.min_dist_mm} gap ${g.gap_mm} centroid (${g.centroid_mm}) ${g.centered ? 'centered' : ''}`));
console.log('\n# dihedral vs HP_IN'); report.dihedral.forEach((l) => console.log(' ', l));
console.log('\n# column diffs'); report.columns.forEach((l) => console.log(' ', l));
