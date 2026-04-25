# Local Discovery And Maeve Playback Proposal

Date: April 18, 2026

Revised after Spotify policy review and aligned to Maeve Knowledge Graph v1.

## Summary

Build a local-only intent, recommendation, and execution layer across Maeve and `jx-spotify`, with Spotify kept in the backend role for catalog resolution, radio, playback, and optional playlist persistence.

The core idea is:

- the user tells Maeve what they want to hear
- Maeve interprets that request locally and turns it into explicit constraints, seeds, and candidate actions
- the user's durable music-domain knowledge lives as a music subdomain inside Maeve's shared knowledge graph only when that knowledge is user-authored or sourced from off-platform research
- `jx-spotify` either:
  - asks Spotify to start the appropriate radio or playback context, or
  - builds a local queue or playlist proposal and, after confirmation, sends the resulting payload to Spotify
- `jx-spotify` keeps Spotify-derived identifiers, execution refs, display metadata, and other execution-facing state in short-lived operational stores rather than in the canonical KG
- local taste state comes from user-owned KG notes and off-platform research, not from mining Spotify's own recommendation surfaces or piping Spotify content into Maeve for analysis

This proposal intentionally does not turn `jx-spotify` into a long-term Spotify-behavior profiling system.

## Product Thesis

`jx-spotify` should become:

- a keyboard-first Spotify playback client
- a safe local execution layer for Maeve
- the music-domain execution adapter for a user-owned KG subdomain covering explicit preferences, saved seeds, blocked items, and off-platform discovery notes

The product advantage is not "out-Spotify Spotify."

The advantage is:

- local control over intent and execution
- transparent recommendation logic
- a clean split between durable knowledge and operational execution
- a faster terminal-native path from "I want this mood" to playback

## Knowledge Graph Alignment

This proposal should align directly with [`docs/planning/maeve/knowledge-graph-v1-proposal.md`](/Users/jane/jxyz/maeve/docs/planning/maeve/knowledge-graph-v1-proposal.md).

The clean authority split is:

- the Maeve KG owns durable music-domain knowledge only when it is user-authored or derived from off-platform sources with provenance
- `jx-spotify` owns playback execution, Spotify resolution, Spotify-derived execution refs, proposal assembly, and short-lived operational cache data
- Spotify owns catalog, radio, playback, and optional persisted playlists

For this domain, the KG should hold the user-owned, long-lived layer:

- reusable recipes and listening modes
- saved seeds and blocked items
- semantic preference notes and anti-preference notes
- off-platform research notes with provenance
- view notes that organize the user's music domain

`jx-spotify` should consume that layer through targeted reads or a small local projection, then keep only what it needs to execute quickly.

Spotify-derived metadata, playlist data, search results, recommendation outputs, and execution refs should not become canonical KG content. If the system needs Spotify ids or display metadata to execute quickly, those fields should remain tool-local, rebuildable, and deletable inside `jx-spotify`.

This is also a useful template for an eventual `init kg subdomain <domain>` pattern:

- establish a domain view and note conventions in the KG
- define which knowledge stays canonical in the KG
- define which operational state stays tool-local
- define how the domain tool projects KG knowledge into its execution model

Music is the first concrete example of that pattern, not a one-off exception.

## Policy Posture

This proposal is deliberately narrower than the earlier version.

It avoids several policy-sensitive shapes by design:

- do not analyze Spotify playlists or Spotify recommendations to derive a taste model
- do not feed Spotify metadata, playlists, or recommendation outputs into Maeve for profiling or summarization
- do not build long-lived local profiles from Spotify listening history, search behavior, skips, or completion data
- do not use Spotify content to train or feed a machine learning or AI model
- do not ingest Spotify search results, metadata, playlists, playback traces, or recommendation outputs into prompts, embeddings, rerankers, eval sets, or any other model context
- do not export Spotify-derived data into the Maeve KG or any downstream recommendation service
- do not treat Spotify as a corpus to export into another recommendation service
- do not add voice-control, mixed-audio, cross-service content, or visual-sync features on top of Spotify playback

Spotify remains the playback and catalog backend.

Maeve remains a local planner operating over:

