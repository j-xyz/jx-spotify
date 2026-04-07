# jx-spotify Functionality Audit

Date: April 6, 2026

## Scope

This audit compares three things:

- The project's original local documentation in `README.md` and `docs/config.md`
- The actual implementation in `spotify_player/src/`
- Spotify's current Web API reference and the February 2026 Web API changelog

Primary goals:

- identify documentation drift
- identify product gaps
- identify API compatibility risks
- propose a roadmap that fits a keyboard-first, search-first TUI workflow

## Sources

Local documentation:

- `README.md`
- `docs/config.md`

Implementation surface sampled:

- `spotify_player/src/command.rs`
- `spotify_player/src/client/request.rs`
- `spotify_player/src/client/mod.rs`
- `spotify_player/src/client/handlers.rs`
- `spotify_player/src/cli/mod.rs`
- `spotify_player/src/cli/commands.rs`
- `spotify_player/src/search_tui.rs`
- `spotify_player/src/event/*.rs`
- `spotify_player/src/ui/page.rs`

Spotify documentation reviewed:

- Web API overview: <https://developer.spotify.com/documentation/web-api>
- February 2026 changelog: <https://developer.spotify.com/documentation/web-api/references/changes/february-2026>
- Get Recommendations: <https://developer.spotify.com/documentation/web-api/reference/get-recommendations>
- Get Artist's Related Artists: <https://developer.spotify.com/documentation/web-api/reference/get-an-artists-related-artists>

## Executive Summary

`jx-spotify` is still strong as a keyboard-first playback client and search-driven TUI, but the current docs and the current API reality are now materially out of sync.

The biggest issue is not missing polish. It is API survivability. Spotify's February 2026 Web API changes removed or superseded several routes that `jx-spotify` still uses directly, especially around library operations, playlist item retrieval, browse categories, artist top tracks, and user-scoped playlist creation. Some of those code paths may still appear to work today, but the official Spotify changelog dated February 2026 treats them as removed, so they should be treated as migration work rather than optional cleanup.

The second issue is documentation overclaim. The README still says the app has "feature parity with the official Spotify application", which is no longer a defensible claim relative to either the shipped code or the public API surface.

One nuance from the Spotify side: the current Web API reference index still lists several older endpoints, while the February 2026 changelog says those same routes were removed or replaced. For this audit, I am treating the February 2026 changelog as the stronger signal for migration planning because it is the newer source and gives explicit replacement guidance.

The good news is that the best next product direction already aligns with your workflow. Rather than chasing the old Browse tab model or recreating Spotify's mobile search behavior locally, `jx-spotify` should lean harder into:

- search-first workflows
- recent seeds and recent plays
- quick drill-through from result to context to radio
- personalized discovery using current-user data instead of generic browse categories

## Current Functionality Inventory

### Strongly Implemented

- Playback control and Connect-style remote control
- Integrated local streaming via librespot
- Queue read/add support
- Current-context navigation
- Top tracks, recently played tracks, liked tracks
- Lyrics page for current track
- Search page with tracks, artists, albums, playlists, shows, and episodes
- Search TUI for fast global search plus playlist/album/artist drill-in
- Radio from selected item, current track, and current context
- Playlist add/remove/reorder and creation workflows
- Album, artist, playlist, and show context pages
- Save/follow/unfollow actions for tracks, albums, artists, playlists, and shows
- Theme system, notifications, media control, and image support

### Partially Implemented

- Podcast/show support exists, but it is uneven:
  - shows and episodes appear in the full search page
  - shows can be opened and saved
  - episodes can be browsed and acted on
  - there is no equivalent first-class saved-shows library flow in the command set
- Search TUI has recent-seed and local fallback logic, but it is intentionally narrower than the full search page:
  - tracks
  - artists
  - albums
  - playlists
- Browse support exists, but only for category list and category playlists

### Not Present As First-Class Features

- Saved episodes browsing
- Audiobook support
- Top artists page
- Featured playlists
- New releases
- Genre seed exploration
- Audio features / audio analysis exploration
- Playlist detail editing from the TUI
- A personalized discovery page that replaces generic browse

## Documentation Gaps

### 1. README overstates parity with Spotify's official app

The README still claims "Feature parity with the official Spotify application" in `README.md:33`.

That is not accurate anymore. The implemented surface is broad, but it does not cover:

- Spotify's richer discovery stack
- autocomplete/suggestions behavior
- modern personalized browse/discovery surfaces
- audiobook support
- saved episodes support
- broad mobile/desktop parity around recommendation UX

Recommendation:

- Replace the parity claim with a more accurate statement such as:
  - "Broad playback, library, search, and playlist coverage for a terminal-first workflow"

### 2. CLI documentation is incomplete

The README's CLI command list in `README.md:335-344` omits at least:

- `generate`
- `features`

Both are real subcommands in `spotify_player/src/cli/mod.rs` and `spotify_player/src/cli/commands.rs`.

Recommendation:

- Expand the CLI command list to match the actual parser surface.

### 3. Fuzzy-search documentation is optimistic

