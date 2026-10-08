//! The edit decision list (docs/design.md §8 `edits`, §18 "EDL maths"): the ranges of the original
//! recording that are kept, in order. Everything not listed is cut. Pure: no I/O.
//!
//! The list is stored and exchanged as `[[start_ms, end_ms], ...]`. [`Edl::new`] is the only way to
//! make one, so every `Edl` is sorted, non-overlapping, inside the recording, and has no range
//! shorter than [`MIN_RANGE_MS`].

use serde::{Deserialize, Serialize};

/// Shortest range kept: anything smaller is a click, not an edit, and FFmpeg cannot cut it cleanly.
pub const MIN_RANGE_MS: u32 = 100;

/// Most ranges one edit may keep (a trim plus 99 cuts).
pub const MAX_RANGES: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EdlError {
    #[error("the recording has no known duration to edit against")]
    NoDuration,

    #[error("an edit must keep at least one range")]
    NoRanges,

    #[error("an edit keeps at most {MAX_RANGES} ranges")]
    TooMany,

    #[error("range {index} ends at or before it starts")]
    Empty { index: usize },

    #[error("range {index} is shorter than {MIN_RANGE_MS} ms")]
    TooShort { index: usize },

    #[error("range {index} ends after the recording does ({duration_ms} ms)")]
    PastEnd { index: usize, duration_ms: u32 },

    #[error("ranges {first} and {second} overlap")]
    Overlap { first: usize, second: usize },
}

/// A kept stretch of the original, `start_ms` inclusive to `end_ms` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeptRange {
    pub start_ms: u32,
    pub end_ms: u32,
}

impl KeptRange {
    pub fn len_ms(self) -> u32 {
        self.end_ms - self.start_ms
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edl {
    ranges: Vec<KeptRange>,
    source_duration_ms: u32,
}

impl Edl {
    /// Validates `ranges` (as `[start_ms, end_ms]` pairs, in any order) against a recording of
    /// `source_duration_ms`. Ranges that touch (one ends where the next starts) are joined;
    /// ranges that overlap are an error. Indexes in errors are positions in the input.
    pub fn new(ranges: &[[u32; 2]], source_duration_ms: u32) -> Result<Self, EdlError> {
        if source_duration_ms == 0 {
            return Err(EdlError::NoDuration);
        }
        if ranges.is_empty() {
            return Err(EdlError::NoRanges);
        }
        if ranges.len() > MAX_RANGES {
            return Err(EdlError::TooMany);
        }
        for (index, [start_ms, end_ms]) in ranges.iter().copied().enumerate() {
            if end_ms <= start_ms {
                return Err(EdlError::Empty { index });
            }
            if end_ms - start_ms < MIN_RANGE_MS {
                return Err(EdlError::TooShort { index });
            }
            if end_ms > source_duration_ms {
                return Err(EdlError::PastEnd {
                    index,
                    duration_ms: source_duration_ms,
                });
            }
        }
        let mut order: Vec<usize> = (0..ranges.len()).collect();
        order.sort_by_key(|&index| (ranges[index][0], ranges[index][1]));
        let mut kept: Vec<KeptRange> = Vec::with_capacity(ranges.len());
        let mut previous: Option<usize> = None;
        for index in order {
            let [start_ms, end_ms] = ranges[index];
            if let (Some(last), Some(before)) = (kept.last_mut(), previous) {
                if start_ms < last.end_ms {
                    return Err(EdlError::Overlap {
                        first: before.min(index),
                        second: before.max(index),
                    });
                }
                if start_ms == last.end_ms {
                    last.end_ms = end_ms;
                    previous = Some(index);
                    continue;
                }
            }
            kept.push(KeptRange { start_ms, end_ms });
            previous = Some(index);
        }
        Ok(Self {
            ranges: kept,
            source_duration_ms,
        })
    }

    /// Reads the stored form (`[[start, end], ...]`) and validates it.
    pub fn from_json(
        value: &serde_json::Value,
        source_duration_ms: u32,
    ) -> Result<Self, EdlJsonError> {
        let pairs: Vec<[u32; 2]> =
            serde_json::from_value(value.clone()).map_err(|_| EdlJsonError::Malformed)?;
        Ok(Self::new(&pairs, source_duration_ms)?)
    }

    /// The stored form: `[[start, end], ...]` in order.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::Value::Array(
            self.ranges
                .iter()
                .map(|range| serde_json::json!([range.start_ms, range.end_ms]))
                .collect(),
        )
    }

    /// The kept ranges: sorted, with a gap between each.
    pub fn ranges(&self) -> &[KeptRange] {
        &self.ranges
    }

    pub fn source_duration_ms(&self) -> u32 {
        self.source_duration_ms
    }

    /// How long the edited recording is: the sum of the kept ranges.
    pub fn kept_ms(&self) -> u32 {
        self.ranges.iter().map(|range| range.len_ms()).sum()
    }

