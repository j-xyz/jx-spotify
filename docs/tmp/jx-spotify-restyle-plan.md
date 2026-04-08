# Task: jx-spotify restyle and help promotion

## Primary objective
- Make playback, album, and radio surfaces feel lighter and less repetitive.
- Promote the global keyboard families in the main help page.
- Remove the SearchTui-specific help surface in favor of the global help page.

## Working plan
- [x] Tighten the playback formatter so repeat/shuffle are de-emphasized and duplicate metadata is collapsed.
- [x] Restyle album and radio track tables so album rows lose redundant album text and radio rows lose the `#` column.
- [x] Promote the first-key shortcut families in the app-wide four-corner chrome and route SearchTui `?` to the global help page.
- [x] Expand the shortcut-family popup titles to cover the `s` and `u` families.
- [x] Verify with a targeted `cargo check`.
- [ ] Visual pass in the live TUI.
- [ ] Review with the user.
- [ ] Primary objective complete.

## Notes
- Keep the changes visually cohesive with the existing `jx-glow` / `jx-twig` direction: quieter chrome, fewer repeated fields, and smaller help-specific surfaces.
- The radio page is a `ContextId::Tracks(...)` context, so it can be restyled in the shared track-table renderer instead of growing a one-off renderer.
