# jx-spotify Implementation Plan

Date: April 6, 2026

This plan turns the accepted audit decisions into an execution sequence.

Related document:

- `docs/planning/functionality-audit-2026-04.md`

## Decision Baseline

### Do Now

- API migration pass
- Playlist `/items` migration
- Playlist creation migration
- Recent seeds page
- Track profile popup
- Documentation correction pass

### Keep For Later

- Generic library adapter
- Discover page
- Top artists page

### Accepted Boundaries

- Use Spotify's February 2026 changelog as the migration source of truth
- Keep Search TUI music-first
- Audiobooks are out of scope
- Heavy local fuzzy search is out of scope

### Dropped

- Spoken-audio shelf
- Playback recovery popup

## Execution Order

The work should be done in this order:

1. API migration groundwork
2. Playlist route migration
3. Playlist creation migration
4. Documentation correction pass
5. Recent seeds page
6. Track profile popup
7. Reassess later items after the migration settles

Reasoning:

- The API work has the highest compatibility risk and should land before feature expansion.
- The docs pass should happen after the migration work so the README reflects the current product truth.
- The recent seeds page and track profile popup are product work that should not be built on top of a moving client layer if avoidable.

## Phase 1: API Migration Groundwork

Goal:

- Stabilize the client around Spotify's post-February 2026 API model.

Scope:

- Inventory every client call that touches:
  - library save/remove/check
  - playlist items
  - playlist creation
  - artist top tracks
  - browse categories
- Classify each one as:
  - migrate now
  - tolerate temporarily
  - retire

Primary files:

- `spotify_player/src/client/mod.rs`
- `spotify_player/src/client/request.rs`
- `spotify_player/src/state/model.rs`

Deliverables:

- a migration map in code comments or a short follow-up planning note if needed
- replacement wrappers for the must-migrate routes

Acceptance criteria:

- all high-risk routes from the audit have an explicit migration path
- no unresolved ambiguity remains for playlist items or playlist creation

Notes:

- Do not broaden the scope into generic feature cleanup here.
- Keep browse, artist top tracks, and library work clearly separated so they can be staged independently.

## Phase 2: Playlist Route Migration

Goal:

- Move playlist context and playlist editing semantics from `/tracks` to `/items`.

Scope:

- playlist context loading
- playlist add/remove/reorder behavior
- any track-only assumptions that break when playlist items are treated more generally

Primary files:

- `spotify_player/src/client/mod.rs`
- `spotify_player/src/event/window.rs`
- `spotify_player/src/ui/page.rs`
- `spotify_player/src/state/model.rs`

Implementation notes:

- Normalize around playlist items as the backing API shape.
- Preserve the current UX if the UI remains track-focused.
- If episodes in playlists are possible, decide explicitly whether to:
  - support them now
  - filter them out for the current TUI
  - surface them as unsupported with a clear comment

Acceptance criteria:

- playlist pages still render correctly
- add/remove/reorder still work
- no code path still relies on `/playlists/{id}/tracks` as the main source of truth

Risk:

- This is the highest-risk code migration because it affects both browsing and editing.

## Phase 3: Playlist Creation Migration

Goal:

- Move playlist creation to the current-user endpoint.

Primary files:

- `spotify_player/src/client/mod.rs`
- `spotify_player/src/event/popup.rs`

Acceptance criteria:

- creating a playlist still works from the popup and CLI-connected flow
- no code path uses the user-scoped create route as the primary implementation

Risk:

- Low. This should be a contained client-layer change.

## Phase 4: Documentation Correction Pass

Goal:

- Make the local docs accurately describe the shipped app.

Primary files:

- `README.md`
- `docs/config.md`

Required changes:

- remove the "feature parity with the official Spotify application" claim
- update the CLI command list to include the real parser surface
- document `search-tui` more directly:
  - drill-in modes
  - local fallback results
  - recent seed suggestions
  - type sigils
