# Task: jx-spotify row highlight and current-player treatment

## Goal
- Make `jx-spotify` selection emphasis feel closer to `jx-glow` by moving away from full-row background inversion.
- Reuse the same visual language for current-playing rows so the app has one coherent highlight system.

## Observations
- `Theme::selection()` currently defaults to a text-forward accent, but some theme configs still override it with a background fill.
- The current tables and lists rely on `row_highlight_style(...)` and `highlight_style(...)`, which can still read as chunky if the row body is the only signal.
- `jx-glow` uses a gutter / pipe marker as the primary row emphasis, so the selection is visible even when the row background stays quiet.

## Plan
1. Audit the list and table renderers that use selection styling.
   - Search page result tables.
   - SearchTui result tables.
   - Context page track and episode tables.
   - Popup list renderers.

2. Introduce a reusable gutter-style row marker.
   - Use a left pipe / marker cell for the selected row.
   - Keep the rest of the row background calm.
   - Preserve the existing `current_playing` accent for rows that are actually playing, but make it cooperate with selection rather than compete with it.

3. Apply the same marker language to current-player surfaces.
   - Track and episode tables should show the same selection cue as the rest of the app.
   - Current-playing rows should remain distinct, but only as a stronger variant of the same pattern.

4. Tighten theme defaults and examples.
   - Make sure the default highlight does not depend on a full-row background fill.
   - Review `examples/theme.toml` so the documented selection style matches the new visual direction.

5. Verify visually and with compile checks.
   - Run `uv run cargo check --manifest-path /Users/jane/jxyz/maeve/projects/jx-spotify/spotify_player/Cargo.toml`.
   - Validate that the highlighted row reads clearly even when the terminal background is dark and the row fill is unchanged.

## Risks
- If the gutter marker is added inconsistently, the UI will feel fragmented instead of unified.
- If theme overrides keep forcing background fill, the new row language will be hidden behind config defaults.
- Current-playing and selected-row cues need to stay distinguishable without becoming visually noisy.

## Status
- Gutter markers added to list widgets and the main selectable tables.
- The marker now uses a dedicated playback-status accent so it stays visibly colored even when theme selection styles are muted.
- Theme example now matches the same direction (`selection` no longer documents a forced background fill).
- Targeted validation pass completed: `cargo check` plus focused tests for gutter selection normalization, pending-family footer hints, popup-local `Esc`, shell alignment, and SearchTui shell rhythm all passed.
- Real PTY matrix pass completed at `100x32`, `120x36`, `140x40`, and `180x48` using `g` -> `Esc` -> `Q`; selected-row readability and current-playing contrast remained clear, and pending-family footer behavior stayed stable (`g: c t r y +11 esc cancel` while popup open).
- Follow-up checks rerun after the matrix pass (`cargo check` plus the same focused regression tests) all passed.
- Broader key-family live PTY permutations completed at `100x32`, `120x36`, `140x40`, and `180x48` (`a/m/r/s/u/g`, popup-local `Esc`, `?` preview/full-help) with no popup/footer alignment regressions and no row-emphasis accent mismatch observed.
- Follow-up checks rerun after the broader permutation pass (`cargo check` plus focused Phase 2/3 tests for gutter style, pending-family hints, popup-local `Esc`, shell alignment, and SearchTui rhythm) all passed.
- Targeted nested-family live PTY spot checks completed at `100x32`, `120x36`, `140x40`, and `180x48` with `g x` and `s l` family branches plus popup-local `Esc`; pending hints (`g x: g`, `s l: a r`) and footer alignment stayed stable.
- Non-search command-path checks now include focused tests for action-list radio dispatch and managed-project external handoff targeting (`action_list_go_to_radio_pushes_radio_context` and `preferred_external_glow_target_dir_`), both passing.
- Next validation: keep accent-level tuning deferred unless a concrete mismatch appears; if needed, prioritize authenticated live checks on non-SearchTui pages (`browse`, `library`, and context pages) before any accent adjustments.
