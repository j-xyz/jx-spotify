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
}

fn push_item(items: &mut Vec<SearchTuiItem>, seen: &mut HashSet<String>, item: SearchTuiItem) {
    let key = item.key();
    if seen.insert(key) {
        items.push(item);
    }
}

pub fn build_items(data: &DataReadGuard, mode: &SearchTuiMode, query: &str) -> Vec<SearchTuiItem> {
    match mode {
        SearchTuiMode::Global => build_global_items(data, query),
        SearchTuiMode::Playlist { .. } => Vec::new(),
    }
}

fn build_global_items(data: &DataReadGuard, query: &str) -> Vec<SearchTuiItem> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    let query = query.trim();

    if let Some(Context::Tracks { tracks, .. }) =
        data.caches.context.get(&USER_RECENTLY_PLAYED_TRACKS_ID.uri)
    {
        let recent_tracks = if query.is_empty() {
            tracks.iter().take(RECENT_TRACK_LIMIT).collect::<Vec<_>>()
        } else {
            filtered_items_from_query(query, tracks)
                .into_iter()
                .take(RECENT_TRACK_LIMIT)
                .collect::<Vec<_>>()
        };

        for track in recent_tracks {
            push_item(
                &mut items,
                &mut seen,
                SearchTuiItem::Track {
                    track: track.clone(),
                },
            );
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

    let playlist_matches = if query.is_empty() {
        library_playlists
            .iter()
            .take(PLAYLIST_LIMIT)
            .collect::<Vec<_>>()
    } else {
        filtered_items_from_query(query, &library_playlists)
            .into_iter()
            .take(PLAYLIST_LIMIT)
            .collect::<Vec<_>>()
    };

    for playlist in playlist_matches {
        push_item(
            &mut items,
            &mut seen,
            SearchTuiItem::Playlist {
                playlist: playlist.clone(),
            },
        );
    }

    if query.is_empty() {
        return items;
    }

    if let Some(results) = data.caches.search.get(query) {
        for track in results.tracks.iter().take(REMOTE_TRACK_LIMIT) {
            push_item(
                &mut items,
                &mut seen,
                SearchTuiItem::Track {
                    track: track.clone(),
                },
            );
        }

        for artist in results.artists.iter().take(REMOTE_ARTIST_LIMIT) {
            push_item(
                &mut items,
                &mut seen,
                SearchTuiItem::Artist {
                    artist: artist.clone(),
                },
            );
        }

        for album in results.albums.iter().take(REMOTE_ALBUM_LIMIT) {
            push_item(
                &mut items,
                &mut seen,
                SearchTuiItem::Album {
                    album: album.clone(),
                },
            );
        }

        for playlist in results.playlists.iter().take(REMOTE_PLAYLIST_LIMIT) {
            push_item(
                &mut items,
                &mut seen,
                SearchTuiItem::Playlist {
                    playlist: playlist.clone(),
                },
            );
        }
    }

    items
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

    let query = query.trim();
    if query.is_empty() {
        return tracks.iter().take(PLAYLIST_TRACK_LIMIT).cloned().collect();
    }

    filtered_items_from_query(query, tracks)
        .into_iter()
        .take(PLAYLIST_TRACK_LIMIT)
        .cloned()
        .collect()
}