The README says fuzzy search can be enabled with the `fzf` feature in `README.md:329-331`, but that description reads more capable than the current behavior. In practice the optional matcher behaves more like subsequence matching than typo-tolerant correction.

Recommendation:

- Document it as "subsequence/fuzzy matching" rather than approximate typo correction.

### 4. Search TUI behavior is underdocumented relative to current implementation

The README now mentions the core `search-tui` interactions, but it still does not fully document:

- local fallback results
- recent seed suggestions
- drill-in search modes
- type sigils as a substantive search grammar

Recommendation:

- Add a short dedicated `search-tui` section rather than only embedding notes under CLI commands.

## API Compatibility Risks

These are the most important findings in the audit.

### 1. Library operations still use routes Spotify says were removed in February 2026

Spotify's February 2026 changelog says:

- `PUT /me/library` and `DELETE /me/library` were added
- item-specific save/remove/check endpoints were removed in favor of the generic library endpoints

Examples from the official changelog:

- `PUT /me/tracks`, `PUT /me/albums`, `PUT /me/shows` removed
- `DELETE /me/tracks`, `DELETE /me/albums`, `DELETE /me/shows` removed
- `GET /me/*/contains` removed in favor of `GET /me/library/contains`
- follow/unfollow artist and playlist routes also moved toward library semantics

Current code still uses the older item-specific routes through the client wrapper:

- saved albums fetch from `GET /me/albums` in `spotify_player/src/client/mod.rs:903-912`
- saved shows fetch from `GET /me/shows` in `spotify_player/src/client/mod.rs:916-924`
- add/check/remove track, album, artist, playlist, and show library state uses older item-specific helpers in `spotify_player/src/client/mod.rs:1235-1353`

Risk:

- High

Inference:

- Based on the official February 2026 changelog, these routes should be treated as legacy or removed even if some still appear to function today through `rspotify` or Spotify compatibility behavior.

Proposal:

- Introduce a single URI-based library adapter in the client layer.
- Migrate all save/remove/check logic to the new generic library endpoints first.
- Treat the current item-specific helpers as compatibility shims to remove.

### 2. Playlist context loading still uses the removed `/playlists/{id}/tracks` route

Spotify's February 2026 changelog says the playlist item routes moved from `/tracks` to `/items`.

Current code still fetches playlist contents using:

- `GET /playlists/{id}/tracks` in `spotify_player/src/client/mod.rs:1382-1393`

Risk:

- High

Proposal:

- Migrate playlist context loading to `/playlists/{id}/items`
- Normalize playlist item handling around "playable items" rather than assuming track-only tables
- Audit add/remove/reorder wrappers to ensure the underlying `rspotify` calls also target the `/items` family

### 3. Artist pages depend on endpoints Spotify says were removed or deprecated

Spotify's February 2026 changelog says:

- `GET /artists/{id}/top-tracks` was removed

Spotify's current related-artists reference page explicitly marks:

- `GET /artists/{id}/related-artists` as deprecated

Current code still builds artist pages with:

- `artist_top_tracks(...)` in `spotify_player/src/client/mod.rs:1450-1456`
- `artist_related_artists(...)` in `spotify_player/src/client/mod.rs:1458-1466`

Risk:

- High for top tracks
- Medium for related artists

Proposal:

- Split artist page sections into:
  - durable: albums
  - opportunistic: related artists
  - replaceable: top tracks
- Replace removed top-track behavior with one of:
  - search-driven artist drill-in
  - local play history for the artist
  - top tracks from cached search results if available

### 4. Browse page depends on category endpoints Spotify says were removed

Spotify's February 2026 changelog says:

- `GET /browse/categories` removed
- `GET /browse/categories/{id}` removed
- `GET /browse/new-releases` removed

Current code still defines browse category requests and handlers:

- `ClientRequest::GetBrowseCategories` and `GetBrowseCategoryPlaylists` in `spotify_player/src/client/request.rs:27-28`
- `browse_categories()` in `spotify_player/src/client/mod.rs:720-725`
- `browse_category_playlists()` in `spotify_player/src/client/mod.rs:729-757`

Risk:

- High

Proposal:

- Stop treating Browse as a durable product pillar.
- Replace it with a personalized Discover page built from endpoints that are still useful and aligned with the TUI:
  - top items
  - recently played
  - recent radio/search seeds
  - queue/current-context shortcuts

### 5. Playlist creation still uses a route Spotify says was removed

Spotify's February 2026 changelog says:

- `POST /users/{user_id}/playlists` removed
- use `POST /me/playlists` instead

Current code still uses:

- `user_playlist_create(...)` in `spotify_player/src/client/mod.rs:1826-1834`

Risk:

- High

Proposal:

- Migrate playlist creation to the current-user playlist endpoint immediately.

## Product Gaps Relative To The Current API

These are capability gaps, not immediate breakage risks.

### 1. Saved episodes are available in the API, but not surfaced as a first-class user flow

Spotify's current overview still lists saved-episode endpoints. The app already supports shows and episode search, but it does not offer a first-class saved-episodes library page or command family.

Recommendation:

