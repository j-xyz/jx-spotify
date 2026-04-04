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
    Artist,
    Album,
    Genre,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SearchTuiQuery {
    general_terms: Vec<String>,
    type_filters: Vec<SearchTuiItemKind>,
    artist_filters: Vec<String>,
    album_filters: Vec<String>,
    genre_filters: Vec<String>,
}

impl SearchTuiQuery {
    fn parse(raw: &str) -> Self {
        let mut query = Self::default();
        let mut section = SearchTuiQuerySection::General;
        let mut buffer = String::new();

        for token in raw.split_whitespace() {
            if let Some((new_section, rest)) = parse_sigil_token(token) {
                query.push_fragment(section, &buffer);
                section = new_section;
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
        let mut parts = Vec::new();
        parts.extend(self.general_terms.iter().cloned());
        parts.extend(self.artist_filters.iter().cloned());
        parts.extend(self.album_filters.iter().cloned());
        parts.extend(self.genre_filters.iter().cloned());
        parts.join(" ")
    }

    fn matches_kind(&self, kind: SearchTuiItemKind) -> bool {
        self.type_filters.is_empty() || self.type_filters.contains(&kind)
    }

    fn matches_free_text(&self, text: &str) -> bool {
        let query = self.general_terms.join(" ");
        text_matches_query(&query, text)
    }

    fn matches_field_filters(&self, filters: &[String], fields: &[String]) -> bool {
        if filters.is_empty() {
            return true;
        }
        if fields.is_empty() {
            return false;
        }

        filters.iter().all(|filter| {
            fields
                .iter()
                .any(|field| text_matches_query(filter, field.as_str()))
        })
    }

    fn matches_item(&self, data: &DataReadGuard, item: &SearchTuiItem) -> bool {
        self.matches_kind(item.kind())
            && self.matches_free_text(&item.search_text())
            && self.matches_field_filters(&self.artist_filters, &item.artist_fields())
            && self.matches_field_filters(&self.album_filters, &item.album_fields())
            && self.matches_field_filters(&self.genre_filters, &item.genre_fields(data))
    }

    fn matches_track(&self, data: &DataReadGuard, track: &Track) -> bool {
        self.matches_kind(SearchTuiItemKind::Track)
            && self.matches_free_text(&track_search_text(track))
            && self.matches_field_filters(&self.artist_filters, &track_artist_fields(track))
            && self.matches_field_filters(&self.album_filters, &track_album_fields(track))
            && self.matches_field_filters(&self.genre_filters, &track_genre_fields(data, track))
    }

    fn push_fragment(&mut self, section: SearchTuiQuerySection, fragment: &str) {
        let Some(fragment) = normalize_fragment(fragment) else {
            return;
        };

        match section {
            SearchTuiQuerySection::General => push_unique(&mut self.general_terms, fragment),
            SearchTuiQuerySection::Artist => push_unique(&mut self.artist_filters, fragment),
            SearchTuiQuerySection::Album => push_unique(&mut self.album_filters, fragment),
            SearchTuiQuerySection::Genre => push_unique(&mut self.genre_filters, fragment),
            SearchTuiQuerySection::Type => {
                for token in fragment.split_whitespace() {
                    if let Some(kind) = parse_item_kind(token) {
                        if !self.type_filters.contains(&kind) {
                            self.type_filters.push(kind);
                        }
                    } else if let Some(token) = normalize_fragment(token) {
                        push_unique(&mut self.general_terms, token);
                    }
                }
            }
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

    fn artist_fields(&self) -> Vec<String> {
        match self {
            Self::Track { track } => track_artist_fields(track),
            Self::Artist { artist } => vec![artist.name.clone()],
            Self::Album { album } => album_artist_fields(album),
            Self::Playlist { .. } => Vec::new(),
        }
    }

    fn album_fields(&self) -> Vec<String> {
        match self {
            Self::Track { track } => track_album_fields(track),
            Self::Artist { .. } => Vec::new(),
            Self::Album { album } => vec![album.name.clone()],
            Self::Playlist { .. } => Vec::new(),
        }
    }

    fn genre_fields(&self, data: &DataReadGuard) -> Vec<String> {
        match self {
            Self::Track { track } => track_genre_fields(data, track),
            Self::Artist { artist } => {
                cached_genres_for_artist_names(data, std::iter::once(artist.name.as_str()))
            }
            Self::Album { album } => album_genre_fields(data, album),
            Self::Playlist { .. } => Vec::new(),
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
        SearchTuiMode::Playlist { .. } => Vec::new(),
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
            if parsed_query.matches_item(data, &item) {
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
        if parsed_query.matches_item(data, &item) {
            push_item(&mut items, &mut seen, item);
            playlist_count += 1;
        }
    }

    let Some(candidate_query) = remote_candidate_query(&SearchTuiMode::Global, query) else {
        return items;
    };

    if let Some(results) = data.caches.search.get(&candidate_query) {
        push_remote_items(
            data,
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
            data,
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
            data,
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
            data,
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
    data: &DataReadGuard,
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
        if parsed_query.matches_item(data, &item) {
            let had_key = seen.contains(&item.key());
            push_item(items, seen, item);
            if !had_key {
                added += 1;
            }
        }
    }
}

pub fn build_playlist_tracks(
    data: &DataReadGuard,
    mode: &SearchTuiMode,
    query: &str,
) -> Vec<Track> {
    let SearchTuiMode::Playlist { playlist_id, .. } = mode else {
        return Vec::new();
    };

    let Some(Context::Playlist { tracks, .. }) = data
        .caches
        .context
        .get(&crate::state::ContextId::Playlist(playlist_id.clone()).uri())
    else {
        return Vec::new();
    };

    let parsed_query = SearchTuiQuery::parse(query);
    tracks
        .iter()
        .filter(|track| parsed_query.matches_track(data, track))
        .take(PLAYLIST_TRACK_LIMIT)
        .cloned()
        .collect()
}

fn parse_sigil_token(token: &str) -> Option<(SearchTuiQuerySection, &str)> {
    let (prefix, rest) = token.split_at(1);
    match prefix {
        "!" => Some((SearchTuiQuerySection::Type, rest)),
        "@" => Some((SearchTuiQuerySection::Artist, rest)),
        "$" => Some((SearchTuiQuerySection::Album, rest)),
        "#" => Some((SearchTuiQuerySection::Genre, rest)),
        _ => None,
    }
}

fn parse_item_kind(token: &str) -> Option<SearchTuiItemKind> {
    match token.to_lowercase().as_str() {
        "song" | "songs" | "track" | "tracks" => Some(SearchTuiItemKind::Track),
        "album" | "albums" => Some(SearchTuiItemKind::Album),
        "artist" | "artists" => Some(SearchTuiItemKind::Artist),
        "playlist" | "playlists" => Some(SearchTuiItemKind::Playlist),
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

fn track_artist_fields(track: &Track) -> Vec<String> {
    track
        .artists
        .iter()
        .map(|artist| artist.name.clone())
        .collect()
}

fn album_artist_fields(album: &crate::state::Album) -> Vec<String> {
    album
        .artists
        .iter()
        .map(|artist| artist.name.clone())
        .collect()
}

fn track_album_fields(track: &Track) -> Vec<String> {
    track
        .album
        .as_ref()
        .map(|album| vec![album.name.clone()])
        .unwrap_or_default()
}

fn track_genre_fields(data: &DataReadGuard, track: &Track) -> Vec<String> {
    cached_genres_for_artist_names(
        data,
        track.artists.iter().map(|artist| artist.name.as_str()),
    )
}

fn album_genre_fields(data: &DataReadGuard, album: &crate::state::Album) -> Vec<String> {
    cached_genres_for_artist_names(
        data,
        album.artists.iter().map(|artist| artist.name.as_str()),
    )
}

fn cached_genres_for_artist_names<'a, I>(data: &DataReadGuard, artist_names: I) -> Vec<String>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut genres = Vec::new();
    for artist_name in artist_names {
        if let Some(cached_genres) = data.caches.genres.get(artist_name) {
            for genre in cached_genres {
                push_unique(&mut genres, genre.clone());
            }
        }
    }
    genres
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_sigils() {
        let query = SearchTuiQuery::parse("quiet !song @phoebe bridgers $punisher #indie");

        assert_eq!(query.general_terms, vec![String::from("quiet")]);
        assert_eq!(query.type_filters, vec![SearchTuiItemKind::Track]);
        assert_eq!(query.artist_filters, vec![String::from("phoebe bridgers")]);
        assert_eq!(query.album_filters, vec![String::from("punisher")]);
        assert_eq!(query.genre_filters, vec![String::from("indie")]);
    }

    #[test]
    fn unknown_type_terms_fall_back_to_general_query() {
        let query = SearchTuiQuery::parse("!mixtape @burial");

        assert!(query.type_filters.is_empty());
        assert_eq!(query.general_terms, vec![String::from("mixtape")]);
        assert_eq!(query.artist_filters, vec![String::from("burial")]);
    }

    #[test]
    fn remote_candidate_query_uses_plain_terms_only() {
        let query = remote_candidate_query(
            &SearchTuiMode::Global,
            "!song quiet @phoebe bridgers $punisher #indie",
        );

        assert_eq!(
            query,
            Some(String::from("quiet phoebe bridgers punisher indie"))
        );
    }
}
