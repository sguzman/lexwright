use std::time::Duration;

#[derive(Clone, Copy, Debug, Default)]
pub struct TimingMetric {
    count: u64,
    last_ns: u64,
    max_ns: u64,
    total_ns: u128,
}

impl TimingMetric {
    pub fn observe(&mut self, duration: Duration) {
        let nanos = duration.as_nanos().min(u64::MAX as u128) as u64;
        self.count = self.count.saturating_add(1);
        self.last_ns = nanos;
        self.max_ns = self.max_ns.max(nanos);
        self.total_ns = self.total_ns.saturating_add(nanos as u128);
    }

    pub fn count(self) -> u64 {
        self.count
    }

    pub fn last_ns(self) -> u64 {
        self.last_ns
    }

    pub fn max_ns(self) -> u64 {
        self.max_ns
    }

    pub fn average_ns(self) -> u64 {
        if self.count == 0 {
            return 0;
        }

        (self.total_ns / self.count as u128).min(u64::MAX as u128) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::TimingMetric;
    use std::time::Duration;

    #[test]
    fn records_last_average_and_max() {
        let mut metric = TimingMetric::default();
        metric.observe(Duration::from_nanos(10));
        metric.observe(Duration::from_nanos(30));

        assert_eq!(metric.count(), 2);
        assert_eq!(metric.last_ns(), 30);
        assert_eq!(metric.average_ns(), 20);
        assert_eq!(metric.max_ns(), 30);
    }
}
