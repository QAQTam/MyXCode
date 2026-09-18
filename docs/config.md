# Configuration

For basic configuration instructions, see [this documentation](https://developers.openai.com/codex/config-basic).

For advanced configuration instructions, see [this documentation](https://developers.openai.com/codex/config-advanced).

For a full configuration reference, see [this documentation](https://developers.openai.com/codex/config-reference).

## Authentication

Codex uses the first credential it finds:

1. `CODEX_API_KEY` in the environment. Explicit, and takes precedence over any
   stored login.
2. `CODEX_ACCESS_TOKEN` in the environment, or auth injected by a host
   application for the current process only.
3. A login stored in `$CODEX_HOME/auth.json`.
4. `OPENAI_API_KEY` in the environment, as a last resort.

Step 4 is the bring-your-own-key path: exporting `OPENAI_API_KEY` is enough to
get a working session, and nothing is written to `auth.json`. It deliberately
runs last so that a key exported for some other tool cannot silently move an
existing session onto API billing; export `CODEX_API_KEY` instead if you want to
override a stored login.

Set `use_env_api_key = false` to ignore both environment variables. A provider's
own `env_key` is a separate mechanism and is unaffected.

## Lifecycle hooks

Admins can set top-level `allow_managed_hooks_only = true` in
`requirements.toml` to ignore user, project, and session hook configs while
still allowing managed hooks from requirements and managed config layers. This
setting is only supported in `requirements.toml`; putting it in `config.toml`
does not enable managed-hooks-only mode.
