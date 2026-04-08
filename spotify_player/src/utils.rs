use std::{
    borrow::Cow,
    fs::{File, OpenOptions},
    io::{self, Write},
    path::Path,
};

use crate::state::Track;

/// formats a time duration into a "{minutes}:{seconds}" format
pub fn format_duration(duration: &chrono::Duration) -> String {
    let secs = duration.num_seconds();
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// formats a time duration into a compact human-readable "h/m/s" format
pub fn format_duration_hms(duration: &chrono::Duration) -> String {
    let total_secs = duration.num_seconds().max(0);
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

pub fn map_join<T, F>(v: &[T], f: F, sep: &str) -> String
where
    F: Fn(&T) -> &str,
{
    v.iter().map(f).fold(String::new(), |x, y| {
        if x.is_empty() {
            x + y
        } else {
            x + sep + y
        }
    })
}

#[allow(dead_code)]
pub fn get_track_album_image_url(track: &rspotify::model::FullTrack) -> Option<&str> {
    if track.album.images.is_empty() {
        None
    } else {
        Some(&track.album.images[0].url)
    }
}

#[allow(dead_code)]
pub fn get_episode_show_image_url(episode: &rspotify::model::FullEpisode) -> Option<&str> {
    if episode.show.images.is_empty() {
        None
    } else {
        Some(&episode.show.images[0].url)
    }
}

pub fn parse_uri(uri: &str) -> Cow<'_, str> {
    let parts = uri.split(':').collect::<Vec<_>>();
    // The below URI probably has a format of `spotify:user:{user_id}:{type}:{id}`,
    // but `rspotify` library expects to receive an URI of format `spotify:{type}:{id}`.
    // We have to modify the URI to a corresponding format.
    // See: https://github.com/aome510/spotify-player/issues/57#issuecomment-1160868626
    if parts.len() == 5 {
        Cow::Owned([parts[0], parts[3], parts[4]].join(":"))
    } else {
        Cow::Borrowed(uri)
    }
}

pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }

    Ok(())
}

pub fn ensure_private_file(path: &Path) -> io::Result<()> {
    let _ = open_private_file(path, false)?;
    Ok(())
}

pub fn create_private_file(path: &Path) -> io::Result<File> {
    open_private_file(path, true)
}

pub fn write_private_file(path: &Path, content: &str) -> io::Result<()> {
    let mut file = create_private_file(path)?;
    file.write_all(content.as_bytes())
}

fn open_private_file(path: &Path, truncate: bool) -> io::Result<File> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        ensure_private_dir(parent)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(truncate)
            .mode(0o600)
            .open(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(file)
    }

    #[cfg(not(unix))]
    {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(truncate)
            .open(path)
    }
}

#[cfg(feature = "fzf")]
use fuzzy_matcher::skim::SkimMatcherV2;

#[cfg(feature = "fzf")]
pub fn fuzzy_search_items<'a, T: std::fmt::Display>(items: &'a [T], query: &str) -> Vec<&'a T> {
    let matcher = SkimMatcherV2::default();
    let mut result = items
        .iter()
        .filter_map(|t| {
            matcher
                .fuzzy(&t.to_string(), query, false)
                .map(|(score, _)| (t, score))
        })
        .collect::<Vec<_>>();

    result.sort_by(|(_, a), (_, b)| b.cmp(a));
    result.into_iter().map(|(t, _)| t).collect::<Vec<_>>()
}

