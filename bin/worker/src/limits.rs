//! Per-kind concurrency limits (docs/design.md §6 "Job poller": per-type limits).

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// How many jobs of a kind this worker runs at once. Kinds without a limit are bounded only by
/// the worker's slots.
#[derive(Clone, Default)]
pub struct KindLimits {
    limits: HashMap<&'static str, Arc<Semaphore>>,
}

/// Permits held while a slot claims: one per limited kind the slot may still take.
pub struct Reserved {
    permits: Vec<(&'static str, OwnedSemaphorePermit)>,
}

impl KindLimits {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn limit(mut self, kind: &'static str, max: usize) -> Self {
        self.limits.insert(kind, Arc::new(Semaphore::new(max)));
        self
    }

    /// Takes a permit for every limited kind that has one free. The slot then claims only
    /// `reserved.kinds(all)`, so it can never claim a job it has no room to run.
    pub fn reserve(&self) -> Reserved {
        let permits = self
            .limits
            .iter()
            .filter_map(|(kind, semaphore)| {
                semaphore
                    .clone()
                    .try_acquire_owned()
                    .ok()
                    .map(|permit| (*kind, permit))
            })
            .collect();
        Reserved { permits }
    }

    fn is_limited(&self, kind: &str) -> bool {
        self.limits.contains_key(kind)
    }
}

impl Reserved {
    /// The kinds from `all` this slot may claim: every unlimited one, plus limited ones it
    /// holds a permit for.
    pub fn kinds(&self, limits: &KindLimits, all: &[&'static str]) -> Vec<&'static str> {
        all.iter()
            .copied()
            .filter(|kind| {
                !limits.is_limited(kind) || self.permits.iter().any(|(held, _)| held == kind)
            })
            .collect()
    }

    /// Keeps the permit for the claimed `kind` (until the returned value drops, when the job
    /// ends) and frees the rest.
    pub fn keep(self, kind: &str) -> Option<OwnedSemaphorePermit> {
        self.permits
            .into_iter()
            .find(|(held, _)| *held == kind)
            .map(|(_, permit)| permit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAVY: &str = "Heavy";
    const LIGHT: &str = "Light";

    #[test]
    fn a_full_limited_kind_is_left_for_other_slots() {
        let limits = KindLimits::new().limit(HEAVY, 1);
        let all = [HEAVY, LIGHT];

        let first = limits.reserve();
        assert_eq!(first.kinds(&limits, &all), vec![HEAVY, LIGHT]);

        // While the first slot holds the only permit, the second may claim only LIGHT.
        let second = limits.reserve();
        assert_eq!(second.kinds(&limits, &all), vec![LIGHT]);

        // The first slot claimed a HEAVY job: it keeps the permit for the job's duration.
        let running = first.keep(HEAVY);
        assert!(running.is_some());
        assert_eq!(limits.reserve().kinds(&limits, &all), vec![LIGHT]);

        // The job ends: HEAVY is claimable again.
        drop(running);
        assert_eq!(limits.reserve().kinds(&limits, &all), vec![HEAVY, LIGHT]);
    }

    #[test]
    fn claiming_an_unlimited_kind_releases_the_reservation() {
        let limits = KindLimits::new().limit(HEAVY, 1);
        let reserved = limits.reserve();
        assert!(reserved.keep(LIGHT).is_none());
        assert_eq!(
            limits.reserve().kinds(&limits, &[HEAVY, LIGHT]),
            vec![HEAVY, LIGHT]
        );
    }
}
