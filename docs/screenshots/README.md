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
