# jx-spotify

## Scope And Authority

- Treat this repository as locally authoritative for `spotify_player` code, tests, and project documentation.
- Use root Maeve guidance only for cross-project coordination or when local guidance is absent.
- Follow `README.md`, `docs/config.md`, `THEMES.md`, and `docs/planning/current_status.md` first when deciding what to change.

## Defaults

- Keep changes surgical and consistent with the terminal-player UI, command grammar, and configuration model already documented here.
- Prefer the existing PTY, unit-test, and config-driven workflows over introducing new project patterns.
- Update the planning breadcrumb when the implementation state materially changes.

## Safety

- Ask before destructive actions, dependency installs, environment changes, or any operation that would touch live Spotify state.
- Treat credentials, cache files, and logs as sensitive; redact tokens and private user data in logs, plans, and summaries.
- Prefer read-only inspection or dry-run style validation unless the task explicitly requires mutation.

## Cross-Project Boundary

- Keep keyboard grammar, shell geometry, and handoff conventions compatible with `jx-glow` and `jx-twig`, but do not modify those repos from here.
- Preserve the `spotify_player` package name, binary name, and documented command surface unless a user-facing change is intended.