    /// What is removed: the gaps before, between and after the kept ranges.
    pub fn cuts(&self) -> Vec<KeptRange> {
        let mut cuts = Vec::with_capacity(self.ranges.len() + 1);
        let mut cursor = 0;
        for range in &self.ranges {
            if range.start_ms > cursor {
                cuts.push(KeptRange {
                    start_ms: cursor,
                    end_ms: range.start_ms,
                });
            }
            cursor = range.end_ms;
        }
        if cursor < self.source_duration_ms {
            cuts.push(KeptRange {
                start_ms: cursor,
                end_ms: self.source_duration_ms,
            });
        }
        cuts
    }

    /// Whether the edit keeps the whole recording (it changes nothing).
    pub fn is_identity(&self) -> bool {
        matches!(self.ranges.as_slice(), [only] if only.start_ms == 0 && only.end_ms == self.source_duration_ms)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EdlJsonError {
    #[error("an edit is a list of [start_ms, end_ms] pairs")]
    Malformed,

    #[error(transparent)]
    Invalid(#[from] EdlError),
}

/// The wire form of an EDL, for request and response bodies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdlRanges(pub Vec<[u32; 2]>);

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn r(start_ms: u32, end_ms: u32) -> KeptRange {
        KeptRange { start_ms, end_ms }
    }

    #[test]
    fn a_trim_and_two_cuts_keep_three_ranges() {
        let edl = Edl::new(
            &[[2_000, 10_000], [12_000, 20_000], [25_000, 30_000]],
            40_000,
        )
        .expect("valid");
        assert_eq!(edl.kept_ms(), 8_000 + 8_000 + 5_000);
        assert_eq!(
            edl.cuts(),
            [
                r(0, 2_000),
                r(10_000, 12_000),
                r(20_000, 25_000),
                r(30_000, 40_000)
            ]
        );
        assert!(!edl.is_identity());
    }

    #[test]
    fn input_order_does_not_matter_and_touching_ranges_join() {
        let edl = Edl::new(&[[5_000, 8_000], [0, 5_000], [9_000, 10_000]], 10_000).expect("valid");
        assert_eq!(edl.ranges(), [r(0, 8_000), r(9_000, 10_000)]);
        assert_eq!(Edl::new(&[[0, 10_000]], 10_000).expect("valid").cuts(), []);
        assert!(
            Edl::new(&[[0, 10_000]], 10_000)
                .expect("valid")
                .is_identity()
        );
    }

    #[test]
    fn the_obvious_mistakes_are_named() {
        let d = 10_000;
        assert_eq!(Edl::new(&[[0, 1_000]], 0), Err(EdlError::NoDuration));
        assert_eq!(Edl::new(&[], d), Err(EdlError::NoRanges));
        assert_eq!(
            Edl::new(&[[5_000, 5_000]], d),
            Err(EdlError::Empty { index: 0 })
        );
        assert_eq!(
            Edl::new(&[[0, 1_000], [6_000, 5_000]], d),
            Err(EdlError::Empty { index: 1 })
        );
        assert_eq!(
            Edl::new(&[[0, 99]], d),
            Err(EdlError::TooShort { index: 0 })
        );
        assert_eq!(
            Edl::new(&[[9_000, 10_001]], d),
            Err(EdlError::PastEnd {
                index: 0,
                duration_ms: d
            })
        );
        assert_eq!(
            Edl::new(&[[0, 5_000], [4_999, 8_000]], d),
            Err(EdlError::Overlap {
                first: 0,
                second: 1
            })
        );
        // Contained, and reported by input position even when given out of order.
        assert_eq!(
            Edl::new(&[[6_000, 7_000], [1_000, 9_000]], d),
            Err(EdlError::Overlap {
                first: 0,
                second: 1
            })
        );
        let many: Vec<[u32; 2]> = (0..=MAX_RANGES as u32)
            .map(|i| [i * 200, i * 200 + 100])
            .collect();
        assert_eq!(Edl::new(&many, 1_000_000), Err(EdlError::TooMany));
    }

    #[test]
    fn it_round_trips_through_the_stored_form() {
        let edl = Edl::new(&[[0, 3_000], [4_000, 9_000]], 10_000).expect("valid");
        assert_eq!(edl.to_json(), serde_json::json!([[0, 3000], [4000, 9000]]));
        assert_eq!(Edl::from_json(&edl.to_json(), 10_000), Ok(edl));
        for bad in [
            serde_json::json!({"a": 1}),
            serde_json::json!([[1, 2, 3]]),
            serde_json::json!([[-1, 5]]),
            serde_json::json!("x"),
        ] {
            assert_eq!(Edl::from_json(&bad, 10_000), Err(EdlJsonError::Malformed));
        }
        assert_eq!(
            Edl::from_json(&serde_json::json!([]), 10_000),
            Err(EdlJsonError::Invalid(EdlError::NoRanges))
        );
    }

    /// An independent statement of "valid", written without the sorting and joining `Edl::new`
    /// does, so the property tests compare two different implementations.
    fn oracle_accepts(ranges: &[[u32; 2]], duration: u32) -> bool {
        if duration == 0 || ranges.is_empty() || ranges.len() > MAX_RANGES {
            return false;
        }
        let each_ok = ranges
            .iter()
            .all(|&[s, e]| e > s && e - s >= MIN_RANGE_MS && e <= duration);
        let none_overlap = ranges.iter().enumerate().all(|(i, a)| {
            ranges
                .iter()
                .skip(i + 1)
                .all(|b| !(a[0] < b[1] && b[0] < a[1]))
        });
        each_ok && none_overlap
    }

    /// Ranges built to be valid: a gap, then a range, repeated; the duration is past the last.
    fn valid_edl() -> impl Strategy<Value = (Vec<[u32; 2]>, u32)> {
        (
            prop::collection::vec((1u32..5_000, MIN_RANGE_MS..5_000), 1..30),
            0u32..5_000,
        )
            .prop_map(|(steps, tail)| {
                let mut cursor = 0;
                let mut ranges = Vec::new();
                for (gap, len) in steps {
                    let start = cursor + gap;
                    ranges.push([start, start + len]);
                    cursor = start + len;
                }
                (ranges, cursor + tail + 1)
            })
    }

    proptest! {
        #[test]
        fn any_accepted_edl_is_sorted_gapped_inside_and_long_enough(
            ranges in prop::collection::vec((0u32..20_000, 0u32..20_000), 0..12),
            duration in 0u32..20_000,
        ) {
            let pairs: Vec<[u32; 2]> = ranges.into_iter().map(|(a, b)| [a, b]).collect();
            let result = Edl::new(&pairs, duration);
            prop_assert_eq!(result.is_ok(), oracle_accepts(&pairs, duration));
            if let Ok(edl) = result {
                let kept = edl.ranges();
                prop_assert!(!kept.is_empty() && kept.len() <= MAX_RANGES);
                for range in kept {
                    prop_assert!(range.start_ms < range.end_ms);
                    prop_assert!(range.len_ms() >= MIN_RANGE_MS);
                    prop_assert!(range.end_ms <= duration);
                }
                for pair in kept.windows(2) {
                    // Strictly after: touching ranges were joined, overlapping ones refused.
                    prop_assert!(pair[0].end_ms < pair[1].start_ms);
                }
            }
        }

        #[test]
        fn overlapping_ranges_are_always_refused(
            (ranges, duration) in valid_edl(),
            pick in any::<prop::sample::Index>(),
            shift in any::<prop::sample::Index>(),
        ) {
            // Add a range that lies inside one that is already kept: they share milliseconds.
            let i = pick.index(ranges.len());
            let [start, end] = ranges[i];
            let offset = shift.index((end - start - MIN_RANGE_MS) as usize + 1) as u32;
            let mut bent = ranges.clone();
            bent.push([start + offset, start + offset + MIN_RANGE_MS]);
            prop_assert!(!oracle_accepts(&bent, duration));
            let overlapping = matches!(Edl::new(&bent, duration), Err(EdlError::Overlap { .. }));
            prop_assert!(overlapping, "{:?} in {}", bent, duration);
        }

        #[test]
        fn a_range_past_the_end_or_not_forward_is_always_refused(
            (ranges, duration) in valid_edl(),
            pick in any::<prop::sample::Index>(),
        ) {
            let i = pick.index(ranges.len());
            let mut past = ranges.clone();
            past[i][1] = duration + 1;
            prop_assert!(Edl::new(&past, duration).is_err());
            let mut backwards = ranges.clone();
            backwards[i] = [ranges[i][1], ranges[i][0]];
            let named = Edl::new(&backwards, duration) == Err(EdlError::Empty { index: i });
            prop_assert!(named);
        }

        #[test]
        fn valid_edits_keep_and_cut_the_whole_recording(
            (ranges, duration) in valid_edl(),
            seed in any::<u64>(),
        ) {
            let edl = Edl::new(&ranges, duration).expect("built valid");
            prop_assert_eq!(edl.ranges().len(), ranges.len());
            let cut_ms: u32 = edl.cuts().iter().map(|c| c.len_ms()).sum();
            prop_assert_eq!(edl.kept_ms() + cut_ms, duration);
            // Cuts and kept ranges never share a millisecond.
            for cut in edl.cuts() {
                for kept in edl.ranges() {
                    prop_assert!(cut.end_ms <= kept.start_ms || kept.end_ms <= cut.start_ms);
                }
            }
            // Any order of the same ranges gives the same edit; so does the stored form.
            let mut shuffled = ranges.clone();
            let n = shuffled.len();
            for k in 0..n {
                shuffled.swap(k, ((seed >> (k % 32)) as usize + k) % n);
            }
            let reordered = Edl::new(&shuffled, duration);
            prop_assert_eq!(reordered, Ok(edl.clone()));
            prop_assert_eq!(Edl::from_json(&edl.to_json(), duration), Ok(edl));
        }
    }
}
