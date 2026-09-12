# GR 콘솔 화면 캡처 (데모 모드)

`gr-console --demo` 로 띄운 화면을 헤드리스 Edge(playwright-core)로 캡처했다. PLC 없이 가짜 S7/OPC UA 서버가 돈다.

## 작업 명령 · 레이아웃 맵
| 파일 | 내용 |
|---|---|
| ![](01_plan_mode_v2.png) | 명령 생성 모드 — 셀을 순서대로 누르면 PICK/DROP 이 번갈아 계획에 쌓인다 |
| ![](01b_plan_card_v2.png) | 계획 표 — 순서·품목·수량 편집, 재고 연쇄 계산, 그립 기준 토글 |
| ![](01_plan_mode.png) | 초기 명령 생성 모드 |
| ![](02_view_settings.png) | 맵 설정 팝업 — 원 지름, 회전, 좌우/상하 반전 |
| ![](03_legend.png) | 범례 팝업 |
| ![](04_monitor_info.png) | 모니터링 모드 — 좌클릭 셀·화물 정보 카드 |
| ![](05_edit_cells.png) | 레이아웃 편집 모드 — 셀 목록 |
| ![](06_edit_stations.png) | 레이아웃 편집 모드 — 스테이션 목록 |
| ![](07_edit_rules.png) | 생성 규칙(격자/허니콤) — 생성 예정 셀 색 구분 |
| ![](08_zoomed_out.png) | 축소 시 재고 숫자만 표시 |
| ![](09_robot_working.png) | 작업 중 로봇 색 테두리 |
| ![](15_layout_cell_grid_edit.png) | 셀 엑셀형 편집 — 칸 직접 수정·붙여넣기, 저장 전 초안이 맵에 바로 반영(수정 3건), 선택 행 = 맵 선택 링 |
| ![](16_layout_station_grid.png) | 스테이션 엑셀형 편집 — 적용 후 로컬 사본 저장 알림 |

## Task 관리
| 파일 | 내용 |
|---|---|
| ![](10_task_manager.png) | Task 목록·종결 이력 |
| ![](14_task_detail_fixed.png) | Task 번호 클릭 → 상세 드로어(명령 헤더 CMD_ID/SEQ/CMD 표시). 흰 화면 버그 수정 후 |

## 측정
| 파일 | 내용 |
|---|---|
| ![](11_measure_dashboard.png) | 측정 모니터 대시보드 |
| ![](12_history_item.png) | 측정 이력 — MeasureItem Data 배치 |
| ![](13_history_sku.png) | 측정 이력 — MeasureSku Data 배치 |

## 셸 — 워크스페이스(도킹)·명령 팔레트·밀도·표 접기

`gr-console --demo`(가짜 PLC)로 띄운 화면을 헤드리스 Chromium(playwright)으로 캡처했다. Task 여섯
건을 넣은 상태다. 규칙은 `docs/DESIGN.md`, 결정 이유는 `docs/ui-ux-plan.md`.

