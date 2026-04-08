use std::time::{Duration, Instant};

use once_cell::sync::Lazy;

static PROCESS_START: Lazy<Instant> = Lazy::new(Instant::now);
static BOOT_ID: Lazy<String> = Lazy::new(compute_boot_id);

pub const MAX_TICK_DELTA: Duration = Duration::from_secs(5);

pub fn boot_id() -> String {
    BOOT_ID.clone()
}

pub fn monotonic_ms() -> u128 {
    PROCESS_START.elapsed().as_millis()
}

pub fn bounded_delta(prev_ms: u128, now_ms: u128) -> Duration {
    if now_ms <= prev_ms {
        return Duration::ZERO;
    }
    let raw = Duration::from_millis(u64::try_from(now_ms - prev_ms).unwrap_or(u64::MAX));
    if raw > MAX_TICK_DELTA {
        MAX_TICK_DELTA
    } else {
        raw
    }
}

#[cfg(target_os = "linux")]
fn compute_boot_id() -> String {
    fs_err::read_to_string("/proc/sys/kernel/random/boot_id")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "linux-unknown".into())
}

#[cfg(target_os = "macos")]
fn compute_boot_id() -> String {
    use std::process::Command;
    Command::new("sysctl")
        .args(["-n", "kern.boottime"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| {
            let body = s.trim();
            let digest = blake3::hash(body.as_bytes());
            format!("macos-{}", hex::encode(&digest.as_bytes()[..8]))
        })
        .unwrap_or_else(|| "macos-unknown".into())
}

#[cfg(target_os = "windows")]
fn compute_boot_id() -> String {
    use std::time::SystemTime;
    let now_sys =
        SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_millis();
    let uptime = monotonic_ms();
    let boot_wall_approx = now_sys.saturating_sub(uptime);
    let digest = blake3::hash(boot_wall_approx.to_le_bytes().as_ref());
    format!("windows-{}", hex::encode(&digest.as_bytes()[..16]))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn compute_boot_id() -> String {
    "unknown".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_bounds_sleep() {
        assert_eq!(bounded_delta(0, 10_000), Duration::from_millis(5_000));
    }

    #[test]
    fn delta_handles_backward() {
        assert_eq!(bounded_delta(500, 400), Duration::ZERO);
    }

    #[test]
    fn boot_id_is_stable_within_process() {
        assert_eq!(boot_id(), boot_id());
    }
}