- the user's current request
- user-authored KG notes in the music subdomain
- explicit accept/reject feedback the user gives inside the local experience
- optional off-platform public web research captured locally with provenance

## Design Guardrails

These are proposal-level constraints, not optional polish.

- User intent is primary. Maeve acts only on the user's current request plus user-owned preferences represented in the music KG subdomain.
- No Spotify-derived profiling. `jx-spotify` should not build a taste profile from Spotify listening history, skips, search logs, or recommendation interactions.
- No Spotify-to-Maeve content pipeline. Spotify metadata may be used to execute the user's request or display options in `jx-spotify`, but it should not become a background corpus for Maeve analysis, prompt assembly, embeddings, or long-lived KG enrichment.
- KG is the durable knowledge layer. Long-lived recipes, saved seeds, blocked artists, and research notes belong in the KG; `jx-spotify` should not grow a second durable taste-memory system beside it.
- No Spotify-derived canonical fields. If a user saves a seed, recipe, or block that references Spotify items, the KG should store user-authored labels and rationale while Spotify ids and fetched metadata remain tool-local execution refs.
- Minimal storage. Store only what is strictly necessary to operate the SDA, and prefer KG refs, user-authored labels, and temporary Spotify execution refs or metadata over durable copies of Spotify content.
- Proposal before execution. By default, Maeve returns a plan or candidate list first. Playback or playlist writes happen only after explicit user intent.
- Fresh confirmation for writes. Creating, saving, or exporting a playlist on Spotify always requires a fresh confirm step.
- Off-platform research is separate. Web research and LLM synthesis may expand the music KG subdomain, but that path must use non-Spotify sources and keep provenance.
- Streaming constraints are explicit. Music playback actions must respect Spotify's Premium-only streaming limits, and any displayed Spotify metadata or cover art must stay tied to playback or resolution flows with Spotify attribution and link-back.
- No prohibited playback overlays. The system must not add voice control, mix or overlap Spotify audio with other audio, synchronize Spotify audio to video or similar media, or integrate Spotify playback with another service's content stream.
- Disconnect and deletion remain first-class. If the user disconnects Spotify or asks to remove local taste data, the relevant KG notes, projections, and operational cache data should be easy to inspect and delete.

## User Experience

### Primary Story

1. The user is working in Maeve and says:
   `ok, maeve. find me deep instrumental jazz for focus work`
2. Maeve interprets the request locally into structured constraints such as:
   - mood: `focused`
   - texture: `instrumental`
   - genre hints: `jazz`, `deep`
   - exclusions: optional
3. Maeve checks the music KG subdomain for:
   - explicit saved seeds
   - blocked artists or styles
   - prior accepted recipes or semantic preference notes
   - off-platform discovery notes that match the request
4. Maeve chooses one of two execution paths:
   - radio path: resolve seeds and ask `jx-spotify` to start Spotify radio or a Spotify playback context
   - local proposal path: assemble a local queue or playlist proposal, then ask `jx-spotify` to execute it after approval
5. `jx-spotify` shows the proposal or starts playback only when the user has clearly asked for play now.
6. If the user likes the result, they can save or update the relevant KG-backed recipe, save a playlist to Spotify with confirmation, or refine the request.

### UX Principles

- Local first: the reasoning and planning live locally.
- User led: Maeve follows explicit user intent, not silent background optimization.
- Transparent: every recommendation should explain which KG notes, request constraints, and execution choices were used.
- Narrow: the system should stay focused on finding and playing music, not on building a shadow analytics stack.
- Inspectable: the user can inspect the music KG notes, linked recipes, saved seeds, blocked items, and research notes.
- Incremental: the first version should build on existing `search`, `playback`, `playlist`, `connect`, and socket-command infrastructure.

## Existing Foundations In `jx-spotify`

The repo already has several strong building blocks:

- CLI and socket control for `search`, `playback`, `connect`, `playlist`, and `get`
- a local cache folder model for private state and JSON caches
- search-first navigation and radio/context playback flows
- script-friendly JSON output on important command surfaces
- a terminal UI that already supports music-first workflows well

The proposal should build on those foundations without introducing a large new behavior-logging subsystem, a second durable memory plane beside the KG, or a standalone Spotify metadata browser layered on top of the player.

