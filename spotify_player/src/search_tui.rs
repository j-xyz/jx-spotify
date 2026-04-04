use std::collections::HashSet;

use crate::{
    state::{
        Context, DataReadGuard, Playlist, PlaylistFolderItem, SearchTuiMode, Track,
        USER_RECENTLY_PLAYED_TRACKS_ID,
    },
    utils::filtered_items_from_query,
};
use rspotify::model::Id;

const RECENT_TRACK_LIMIT: usize = 8;
const PLAYLIST_LIMIT: usize = 10;
const REMOTE_TRACK_LIMIT: usize = 8;
const REMOTE_ARTIST_LIMIT: usize = 5;
const REMOTE_ALBUM_LIMIT: usize = 5;
const REMOTE_PLAYLIST_LIMIT: usize = 5;
const PLAYLIST_TRACK_LIMIT: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum SearchTuiItemKind {
    Track,
    Artist,
    Album,
    Playlist,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchTuiQuerySection {
    General,
    Type,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SearchTuiQuery {
    general_terms: Vec<String>,
    type_filters: Vec<SearchTuiItemKind>,
}

impl SearchTuiQuery {
    fn parse(raw: &str) -> Self {
        let mut query = Self::default();
        let mut section = SearchTuiQuerySection::General;
        let mut buffer = String::new();

        for token in raw.split_whitespace() {
            if let Some((kind, rest)) = parse_sigil_token(token) {
                query.push_fragment(section, &buffer);
                query.push_type_filter(kind);
                section = SearchTuiQuerySection::Type;
                buffer.clear();
                if !rest.is_empty() {
                    buffer.push_str(rest);
                }
            } else {
                if !buffer.is_empty() {
                    buffer.push(' ');
                }
                buffer.push_str(token);
            }
        }

        query.push_fragment(section, &buffer);
        query
    }

    fn candidate_query(&self) -> String {
        self.general_terms.join(" ")
    }

    fn matches_kind(&self, kind: SearchTuiItemKind) -> bool {
        self.type_filters.is_empty() || self.type_filters.contains(&kind)
    }

    fn matches_free_text(&self, text: &str) -> bool {
        text_matches_query(&self.candidate_query(), text)
    }

    fn matches_item(&self, item: &SearchTuiItem) -> bool {
        self.matches_kind(item.kind()) && self.matches_free_text(&item.search_text())
    }

    fn matches_context_track(&self, track: &Track) -> bool {
        // Context drill-in is always a track list, so keep sigil text usable there
        // by ignoring type-only narrowing and filtering on the query terms.
        self.matches_free_text(&track_search_text(track))
    }

    fn push_fragment(&mut self, section: SearchTuiQuerySection, fragment: &str) {
        let Some(fragment) = normalize_fragment(fragment) else {
            return;
        };

        match section {
            SearchTuiQuerySection::General | SearchTuiQuerySection::Type => {
                push_unique(&mut self.general_terms, fragment);
            }
        }
    }

    fn push_type_filter(&mut self, kind: SearchTuiItemKind) {
        if !self.type_filters.contains(&kind) {
            self.type_filters.push(kind);
        }
    }
}

#[derive(Clone, Debug)]
pub enum SearchTuiItem {
    Track { track: Track },
    Artist { artist: crate::state::Artist },
    Album { album: crate::state::Album },
    Playlist { playlist: Playlist },
}

impl SearchTuiItem {
    pub fn key(&self) -> String {
        match self {
            Self::Track { track } => track.id.uri(),
            Self::Artist { artist } => artist.id.uri(),
            Self::Album { album } => album.id.uri(),
            Self::Playlist { playlist } => playlist.id.uri(),
        }
    }

    fn kind(&self) -> SearchTuiItemKind {
        match self {
            Self::Track { .. } => SearchTuiItemKind::Track,
            Self::Artist { .. } => SearchTuiItemKind::Artist,
            Self::Album { .. } => SearchTuiItemKind::Album,
            Self::Playlist { .. } => SearchTuiItemKind::Playlist,
        }
    }

    fn search_text(&self) -> String {
        match self {
            Self::Track { track } => track_search_text(track),
            Self::Artist { artist } => artist.name.clone(),
            Self::Album { album } => album_search_text(album),
            Self::Playlist { playlist } => playlist_search_text(playlist),
        }
    }
}

fn push_item(items: &mut Vec<SearchTuiItem>, seen: &mut HashSet<String>, item: SearchTuiItem) {
    let key = item.key();
    if seen.insert(key) {
        items.push(item);
    }
}

pub fn remote_candidate_query(mode: &SearchTuiMode, query: &str) -> Option<String> {
    if !matches!(mode, SearchTuiMode::Global) {
        return None;
    }

    let candidate_query = SearchTuiQuery::parse(query).candidate_query();
    if candidate_query.is_empty() {
        None
    } else {
        Some(candidate_query)
    }
}

pub fn build_items(data: &DataReadGuard, mode: &SearchTuiMode, query: &str) -> Vec<SearchTuiItem> {
    match mode {
        SearchTuiMode::Global => build_global_items(data, query),
        SearchTuiMode::Playlist { .. } | SearchTuiMode::Album { .. } => Vec::new(),
    }
}

fn build_global_items(data: &DataReadGuard, query: &str) -> Vec<SearchTuiItem> {
    let parsed_query = SearchTuiQuery::parse(query);
    let mut items = Vec::new();
    let mut seen = HashSet::new();

    if let Some(Context::Tracks { tracks, .. }) =
        data.caches.context.get(&USER_RECENTLY_PLAYED_TRACKS_ID.uri)
    {
        for track in tracks {
            let item = SearchTuiItem::Track {
                track: track.clone(),
            };
            if parsed_query.matches_item(&item) {
                push_item(&mut items, &mut seen, item);
            }
            if items.len() >= RECENT_TRACK_LIMIT {
                break;
            }
        }
    }

    let library_playlists = data
        .user_data
        .playlists
        .iter()
        .filter_map(|item| match item {
            PlaylistFolderItem::Playlist(playlist) => Some(playlist.clone()),
            PlaylistFolderItem::Folder(_) => None,
        })
        .collect::<Vec<_>>();

    let mut playlist_count = items
        .iter()
        .filter(|item| matches!(item, SearchTuiItem::Playlist { .. }))
        .count();
    for playlist in &library_playlists {
        if playlist_count >= PLAYLIST_LIMIT {
            break;
        }

        let item = SearchTuiItem::Playlist {
            playlist: playlist.clone(),
        };
        if parsed_query.matches_item(&item) {
            push_item(&mut items, &mut seen, item);
            playlist_count += 1;
        }
    }

    let Some(candidate_query) = remote_candidate_query(&SearchTuiMode::Global, query) else {
        return items;
    };

    if let Some(results) = data.caches.search.get(&candidate_query) {
        push_remote_items(
            &parsed_query,
            &mut items,
            &mut seen,
            results
                .tracks
                .iter()
                .cloned()
                .map(|track| SearchTuiItem::Track { track }),
            REMOTE_TRACK_LIMIT,
        );
        push_remote_items(
            &parsed_query,
            &mut items,
            &mut seen,
            results
                .artists
                .iter()
                .cloned()
                .map(|artist| SearchTuiItem::Artist { artist }),
            REMOTE_ARTIST_LIMIT,
        );
        push_remote_items(
            &parsed_query,
            &mut items,
            &mut seen,
            results
                .albums
                .iter()
                .cloned()
                .map(|album| SearchTuiItem::Album { album }),
            REMOTE_ALBUM_LIMIT,
        );
        push_remote_items(
            &parsed_query,
            &mut items,
            &mut seen,
            results
                .playlists
                .iter()
                .cloned()
                .map(|playlist| SearchTuiItem::Playlist { playlist }),
            REMOTE_PLAYLIST_LIMIT,
        );
    }

    items
}

fn push_remote_items<I>(
    parsed_query: &SearchTuiQuery,
    items: &mut Vec<SearchTuiItem>,
    seen: &mut HashSet<String>,
    source: I,
    limit: usize,
) where
    I: IntoIterator<Item = SearchTuiItem>,
{
    let mut added = 0;
    for item in source {
        if added >= limit {
            break;
        }
        if parsed_query.matches_item(&item) {
            let had_key = seen.contains(&item.key());
            push_item(items, seen, item);
            if !had_key {
                added += 1;
            }
        }
    }
}

pub fn build_context_tracks(data: &DataReadGuard, mode: &SearchTuiMode, query: &str) -> Vec<Track> {
    let Some(tracks) = context_tracks(data, mode) else {
        return Vec::new();
    };

    let parsed_query = SearchTuiQuery::parse(query);
    tracks
        .iter()
        .filter(|track| parsed_query.matches_context_track(track))
        .take(PLAYLIST_TRACK_LIMIT)
        .cloned()
        .collect()
}

fn context_tracks<'a>(data: &'a DataReadGuard, mode: &SearchTuiMode) -> Option<&'a [Track]> {
    match mode {
        SearchTuiMode::Global => None,
        SearchTuiMode::Playlist { playlist_id, .. } => match data
            .caches
            .context
            .get(&crate::state::ContextId::Playlist(playlist_id.clone()).uri())
        {
            Some(Context::Playlist { tracks, .. }) => Some(tracks.as_slice()),
            _ => None,
        },
        SearchTuiMode::Album { album_id, .. } => match data
            .caches
            .context
            .get(&crate::state::ContextId::Album(album_id.clone()).uri())
        {
            Some(Context::Album { tracks, .. }) => Some(tracks.as_slice()),
            _ => None,
        },
    }
}

