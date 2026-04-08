# Current Status

- The help-splitting direction has been reviewed and scoped.
- Current recommendation: keep the global command help page, then add smaller contextual surfaces starting with SearchTui.
- Implemented slices: the app now carries a persistent four-corner shortcut frame, `?` routes to the global help page, playback metadata is more compact, album views drop redundant album columns, radio views use a lighter search-tui-like table, and the shortcut-family popup titles now cover `s` and `u`.
- Last verification: `cargo check --manifest-path /Users/jane/jxyz/maeve/projects/jx-spotify/spotify_player/Cargo.toml` passed after adding worksuite handoff plumbing.
- Next slice: visual validation in the live TUI and any follow-up spacing or accent cleanup that falls out of that pass.
- Recent related commit: `eb552eb` (`fix: move spotify badge to global header`).
- New restyle slice: compact now-playing footer, shorter playback window, album/radio table cleanup, global help family promotion, mouse-volume cleanup, and a footer mini-help preview on `?`.
- Mini-help preview update: the top-right family banner is gone, `?` now reveals a one-line footer preview first, and a second `?` falls through to full help.
- Footer layout update: the preview row and now-playing row now have separate footer lines, so they no longer fight for the same space.
- Worksuite handoff slice: `g x g` now maps to `GoExternalGlow`, emits a scoped JSON handoff envelope in the app cache, passes its path via `JX_GLOW_HANDOFF_FILE` for binary-compatibility, and exits spotify foreground on successful launch so terminal ownership is handed off cleanly; the shortcut-family popup supports nested families so multi-key paths like `g x g` are discoverable.
- Handoff teardown hardening: `GoExternalGlow` now queues a pending external launch in UI state and executes it only after raw mode is disabled and the alternate screen is left, preventing stacked-TUI/raw-mode race conditions during `jx-spotify` -> `jx-glow` transitions.
- Device preference tweak: `default_device` now wins over the currently active device when choosing a startup/auto-connect target, which should make home speaker handoff more predictable when headphones are also present.
- Radio resume slice: the last radio seed is now cached and loaded on startup so the current-playing page can reopen the last radio station after relaunch when Spotify no longer reports a live playback context.
