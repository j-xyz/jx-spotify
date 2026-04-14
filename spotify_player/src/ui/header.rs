use crate::{
    search_tui,
    state::{
        BrowsePageUIState, Context, ContextId, ContextPageType, Id, PageState, SearchTuiMode,
        SharedState, UIStateGuard,
    },
    utils::map_join,
};
use ratatui::text::{Line, Span};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BreadcrumbSegment {
    sigil: Option<char>,
    label: String,
}

impl BreadcrumbSegment {
    pub(crate) fn plain<T>(label: T) -> Self
    where
        T: Into<String>,
    {
        Self {
            sigil: None,
            label: label.into(),
        }
    }

    pub(crate) fn sigil<T>(sigil: char, label: T) -> Self
    where
        T: Into<String>,
    {
        Self {
            sigil: Some(sigil),
            label: label.into(),
        }
    }

    fn render(&self) -> String {
        let label = self.label.to_lowercase();
        match self.sigil {
            Some(sigil) => format!("{sigil} {label}"),
            None => label,
        }
    }
}

pub(crate) fn app_header_line(state: &SharedState, ui: &UIStateGuard, max_chars: usize) -> String {
    format_segments(&page_breadcrumb_segments(state, ui), max_chars)
}

pub(crate) fn app_header_meta_line(
    state: &SharedState,
    ui: &UIStateGuard,
) -> Option<Line<'static>> {
    match ui.current_page() {
        PageState::SearchTui {
            state: page_state,
            line_input,
        } => {
            let query = line_input.get_text();
            let data = state.data.read();
            let (count, source) = match &page_state.mode {
                SearchTuiMode::Global => {
                    let results = search_tui::build_items(&data, &page_state.mode, &query);
                    (results.items.len(), results.source)
                }
                SearchTuiMode::Playlist { .. }
                | SearchTuiMode::Album { .. }
                | SearchTuiMode::Artist { .. } => (
                    search_tui::build_context_tracks(&data, &page_state.mode, &query).len(),
                    search_tui::SearchTuiResultsSource::Standard,
                ),
            };

            let mut spans = vec![Span::styled(format!("{count} items"), ui.theme.page_desc())];
            match source {
                search_tui::SearchTuiResultsSource::Standard => {}
                search_tui::SearchTuiResultsSource::RecentSeeds => {
                    spans.push(Span::styled(" · ", ui.theme.playback_metadata()));
                    spans.push(Span::styled("recent seeds", ui.theme.playlist_desc()));
                }
                search_tui::SearchTuiResultsSource::LocalFallback => {
                    spans.push(Span::styled(" · ", ui.theme.playback_metadata()));
                    spans.push(Span::styled("local fallback", ui.theme.playlist_desc()));
                }
            }

            Some(Line::from(spans))
        }
        _ => None,
    }
}

pub(crate) fn page_breadcrumb_segments(
    state: &SharedState,
    ui: &UIStateGuard,
) -> Vec<BreadcrumbSegment> {
    let data = state.data.read();

    match ui.current_page() {
        PageState::Library { .. } => vec![BreadcrumbSegment::plain("playlists")],
        PageState::Search { current_query, .. } => {
            if current_query.is_empty() {
                vec![BreadcrumbSegment::plain("results")]
            } else {
                vec![BreadcrumbSegment::plain(format!("results/{current_query}"))]
            }
        }
        PageState::SearchTui { state, .. } => {
            search_tui_breadcrumb_segments(&state.mode, ui, &data)
        }
        PageState::Context {
            id,
            context_page_type,
            ..
        } => {
            if let Some(id) = id {
                if let Some(context) = data.caches.context.get(&id.uri()) {
                    return context_breadcrumb_segments(context);
                }

                return context_id_fallback_segments(id);
            }

            match context_page_type {
                ContextPageType::CurrentPlaying => {
                    let context_id = state.player.read().playing_context_id();
                    context_id
                        .as_ref()
                        .map(|id| {
                            data.caches
                                .context
                                .get(&id.uri())
                                .map(context_breadcrumb_segments)
                                .unwrap_or_else(|| context_id_fallback_segments(id))
                        })
                        .unwrap_or_else(|| vec![BreadcrumbSegment::plain("current playing")])
                }
                ContextPageType::Browsing(id) => context_id_fallback_segments(id),
            }
        }
        PageState::Browse { state } => match state {
            BrowsePageUIState::CategoryList { .. } => vec![BreadcrumbSegment::plain("catalog")],
            BrowsePageUIState::CategoryPlaylistList { category, .. } => {
                vec![BreadcrumbSegment::plain(category.name.clone())]
            }
        },
        PageState::Lyrics { track, .. } => vec![
            BreadcrumbSegment::plain("lyrics"),
            BreadcrumbSegment::plain(track.clone()),
        ],
        PageState::Queue { .. } => vec![BreadcrumbSegment::plain("up next")],
        PageState::CommandHelp { .. } => vec![BreadcrumbSegment::plain("families")],
        PageState::Logs { .. } => vec![BreadcrumbSegment::plain("session")],
    }
}

