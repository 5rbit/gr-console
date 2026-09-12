// 앱 전역 SSE 피드 — 스트림당 인스턴스 하나. 화면은 `useSse(statusFeed)`로 읽는다.
import { STREAM_URL } from './api'
import { SseFeed } from './sse'
import type { ScenarioRun, StatusEvent, StockEvent, TasksEvent } from './types'

/** PLC 상태(WebMon) — 20Hz 안팎. */
export const statusFeed = new SseFeed<StatusEvent>(STREAM_URL.status)
/** Task 상태 이벤트(스냅샷/갱신/삭제) — `lib/tasks.ts`가 Map에 적용한다. */
export const tasksFeed = new SseFeed<TasksEvent>(STREAM_URL.tasks)
/** 시나리오 실행 진행. */
export const runsFeed = new SseFeed<ScenarioRun>(STREAM_URL.runs)
/** 셀 재고 이벤트(스냅샷/갱신/삭제) — `lib/stock.ts`가 Map에 적용한다. */
export const stockFeed = new SseFeed<StockEvent>(STREAM_URL.stock)
