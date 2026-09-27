// 알림 스토어 — 확인 안 된 알림 목록 + 수. 상태바 배지가 잡고 있는 동안 `?alerts=only` 스트림을 듣는다.
// 이벤트 화면의 라이브 스트림(`evtStream`)이 열려 있으면 그 연결로 받고 자기 연결은 닫는다 — 배지가
// 브라우저의 호스트당 연결 수를 늘리지 않게(최대치는 배지 전과 같다). 갈아탈 때마다 목록을 다시 받아 틈을 메운다.
//
// 이름 붙은 SSE 이벤트(`alert` · `alert_ack`)는 `onmessage` 로 오지 않는다 — `addEventListener` 로 받는다.
// 소리·브라우저 알림은 이 브라우저의 설정이라 localStorage 에 둔다(서버 규칙과 무관).
import { Store } from '../store'
import { EVT_ALERT_STREAM_URL, evtApi, type AlertRec } from './api'
import { alertBody, applyAck, mergeAlerts } from './alertsModel'
import { evtStream } from './stream'

const PREFS_KEY = 'gr-evt-alert-prefs'

interface Prefs {
  beep: boolean
  notify: boolean
}

function loadPrefs(): Prefs {
  try {
    const p = JSON.parse(localStorage.getItem(PREFS_KEY) ?? '{}') as Partial<Prefs>
    return { beep: p.beep === true, notify: p.notify === true }
  } catch {
    return { beep: false, notify: false }
  }
}

class AlertsStore extends Store {
  #rows: AlertRec[] = []
  #unacked = 0
  /** 이벤트 로그가 꺼진 백엔드면 `false` — 배지를 그리지 않는다. */
  #available = true
  #error: string | null = null
  #refs = 0
  #es: EventSource | null = null
  #offShared: (() => void) | null = null
  #offStream: (() => void) | null = null
  #prefs: Prefs = typeof localStorage === 'undefined' ? { beep: false, notify: false } : loadPrefs()
  #audio: AudioContext | null = null

  get rows(): readonly AlertRec[] {
    return this.#rows
  }
  get unacked(): number {
    return this.#unacked
  }
  get available(): boolean {
    return this.#available
  }
  get error(): string | null {
    return this.#error
  }
  get beepOn(): boolean {
    return this.#prefs.beep
  }
  get notifyOn(): boolean {
    return this.#prefs.notify
  }

  start(): () => void {
    this.#refs++
    if (this.#refs === 1) {
      void this.load()
      this.#offStream = evtStream.subscribe(() => this.#sync())
      this.#sync()
    }
    let released = false
    return () => {
      if (released) return
      released = true
      this.#refs--
      if (this.#refs === 0) {
        this.#offStream?.()
        this.#offStream = null
        this.#offShared?.()
        this.#offShared = null
        this.#es?.close()
        this.#es = null
      }
    }
  }

  /** 이벤트 스트림이 열려 있으면 그것을, 아니면 자기 연결을. */
  #sync(): void {
    if (this.#refs === 0 || !this.#available) return
    const shared = evtStream.active
    if (shared && !this.#offShared) {
      this.#es?.close()
      this.#es = null
      this.#offShared = evtStream.onAlert(
        (a) => this.#onAlert(a),
        (ids) => this.#onAck(ids),
      )
      void this.load()
    } else if (!shared && !this.#es) {
      this.#offShared?.()
      this.#offShared = null
      this.#open()
    }
  }

  async load(): Promise<void> {
    try {
      const l = await evtApi.alerts(true, 100)
      this.#rows = l.rows
      this.#unacked = l.unacked
      this.#available = true
      this.#error = null
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      // 400 = 이벤트 로그 꺼짐(설정), 404 = 알림이 없는 백엔드 — 오류가 아니라 기능이 없는 것
      this.#available = !/→ 40[04]/.test(msg)
      this.#error = msg
      if (!this.#available) {
        this.#es?.close()
        this.#es = null
      }
    }
    this.notify()
  }

  async ack(id: number): Promise<void> {
    const r = await evtApi.ack(id)
    this.#rows = applyAck(this.#rows, [id])
    this.#unacked = r.unacked
    this.notify()
  }

  async ackAll(): Promise<void> {
    await evtApi.ackAll()
    this.#rows = []
    this.#unacked = 0
    this.notify()
  }

  setBeep(on: boolean): void {
    this.#prefs = { ...this.#prefs, beep: on }
    this.#save()
    if (on) this.#playBeep()
  }

  /** 켤 때만 권한을 묻는다. 거부되면 꺼진 채로 두고 이유를 돌려준다. */
  async setNotify(on: boolean): Promise<string | null> {
    if (on) {
      if (typeof Notification === 'undefined') return '이 브라우저는 알림을 지원하지 않습니다'
      const p =
        Notification.permission === 'default'
          ? await Notification.requestPermission()
          : Notification.permission
      if (p !== 'granted') return '브라우저 알림 권한이 거부되었습니다 — 사이트 설정에서 허용하세요'
    }
    this.#prefs = { ...this.#prefs, notify: on }
    this.#save()
    return null
  }

  #save(): void {
    try {
      localStorage.setItem(PREFS_KEY, JSON.stringify(this.#prefs))
    } catch {
      // 저장 못 해도 이번 화면에서는 켜진 대로 간다
    }
    this.notify()
  }

  #open(): void {
    if (typeof EventSource === 'undefined') return
    const es = new EventSource(EVT_ALERT_STREAM_URL)
    es.addEventListener('alert', (ev) => {
      try {
        this.#onAlert(JSON.parse((ev as MessageEvent<string>).data) as AlertRec)
      } catch {
        // 깨진 프레임은 버린다
      }
    })
    es.addEventListener('alert_ack', (ev) => {
      try {
        this.#onAck(JSON.parse((ev as MessageEvent<string>).data) as number[])
      } catch {
        // 깨진 프레임은 버린다
      }
    })
    // 재연결 사이에 놓친 알림을 메운다
    es.onopen = () => void this.load()
    this.#es = es
  }

  #onAlert(a: AlertRec): void {
    if (this.#rows.some((r) => r.id === a.id)) return
    this.#rows = mergeAlerts(this.#rows, [a])
    this.#unacked++
    this.#available = true
    this.notify()
    if (this.#prefs.beep) this.#playBeep()
    if (this.#prefs.notify) this.#show(a)
  }

  /** 다른 화면의 확인 — 수는 서버가 세므로 다시 받는다. */
  #onAck(ids: number[]): void {
    this.#rows = applyAck(this.#rows, ids)
    void this.load()
  }

  #playBeep(): void {
    try {
      const Ctx =
        window.AudioContext ??
        (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext
      if (!Ctx) return
      this.#audio ??= new Ctx()
      const ctx = this.#audio
      const osc = ctx.createOscillator()
      const gain = ctx.createGain()
      osc.frequency.value = 880
      gain.gain.value = 0.08
      osc.connect(gain).connect(ctx.destination)
      osc.start()
      osc.stop(ctx.currentTime + 0.15)
    } catch {
      // 오디오가 막힌 환경(자동 재생 정책) — 소리 없이 넘어간다
    }
  }

  #show(a: AlertRec): void {
    try {
      if (typeof Notification === 'undefined' || Notification.permission !== 'granted') return
      new Notification(a.rule_name, { body: alertBody(a), tag: `gr-alert-${a.id}` })
    } catch {
      // 알림을 못 띄워도 배지는 남는다
    }
  }
}

export const alerts = new AlertsStore()