## Product Surface

### 1. Music KG Subdomain

Treat the user's music profile as a domain slice inside the shared Maeve KG rather than as a standalone `jx-spotify` datastore.

The canonical durable layer should live in KG notes and linked resources:

- reusable music recipes such as `focus`, `late-night`, or `new-to-me guitar`
- saved seeds the user explicitly cares about, expressed as user-authored notes with rationale and human-readable labels
- blocked artists, genres, moods, or descriptors
- semantic preference notes and anti-preference notes
- provenance-tagged off-platform research notes
- view notes that organize a music cluster for recall and retrieval

This lets the music domain participate in the same user-owned context system as other Maeve domains instead of becoming a special-case memory product.

What does not belong in canonical music KG notes:

- long-term logs of Spotify listening history for profiling
- derived affinity scores computed from Spotify playback behavior
- mined patterns from Spotify playlists or Spotify recommendations
- Spotify search results, recommendation outputs, or fetched metadata snapshots
- Spotify ids or playlist snapshots stored as canonical long-lived fields
- broad copies of Spotify metadata kept indefinitely

### 2. Execution Paths

Maeve should use one of two narrow execution paths.

`radio` path

- Maeve resolves a small set of explicit seeds or query terms
- `jx-spotify` uses Spotify search, context playback, or radio endpoints to start playback
- the main output is a started station or context, not a mined recommendation graph

`local playlist` path

- Maeve assembles a finite local proposal using:
  - the user's current request
  - KG-backed recipes and saved seeds
  - KG-backed research notes
  - live Spotify search only to resolve playable catalog matches
- `jx-spotify` presents the proposal
- the user may:
  - play it locally as a queue
  - save it to Spotify after fresh confirmation
  - refine it

In both paths, Spotify-derived ids and display metadata remain inside `jx-spotify` as temporary execution material. They should not be promoted into canonical KG records merely because a proposal was accepted.

### 3. Tool-Local Projection And Cache

`jx-spotify` still needs a narrow local layer, but it should be execution-facing rather than canonical.

This layer may include:

- a small projection of the music KG subdomain for fast local reads
- proposal-run records
- short-lived Spotify item and playlist cache data

Some local caching is useful, but it should stay narrow and operational.

Recommended cache rules:

- cache Spotify ids, temporary display metadata, cover-art refs, and timestamps only when needed to run the current flow well
- prefer short-lived caches with refresh-on-use behavior
- if the user explicitly saves a KG recipe that references Spotify items, store any needed Spotify ids and minimal display fields only in the tool-local execution layer
- do not keep a broad local mirror of Spotify metadata, playlists, or recommendation outputs
- separate KG-backed user-owned data from fetched Spotify data so deletion and refresh behavior stay clear

## Data Model

The data model should be intentionally modest.

### KG-Backed Canonical Entities

The durable entities should map to KG note classes rather than app-specific local tables.

Suggested canonical shapes:

- `semantic` notes for enduring music preferences, anti-preferences, and recurring modes
- `resource` notes for trusted discovery sources, important playlists the user explicitly tracks, and recurring references
- `view` notes for topic maps such as `music`, `focus music`, or `new-to-me guitar`
- linked artifacts for recipe definitions, saved seeds, and blocked items when those need a more structured representation than free text

Suggested music-domain entities:

- recipe
- seed
- blocked item
- research note
- preference note
- music view

The important rule is that these remain KG-backed canonical records rather than becoming a second durable database owned only by `jx-spotify`.

### Tool-Local Operational Tables

`proposal_runs`

- id
- created_at
- source: `maeve`, `tui`, `cli`
- request_summary_json
- kg_note_refs_json
- execution_path: `radio`, `local_playlist`
- execution_state: `proposed`, `played`, `saved_to_spotify`, `dismissed`
- user_feedback nullable: `accepted`, `rejected`, `refined`
- expires_at

`kg_music_projection`

- kg_note_id
- entity_type
- display_name
- execution_refs_json nullable
- last_synced_at
- projection_version

### Operational Cache Tables

`spotify_item_cache`

- spotify_id
- item_type
- display_name
- artist_names_json nullable
- last_refreshed_at
- expires_at

