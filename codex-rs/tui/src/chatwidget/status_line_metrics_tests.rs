use std::time::Duration;

use pretty_assertions::assert_eq;

use super::format_duration;
use super::format_runtime_metrics;

#[test]
fn formats_complete_runtime_metrics() {
    assert_eq!(
        format_runtime_metrics(
            Some(98.25),
            Some(3),
            Some(Duration::from_millis(3_240)),
            Some(54.12),
        ),
        Some("cache 98.2% · ctx 3% · 3.2s · 54.1 tok/s".to_string())
    );
}

#[test]
fn omits_unavailable_runtime_metrics() {
    assert_eq!(format_runtime_metrics(None, None, None, None), None);
}

#[test]
fn formats_long_durations_as_minutes_and_seconds() {
    assert_eq!(format_duration(Duration::from_secs(125)), "2m05s");
}