fn search_tui_breadcrumb_segments(
    mode: &SearchTuiMode,
    ui: &UIStateGuard,
    data: &crate::state::DataReadGuard,
) -> Vec<BreadcrumbSegment> {
    let mut segments = search_tui_history_segments(ui, data);
    if segments.is_empty() {
        match mode {
            SearchTuiMode::Global => segments.push(BreadcrumbSegment::plain("search-tui")),
            SearchTuiMode::Playlist { title, .. } => {
                segments.push(BreadcrumbSegment::plain(title.clone()))
            }
            SearchTuiMode::Album { album_id, title } => {
                segments.extend(
                    data.caches
                        .context
                        .get(&album_id.uri())
                        .map(context_breadcrumb_segments)
                        .unwrap_or_else(|| vec![BreadcrumbSegment::sigil('!', title.clone())]),
                );
            }
            SearchTuiMode::Artist { title, .. } => {
                segments.push(BreadcrumbSegment::sigil('@', title.clone()))
            }
        }
    }

    segments.push(BreadcrumbSegment::plain(match mode {
        SearchTuiMode::Global => "results",
        SearchTuiMode::Playlist { .. }
        | SearchTuiMode::Album { .. }
        | SearchTuiMode::Artist { .. } => "tracks",
    }));
    segments
}

fn search_tui_history_segments(
    ui: &UIStateGuard,
    data: &crate::state::DataReadGuard,
) -> Vec<BreadcrumbSegment> {
    let mut segments = Vec::new();

    for page in &ui.history {
        let PageState::SearchTui { state, .. } = page else {
            continue;
        };

        match &state.mode {
            SearchTuiMode::Global => {}
            SearchTuiMode::Playlist { title, .. } => {
                push_unique(&mut segments, BreadcrumbSegment::plain(title.clone()));
            }
            SearchTuiMode::Artist { title, .. } => {
                push_unique(&mut segments, BreadcrumbSegment::sigil('@', title.clone()));
            }
            SearchTuiMode::Album { album_id, title } => {
                if !segments.iter().any(|segment| segment.sigil == Some('@')) {
                    if let Some(Context::Album { album, .. }) =
                        data.caches.context.get(&album_id.uri())
                    {
                        let artist = map_join(&album.artists, |artist| &artist.name, ", ");
                        if !artist.is_empty() {
                            push_unique(&mut segments, BreadcrumbSegment::sigil('@', artist));
                        }
                    }
                }

                push_unique(&mut segments, BreadcrumbSegment::sigil('!', title.clone()));
            }
        }
    }

    segments
}

fn context_breadcrumb_segments(context: &Context) -> Vec<BreadcrumbSegment> {
    match context {
        Context::Playlist { playlist, .. } => vec![BreadcrumbSegment::plain(playlist.name.clone())],
        Context::Album { album, .. } => {
            let artist = map_join(&album.artists, |artist| &artist.name, ", ");
            let mut segments = Vec::new();
            if !artist.is_empty() {
                segments.push(BreadcrumbSegment::sigil('@', artist));
            }
            segments.push(BreadcrumbSegment::sigil('!', album.name.clone()));
            segments
        }
        Context::Artist { artist, .. } => vec![BreadcrumbSegment::sigil('@', artist.name.clone())],
        Context::Tracks { desc, .. } => vec![BreadcrumbSegment::plain(desc.clone())],
        Context::Show { show, .. } => vec![BreadcrumbSegment::plain(show.name.clone())],
    }
}

fn context_id_fallback_segments(id: &ContextId) -> Vec<BreadcrumbSegment> {
    match id {
        ContextId::Playlist(_) => vec![BreadcrumbSegment::plain("playlist")],
        ContextId::Album(_) => vec![BreadcrumbSegment::sigil('!', "album")],
        ContextId::Artist(_) => vec![BreadcrumbSegment::sigil('@', "artist")],
        ContextId::Tracks(id) => vec![BreadcrumbSegment::plain(id.kind.clone())],
        ContextId::Show(_) => vec![BreadcrumbSegment::plain("show")],
    }
}

fn push_unique(segments: &mut Vec<BreadcrumbSegment>, segment: BreadcrumbSegment) {
    if segments.last() != Some(&segment) {
        segments.push(segment);
    }
}

fn format_segments(segments: &[BreadcrumbSegment], max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let rendered = segments
        .iter()
        .map(BreadcrumbSegment::render)
        .collect::<Vec<_>>()
        .join(" / ");
    truncate_with_ellipsis(&rendered, max_chars)
}

fn truncate_with_ellipsis(value: &str, max_chars: usize) -> String {
    let length = value.chars().count();
    if length <= max_chars {
        return value.to_string();
    }

    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let keep = max_chars - 3;
    let prefix = value.chars().take(keep).collect::<String>();
    format!("{prefix}...")
}

#[cfg(test)]
mod tests {
    use super::{format_segments, BreadcrumbSegment};

    #[test]
    fn breadcrumb_segments_render_sigils_and_lineage() {
        let line = format_segments(
            &[
                BreadcrumbSegment::sigil('@', "Halsey"),
                BreadcrumbSegment::sigil('!', "If I Can't Have Love, I Want Power"),
            ],
            80,
        );

        assert_eq!(line, "@ halsey / ! if i can't have love, i want power");
    }

    #[test]
    fn breadcrumb_truncates_full_line() {
        let line = format_segments(
            &[
                BreadcrumbSegment::plain("search-tui"),
                BreadcrumbSegment::sigil('!', "a-very-very-long-album-name"),
            ],
            20,
        );

        assert_eq!(line.chars().count(), 20);
        assert!(line.ends_with("..."));
    }

    #[test]
    fn breadcrumb_uses_plain_label_when_no_sigil_exists() {
        let line = format_segments(&[BreadcrumbSegment::plain("Playlists")], 20);
        assert_eq!(line, "playlists");
    }
}
