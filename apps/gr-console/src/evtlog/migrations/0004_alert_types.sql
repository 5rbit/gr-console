-- 알림 규칙의 ErrorList 유형(types · trans), 2026-09-28. 기본 규칙 = Alarm 발생 · EMS.
-- 손대지 않은 기본 규칙만 바꾼다: 'ERROR 레벨 전체' → 'Alarm 발생', 'FAULT 전환' 은 끈다(Alarm 발생과 겹친다).
UPDATE alert_rules SET name = 'Alarm 발생', match_json = '{"types":["Alarm"],"trans":"raise"}', updated_at = datetime('now')
    WHERE name = 'ERROR 레벨 전체' AND match_json = '{"min_lvl":"ERROR"}';
UPDATE alert_rules SET enabled = 0, updated_at = datetime('now')
    WHERE name = 'FAULT 전환' AND match_json = '{"codes":["ALM_TO_FAULT"]}';
