# jx-spotify Project Brief

## Summary

`spotify_player` is a terminal Spotify client with Spotify Connect, streaming, playback control, search, library browsing, lyrics, notifications, and configurable keymaps.

## Role In The System

- Provides the music and playback control surface in the Maeve workspace.
- Uses config and cache folders for user state, logs, credentials, and audio-related artifacts.
- Serves as the playback-oriented counterpart to the structured-data shell in `jx-twig`.

## Current Focus

- Keep the bounded-shell layout, popup behavior, and shortcut grammar aligned with the documented UI direction.
- Preserve the documented command/help structure and config/theme split.
- Use `docs/planning/current_status.md` as the live breadcrumb for verification and next-step tracking.

## Boundary

- This repo should remain a terminal player and control surface, not a general task or document system.
- `jx-glow` handoff support and cross-project UI conventions are shared boundaries, not invitations to widen the repo's scope.