`playlist_cache_index`

- spotify_id
- display_name
- owner_name nullable
- snapshot_version nullable
- last_refreshed_at
- expires_at

The operational cache is not the product. It exists only to keep the user experience responsive and should remain short-lived and rebuildable from KG refs plus fresh Spotify lookups.

`proposal_runs` should be treated as short-lived operational history for transparency and active refinement, not as a foundation for hidden preference scoring, user profiling, or Spotify-derived analytics.

## How The System Learns

The system should learn mainly from explicit KG-backed user inputs, not from mining Spotify behavior.

### Allowed Inputs

- the user's current natural-language request
- KG-backed recipes
- KG-backed saved seeds
- KG-backed blocked items and exclusions
- explicit accept/reject/refine feedback on Maeve proposals when used only for immediate refinement or user-authored recipe updates
- user-authored KG notes
- off-platform public web research captured with provenance

### Excluded Inputs

- Spotify-generated recommendations as training material
- Spotify playlists treated as a recommendation corpus
- Spotify listening-history logs used to infer latent preferences
- skip-rate or completion-rate analytics over Spotify playback
- long-form Spotify metadata piped into Maeve for summarization
- Spotify search results, playlist snapshots, or metadata used as prompt context, embeddings, or eval material

### Recommendation Strategy

Use a simple hybrid system:

1. local intent interpretation
2. KG recipe and seed selection
3. optional off-platform research expansion
4. live Spotify resolution and execution

The reasoning order should be:

1. what did the user ask for right now?
2. what explicit KG-backed preferences should shape that request?
3. what off-platform research might broaden the result responsibly?
4. what Spotify entity ids best satisfy the resulting plan?

That keeps Maeve in the role of interpreter and planner, rather than making Spotify behavior itself the training signal.

## Concrete Command Proposal

### Read Commands

`spotify_player maeve profile show --json`

- show the music-domain projection assembled from KG notes plus current short-lived proposal state

`spotify_player maeve recipes list --json`

- list reusable KG-backed music recipes

`spotify_player maeve research list --json`

- list KG-backed off-platform research notes and provenance

`spotify_player maeve cache status`

- show cache counts, expiry windows, and refresh health

### Write Commands

`spotify_player maeve recipe save --name <name> [--query "..."] [--mode focus]`

- save or update a reusable music recipe in the KG-backed domain model

`spotify_player maeve seed add --type artist --label "Artist Name" [--spotify-id <spotify-id>]`

- save an explicit music seed in the KG-backed domain model while keeping any Spotify id as a tool-local execution ref

`spotify_player maeve block add --type artist --label "Artist Name" [--spotify-id <spotify-id>]`

- add a music-domain exclusion in the KG-backed domain model while keeping any Spotify id as a tool-local execution ref

`spotify_player maeve research add --from-url <url>`

- import an off-platform research note into the music KG subdomain

`spotify_player maeve profile purge [--all | --recipes | --research | --cache]`

- remove KG-backed music-domain data or tool-local cache data by category

### Execution Commands

`spotify_player radio start --query "deep instrumental jazz" [--mode focus]`

- resolve the request into a narrow Spotify radio or playback action

`spotify_player playlist propose --query "warm instrumental focus" --duration 90m`

- build a local finite proposal

`spotify_player playback execute --proposal <id>`

- execute a previously built proposal

`spotify_player playlist save --proposal <id> --confirm`

- persist a proposal to Spotify only after fresh confirmation

## Maeve Integration Proposal

### User-Level Experience

Example:

- user says:
  `maeve, I want something warm and instrumental for focused work`

Maeve flow:

1. parse the request into structured constraints
2. read the music KG subdomain through a local projection or targeted KG lookup
3. optionally consult KG-backed off-platform research notes
4. choose `radio` or `local playlist` path
5. resolve candidate Spotify ids through `jx-spotify` without promoting those ids into canonical KG records
6. return either:
   - a single play-now action when the user clearly asked to play immediately, or
   - a small proposal set when the user asked for help deciding
7. execute only the chosen or clearly requested action
8. record the proposal outcome locally in a lightweight, inspectable, expiring way while linking it back to any relevant KG note refs

