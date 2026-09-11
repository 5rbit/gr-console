// 시맨틱 상태 인디케이터(+선택 라벨) — statusTone 단일 표를 소비.
//
// 모양이 종류를, 채움이 실체를 말한다(색에 앞서는 신호를 둘 둔다). 규약의 진실원은 여기이고
// 화면 범례(`IndicatorLegend`)가 같은 표를 그린다.
//
//   실 로봇  = 정사각. 채움 = 로봇 웹(:8088) · 테두리 = 사이드카(:8090)
//   sim      = 속 빈 원. 테두리 = 상태 · 중앙 점 = 부팅됨
//
// 기본은 정사각이고, 원은 sim을 그리는 자리에서만 켠다(`shape="circle" hollow`).
import { statusTone, type Status } from './status'

export interface StatusDotProps {
  /** 주 상태 — 채움색(hollow면 테두리 기본색) */
  status?: Status | string
  /** 옆에 붙는 텍스트(선택) */
  label?: string
  /** 깜빡임 — **sim 재생 표시 전용**. 실장비를 깜빡이면 알람으로 오해된다 */
  pulse?: boolean
  size?: 'sm' | 'md'
  /** 정사각(기본) = 물리적 실체 · 원 = 가상(sim) */
  shape?: 'square' | 'circle'
  /** 채움 없음 — sim(가상)이거나 로봇 미도달 */
  hollow?: boolean
  /** 테두리 상태 — 로봇의 **2번째 축**(사이드카). 미지정이면 `status`를 따른다 */
  outline?: Status | string
  /** 테두리 점선 — 2번째 축이 정상이 아님(사이드카 미도달·인증 거부) */
  dashed?: boolean
  /** hollow일 때 중앙 점 — sim이 부팅됐음 */
  core?: boolean
  /** 빗금 — 어느 통로도 없음(로봇도 펌웨어도 미도달, 또는 미연결). 빈 사각형이 체크박스로
   *  읽히는 것을 끊는다 */
  slash?: boolean
  /** 스크린리더·툴팁 설명(예: `로봇 정상 · 사이드카 미도달`) */
  title?: string
}

export function StatusDot({
  status = 'neutral',
  label = '',
  pulse = false,
  size = 'md',
  shape = 'square',
  hollow = false,
  outline,
  dashed = false,
  core = false,
  slash = false,
  title,
}: StatusDotProps) {
  const fill = statusTone(status)
  const edge = statusTone(outline ?? status)
  // 8px에서는 테두리·중앙 점이 뭉개진다 — 두 축을 싣는 이상 최소 10px가 필요하다.
  const dim = size === 'sm' ? 'h-2.5 w-2.5' : 'h-3 w-3'
  const round = shape === 'circle' ? 'rounded-full' : 'rounded-[2px]'

  return (
    <span className="inline-flex items-center gap-1.5">
      <span
        className={`relative inline-flex shrink-0 ${dim}`}
        role={title ? 'img' : undefined}
        aria-label={title}
        title={title}
        data-testid="status-mark"
        data-shape={shape}
        data-fill={hollow ? 'hollow' : 'solid'}
        data-outline={outline ?? status}
        data-dashed={dashed ? 'true' : 'false'}
        data-slash={slash ? 'true' : 'false'}
      >
        {pulse && (
          <span
            className={`absolute inline-flex h-full w-full animate-ping opacity-60 ${round} ${fill.dot}`}
          />
        )}
        {/* 테두리는 **항상** 그린다 — 실선/점선·색 차이가 2번째 축을 싣는 자리다. */}
        <span
          className={`relative inline-flex ${dim} ${round} border-[1.5px] ${edge.border} ${
            dashed ? 'border-dashed' : 'border-solid'
          } ${hollow ? '' : fill.dot}`}
        >
          {/* 중앙 점 — 속 빈 원(sim)이 부팅됐음. 테두리(상태)와 같은 색으로 채운다. */}
          {hollow && core && <span className={`m-auto h-1 w-1 rounded-full ${fill.dot}`} />}
          {/* 빗금 — 빈 사각형이 체크박스로 읽히는 것을 끊는다(20대 실측에서 잡힌 오해). */}
          {slash && (
            <svg className="absolute inset-0 h-full w-full" viewBox="0 0 10 10" aria-hidden="true">
              {/* 방향은 `＼`다. `／`는 체크 표시의 획으로 읽혀 빈 사각형을 **체크된 체크박스**로
                  보이게 만든다(고치려던 오해가 더 나빠진다). */}
              <line
                x1="1.5"
                y1="1.5"
                x2="8.5"
                y2="8.5"
                stroke="currentColor"
                strokeWidth="1.4"
                className={edge.text}
                strokeLinecap="round"
              />
            </svg>
          )}
        </span>
      </span>
      {label && <span className={`text-xs ${fill.text}`}>{label}</span>}
    </span>
  )
}
