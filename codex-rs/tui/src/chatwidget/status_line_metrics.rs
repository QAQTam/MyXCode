//! Runtime metrics rendered in the compact status line.

use std::time::Duration;
use std::time::Instant;

use crate::token_usage::TokenUsageInfo;

/// Tracks the small amount of turn-local state needed for compact runtime metrics.
#[derive(Debug, Default)]
pub(super) struct StatusLineMetrics {
    turn_started_at: Option<Instant>,
    response_started_at: Option<Instant>,
    turn_start_output_tokens: i64,
    turn_output_tokens: i64,
    last_turn_duration: Option<Duration>,
    last_output_tokens_per_second: Option<f64>,
}

impl StatusLineMetrics {
    pub(super) fn start_turn(&mut self, token_info: Option<&TokenUsageInfo>) {
        self.turn_started_at = Some(Instant::now());
        self.response_started_at = None;
        self.turn_start_output_tokens = token_info
            .map(|info| info.total_token_usage.output_tokens)
            .unwrap_or(0);
        self.turn_output_tokens = 0;
        self.last_turn_duration = None;
        self.last_output_tokens_per_second = None;
    }

    pub(super) fn observe_model_delta(&mut self) {
        if self.turn_started_at.is_some() && self.response_started_at.is_none() {
            self.response_started_at = Some(Instant::now());
        }
    }

    pub(super) fn observe_usage(&mut self, info: &TokenUsageInfo) {
        if self.turn_started_at.is_none() {
            return;
        }
        self.turn_output_tokens = info
            .total_token_usage
            .output_tokens
            .saturating_sub(self.turn_start_output_tokens)
            .max(0);
        if let Some(response_started_at) = self.response_started_at.take() {
            self.last_output_tokens_per_second = output_tokens_per_second(
                info.last_token_usage.output_tokens,
                response_started_at.elapsed(),
            );
        }
    }

    pub(super) fn finish_turn(&mut self, duration_ms: Option<i64>) {
        let duration = duration_ms
            .and_then(|duration_ms| u64::try_from(duration_ms).ok())
            .map(Duration::from_millis)
            .or_else(|| self.turn_started_at.map(|started_at| started_at.elapsed()));

        if let Some(duration) = duration {
            self.last_turn_duration = Some(duration);
            self.last_output_tokens_per_second = self
                .last_output_tokens_per_second
                .or_else(|| output_tokens_per_second(self.turn_output_tokens, duration));
        }

        self.turn_started_at = None;
        self.response_started_at = None;
        self.turn_start_output_tokens = 0;
        self.turn_output_tokens = 0;
    }

    fn duration(&self) -> Option<Duration> {
        self.turn_started_at
            .map(|started_at| started_at.elapsed())
            .or(self.last_turn_duration)
    }

    fn output_tokens_per_second(&self) -> Option<f64> {
        self.last_output_tokens_per_second
    }
}

impl super::ChatWidget {
    pub(super) fn status_line_runtime_metrics_value(&self) -> Option<String> {
        format_runtime_metrics(
            self.status_line_cache_hit_rate(),
            self.status_line_context_used_percent(),
            self.status_line_metrics.duration(),
            self.status_line_metrics.output_tokens_per_second(),
        )
    }

    fn status_line_cache_hit_rate(&self) -> Option<f64> {
        let usage = &self.token_info.as_ref()?.last_token_usage;
        let input_tokens = usage.input_tokens.max(0);
        if input_tokens == 0 {
            return None;
        }
        Some(
            (usage.cached_input_tokens.max(0) as f64 / input_tokens as f64 * 100.0)
                .clamp(0.0, 100.0),
        )
    }
}

fn format_runtime_metrics(
    cache_hit_rate: Option<f64>,
    context_used_percent: Option<i64>,
    duration: Option<Duration>,
    output_tokens_per_second: Option<f64>,
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(cache_hit_rate) = cache_hit_rate {
        parts.push(format!("cache {cache_hit_rate:.1}%"));
    }
    if let Some(context_used_percent) = context_used_percent {
        parts.push(format!("ctx {context_used_percent}%"));
    }
    if let Some(duration) = duration {
        parts.push(format_duration(duration));
    }
    if let Some(output_tokens_per_second) = output_tokens_per_second {
        parts.push(format!("{output_tokens_per_second:.1} tok/s"));
    }

    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn output_tokens_per_second(output_tokens: i64, duration: Duration) -> Option<f64> {
    let seconds = duration.as_secs_f64();
    if output_tokens <= 0 || !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    Some(output_tokens as f64 / seconds)
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs_f64();
    if seconds < 10.0 {
        format!("{seconds:.1}s")
    } else if seconds < 60.0 {
        format!("{seconds:.0}s")
    } else {
        let total_seconds = duration.as_secs();
        format!("{}m{:02}s", total_seconds / 60, total_seconds % 60)
    }
}

#[cfg(test)]
#[path = "status_line_metrics_tests.rs"]
mod tests;
