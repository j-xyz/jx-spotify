# jx-spotify Sigil Entity Grammar Proposal

Date: April 8, 2026

Status: Accepted direction for search/filter grammar with navigation bindings deferred after remap

## Decision Under Review

Should `jx-spotify` promote the existing search sigils into a shared entity grammar that applies across:

- top-level navigation
- search scoping
- in-list filtering

The motivating idea is to make the same symbols mean the same entity everywhere:

- `@` artist
- `!` album
- `$` song
- `#` reserved for later
- `%` reserved for later

## Accepted Scope

Accepted:

- shared sigils as entity grammar
- sigil-driven search scoping
- sigil-driven field filtering in track-bearing lists

Deferred:

- sigil-driven `g` navigation bindings until the remapped symbols have clearer usage patterns

## Why This Is Interesting

`jx-spotify` already has sigil parsing in `search_tui.rs`, but it is currently buried as a search detail. Promoting sigils to a first-class grammar would let the app feel less like a collection of separate commands and more like a compact keyboard language.

The strongest version of the idea is not "sigils as a search trick." It is "sigils as the app's shared entity vocabulary."

That creates one mental model for three jobs:

1. tell the app what kind of thing you mean
2. narrow a result set to that kind of thing
3. jump to the matching context for that kind of thing

## Current Baseline

Today, `jx-spotify` already supports sigils inside Search TUI parsing:

- `!` album
- `@` artist
- `$` song

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
- `!punisher` means album-first search
- `$kyoto` means song-first search

Recommended behavior:

- preserve existing mixed-query support such as `@phoebe !punisher`
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
- `!name` filters by album field
- `$name` filters by track title
- `#name` is reserved until `#` has a settled meaning

Important rule:

- on track-bearing surfaces, sigils should target the corresponding metadata field instead of acting as a type filter

That keeps the grammar coherent:

- in global search, sigils narrow result kinds
- inside a concrete list, sigils narrow metadata fields

### 3. Top-Level Navigation

Sigils may eventually become part of the `g` navigation family, but the remap changes the tradeoffs:

- `g @` still has a natural "go to artist" meaning
- `g !` now points at albums, which may or may not deserve a direct navigation affordance
- `g $` now points at songs, which is semantically intuitive but operationally ambiguous

Recommendation:

- treat sigil-driven navigation as a follow-up decision after hands-on use of the remapped search/filter grammar
- do not lock `g $`, `g !`, `g #`, or `g %` yet

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
4. observe real usage under the remapped grammar
5. revisit whether any sigil deserves a direct `g` navigation binding under the remapped grammar

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
- `!` album
- `$` song

These feel semantically strong enough to use broadly.

### Weak Mapping

- `#` reserved
- `%` reserved

These should not yet be assumed to support search or navigation semantics until real usage suggests a stable meaning.

## Open Questions

### Which, If Any, Sigils Deserve Direct `g` Navigation?

There are at least three open questions now:

- should `g @` become "go to artist" for the selected/current item
- should `g !` become "go to album" for the selected/current item
- should `g $` map to any direct navigation at all, given that "song" is more action-oriented than context-oriented

Recommendation:

- let the remapped grammar prove itself in search and filtering first
- then decide which sigils merit direct navigation bindings

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
4. revisit navigation commands after the remapped grammar has real usage

## Recommendation

Accept the proposal in principle, but stage it.

Recommended acceptance scope:

- yes to shared sigils as entity grammar
- yes to sigil-driven search scoping
- yes to sigil-driven field filtering in track-bearing lists
- no immediate commitment on sigil-driven `g` navigation under the remapped grammar

That preserves the fire in the idea without forcing the weakest symbol to define the whole system too early.
