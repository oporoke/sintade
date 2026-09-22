use time::OffsetDateTime;

pub trait Clock: Send + Sync {
    fn now(&self) -> OffsetDateTime;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

pub struct FixedClock(pub OffsetDateTime);

impl Clock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_reports_utc_now() {
        let before = OffsetDateTime::now_utc();
        let now = SystemClock.now();
        let after = OffsetDateTime::now_utc();
        assert!(before <= now && now <= after);
    }

    #[test]
    fn fixed_clock_always_returns_same_instant() {
        let instant = OffsetDateTime::now_utc();
        let clock = FixedClock(instant);
        assert_eq!(clock.now(), instant);
    }
}