fn parse_sigil_token(token: &str) -> Option<(SearchTuiItemKind, &str)> {
    let (prefix, rest) = token.split_at(1);
    match prefix {
        "!" => Some((SearchTuiItemKind::Track, rest)),
        "@" => Some((SearchTuiItemKind::Artist, rest)),
        "$" => Some((SearchTuiItemKind::Album, rest)),
        "#" => Some((SearchTuiItemKind::Playlist, rest)),
        _ => None,
    }
}

fn normalize_fragment(fragment: &str) -> Option<String> {
    let fragment = fragment.trim();
    if fragment.is_empty() {
        return None;
    }

    let fragment = fragment
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(fragment)
        .trim();
    if fragment.is_empty() {
        None
    } else {
        Some(fragment.to_string())
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn text_matches_query(query: &str, text: &str) -> bool {
    let haystack = [text.to_string()];
    !filtered_items_from_query(query, &haystack).is_empty()
}

fn track_search_text(track: &Track) -> String {
    format!(
        "{} {} {}",
        track.display_name(),
        track.artists_info(),
        track.album_info()
    )
}

fn album_search_text(album: &crate::state::Album) -> String {
    format!(
        "{} {}",
        album.name,
        album
            .artists
            .iter()
            .map(|artist| artist.name.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    )
}

fn playlist_search_text(playlist: &Playlist) -> String {
    format!("{} {}", playlist.name, playlist.owner.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sigils_as_type_filters_with_embedded_terms() {
        let query = SearchTuiQuery::parse("quiet !phoebe @bridgers $punisher #mix");

        assert_eq!(
            query.general_terms,
            vec![
                String::from("quiet"),
                String::from("phoebe"),
                String::from("bridgers"),
                String::from("punisher"),
                String::from("mix"),
            ]
        );
        assert_eq!(
            query.type_filters,
            vec![
                SearchTuiItemKind::Track,
                SearchTuiItemKind::Artist,
                SearchTuiItemKind::Album,
                SearchTuiItemKind::Playlist,
            ]
        );
    }

    #[test]
    fn trailing_sigil_sets_type_filter_without_losing_existing_terms() {
        let query = SearchTuiQuery::parse("halsey $");

        assert_eq!(query.general_terms, vec![String::from("halsey")]);
        assert_eq!(query.type_filters, vec![SearchTuiItemKind::Album]);
    }

    #[test]
    fn remote_candidate_query_uses_plain_terms_only() {
        let query = remote_candidate_query(&SearchTuiMode::Global, "!quiet @phoebe $punisher #mix");

        assert_eq!(query, Some(String::from("quiet phoebe punisher mix")));
    }
}
