#!/usr/bin/env python3
"""Report which commercial telemetry payloads a MyCode binary carries.

`codex-mycode-policy` blocks telemetry with `const`s that make the guarded
branches unreachable, and `const if` expressions that never materialize the
upstream URL. Both rely on the compiler removing the dead code, which is what
this script checks against a *built* binary rather than against the source.

Usage:
    python3 scripts/mycode-verify-telemetry-blocked.py <binary> [<binary> ...]

Exits non-zero when a payload listed in `BLOCKED_PAYLOADS` is present. Payloads
listed in `DEFERRED_PAYLOADS` are only reported: they are behaviorally blocked
by default but their upstream code still has to be removed, not just gated.
"""

from __future__ import annotations

import sys
from pathlib import Path

# Commercial endpoints, credentials, and DSNs that MyCode must not embed.
# Keep this list in sync with the guarded call sites described in
# `codex-rs/ext/mycode/policy/src/lib.rs`.
BLOCKED_PAYLOADS: tuple[tuple[str, str], ...] = (
    ("statsig endpoint", "ab.chatgpt.com"),
    ("statsig api key", "client-MkRuleRQBd6qakfnDYqJVR9JuXcY57Ljly3vi5JVUIO"),
    ("sentry dsn", "o33249.ingest.us.sentry.io"),
    (
        "announcement tip url",
        "raw.githubusercontent.com/openai/codex/main/announcement_tip.toml",
    ),
    ("github release probe", "api.github.com/repos/openai/codex/releases/latest"),
    ("homebrew cask probe", "formulae.brew.sh/api/cask/codex.json"),
    ("desktop update cdn", "persistent.oaistatic.com"),
)

# Blocked by default configuration, but the upstream code is still compiled in.
# These are the follow-up "remove the code" targets.
DEFERRED_PAYLOADS: tuple[tuple[str, str], ...] = (
    ("analytics events path", "/codex/analytics-events/events"),
    ("curated plugins endpoint", "plugins/export/curated"),
    ("curated plugins git url", "github.com/openai/plugins.git"),
)


def scan(path: Path, payloads: tuple[tuple[str, str], ...]) -> list[str]:
    """Return one human-readable line per payload found in `path`."""
    try:
        blob = path.read_bytes()
    except OSError as err:
        return [f"{path}: cannot read binary: {err}"]

    return [
        f"{path}: found {label} ({needle!r})"
        for label, needle in payloads
        if needle.encode() in blob
    ]


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2

    binaries = [Path(raw) for raw in argv[1:]]
    missing = [path for path in binaries if not path.is_file()]
    if missing:
        for path in missing:
            print(f"{path}: not a file", file=sys.stderr)
        return 2

    failures = []
    deferred = []
    for path in binaries:
        failures.extend(scan(path, BLOCKED_PAYLOADS))
        deferred.extend(scan(path, DEFERRED_PAYLOADS))

    for line in deferred:
        print(f"warning: still compiled in: {line}", file=sys.stderr)

    if failures:
        print(
            "commercial telemetry payloads are present in the build:", file=sys.stderr
        )
        for line in failures:
            print(f"  {line}", file=sys.stderr)
        return 1

    print(f"ok: {len(binaries)} binary/binaries carry no blocked telemetry payloads")
    if deferred:
        print(f"note: {len(deferred)} deferred payload(s) are still compiled in")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
