# jx-spotify Sigil Entity Grammar Proposal

Date: April 8, 2026

Status: Accepted direction with deferred `g !` and `g #` navigation semantics

## Decision Under Review

Should `jx-spotify` promote the existing search sigils into a shared entity grammar that applies across:

- top-level navigation
- search scoping
- in-list filtering

The motivating idea is to make the same symbols mean the same entity everywhere:

- `@` artist
- `$` album
- `!` track
- `#` playlist

## Accepted Scope

Accepted:

- shared sigils as entity grammar
- sigil-driven search scoping
- sigil-driven field filtering in track-bearing lists
- `g @`
- `g $`

Deferred:

- `g !` until real usage reveals a stable target meaning
- `g #` as top-level navigation until it has a clearer semantic home

## Why This Is Interesting

`jx-spotify` already has sigil parsing in `search_tui.rs`, but it is currently buried as a search detail. Promoting sigils to a first-class grammar would let the app feel less like a collection of separate commands and more like a compact keyboard language.

The strongest version of the idea is not "sigils as a search trick." It is "sigils as the app's shared entity vocabulary."

That creates one mental model for three jobs:

1. tell the app what kind of thing you mean
2. narrow a result set to that kind of thing
3. jump to the matching context for that kind of thing

## Current Baseline

Today, `jx-spotify` already supports sigils inside Search TUI parsing:

- `!` track
- `@` artist
- `$` album
- `#` playlist

Those sigils already influence:

- remote search candidate construction
- local fallback filtering
- recent-seed result shaping
- context drill-in filtering

So the proposal is evolutionary, not greenfield.

## Proposed Grammar

### 1. Search Scope

Sigils should become first-class search prefixes in both search surfaces:

- `@phoebe` means artist-first search
- `$punisher` means album-first search
- `!kyoto` means track-first search
- `#ambient` means playlist-first search

Recommended behavior:

- preserve existing mixed-query support such as `@phoebe $punisher`
- keep plain text search working as the default
- make the UI visibly acknowledge the active sigil filter

This is the least risky slice because the parser already exists.

### 2. In-List Filtering

Sigils should also apply when filtering an already-open list or context.

Example:

1. user opens a playlist
2. user enters `@phoebe bridgers`
3. the playlist filters to tracks whose artist metadata matches that artist

Suggested list-filter meanings:

- `@name` filters by artist field
- `$name` filters by album field
- `!name` filters by track title
- `#name` filters by playlist name when the current surface is a playlist list

Important rule:

- on track-bearing surfaces, sigils should target the corresponding metadata field instead of acting as a type filter

That keeps the grammar coherent:

- in global search, sigils narrow result kinds
- inside a concrete list, sigils narrow metadata fields

### 3. Top-Level Navigation

Sigils can also become part of the `g` navigation family.

Proposed first pass:

- `g @` go to artist
- `g $` go to album
- `g !` go to current track context

`g #` is the least settled and should be treated as provisional.

Possible meanings for `g #`:

- go to playlists search
- go to a playlists/library page
- go to the source playlist of the selected item when that relationship is known

Recommendation:

- do not lock `g #` yet
- let `@`, `$`, and `!` graduate first

## Decision Review

### In Favor

- It gives `jx-spotify` a recognizable grammar instead of a growing pile of unrelated shortcuts.
- It builds on an existing implementation detail that already works in Search TUI.
- It matches the app's keyboard-first personality better than longer mnemonic bindings.
- It allows search, navigation, and filtering to reinforce each other instead of competing.
- It gives advanced users a fast path without taking plain-text search away from anyone else.

### Against

- Overloading the same character across multiple surfaces can become opaque if the rules are not strict.
- `#` does not currently have the same semantic clarity as `@`, `$`, and `!`.
- Users may not discover sigils naturally unless the UI advertises them.
- Literal sigil characters in text could become awkward without an escape rule.
- If every list starts interpreting sigils differently, the grammar will feel clever rather than trustworthy.

### Hybrid Path

The best path is probably staged promotion, not a single big-bang conversion.

Recommended order:

1. formalize sigils as a documented entity grammar
2. expose sigil scoping more visibly in Search TUI and the classic search page
3. add sigil-aware filtering to track-bearing lists
4. add `g @`, `g $`, and `g !`
5. decide whether `g #` deserves a real semantic home

This keeps the strongest parts of the idea and delays the fuzziest one.

## Product Rules

If this proposal is accepted, the grammar should follow these rules:

1. The same sigil always points to the same entity class.
2. Search surfaces interpret sigils as result-kind narrowing.
3. Open lists interpret sigils as field-specific filtering.
4. Navigation only uses sigils whose target is unambiguous from current context.
5. The UI should surface the active sigil meaning somewhere visible.

## Recommended Mappings

### Strong Mappings

- `@` artist
- `$` album
- `!` track

These feel semantically strong enough to use broadly.

### Weak Mapping

- `#` playlist

This is still probably worth keeping in search because it already exists and is memorable enough there, but it should not yet be assumed to support every navigation scenario.

## Open Questions

### What Should `g !` Mean Exactly?

There are at least three plausible meanings:

- open the current playback context page
- jump to the current track inside the current context
- open a track-focused page for the currently playing song

Recommendation:

- define `g !` as "go to current track context" only if that meaning can be made concrete and stable
- otherwise prefer a more explicit command for current playback and reserve `!` for track search/filter semantics

### Should Search And Filter Share Exact Parsing Rules?

Probably not completely.

Recommended split:

- global search: sigils narrow entity kinds
- local list filtering: sigils narrow metadata fields

The sigil stays the same, but the operation changes with surface type.

### How Should Literal Sigils Be Escaped?

If needed later, the simplest rule would be:

- only treat a sigil specially when it starts a token

That preserves ordinary text like:

- `love!`
- `cash$`

without needing a heavy escape syntax.

## Implementation Direction

Primary files likely involved:

- `spotify_player/src/search_tui.rs`
- `spotify_player/src/event/page.rs`
- `spotify_player/src/event/mod.rs`
- `spotify_player/src/config/keymap.rs`
- `spotify_player/src/ui/page.rs`

Likely slices:

1. document the grammar and expose it in help text
2. bring the same sigil parsing to the classic search page
3. extend list filtering to inspect sigil-prefixed field queries
4. add navigation commands for the accepted sigils

## Recommendation

Accept the proposal in principle, but stage it.

Recommended acceptance scope:

- yes to shared sigils as entity grammar
- yes to sigil-driven search scoping
- yes to sigil-driven field filtering in track-bearing lists
- yes to `g @` and `g $`
- cautious yes to `g !`, pending exact target semantics
- no immediate commitment on `g #` beyond search

That preserves the fire in the idea without forcing the weakest symbol to define the whole system too early.