- Add a saved-episodes page only if podcast usage matters
- otherwise keep episode support contextual and avoid product sprawl

### 2. Audiobooks are entirely absent from the app model

Spotify's current overview includes audiobook endpoints, but the local app model has no audiobook type in its core state or command surfaces.

Recommendation:

- Do not add audiobooks by default
- only revisit if your own workflow starts to depend on long-form spoken content in the terminal

This is a deliberate non-goal candidate rather than an obvious must-build.

### 3. Search TUI is narrower than the full search page

The full search page already handles:

- tracks
- artists
- albums
- playlists
- shows
- episodes

The Search TUI intentionally handles only:

- tracks
- artists
- albums
- playlists

This is visible in `spotify_player/src/search_tui.rs`, where `SearchTuiItem` does not include show or episode variants.

Recommendation:

- Keep Search TUI narrow unless shows/episodes are part of your everyday workflow
- if expanded, do it explicitly as a "spoken audio lane", not by silently bloating the core music flow

### 4. Top artists are missing even though the API exposes top items

The app already supports a top-tracks page, but not a top-artists page.

Recommendation:

- Add a top-artists page before adding any broad new discovery system
- it is a low-complexity, high-signal personalized view

### 5. Audio feature and analysis endpoints are unused

Spotify still exposes:

- track audio features
- track audio analysis
- genre seeds

The app does not surface them.

Recommendation:

- Avoid turning the app into a DAW-style analytics client
- but consider a lightweight "why this radio works" or "track profile" popup if you want a novel power-user feature

## Workflow-Aligned Opportunities

These are the proposals that best fit your actual usage pattern and the philosophy established in the recent work.

### 1. Replace Browse with Discover

This is the clearest strategic move.

Why it fits:

- Spotify's old browse-category model is a poor long-term foundation after the February 2026 changes
- your workflow is search-first, radio-heavy, and keyboard-driven
- a terminal UI benefits more from "fast personal entry points" than from generic merchandising shelves

Recommended sections:

- recent seeds
- recently played
- top tracks
- top artists
- continue from queue/current context

This would be a better home page than categories.

### 2. Make recent seeds a first-class page, not just an empty-result fallback

You already have the beginnings of this with the recent track-seed cache.

Why it fits:

- it reinforces your radio/search workflow
- it is local, fast, and philosophically aligned
- it does not pretend to be Spotify's recommendation engine

Recommended actions on each seed:

- play
- radio
- go to artist
- go to album

### 3. Add a saved-shows or spoken-audio shelf only if kept separate from music navigation

If podcast/show support stays in the product, it should be visually and mentally separate from the music-first flow.

Recommendation:

- do not mix shows into Search TUI by default
- instead expose them through:
  - the full search page
  - a saved-shows page
  - explicit show actions

### 4. Add a compact "track profile" popup powered by audio features

This is the most interesting novel feature if you want one capability that feels more "TUI-native" than "mobile Spotify clone".

Possible contents:

- energy
- danceability
- acousticness
- instrumentalness
- tempo
- key / mode

Why it fits:

- it is fast, inspectable, and useful for power listening
- it could improve radio-seed decisions without building a whole recommendation engine

This should stay an inspect/debug affordance, not a core navigation path.

### 5. Add a playback recovery panel under the `m` family

You already have the beginnings of this with:

- refresh playback
- switch device
- restart integrated client

Opportunity:

- render a small popup explaining likely stale-state causes and available recovery actions
- include current device, active playback visibility, and queue freshness

This is especially useful in a terminal app where state drift is more noticeable than in the official client.

## Recommended Roadmap

### Phase 1: Compatibility Stabilization

- Migrate save/remove/check operations to the generic library endpoints
- Migrate playlist context loading from `/tracks` to `/items`
- Migrate playlist creation from user-scoped create to current-user create
- Reduce reliance on removed artist-top-tracks behavior
- Treat browse categories as legacy

### Phase 2: Documentation Correction

- remove the official-app parity claim
- document the actual CLI surface
- clarify fuzzy-search limitations
- add a dedicated `search-tui` section
- document which features are intentionally partial:
  - spoken audio
  - browse/discovery
  - recent-seed fallback behavior

### Phase 3: Personalized Discovery

- replace Browse with Discover
- add top artists
- add a recent seeds page
- optionally add a saved shows page

### Phase 4: Optional Power-User Features

- track profile popup using audio features
- stronger playback recovery popup
- context-aware seed suggestions

## Recommended Non-Goals

These are features I would explicitly avoid for now.

- Recreating Spotify's mobile autocomplete and typo-tolerant search stack locally
- Broad audiobook support unless your workflow actually needs it
- Heavy recommendation logic that tries to outguess Spotify
- A generic browse clone built on unstable or deprecated API surfaces

## Bottom Line

The project is still compelling, but the next important move is not "more features". It is "stabilize the app against the Spotify API as it exists in April 2026".

After that, the highest-leverage product move is to lean into a personal, keyboard-first discovery model:

- search
- recent seeds
- recent plays
- top items
- fast context/radio transitions

That direction is both more robust than the old browse model and more aligned with how you actually use the TUI.
