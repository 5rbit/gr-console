// 클립보드 복사 — **평문 HTTP에서도 동작해야 한다**.
//
// `navigator.clipboard.writeText`는 보안 컨텍스트(HTTPS 또는 localhost)에서만 동작한다. 현장
// 태블릿은 HTTP로 관리툴에 붙으므로 그 경로만 쓰면 조용히 실패한다. 최신 API를 먼저 시도하고,
// 실패하면 textarea + `execCommand('copy')` 구식 경로로 떨어진다(사용자 제스처 안에서는 동작).
// 성공 여부를 돌려주므로 호출자가 토스트를 고를 수 있다.
export async function copyText(text: string): Promise<boolean> {
  if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text)
      return true
    } catch {
      // 아래 구식 경로로.
    }
  }
  if (typeof document === 'undefined') return false
  try {
    const ta = document.createElement('textarea')
    ta.value = text
    ta.setAttribute('readonly', '')
    ta.style.cssText = 'position:fixed;top:0;left:0;opacity:0;pointer-events:none'
    document.body.appendChild(ta)
    ta.focus()
    ta.select()
    const ok = document.execCommand('copy')
    document.body.removeChild(ta)
    return ok
  } catch {
    return false
  }
}
