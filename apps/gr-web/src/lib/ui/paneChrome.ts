// 껍데기가 이미 이름을 말했는가 — 화면이 자기 제목을 낼지 말지 정하는 한 비트.
//
// 도킹 모드에서는 탭 띠가 `Task 관리`를 말하고 그 바로 아래 `ScreenHeader`가 다시 `Task 관리`를
// 말했다. 같은 이름이 4px 떨어져 두 번 서면 둘 중 하나는 읽히지 않고, 30px(조밀 모드에서도 30px)을
// **아무 정보 없이** 먹는다. 값이 빽빽한 화면에서 그 줄은 표 한 행이다.
//
// 화면이 "나는 도킹 안이다"를 알아서 prop으로 받게 하면 화면 넷이 각자 그것을 릴레이해야 하고,
// 한 화면만 빠뜨려도 제목이 두 겹으로 남는다. 그래서 **껍데기가 문맥으로 선언하고 킷이 읽는다** —
// 새 화면은 아무것도 하지 않아도 규칙을 따른다.
import { createContext, useContext } from 'react'

export interface PaneChromeValue {
  /** 감싸는 껍데기가 이 면의 이름을 이미 보이고 있는가(도킹 탭 띠). */
  titled: boolean
}

const PaneChromeContext = createContext<PaneChromeValue>({ titled: false })

/** 껍데기 쪽 — 도킹 존이 자기 몸을 이것으로 감싼다. */
export const PaneChromeProvider = PaneChromeContext.Provider

/** 킷 쪽 — `ScreenHeader`가 제목을 낼지 정할 때 읽는다. */
export function usePaneChrome(): PaneChromeValue {
  return useContext(PaneChromeContext)
}