- describe fuzzy search honestly as subsequence-style fuzzy matching, not typo correction

Acceptance criteria:

- README claims match actual implementation
- no major Search TUI behavior added recently is undocumented

## Phase 5: Recent Seeds Page

Goal:

- Promote recent seeds into a first-class page instead of keeping them only as an empty-result fallback mechanism.

Why now:

- It directly matches the chosen workflow direction.
- It builds on data already introduced in the app.
- It adds value without requiring a broader discovery system.

Recommended product shape:

- a dedicated page reachable with a `g` family command
- list recent track seeds gathered from:
  - search-triggered plays
  - radio-triggered seeds
- default actions:
  - play
  - radio
  - go to artist
  - go to album

Primary files:

- `spotify_player/src/state/data.rs`
- `spotify_player/src/state/ui/page.rs`
- `spotify_player/src/event/mod.rs`
- `spotify_player/src/event/page.rs`
- `spotify_player/src/ui/page.rs`
- `spotify_player/src/config/keymap.rs`

Acceptance criteria:

- user can open a dedicated recent seeds page
- the page is keyboard-first and consistent with the current command grammar
- it does not broaden into generic recommendation logic

Boundary:

- Keep this page music-first.
- Do not turn it into a mixed recents/history dashboard.

## Phase 6: Track Profile Popup

Goal:

- Add a compact, TUI-native power-user popup for the current or selected track.

Why now:

- It is the most interesting accepted feature that does not pull the product toward a fat-client clone.
- It can improve radio and playback exploration without changing the search model.

Recommended contents:

- energy
- danceability
- acousticness
- instrumentalness
- tempo
- key / mode

Recommended access pattern:

- make it an action on current track and selected track, not a global page
- keep the popup compact and read-only

Primary files:

- `spotify_player/src/client/mod.rs`
- `spotify_player/src/client/request.rs`
- `spotify_player/src/command.rs`
- `spotify_player/src/event/mod.rs`
- `spotify_player/src/event/popup.rs`
- `spotify_player/src/ui/popup.rs`
- `spotify_player/src/state/model.rs`

Acceptance criteria:

- popup opens from a clear action path
- no permanent layout expansion is required
- failure to fetch audio features degrades cleanly

Boundary:

- Do not expand this into a dense analytics screen.
- Keep it as a focused inspection tool.

## Later Queue

These remain intentionally deferred.

### Generic Library Adapter

Why later:

- It is architecturally right, but larger than the immediate must-migrate changes.
- The first pass can migrate the hottest compatibility paths without fully re-abstracting the library layer.

Trigger to pull forward:

- multiple migration fixes start duplicating URI-based library logic

### Discover Page

Why later:

- It is the right browse replacement, but it depends on the migration work being settled first.
- The recent seeds page covers the most workflow-specific value sooner.

Trigger to pull forward:

- Browse becomes unstable enough that the product needs a new home page immediately

### Top Artists Page

Why later:

- It is useful and low-risk, but less urgent than migration, docs, and recent seeds.

Trigger to pull forward:

- Discover page work starts

## Non-Goals For This Cycle

- Audiobook support
- Heavy local typo-tolerant search
- Spoken-audio shelf
- Playback recovery popup
- Expanding Search TUI into a broad mixed-media search surface

## Verification Strategy

For migration work:

- `cargo check`
- targeted manual testing of:
  - playlist open
  - playlist add/remove/reorder
  - playlist creation
  - like/save/follow flows touched by migration

For documentation work:

- local doc review against the actual CLI and current Search TUI behavior

For feature work:

- `cargo check`
- one focused manual workflow per feature:
  - recent seeds page open -> play -> radio
  - track profile popup open from current track and selected item

## Recommended First Implementation Slice

If this is executed as code work immediately, the first slice should be:

1. playlist `/items` migration
2. playlist creation migration
3. README correction for parity and CLI/search-tui docs

That gets the riskiest compatibility work moving and improves the documentation before the next feature pass.