### Maeve Operating Modes

`proposal` mode

- default mode
- Maeve returns 1 to 3 candidate plans
- the user chooses before playback starts

`play-now` mode

- available when the user clearly asks to start playback now
- Maeve still uses the same narrow execution contract
- Spotify-side write actions remain blocked pending fresh confirmation

`refine` mode

- Maeve revises the active plan with follow-ups such as:
  - `less sleepy`
  - `more guitar`
  - `keep this mood but make it newer`
- refinements update the current proposal and, when appropriate, the linked KG recipe rather than a hidden behavior model

## Off-Platform Discovery

This is where the project can become meaningfully smarter without treating Spotify as a dataset to mine.

Allowed discovery inputs:

- public web research about artists, scenes, labels, genres, and recommendations
- user-authored links and notes
- locally maintained lists of trusted discovery sources

Recommended flow:

1. Maeve researches non-Spotify sources in the background
2. Maeve writes short KG-backed research notes with provenance
3. those notes extract candidate artist, album, track, and descriptor ideas
4. `jx-spotify` resolves those ideas against Spotify only when needed for playback or proposal assembly, with fetched Spotify data staying tool-local

This keeps web research and LLM work anchored in off-platform material rather than Spotify content.

## TUI Additions

The terminal UI should grow carefully and stay music-first.

Recommended additions:

- `Discover` page
  Shows KG-backed recipes, expiring proposals, and off-platform research leads ready to resolve.
- `Recipe` page
  Lets the user inspect and edit a KG-backed recipe, seeds, exclusions, and execution defaults.
- `Research` page
  Shows KG-backed off-platform notes with provenance and quick actions to promote an item into a saved seed.
- `Why this?` popup
  Explains which KG notes, recipe refs, seeds, exclusions, and research notes shaped the current proposal while clearly attributing any displayed Spotify content and linking back to Spotify.

Suggested command grammar:

- keep Maeve-related actions inside a stable `g d` or equivalent discovery family
- keep `propose`, `play`, and `save` visibly separate
- make local edits to recipes and blocks fast from the keyboard

## Transparency And Privacy

This proposal should be explicitly privacy-forward.

Defaults:

- all planning state is local-only
- durable music knowledge lives in the Maeve KG rather than an app-private silo
- Spotify remains the playback backend, not the recommendation-memory backend or the canonical KG source
- no Spotify listening-history profiler is built
- no Spotify recommendation corpus is analyzed
- no Spotify-derived data is fed into LLM prompts, embeddings, or hidden taste-memory systems
- off-platform research notes keep provenance
- proposal mode is the default
- Spotify-side write actions require fresh confirmation
- Spotify-derived operational data is short-lived and purged on disconnect

Recommended controls:

- `spotify_player maeve profile show --json`
- `spotify_player maeve profile purge --all`
- controls to rebuild or clear the local music KG projection
- controls to purge all Spotify-derived execution refs and caches on disconnect
- config flag to disable local research storage
- config flag to disable proposal history
- config flag to disable Maeve access to local taste data
- short TTLs for operational Spotify caches

The user should always be able to answer:

- what KG notes or local request data shaped this recommendation?
- did this come from a saved recipe, a research note, a preference note, or my current request?
- what is cached locally from Spotify, and for how long?
- how do I delete it?

## Delivery Plan

### Phase 1: KG Contract And Execution Foundation

Goal:

- establish the music KG subdomain contract and the explicit Maeve execution contract

Scope:

- define KG-backed recipe, seed, block, preference, and research-note shapes for the music domain
- add the tool-local music projection plus proposal-run storage
- add cache-health commands and short-lived cache bookkeeping
- avoid adding playback-behavior logging or affinity modeling
- define explicit deletion, disconnect, attribution, and Premium-only streaming rules in the contract

Acceptance criteria:

- the user can save and inspect KG-backed music recipes and seeds
- the app can explain what KG inputs a proposal used
- there is no Spotify-behavior profiling subsystem
- no Spotify-derived ids or metadata are required as canonical KG fields
- Spotify-derived operational data can be purged cleanly on disconnect

### Phase 2: Radio And Local Playlist Proposals