| 파일 | 내용 |
|---|---|
| ![](20_single_screen_mode.png) | 단일 화면 모드 — 사이드바(로봇·PLC·상태) + 화면 하나. 기본값이고 예전 셸과 같다 |
| ![](21_view_menu.png) | `보기` 메뉴 — 모드·배치 프리셋·저장·존 토글·밀도·테마 |
| ![](22_workspace_standard.png) | 워크스페이스 모드 `기본` 배치 — 왼쪽 목록 + 중앙 화면 탭 |
| ![](23_preset_command.png) | `명령 중심` — 중앙 작업 명령, 하단 Task 관리, 오른쪽 상태. 명령→결과가 한 화면 |
| ![](24_preset_triage.png) | `데이터 3분할` — 중앙 Task 관리, 오른쪽 상태, 하단 측정 |
| ![](25_preset_monitor.png) | `모니터링` — 중앙 측정 모니터, 오른쪽에 상태·PLC |
| ![](26_tab_drag.png) | 탭 드래그 — 놓을 존이 점선으로 비친다(`오른쪽에 도킹`) |
| ![](27_after_dock.png) | 도킹 후 — Task 관리가 하단에서 오른쪽으로. 좁은 존에서는 머리띠 요약이 접힌다. 탭이 이름을 말하므로 화면 머리띠는 제목을 내지 않는다 |
| ![](28_window_menu.png) | 창 메뉴(탭 우클릭 / `⋮`) — 최대화·존 이동·닫기. 드래그를 못 쓰는 자리의 경로 |
| ![](29_zone_collapsed_rail.png) | 존 접기 — 아이콘 레일로 남는다(무엇이 들었는지 사라지지 않는다) |
| ![](30_command_palette.png) | 명령 팔레트 `Ctrl/⌘+K` — 보기·레이아웃·존·창·설정. 체크와 존 이름이 붙는다 |
| ![](31_palette_query.png) | 팔레트 검색 — `배치`로 프리셋·저장·초기화만 남는다 |
| ![](32_maximized.png) | 최대화 `Alt+Enter` — 그 존 하나만 그린다(뒤 화면은 언마운트) |
| ![](33_compact_density.png) | 조밀 밀도 — 메뉴바 36→30, 머리띠 36→30, 컨트롤 32→28(상태바는 그대로) |
| ![](34_after_reload.png) | 새로고침 후 — 배치·모드·밀도·저장한 배치가 그대로 복원된다 |
| ![](35_focused_zone_right.png) | 활성 패널 — 오른쪽 존을 만지면 그 탭 띠가 밝아지고 accent 밑줄을 받는다(다른 존은 회색). `Alt+Enter`의 대상이 이것이다 |
| ![](36_empty_zone_rail.png) | 드래그 중에는 **빈 존도** 점선 레일로 뜬다(오른쪽 끝) — 놓을 자리가 화면에서 사라지지 않는다 |
| ![](37_table_narrow_zone.png) | 273px 존에 들어간 Task 표 — 열 열둘이 `#·상태·대상` 셋으로 접힌다(가로 스크롤이 아니라) |
| ![](38_table_wide_after_maximize.png) | 같은 표, 최대화(1574px) — 열 열둘이 전부 돌아온다 |
| ![](39_table_row_expanded.png) | 접힌 열은 버리지 않는다 — 행을 펼치면 아홉 열이 라벨+값 짝으로 나온다 |
| ![](40_theme_light.png) | **기본은 화이트톤** — 색은 전부 시맨틱 토큰에서 나온다 |
| ![](41_theme_dark.png) | 다크는 토큰 한 층(`[data-theme='dark']`) — 킷·셸에 `dark:` 짝이 0개다 |
| ![](43_theme_dark_triage.png) | 다크 · 데이터 3분할 — 상태 배지·점이 두 테마에서 같은 뜻으로 읽힌다 |
| ![](44_dark_task.png) | 다크 · 작업 명령 — 셀 맵의 판정색(`var(--color-fault/ok)`)과 스테이션 hue가 토큰·데이터로 갈려 있다 |
| ![](45_dark_taskmgr.png) | 다크 · Task 관리 — 표·배지·필터 칩이 전부 토큰이라 화면 코드에 `dark:`가 없다 |
| ![](46_dark_measure.png) | 다크 · 측정 모니터 — 상태 칩 soft 900 틴트가 panel과 갈린다(1.25~1.73) |
| ![](47_dark_scenario.png) | 다크 · 시나리오 — 인스펙터 머리띠(control-header)와 위험 버튼(danger)도 토큰 |
| ![](42_view_menu_fixed.png) | `보기` 메뉴 — 라벨이 줄어들지 않고 힌트가 먼저 잘린다(예전에는 `명령 중 / 심`으로 끊겼다) |