/// Get a list of items filtered by a search query.
pub fn filtered_items_from_query<'a, T: std::fmt::Display>(
    query: &str,
    items: &'a [T],
) -> Vec<&'a T> {
    let query = query.to_lowercase();

    #[cfg(feature = "fzf")]
    return fuzzy_search_items(items, &query);

    #[cfg(not(feature = "fzf"))]
    items
        .iter()
        .filter(|t| {
            if query.is_empty() {
                true
            } else {
                let t = t.to_string().to_lowercase();
                query
                    .split(' ')
                    .filter(|q| !q.is_empty())
                    .all(|q| t.contains(q))
            }
        })
        .collect::<Vec<_>>()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackFilterSection {
    General,
    Title,
    Artist,
    Album,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct TrackFieldQuery {
    general_terms: Vec<String>,
    title_terms: Vec<String>,
    artist_terms: Vec<String>,
    album_terms: Vec<String>,
    has_field_markers: bool,
}

impl TrackFieldQuery {
    fn parse(raw: &str) -> Self {
        let mut query = Self::default();
        let mut section = TrackFilterSection::General;
        let mut buffer = String::new();

        for token in raw.split_whitespace() {
            if let Some((next_section, rest)) = parse_track_filter_token(token) {
                query.push_fragment(section, &buffer);
                query.has_field_markers = true;
                section = next_section;
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

    fn push_fragment(&mut self, section: TrackFilterSection, fragment: &str) {
        let Some(fragment) = normalize_query_fragment(fragment) else {
            return;
        };

        let target = match section {
            TrackFilterSection::General => &mut self.general_terms,
            TrackFilterSection::Title => &mut self.title_terms,
            TrackFilterSection::Artist => &mut self.artist_terms,
            TrackFilterSection::Album => &mut self.album_terms,
        };

        if !target.contains(&fragment) {
            target.push(fragment);
        }
    }

    fn matches_track(&self, track: &Track) -> bool {
        matches_query_terms(&track.to_string(), &self.general_terms)
            && matches_query_terms(track.display_name().as_ref(), &self.title_terms)
            && matches_query_terms(&track.artists_info(), &self.artist_terms)
            && matches_query_terms(&track.album_info(), &self.album_terms)
    }
}

fn parse_track_filter_token(token: &str) -> Option<(TrackFilterSection, &str)> {
    let (prefix, rest) = token.split_at(1);
    match prefix {
        "!" => Some((TrackFilterSection::Album, rest)),
        "@" => Some((TrackFilterSection::Artist, rest)),
        "$" => Some((TrackFilterSection::Title, rest)),
        _ => None,
    }
}

fn normalize_query_fragment(fragment: &str) -> Option<String> {
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
        Some(fragment.to_lowercase())
    }
}

fn matches_query_terms(text: &str, terms: &[String]) -> bool {
    if terms.is_empty() {
        return true;
    }

    let lower = text.to_lowercase();
    terms.iter().all(|term| lower.contains(term))
}

pub fn filtered_tracks_from_query<'a>(query: &str, items: &'a [Track]) -> Vec<&'a Track> {
    let parsed = TrackFieldQuery::parse(query);
    if !parsed.has_field_markers {
        return filtered_items_from_query(query, items);
    }

    items
        .iter()
        .filter(|track| parsed.matches_track(track))
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Album, Artist, TrackId};
    use rspotify::model::{AlbumId, ArtistId};

    fn test_artist(id: &str, name: &str) -> Artist {
        Artist {
            id: ArtistId::from_id(id).unwrap().into_static(),
            name: name.to_string(),
        }
    }

    fn test_album(id: &str, name: &str, artists: Vec<Artist>) -> Album {
        Album {
            id: AlbumId::from_id(id).unwrap().into_static(),
            release_date: "2023".to_string(),
            name: name.to_string(),
            artists,
            typ: None,
            added_at: 0,
        }
    }

    fn test_track(id: &str, name: &str, artists: Vec<Artist>, album: Option<Album>) -> Track {
        Track {
            id: TrackId::from_id(id).unwrap().into_static(),
            name: name.to_string(),
            artists,
            album,
            duration: std::time::Duration::from_secs(180),
            explicit: false,
            added_at: 0,
        }
    }

    #[test]
    fn parses_track_field_sigils_with_multitoken_fragments() {
        let query = TrackFieldQuery::parse("quiet @phoebe bridgers !punisher $kyoto");

        assert_eq!(query.general_terms, vec![String::from("quiet")]);
        assert_eq!(query.artist_terms, vec![String::from("phoebe bridgers")]);
        assert_eq!(query.album_terms, vec![String::from("punisher")]);
        assert_eq!(query.title_terms, vec![String::from("kyoto")]);
        assert!(query.has_field_markers);
    }

    #[test]
    fn filters_tracks_by_artist_and_album_fields() {
        let artist = test_artist("1111111111111111111111", "Phoebe Bridgers");
        let album = test_album("2222222222222222222222", "Punisher", vec![artist.clone()]);
        let matching = test_track(
            "3333333333333333333333",
            "Kyoto",
            vec![artist.clone()],
            Some(album.clone()),
        );
        let other = test_track(
            "4444444444444444444444",
            "Kyoto",
            vec![test_artist("5555555555555555555555", "Mitski")],
            Some(album),
        );

        let tracks = vec![matching.clone(), other];
        let filtered = filtered_tracks_from_query("@phoebe !punisher", &tracks);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, matching.id);
    }

    #[test]
    fn remapped_song_sigil_filters_track_titles() {
        let artist = test_artist("1111111111111111111111", "Phoebe Bridgers");
        let matching = test_track(
            "3333333333333333333333",
            "Kyoto",
            vec![artist.clone()],
            None,
        );
        let other = test_track(
            "4444444444444444444444",
            "Motion Sickness",
            vec![artist],
            None,
        );

        let tracks = vec![matching.clone(), other];
        let filtered = filtered_tracks_from_query("$kyoto", &tracks);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, matching.id);
    }

    #[test]
    fn empty_trailing_track_filter_sigil_keeps_items_visible() {
        let artist = test_artist("1111111111111111111111", "Phoebe Bridgers");
        let track = test_track("3333333333333333333333", "Kyoto", vec![artist], None);
        let tracks = vec![track];

        let filtered = filtered_tracks_from_query("@", &tracks);

        assert_eq!(filtered.len(), 1);
    }
}
