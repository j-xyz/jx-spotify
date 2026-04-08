# Current Status

- The help-splitting direction has been reviewed and scoped.
- Current recommendation: keep the global command help page, then add smaller contextual surfaces starting with SearchTui.
- Implemented slices: the app now carries a persistent four-corner shortcut frame, `?` routes to the global help page, playback metadata is more compact, album views drop redundant album columns, radio views use a lighter search-tui-like table, and the shortcut-family popup titles now cover `s` and `u`.
- Last verification: `cargo check --manifest-path /Users/jane/jxyz/maeve/projects/jx-spotify/spotify_player/Cargo.toml` passed after the restyle pass.
- Next slice: visual validation in the live TUI and any follow-up spacing or accent cleanup that falls out of that pass.
- Recent related commit: `eb552eb` (`fix: move spotify badge to global header`).
- New restyle slice: compact now-playing row, shorter playback window, album/radio table cleanup, and global help family promotion.