Goal:

- let Maeve and `jx-spotify` execute the two narrow playback paths

Scope:

- radio-start command path
- finite local playlist proposal path
- explicit execute and save-confirm steps
- required Spotify attribution, metadata, and link-back behavior on displayed Spotify content

Acceptance criteria:

- a user request can start a radio-style session
- a user request can produce a finite proposal
- Spotify writes require fresh confirmation
- streaming behavior remains inside Spotify's Premium-only and display requirements

### Phase 3: Off-Platform Research Integration

Goal:

- improve discovery quality without mining Spotify data

Scope:

- research-note import into the KG-backed music subdomain
- provenance-aware note summaries
- quick promotion of researched items into saved seeds or recipes

Acceptance criteria:

- Maeve can broaden a request with off-platform inputs
- the provenance of a recommendation is inspectable
- Spotify content is not used as the research corpus

### Phase 4: TUI Discoverability

Goal:

- expose KG-backed music recipes, research, and local proposals inside the TUI

Scope:

- Discover, Recipe, Research, and Why-this surfaces
- keyboard-native editing for seeds and exclusions

Acceptance criteria:

- the user can manage the music KG projection and linked recipe data without leaving `jx-spotify`
- the recommendation flow remains legible and music-first

## Risks And Mitigations

### Risk: The scope quietly expands back into profiling Spotify behavior

Mitigation:

- keep explicit excluded inputs in the implementation plan
- reject features that depend on long-term Spotify behavior logging
- treat KG-backed recipes and proposal history as user-authored planning artifacts, not analytics data

### Risk: `jx-spotify` grows a second durable memory layer beside the KG

Mitigation:

- keep the KG authoritative for durable music knowledge
- keep `jx-spotify` projections rebuildable and disposable
- review new storage surfaces by asking whether they are canonical knowledge or operational state

### Risk: Spotify-derived execution data leaks into the canonical KG or another service

Mitigation:

- treat Spotify ids, metadata, and playlist snapshots as tool-local execution material only
- require any KG record to remain understandable without a Spotify payload attached
- reject designs that export Spotify-derived data into Maeve memory, background analytics, or downstream recommenders

### Risk: Local cache grows into a broad mirror of Spotify data

Mitigation:

- store ids and short-lived display fields only
- use TTLs and refresh on demand
- keep cache tables operational and separate from KG-backed user-owned taste data

### Risk: Off-platform research becomes noisy or low quality

Mitigation:

- keep provenance on every KG-backed research note
- let the user pin trusted sources
- keep promotion from research note to seed or recipe explicit

### Risk: Maeve becomes too autonomous

Mitigation:

- keep proposal mode as the default
- require explicit play intent for immediate execution
- require fresh confirmation for Spotify playlist writes

### Risk: the product drifts into prohibited Spotify surfaces

Mitigation:

- do not add voice control, mixed-audio playback, video-sync, or cross-service content integrations
- keep Spotify displays tied to playback and resolution workflows rather than a standalone catalog browser
- keep the product positioned as a local planner and execution assistant, not a Spotify replacement

## Recommended First Slice

The best first implementation slice is:

1. define the music KG subdomain note shapes and refs
2. add a lightweight local projection plus proposal-run record
3. add a narrow `radio start` flow
4. add a local finite `playlist propose` flow
5. add `profile show`, `recipe save`, `seed add`, and `block add` commands as KG-backed mutations

Why this slice first:

- it matches the narrow product thesis
- it avoids building policy-sensitive Spotify-behavior analytics
- it creates a useful KG-backed domain layer quickly without duplicating memory systems
- it keeps Maeve and `jx-spotify` integrated through a clear contract

## Recommendation

Proceed with this as a narrow local-first discovery and playback layer:

- Maeve owns local intent translation and optional off-platform research synthesis
- the Maeve KG owns durable music-domain knowledge
- `jx-spotify` owns Spotify search, Spotify-derived execution refs, playback execution, confirmed playlist writes, and short-lived operational projections
- Spotify remains the backend, not the data source for a profiling engine

That is a much cleaner fit for both the product vision and the current policy posture, and it establishes a reusable template for future KG subdomains that need a separate execution tool.
