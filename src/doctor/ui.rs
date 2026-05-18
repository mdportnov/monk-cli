use std::time::Duration;

use super::{Check, Status};

impl Status {
    pub fn icon(self) -> &'static str {
        match self {
            Status::Ok => "✓",
            Status::Warn => "!",
            Status::Fail => "✗",
            Status::Info => "·",
            Status::Skipped => "—",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
            Status::Info => "info",
            Status::Skipped => "skip",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Report {
    pub checks: Vec<Check>,
    #[serde(serialize_with = "ser_duration_ms", rename = "duration_ms")]
    pub duration: Duration,
}

fn ser_duration_ms<S: serde::Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u128(d.as_millis())
}

impl Report {
    pub fn summary(&self) -> (usize, usize, usize) {
        let mut ok = 0;
        let mut warn = 0;
        let mut fail = 0;
        for c in &self.checks {
            match c.status {
                Status::Ok => ok += 1,
                Status::Warn => warn += 1,
                Status::Fail => fail += 1,
                _ => {}
            }
        }
        (ok, warn, fail)
    }

    pub fn has_failures(&self) -> bool {
        self.checks.iter().any(|c| c.status == Status::Fail)
    }

    /// Indices into `checks`, sorted by severity (Fail → Warn → Ok → Info → Skipped),
    /// preserving original order within the same severity.
    pub fn display_order(&self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.checks.len()).collect();
        idx.sort_by_key(|&i| (self.checks[i].status.severity_rank(), i));
        idx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &'static str, s: Status) -> Check {
        Check::new(id, id, s, "")
    }

    #[test]
    fn summary_counts_ok_warn_fail() {
        let r = Report {
            checks: vec![
                c("a", Status::Ok),
                c("b", Status::Ok),
                c("c", Status::Warn),
                c("d", Status::Fail),
                c("e", Status::Info),
                c("f", Status::Skipped),
            ],
            duration: Duration::ZERO,
        };
        assert_eq!(r.summary(), (2, 1, 1));
        assert!(r.has_failures());
    }

    #[test]
    fn display_order_sorts_by_severity_stable() {
        let r = Report {
            checks: vec![
                c("a", Status::Ok),
                c("b", Status::Fail),
                c("c", Status::Ok),
                c("d", Status::Warn),
                c("e", Status::Fail),
                c("f", Status::Info),
            ],
            duration: Duration::ZERO,
        };
        let order = r.display_order();
        let ids: Vec<&str> = order.iter().map(|&i| r.checks[i].id).collect();
        assert_eq!(ids, vec!["b", "e", "d", "a", "c", "f"]);
    }
}