# gr-console

겐트리 로봇(GR2) 엔지니어링 테스트 콘솔. Rust(axum) 백엔드 + React 프론트.
- 명령: OPC UA → GRM `"OPCUA".GR[2].CMD` (GCS 와 같은 경로)
- 읽기/쓰기: S7comm 직접 (표준 접근 DB), 레이아웃은 `plc/contract` 의 TIA 소스에서 생성하고 접속 시 검증
- 계획 문서: `C:\Users\hmx2210256\.claude\plans\lovely-doodling-graham.md`
