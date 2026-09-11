//! Local (sqlite) vs PLC (latest snapshot) classification — pure so it is unit-testable.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffStatus {
    Same,
    LocalOnly,
    PlcOnly,
    Changed,
}

#[derive(Clone, Debug, Serialize)]
pub struct Diff<T> {
    pub id: u16,
    pub status: DiffStatus,
    pub local: Option<T>,
    pub plc: Option<T>,
}

/// Joins two id-keyed tables with a custom equality (entries carry timestamps / flags that must not count).
/// Output is sorted by id; duplicates within one side keep the last one.
pub fn classify_by<T: Clone>(local: &[(u16, T)], plc: &[(u16, T)], eq: impl Fn(&T, &T) -> bool) -> Vec<Diff<T>> {
    let mut ids: Vec<u16> = local.iter().chain(plc).map(|(id, _)| *id).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.into_iter()
        .map(|id| {
            let l = local.iter().rev().find(|(i, _)| *i == id).map(|(_, v)| v.clone());
            let p = plc.iter().rev().find(|(i, _)| *i == id).map(|(_, v)| v.clone());
            let status = match (&l, &p) {
                (Some(a), Some(b)) if eq(a, b) => DiffStatus::Same,
                (Some(_), Some(_)) => DiffStatus::Changed,
                (Some(_), None) => DiffStatus::LocalOnly,
                (None, Some(_)) => DiffStatus::PlcOnly,
                (None, None) => unreachable!("id came from one of the sides"),
            };
            Diff { id, status, local: l, plc: p }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify<T: PartialEq + Clone>(local: &[(u16, T)], plc: &[(u16, T)]) -> Vec<Diff<T>> {
        classify_by(local, plc, |a, b| a == b)
    }

    #[test]
    fn classifies_all_four_states_sorted() {
        let local = vec![(3u16, "c"), (1, "a"), (2, "b-local")];
        let plc = vec![(2u16, "b-plc"), (1, "a"), (4, "d")];
        let d = classify(&local, &plc);
        let got: Vec<(u16, DiffStatus)> = d.iter().map(|x| (x.id, x.status)).collect();
        assert_eq!(got, vec![(1, DiffStatus::Same), (2, DiffStatus::Changed), (3, DiffStatus::LocalOnly), (4, DiffStatus::PlcOnly)]);
        assert_eq!(d[1].local, Some("b-local"));
        assert_eq!(d[1].plc, Some("b-plc"));
        assert_eq!(d[2].plc, None);
        assert_eq!(d[3].local, None);
    }

    #[test]
    fn custom_equality_ignores_metadata() {
        let local = vec![(1u16, ("a", 10))];
        let plc = vec![(1u16, ("a", 99))];
        assert_eq!(classify(&local, &plc)[0].status, DiffStatus::Changed);
        assert_eq!(classify_by(&local, &plc, |x, y| x.0 == y.0)[0].status, DiffStatus::Same);
    }

    #[test]
    fn empty_sides() {
        assert!(classify::<u8>(&[], &[]).is_empty());
        let d = classify(&[], &[(5u16, 1u8)]);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].status, DiffStatus::PlcOnly);
    }
}
