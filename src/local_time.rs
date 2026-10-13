//! Local rendering of UTC evidence seconds. No model dates, mtime or clock substitution.
use crate::process::run_with_timeout;
use std::collections::BTreeMap;
use std::fmt::Write;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct LocalTimes {
    values: BTreeMap<u64, String>,
}
impl LocalTimes {
    pub fn new(times: impl IntoIterator<Item = u64>) -> Self {
        static CACHE: OnceLock<Mutex<BTreeMap<(String, u64), String>>> = OnceLock::new();
        let zone = std::env::var("TZ").unwrap_or_default();
        let Ok(mut cache) = CACHE.get_or_init(|| Mutex::new(BTreeMap::new())).lock() else {
            return Self::default();
        };
        let mut requested: Vec<_> = times.into_iter().collect();
        requested.sort_unstable();
        requested.dedup();
        let missing: Vec<_> = requested
            .iter()
            .copied()
            .filter(|t| !cache.contains_key(&(zone.clone(), *t)))
            .collect();
        for (t, value) in format_in_zone(&missing, &zone) {
            cache.insert((zone.clone(), t), value);
        }
        Self {
            values: requested
                .into_iter()
                .filter_map(|t| cache.get(&(zone.clone(), t)).cloned().map(|v| (t, v)))
                .collect(),
        }
    }
    pub fn day(&self, at: Option<u64>) -> Option<String> {
        self.values
            .get(&at?)
            .and_then(|value| value.get(..10))
            .map(|day| day.replace('/', "-"))
    }
    pub fn display(&self, at: Option<u64>) -> String {
        at.map_or_else(
            || crate::tr!("시각 미확인", "Time unknown").into(),
            |t| {
                self.values
                    .get(&t)
                    .cloned()
                    .unwrap_or_else(|| crate::tr!("UTC epoch {t} · 로컬 시각 변환 불가", "UTC epoch {t} · Local time unavailable";))
            },
        )
    }

    /// Shorten only cached local times whose date is the displayed day.
    /// Unknown dates and conversion failures keep their complete explanation.
    pub fn compact(&self, at: Option<u64>, today: Option<&str>) -> String {
        if today.is_some()
            && self.day(at).as_deref() == today
            && let Some(value) = at.and_then(|at| self.values.get(&at))
            && let Some((_, clock)) = value.split_once(' ')
        {
            return clock.to_string();
        }
        self.display(at)
    }
}

fn format_in_zone(times: &[u64], zone: &str) -> BTreeMap<u64, String> {
    if times.is_empty() {
        return BTreeMap::new();
    }
    let mut command = Command::new("date");
    if !zone.is_empty() {
        command.env("TZ", zone);
    }
    #[cfg(not(target_os = "macos"))]
    {
        command.args(["--file=-", "+%Y/%m/%d %H:%M"]);
        let input = times
            .iter()
            .fold(String::new(), |mut output, t| {
                let _ = writeln!(output, "@{t}");
                output
            })
            .into_bytes();
        let Ok(result) = run_with_timeout(
            &mut command,
            Some(input),
            Duration::from_secs(3),
            1024 * 1024,
        ) else {
            return BTreeMap::new();
        };
        if !result.success {
            return BTreeMap::new();
        }
        times
            .iter()
            .copied()
            .zip(result.stdout.lines().map(String::from))
            .collect()
    }
    #[cfg(target_os = "macos")]
    {
        times
            .iter()
            .filter_map(|t| {
                let mut command = Command::new("date");
                command.args(["-r", &t.to_string(), "+%Y/%m/%d %H:%M"]);
                if !zone.is_empty() {
                    command.env("TZ", zone);
                }
                let result =
                    run_with_timeout(&mut command, None, Duration::from_secs(3), 1024).ok()?;
                result
                    .success
                    .then(|| (*t, result.stdout.trim().to_string()))
            })
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_time_only_shortens_the_cached_display_day() {
        let times = LocalTimes {
            values: BTreeMap::from([
                (1, "2026/10/12 09:47".into()),
                (2, "2025/10/12 09:47".into()),
            ]),
        };
        assert_eq!(times.compact(Some(1), Some("2026-10-12")), "09:47");
        assert_eq!(
            times.compact(Some(2), Some("2026-10-12")),
            "2025/10/12 09:47"
        );
        assert_eq!(times.compact(Some(1), None), "2026/10/12 09:47");
        assert_eq!(
            times.compact(None, Some("2026-10-12")),
            crate::tr!("시각 미확인", "Time unknown")
        );
        assert_eq!(
            times.compact(Some(3), Some("2026-10-12")),
            crate::tr!(
                "UTC epoch 3 · 로컬 시각 변환 불가",
                "UTC epoch 3 · Local time unavailable"
            )
        );
    }
    #[test]
    fn zones_and_dst_are_rendered_from_each_source_timestamp() {
        let at = crate::transcript::utc_timestamp_seconds("2026-09-14T01:40:00Z").unwrap();
        assert_eq!(format_in_zone(&[at], "Asia/Seoul")[&at], "2026/09/14 10:40");
        assert_eq!(
            format_in_zone(&[at], "America/Los_Angeles")[&at],
            "2026/09/13 18:40"
        );
        let winter = crate::transcript::utc_timestamp_seconds("2026-01-14T01:40:00Z").unwrap();
        assert_eq!(
            format_in_zone(&[winter], "America/Los_Angeles")[&winter],
            "2026/01/13 17:40"
        );
        assert_eq!(
            LocalTimes::default().display(None),
            crate::tr!("시각 미확인", "Time unknown")
        );
    }
}
