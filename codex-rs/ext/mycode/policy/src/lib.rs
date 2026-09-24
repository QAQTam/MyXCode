//! Fork-level product policy for MyCode.
//!
//! Upstream resolves several telemetry and commercial defaults inline, at the
//! call site that consumes them. MyCode pins every one of those decisions here
//! so a single file describes what a MyCode build does, and so syncing an
//! upstream patch never requires re-auditing each call site.
//!
//! # Why constants instead of `#[cfg]`
//!
//! Every guarded call site early-returns when [`BLOCK_COMMERCIAL_TELEMETRY`] is
//! `true`. Because that value is a `const`, the compiler removes the guarded
//! branch, so the upstream endpoints, credentials, and DSNs never reach the
//! shipped binary even though the upstream source stays in the tree and keeps
//! accepting upstream patches. Using a constant rather than a `--cfg` flag also
//! keeps the repository free of build-system plumbing, so `just test` still
//! compiles and runs the code paths upstream tests expect.
//!
//! A constant cannot guarantee elimination across rustc versions, so the release
//! pipeline also greps the packaged binary for the blocked strings.

/// Master switch for commercial telemetry egress.
///
/// When `true`, every guarded call site returns before performing network work:
///
/// - Statsig metrics export (`codex-otel`),
/// - analytics event delivery (`codex-analytics`),
/// - feedback uploads to Sentry (`codex-feedback`),
/// - startup update probes (`codex-tui`, `codex-cli`),
/// - announcement tip fetches (`codex-tui`),
/// - built-in pet asset downloads (`codex-tui`).
///
/// Flip this to `false` to restore upstream behavior; the derived defaults below
/// follow automatically.
pub const BLOCK_COMMERCIAL_TELEMETRY: bool = true;

/// Default for `check_for_update_on_startup` when the user has not configured it.
pub const CHECK_FOR_UPDATE_ON_STARTUP_BY_DEFAULT: bool = !BLOCK_COMMERCIAL_TELEMETRY;

/// Whether the built-in OTEL metrics exporter is enabled by default.
///
/// Upstream defaults `otel.metrics_exporter` to the Statsig ingestion route.
pub const OTEL_METRICS_ENABLED_BY_DEFAULT: bool = !BLOCK_COMMERCIAL_TELEMETRY;
