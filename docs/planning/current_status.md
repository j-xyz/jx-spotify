# Current Status

- The help-splitting direction has been reviewed and scoped.
- Current recommendation: keep the global command help page, then add smaller contextual surfaces starting with SearchTui.
- Implemented slices: SearchTui now opens a bottom-sheet contextual help popup on `?`, drill-through context pages now open matching page-specific context help with album / playlist / show / artist / current-playing hints, and the existing first-key help families now hide clearly inapplicable current-context/current-track entries off pages where they do not apply.
- Last verification: `uv run cargo check --manifest-path /Users/jane/jxyz/maeve/projects/jx-spotify/spotify_player/Cargo.toml` passed after tightening the context-help rows and page-aware shortcut family popup.
- Recent related commit: `eb552eb` (`fix: move spotify badge to global header`).
