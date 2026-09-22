//! PLC `Real` 값이 NaN 이면 디코더(`plc-layout`)가 JSON `null` 로 낸다(JSON 에는 NaN 이 없다). 그런 값 하나 때문에
//! 상태 전체(`StatusView`) 해석이 실패하면 게이트가 "OPCUA.STAT 을 읽지 못함" 으로 막힌다 — 실기 2026-09-22:
//! GR2 `STAT.Drive[0].JerkTime` 이 NaN. `null` 은 NaN 으로 읽는다(값 자체는 보존, 해석만 계속).

use serde::{Deserialize, Deserializer};

/// `f32` 필드: 숫자 또는 `null`(= NaN).
pub fn f32<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    Ok(Option::<f32>::deserialize(d)?.unwrap_or(f32::NAN))
}

/// `[f32; N]` 필드: 원소마다 숫자 또는 `null`(= NaN).
pub fn arr<'de, D: Deserializer<'de>, const N: usize>(d: D) -> Result<[f32; N], D::Error> {
    let v = Vec::<Option<f32>>::deserialize(d)?;
    let n = v.len();
    let v: Vec<f32> = v.into_iter().map(|x| x.unwrap_or(f32::NAN)).collect();
    v.try_into().map_err(|_| serde::de::Error::invalid_length(n, &format!("an array of {N}").as_str()))
}

#[cfg(test)]
mod tests {
    use crate::StatusView;

    #[test]
    fn nan_real_does_not_break_the_status() {
        let mut v = serde_json::to_value(StatusView::default()).unwrap();
        v["Drive"] = serde_json::json!([{ "JerkTime": null, "Position": 12.5 }]);
        v["Task"]["Now"]["Position"] = serde_json::json!([1.0, null, 3.0, 4.0]);
        let s = StatusView::from_json(&v).expect("null (NaN) must not fail the whole status");
        assert!(s.drive[0].jerk_time.is_nan());
        assert_eq!(s.drive[0].position, 12.5);
        assert!(s.task.now.position[1].is_nan());
    }
}
