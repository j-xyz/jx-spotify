use std::{
    collections::{btree_map::Entry, BTreeMap},
    fmt::Display,
};

use ratatui::text::Line;

use crate::{
    command::Command,
    key::KeySequence,
    search_tui,
    state::{ContextPageType, Episode, SearchTuiFocus, SearchTuiMode},
    utils::format_duration_hms,
};

use super::{
    config, utils, Album, Alignment, Artist, ArtistFocusState, BrowsePageUIState, Cell, Constraint,
    Context, ContextPageUIState, DataReadGuard, Frame, Id, Layout, LibraryFocusState, Modifier,
    MutableWindowState, Orientation, PageState, Paragraph, PlaylistFolderItem, Rect, Row,
    SearchFocusState, SharedState, Span, Style, Table, Text, Track, UIStateGuard,
};
use crate::state::BidiDisplay;
use crate::ui::utils::to_bidi_string;

const COMMAND_TABLE_CONSTRAINTS: [Constraint; 2] =
    [Constraint::Percentage(28), Constraint::Percentage(72)];
const SEARCH_TUI_HEADER_HEIGHT: u16 = 1;
const SEARCH_TUI_QUERY_SHELF_HEIGHT: u16 = 2;
const SEARCH_TUI_SECTION_GAP_HEIGHT: u16 = 1;
const SEARCH_TUI_SECTION_GAP_THRESHOLD: u16 = 10;
const SPLIT_ROW_MIN_RIGHT_WIDTH: u16 = 12;
const SPLIT_ROW_RESERVED_LEFT_WIDTH: u16 = 10;

#[derive(Clone, Debug)]
struct SplitRowRightMeta {
    leading: Option<String>,
    trailing: String,
}

#[derive(Clone, Copy, Debug)]
struct SplitRowRightLayout {
    total_width: u16,
    trailing_width: u16,
}

impl SplitRowRightMeta {
    fn plain<T>(trailing: T) -> Self
    where
        T: Into<String>,
    {
        Self {
            leading: None,
            trailing: trailing.into(),
        }
    }

    fn pair<L, T>(leading: L, trailing: T) -> Self
    where
        L: Into<String>,
        T: Into<String>,
    {
        let leading = leading.into();
        if leading.is_empty() {
            return Self::plain(trailing);
        }

        Self {
            leading: Some(leading),
            trailing: trailing.into(),
        }
    }

    fn display_width(&self) -> u16 {
        let trailing_width = self.trailing.chars().count() as u16;
        match &self.leading {
            Some(leading) if !leading.is_empty() => {
                leading.chars().count() as u16 + 1 + trailing_width
            }
            _ => trailing_width,
        }
    }

    fn trailing_width(&self) -> u16 {
        self.trailing.chars().count() as u16
    }

    fn render(&self, layout: SplitRowRightLayout) -> String {
        let total_width = usize::from(layout.total_width);
        if total_width == 0 {
            return String::new();
        }

        match &self.leading {
            Some(leading)
                if !leading.is_empty()
                    && layout.trailing_width > 0
                    && layout.total_width > layout.trailing_width + 1 =>
            {
                let leading_width =
                    usize::from(layout.total_width.saturating_sub(layout.trailing_width + 1));
                let trailing_width = usize::from(layout.trailing_width);
                let leading = truncate_for_width(leading, leading_width);
                format!(
                    "{leading:>leading_width$} {trailing:<trailing_width$}",
                    trailing = self.trailing,
                )
            }
            _ => format!(
                "{:>total_width$}",
                truncate_for_width(&self.trailing, total_width)
            ),
        }
    }
}

fn truncate_for_width(value: &str, width: usize) -> String {
    let value_width = value.chars().count();
    if value_width <= width {
        return value.to_string();
    }

    if width == 0 {
        return String::new();
    }

    if width <= 3 {
        return ".".repeat(width);
    }

    let keep = width - 3;
    let prefix = value.chars().take(keep).collect::<String>();
    format!("{prefix}...")
}

#[derive(Clone)]
pub(crate) struct HelpRow {
    section: &'static str,
    shortcuts: String,
    description: String,
}

impl Display for HelpRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} {}",
            self.section, self.shortcuts, self.description
        )
    }
}

fn split_row_right_layout(values: &[SplitRowRightMeta], rect: Rect) -> SplitRowRightLayout {
    let max_allowed = rect.width.saturating_sub(SPLIT_ROW_RESERVED_LEFT_WIDTH);
    if max_allowed == 0 {
        return SplitRowRightLayout {
            total_width: 0,
            trailing_width: 0,
        };
    }

    let min_allowed = SPLIT_ROW_MIN_RIGHT_WIDTH.min(max_allowed);
    let max_content = values
        .iter()
        .map(SplitRowRightMeta::display_width)
        .max()
        .unwrap_or(min_allowed);
    let total_width = max_content.clamp(min_allowed, max_allowed);
    let trailing_width = values
        .iter()
        .filter(|value| value.leading.is_some())
        .map(SplitRowRightMeta::trailing_width)
        .max()
        .unwrap_or(0)
        .min(total_width.saturating_sub(1));

    SplitRowRightLayout {
        total_width,
        trailing_width,
    }
}

fn track_left_line(track: &Track, ui: &UIStateGuard, is_current: bool) -> Line<'static> {
    let main_style = if is_current {
        ui.theme.current_playing()
    } else {
        Style::default()
    };
    let secondary_style = if is_current {
        ui.theme.current_playing()
    } else {
        ui.theme.playlist_desc()
    };

    Line::from(vec![
        Span::styled(to_bidi_string(&track.display_name()), main_style),
        Span::styled(" - ", secondary_style),
        Span::styled(to_bidi_string(&track.artists_info()), secondary_style),
    ])
}

fn track_right_meta(track: &Track, include_album: bool) -> SplitRowRightMeta {
    let duration =
        format_duration_hms(&chrono::Duration::from_std(track.duration).unwrap_or_default());
    let album = track.album_info();
    if include_album && !album.is_empty() {
        SplitRowRightMeta::pair(to_bidi_string(&album), duration)
    } else {
        SplitRowRightMeta::plain(duration)
    }
}

fn search_tui_workspace_rect(rect: Rect) -> Rect {
    rect
}

// UI codes to render a page.
// A `render_*_page` function should follow (not strictly) the below steps
// 1. get data from the application's states
// 2. construct the page's layout
// 3. construct the page's widgets
// 4. render the widgets

pub fn render_search_page(
    is_active: bool,
    frame: &mut Frame,
    state: &SharedState,
    ui: &mut UIStateGuard,
    rect: Rect,
) {
    fn search_items<T: Display>(items: &[T]) -> Vec<(String, bool)> {
        items
            .iter()
            .map(|i| (to_bidi_string(&i.to_string()), false))
            .collect()
    }

    fn search_episode_items(items: &[Episode]) -> Vec<(String, bool)> {
        items
            .iter()
            .map(|episode| {
                let duration = format_duration_hms(
                    &chrono::Duration::from_std(episode.duration).unwrap_or_default(),
                );
                let label = if let Some(show) = &episode.show {
                    format!("{} • {} • {}", episode.name, show.name, duration)
                } else {
                    format!("{} • {}", episode.name, duration)
                };
                (to_bidi_string(&label), false)
            })
            .collect()
    }

    // 1. Get data
    let data = state.data.read();

    let (focus_state, current_query, line_input) = match ui.current_page() {
        PageState::Search {
            state,
            current_query,
            line_input,
        } => (state.focus, current_query, line_input),
        _ => return,
    };

    let search_results = data.caches.search.get(current_query);

    // 2. Construct the page's layout
    let rect = utils::render_panel(frame, &ui.theme, rect, "search", None, true);

    // search input's layout
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Fill(0)]).split(rect);
    let search_input_rect = chunks[0];
    let rect = chunks[1];

    // track/album/artist/playlist/show/episode search results layout
    let chunks = match ui.orientation {
        // 1x6
        Orientation::Vertical => {
            let constraints = if focus_state == SearchFocusState::Input {
                [Constraint::Ratio(1, 6); 6]
            } else {
                let mut constraints = [Constraint::Percentage(15); 6];
                constraints[focus_state as usize - 1] = Constraint::Percentage(25);
                constraints
            };

            Layout::vertical(constraints).split(rect)
        }
        // 2x3
        Orientation::Horizontal => Layout::vertical([Constraint::Ratio(1, 3); 3])
            .split(rect)
            .iter()
            .flat_map(|rect| {
                Layout::horizontal([Constraint::Ratio(1, 2); 2])
                    .split(*rect)
                    .to_vec()
            })
            .collect(),
    };

    // 3. Construct the page's widgets
    let search_tracks = search_results.map(|s| &s.tracks[..]).unwrap_or(&[]);
    let n_tracks = search_tracks.len();

    let (album_list, n_albums) = {
        let album_items = search_results
            .map(|s| search_items(&s.albums))
            .unwrap_or_default();

        let is_active = is_active && focus_state == SearchFocusState::Albums;

        utils::construct_list_widget(&ui.theme, album_items, is_active)
    };

    let (artist_list, n_artists) = {
        let artist_items = search_results
            .map(|s| search_items(&s.artists))
            .unwrap_or_default();

        let is_active = is_active && focus_state == SearchFocusState::Artists;

        utils::construct_list_widget(&ui.theme, artist_items, is_active)
    };

    let (playlist_list, n_playlists) = {
        let playlist_items = search_results
            .map(|s| search_items(&s.playlists))
            .unwrap_or_default();

        let is_active = is_active && focus_state == SearchFocusState::Playlists;

        utils::construct_list_widget(&ui.theme, playlist_items, is_active)
    };

    let (show_list, n_shows) = {
        let show_items = search_results
            .map(|s| search_items(&s.shows))
            .unwrap_or_default();
        let is_active = is_active && focus_state == SearchFocusState::Shows;

        utils::construct_list_widget(&ui.theme, show_items, is_active)
    };

    let (episode_list, n_episodes) = {
        let episode_items = search_results
            .map(|s| search_episode_items(&s.episodes))
            .unwrap_or_default();

        let is_active = is_active && focus_state == SearchFocusState::Episodes;

        utils::construct_list_widget(&ui.theme, episode_items, is_active)
    };

    let search_input_rect = utils::render_panel(
        frame,
        &ui.theme,
        search_input_rect,
        "query",
        Some(Line::from(format!(
            "{} chars",
            current_query.chars().count()
        ))),
        is_active && focus_state == SearchFocusState::Input,
    );
    let track_rect = utils::render_panel(
        frame,
        &ui.theme,
        chunks[0],
        "tracks",
        Some(Line::from(format!("{n_tracks} items"))),
        is_active && focus_state == SearchFocusState::Tracks,
    );
    let album_rect = utils::render_panel(
        frame,
        &ui.theme,
        chunks[1],
        "albums",
        Some(Line::from(format!("{n_albums} items"))),
        is_active && focus_state == SearchFocusState::Albums,
    );
    let artist_rect = utils::render_panel(
        frame,
        &ui.theme,
        chunks[2],
        "artists",
        Some(Line::from(format!("{n_artists} items"))),
        is_active && focus_state == SearchFocusState::Artists,
    );
    let playlist_rect = utils::render_panel(
        frame,
        &ui.theme,
        chunks[3],
        "playlists",
        Some(Line::from(format!("{n_playlists} items"))),
        is_active && focus_state == SearchFocusState::Playlists,
    );
    let show_rect = utils::render_panel(
        frame,
        &ui.theme,
        chunks[4],
        "shows",
        Some(Line::from(format!("{n_shows} items"))),
        is_active && focus_state == SearchFocusState::Shows,
    );
    let episode_rect = utils::render_panel(
        frame,
        &ui.theme,
        chunks[5],
        "episodes",
        Some(Line::from(format!("{n_episodes} items"))),
        is_active && focus_state == SearchFocusState::Episodes,
    );

    // 4. Render the page's widgets
    // Render the query input box
    frame.render_widget(
        line_input.widget(is_active && focus_state == SearchFocusState::Input),
        search_input_rect,
    );

    let track_right_meta = search_tracks
        .iter()
        .map(|track| track_right_meta(track, true))
        .collect::<Vec<_>>();
    let track_right_layout = split_row_right_layout(&track_right_meta, track_rect);
    let track_rows = search_tracks
        .iter()
        .zip(track_right_meta)
        .map(|(track, right_meta)| {
            let is_current = false;
            Row::new(vec![
                if data.user_data.is_liked_track(track) {
                    Cell::from(&config::get_config().app_config.liked_icon as &str)
                        .style(ui.theme.like())
                } else {
                    Cell::from("")
                },
                Cell::from(track_left_line(track, ui, is_current)),
                Cell::from(right_meta.render(track_right_layout)).style(ui.theme.playlist_desc()),
            ])
            .style(Style::default())
        })
        .collect::<Vec<_>>();
    let track_table = Table::new(
        track_rows,
        [
            Constraint::Length(config::get_config().app_config.liked_icon.chars().count() as u16),
            Constraint::Fill(1),
            Constraint::Length(track_right_layout.total_width),
        ],
    )
    .column_spacing(1)
    .highlight_symbol(utils::highlight_symbol(
        &ui.theme,
        is_active && focus_state == SearchFocusState::Tracks,
    ))
    .row_highlight_style(if is_active && focus_state == SearchFocusState::Tracks {
        ui.theme.selection(true)
    } else {
        Style::default()
    });

    // Render the search result windows.
    // Need mutable access to the list/table states stored inside the page state for rendering.
    let PageState::Search {
        state: page_state, ..
    } = ui.current_page_mut()
    else {
        return;
    };
    utils::render_table_window_from_list_state(
        frame,
        track_table,
        track_rect,
        n_tracks,
        &mut page_state.track_list,
    );
    utils::render_list_window(
        frame,
        album_list,
        album_rect,
        n_albums,
        &mut page_state.album_list,
    );
    utils::render_list_window(
        frame,
        artist_list,
        artist_rect,
        n_artists,
        &mut page_state.artist_list,
    );
    utils::render_list_window(
        frame,
        playlist_list,
        playlist_rect,
        n_playlists,
        &mut page_state.playlist_list,
    );
    utils::render_list_window(
        frame,
        show_list,
        show_rect,
        n_shows,
        &mut page_state.show_list,
    );
    utils::render_list_window(
        frame,
        episode_list,
        episode_rect,
        n_episodes,
        &mut page_state.episode_list,
    );
}

pub fn render_search_tui_page(
    is_active: bool,
    frame: &mut Frame,
    state: &SharedState,
    ui: &mut UIStateGuard,
    rect: Rect,
) {
    let (mode, line_input, focus) = match ui.current_page() {
        PageState::SearchTui { state, line_input } => {
            (state.mode.clone(), line_input.clone(), state.focus)
        }
        _ => return,
    };

    let query = line_input.get_text();
    let (title, source, items): (
        String,
        search_tui::SearchTuiResultsSource,
        Vec<SearchTuiDisplayRow>,
    ) = {
        let data = state.data.read();
        match &mode {
            SearchTuiMode::Global => {
                let results = search_tui::build_items(&data, &mode, &query);
                (
                    "Search Results".to_string(),
                    results.source,
                    results
                        .items
                        .into_iter()
                        .map(|item| SearchTuiDisplayRow::from_item(item, &data))
                        .collect(),
                )
            }
            SearchTuiMode::Playlist { title, .. }
            | SearchTuiMode::Album { title, .. }
            | SearchTuiMode::Artist { title, .. } => (
                title.clone(),
                search_tui::SearchTuiResultsSource::Standard,
                search_tui::build_context_tracks(&data, &mode, &query)
                    .into_iter()
                    .map(|track| search_tui_playlist_row(track, &data))
                    .collect::<Vec<_>>(),
            ),
        }
    };

    let search_visible = focus == SearchTuiFocus::Search || !query.is_empty();
    let workspace = search_tui_workspace_rect(rect);
    let gap_height = if workspace.height >= SEARCH_TUI_SECTION_GAP_THRESHOLD {
        SEARCH_TUI_SECTION_GAP_HEIGHT
    } else {
        0
    };
    let layout = if search_visible {
        Layout::vertical([
            Constraint::Length(SEARCH_TUI_HEADER_HEIGHT),
            Constraint::Min(1),
            Constraint::Length(gap_height),
            Constraint::Length(SEARCH_TUI_QUERY_SHELF_HEIGHT),
        ])
        .split(workspace)
    } else {
        Layout::vertical([
            Constraint::Length(SEARCH_TUI_HEADER_HEIGHT),
            Constraint::Min(1),
        ])
        .split(workspace)
    };

    render_search_tui_header(
        frame,
        layout[0],
        &title,
        Some(search_tui_results_meta_line(items.len(), source, ui)),
        is_active && focus == SearchTuiFocus::Results,
        ui,
    );

    let results_rect = layout[1];

    if search_visible {
        let query_rect = layout[3];
        frame.render_widget(
            Paragraph::new(Text::from(vec![
                Line::from(vec![
                    Span::styled(
                        "search",
                        if is_active && focus == SearchTuiFocus::Search {
                            ui.theme.page_desc()
                        } else {
                            ui.theme.playback_metadata()
                        },
                    ),
                    Span::styled("  ", ui.theme.playback_metadata()),
                    Span::styled(
                        search_tui_sigil_meta_line(&mode),
                        ui.theme.playback_metadata(),
                    ),
                ]),
                Line::default(),
            ]))
            .style(
                ui.theme
                    .app()
                    .patch(ui.theme.playback_progress_bar_unfilled()),
            ),
            search_tui_text_rect(query_rect),
        );

        let input_rect = search_tui_query_input_rect(query_rect);
        frame.render_widget(
            line_input.widget(is_active && focus == SearchTuiFocus::Search),
            input_rect,
        );
    }

    render_search_tui_results(frame, results_rect, items, is_active, focus, ui);
}

fn search_tui_results_meta_line(
    count: usize,
    source: search_tui::SearchTuiResultsSource,
    ui: &UIStateGuard,
) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("{count} items"),
        ui.theme.playback_metadata(),
    )];
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

    Line::from(spans)
}

fn search_tui_text_rect(rect: Rect) -> Rect {
    utils::shell_text_rect(rect)
}

fn search_tui_query_input_rect(rect: Rect) -> Rect {
    let text_rect = search_tui_text_rect(rect);
    Rect::new(
        text_rect.x,
        rect.y.saturating_add(1),
        text_rect.width,
        rect.height.saturating_sub(1),
    )
}

fn render_search_tui_header(
    frame: &mut Frame,
    rect: Rect,
    title: &str,
    meta: Option<Line<'static>>,
    is_active: bool,
    ui: &UIStateGuard,
) {
    utils::render_section_header(frame, &ui.theme, rect, title, meta, is_active);
}

fn search_tui_sigil_meta_line(mode: &SearchTuiMode) -> &'static str {
    match mode {
        SearchTuiMode::Global => "! album  @ artist  $ song",
        SearchTuiMode::Playlist { .. }
        | SearchTuiMode::Album { .. }
        | SearchTuiMode::Artist { .. } => "! album  @ artist  $ song",
    }
}

fn search_tui_left_line(row: &SearchTuiDisplayRow, ui: &UIStateGuard) -> Line<'static> {
    let glyph_style = if row.is_liked {
        if row.is_selected {
            ui.theme.like().add_modifier(Modifier::BOLD)
        } else {
            ui.theme.like().add_modifier(Modifier::DIM)
        }
    } else if row.is_selected {
        ui.theme.page_desc()
    } else {
        ui.theme.playback_metadata()
    };
    let main_style = if row.is_current {
        ui.theme.current_playing()
    } else {
        Style::default()
    };
    let title_style = if row.title_bold {
        main_style.add_modifier(Modifier::BOLD)
    } else {
        main_style
    };
    let secondary_style = if row.is_current {
        ui.theme.current_playing()
    } else {
        ui.theme.playlist_desc()
    };

    let mut spans = vec![
        Span::styled(row.glyph().to_string(), glyph_style),
        Span::styled(" ", glyph_style),
        Span::styled(to_bidi_string(&row.title), title_style),
    ];
    if let Some(subtitle) = row
        .subtitle
        .as_deref()
        .filter(|subtitle| !subtitle.is_empty())
    {
        spans.push(Span::styled(" - ", secondary_style));
        spans.push(Span::styled(to_bidi_string(subtitle), secondary_style));
    }

    Line::from(spans)
}

fn render_search_tui_results(
    frame: &mut Frame,
    rect: Rect,
    items: Vec<SearchTuiDisplayRow>,
    is_active: bool,
    focus: SearchTuiFocus,
    ui: &mut UIStateGuard,
) {
    let selected_row = match ui.current_page() {
        PageState::SearchTui { state, .. } => state.result_list.selected(),
        _ => None,
    };
    let right_meta_values = items
        .iter()
        .map(|row| row.right_meta.clone())
        .collect::<Vec<_>>();
    let right_layout = split_row_right_layout(&right_meta_values, rect);
    let rows = items
        .into_iter()
        .enumerate()
        .map(|(index, mut row)| {
            row.is_selected = selected_row == Some(index);
            Row::new(vec![
                Cell::from(search_tui_left_line(&row, ui)),
                Cell::from(row.right_meta.render(right_layout))
                    .style(ui.theme.playlist_desc().add_modifier(Modifier::ITALIC)),
            ])
        })
        .collect::<Vec<_>>();
    let len = rows.len();
    let table = Table::new(
        rows,
        [
            Constraint::Fill(1),
            Constraint::Length(right_layout.total_width),
        ],
    )
    .column_spacing(1)
    .highlight_symbol(search_tui_highlight_symbol(
        &ui.theme,
        is_active && focus == SearchTuiFocus::Results,
    ))
    .row_highlight_style(if is_active && focus == SearchTuiFocus::Results {
        ui.theme
            .app()
            .patch(ui.theme.playback_progress_bar_unfilled())
    } else {
        Style::default()
    });

    let PageState::SearchTui {
        state: page_state, ..
    } = ui.current_page_mut()
    else {
        return;
    };
    utils::render_table_window(frame, table, rect, len, &mut page_state.result_list);
}

fn search_tui_highlight_symbol(theme: &config::Theme, is_active: bool) -> Line<'static> {
    let symbol_style = if is_active {
        theme.playback_status()
    } else {
        Style::default()
    };
    Line::from(vec![Span::styled(" |", symbol_style)])
}

#[derive(Debug)]
struct SearchTuiDisplayRow {
    title: String,
    subtitle: Option<String>,
    right_meta: SplitRowRightMeta,
    is_current: bool,
    is_liked: bool,
    is_selected: bool,
    title_bold: bool,
}

impl SearchTuiDisplayRow {
    fn from_item(item: search_tui::SearchTuiItem, data: &DataReadGuard) -> Self {
        match item {
            search_tui::SearchTuiItem::Track { track } => {
                let album = track.album_info();
                let duration = format_duration_hms(
                    &chrono::Duration::from_std(track.duration).unwrap_or_default(),
                );
                let is_liked = data.user_data.is_liked_track(&track);
                Self {
                    title: track.display_name().to_string(),
                    subtitle: Some(track.artists_info()),
                    right_meta: if album.is_empty() {
                        SplitRowRightMeta::plain(duration)
                    } else {
                        SplitRowRightMeta::pair(to_bidi_string(&album), duration)
                    },
                    is_current: false,
                    is_liked,
                    is_selected: false,
                    title_bold: false,
                }
            }
            search_tui::SearchTuiItem::Artist { artist } => Self {
                title: artist.name,
                subtitle: None,
                right_meta: SplitRowRightMeta::plain("artist"),
                is_current: false,
                is_liked: false,
                is_selected: false,
                title_bold: false,
            },
            search_tui::SearchTuiItem::Album { album } => {
                let year = album.year();
                Self {
                    title: album.name,
                    subtitle: Some(
                        album
                            .artists
                            .iter()
                            .map(|a| a.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                    right_meta: SplitRowRightMeta::plain(format!("album {year}")),
                    is_current: false,
                    is_liked: false,
                    is_selected: false,
                    title_bold: false,
                }
            }
            search_tui::SearchTuiItem::Playlist { playlist } => Self {
                title: playlist.name,
                subtitle: None,
                right_meta: SplitRowRightMeta::plain(format!("playlist {}", playlist.owner.0)),
                is_current: false,
                is_liked: false,
                is_selected: false,
                title_bold: true,
            },
        }
    }

    fn glyph(&self) -> &str {
        if self.is_liked {
            &config::get_config().app_config.liked_icon
        } else {
            "·"
        }
    }
}

fn search_tui_playlist_row(track: Track, data: &DataReadGuard) -> SearchTuiDisplayRow {
    let album = track.album_info();
    let duration =
        format_duration_hms(&chrono::Duration::from_std(track.duration).unwrap_or_default());
    SearchTuiDisplayRow {
        title: track.display_name().to_string(),
        subtitle: Some(track.artists_info()),
        right_meta: if album.is_empty() {
            SplitRowRightMeta::plain(duration)
        } else {
            SplitRowRightMeta::pair(to_bidi_string(&album), duration)
        },
        is_current: false,
        is_liked: data.user_data.is_liked_track(&track),
        is_selected: false,
        title_bold: false,
    }
}

pub fn render_context_page(
    is_active: bool,
    frame: &mut Frame,
    state: &SharedState,
    ui: &mut UIStateGuard,
    rect: Rect,
) {
    // 1. Get data
    let PageState::Context {
        id,
        context_page_type,
        ..
    } = ui.current_page()
    else {
        return;
    };

    let data = state.data.read();
    let (panel_title, panel_meta) = match id
        .as_ref()
        .and_then(|id| data.caches.context.get(&id.uri()))
    {
        Some(context) => context_panel_title_meta(context),
        None => (context_page_type.title(), None),
    };

    // 2. Construct the page's layout
    let rect = utils::render_panel(frame, &ui.theme, rect, &panel_title, panel_meta, true);

    // 3+4. Construct and render the page's widgets
    let Some(id) = id else {
        frame.render_widget(
            Paragraph::new("Cannot determine the current page's context"),
            rect,
        );
        return;
    };

    match data.caches.context.get(&id.uri()) {
        Some(context) => match context {
            Context::Artist {
                top_tracks,
                albums,
                related_artists,
                ..
            } => {
                render_artist_context_page_windows(
                    is_active,
                    frame,
                    state,
                    ui,
                    &data,
                    rect,
                    (top_tracks, albums, related_artists),
                );
            }
            Context::Playlist { tracks, playlist } => {
                let rect = if playlist.desc.is_empty() {
                    rect
                } else {
                    let chunks =
                        Layout::vertical([Constraint::Length(1), Constraint::Fill(0)]).split(rect);
                    frame.render_widget(
                        Paragraph::new(playlist.desc.clone()).style(ui.theme.playlist_desc()),
                        chunks[0],
                    );
                    chunks[1]
                };

                render_track_table(
                    frame,
                    rect,
                    is_active,
                    state,
                    ui.search_filtered_tracks(tracks),
                    ui,
                    &data,
                );
            }
            Context::Tracks { tracks, .. } | Context::Album { tracks, .. } => {
                render_track_table(
                    frame,
                    rect,
                    is_active,
                    state,
                    ui.search_filtered_tracks(tracks),
                    ui,
                    &data,
                );
            }
            Context::Show { episodes, .. } => {
                render_episode_table(
                    frame,
                    rect,
                    is_active,
                    state,
                    ui.search_filtered_items(episodes),
                    ui,
                );
            }
        },
        None => {
            frame.render_widget(Paragraph::new("Loading..."), rect);
        }
    }
}

fn context_panel_title_meta(context: &Context) -> (String, Option<Line<'static>>) {
    match context {
        Context::Playlist { playlist, tracks } => (
            playlist.name.clone(),
            Some(Line::from(format!(
                "{}  {} songs  {}",
                playlist.owner.0,
                tracks.len(),
                context_play_time(tracks),
            ))),
        ),
        Context::Album { album, tracks } => (
            album.name.clone(),
            Some(Line::from(format!(
                "{}  {} songs  {}",
                album.release_date,
                tracks.len(),
                context_play_time(tracks),
            ))),
        ),
        Context::Artist { artist, .. } => (artist.name.clone(), None),
        Context::Tracks { desc, tracks } => (
            desc.clone(),
            Some(Line::from(format!(
                "{} songs  {}",
                tracks.len(),
                context_play_time(tracks),
            ))),
        ),
        Context::Show { show, episodes } => (
            show.name.clone(),
            Some(Line::from(format!("{} episodes", episodes.len()))),
        ),
    }
}

fn context_play_time(tracks: &[Track]) -> String {
    let duration = tracks
        .iter()
        .map(|track| track.duration)
        .sum::<std::time::Duration>();
    format_duration_hms(&chrono::Duration::from_std(duration).unwrap_or_default())
}

pub fn render_library_page(
    is_active: bool,
    frame: &mut Frame,
    state: &SharedState,
    ui: &mut UIStateGuard,
    rect: Rect,
) {
    // 1. Get data
    let curr_context_uri = state.player.read().playing_context_id().map(|c| c.uri());
    let data = state.data.read();
    let configs = config::get_config();

    let (focus_state, playlist_folder_id) = match ui.current_page() {
        PageState::Library { state } => (state.focus, state.playlist_folder_id),
        _ => return,
    };

    // 2. Construct the page's layout
    // Split the library page into 3 windows:
    // - a playlists window
    // - a saved albums window
    // - a followed artists window

    let chunks = ui
        .orientation
        .layout([
            Constraint::Percentage(configs.app_config.layout.library.playlist_percent),
            Constraint::Percentage(configs.app_config.layout.library.album_percent),
            Constraint::Percentage(
                100 - (configs.app_config.layout.library.album_percent
                    + configs.app_config.layout.library.playlist_percent),
            ),
        ])
        .split(rect);

    // 3. Construct the page's widgets
    let items = ui
        .search_filtered_items(&data.user_data.folder_playlists_items(playlist_folder_id))
        .into_iter()
        .map(|item| match item {
            PlaylistFolderItem::Playlist(p) => {
                (p.to_bidi_string(), curr_context_uri == Some(p.id.uri()))
            }
            PlaylistFolderItem::Folder(f) => (f.to_bidi_string(), false),
        })
        .collect::<Vec<_>>();
    let playlist_count = items.len();
    let playlist_rect = utils::render_panel(
        frame,
        &ui.theme,
        chunks[0],
        "playlists",
        Some(Line::from(format!("{playlist_count} items"))),
        is_active
            && focus_state != LibraryFocusState::SavedAlbums
            && focus_state != LibraryFocusState::FollowedArtists,
    );

    let saved_albums = ui
        .search_filtered_items(&data.user_data.saved_albums)
        .into_iter()
        .map(|a| (a.to_bidi_string(), curr_context_uri == Some(a.id.uri())))
        .collect::<Vec<_>>();
    let album_count = saved_albums.len();
    let album_rect = utils::render_panel(
        frame,
        &ui.theme,
        chunks[1],
        "albums",
        Some(Line::from(format!("{album_count} items"))),
        is_active && focus_state == LibraryFocusState::SavedAlbums,
    );

    let followed_artists = ui
        .search_filtered_items(&data.user_data.followed_artists)
        .into_iter()
        .map(|a| (a.to_bidi_string(), curr_context_uri == Some(a.id.uri())))
        .collect::<Vec<_>>();
    let artist_count = followed_artists.len();
    let artist_rect = utils::render_panel(
        frame,
        &ui.theme,
        chunks[2],
        "artists",
        Some(Line::from(format!("{artist_count} items"))),
        is_active && focus_state == LibraryFocusState::FollowedArtists,
    );

    let (playlist_list, n_playlists) = utils::construct_list_widget(
        &ui.theme,
        items,
        is_active
            && focus_state != LibraryFocusState::SavedAlbums
            && focus_state != LibraryFocusState::FollowedArtists,
    );
    // Construct the saved album window
    let (album_list, n_albums) = utils::construct_list_widget(
        &ui.theme,
        saved_albums,
        is_active && focus_state == LibraryFocusState::SavedAlbums,
    );
    // Construct the followed artist window
    let (artist_list, n_artists) = utils::construct_list_widget(
        &ui.theme,
        followed_artists,
        is_active && focus_state == LibraryFocusState::FollowedArtists,
    );

    // 4. Render the page's widgets
    // Render the library page's windows.
    // Will need mutable access to the list/table states stored inside the page state for rendering.
    let PageState::Library { state: page_state } = ui.current_page_mut() else {
        return;
    };

    utils::render_list_window(
        frame,
        playlist_list,
        playlist_rect,
        n_playlists,
        &mut page_state.playlist_list,
    );
    utils::render_list_window(
        frame,
        album_list,
        album_rect,
        n_albums,
        &mut page_state.saved_album_list,
    );
    utils::render_list_window(
        frame,
        artist_list,
        artist_rect,
        n_artists,
        &mut page_state.followed_artist_list,
    );
}

pub fn render_browse_page(
    is_active: bool,
    frame: &mut Frame,
    state: &SharedState,
    ui: &mut UIStateGuard,
    mut rect: Rect,
) {
    // 1. Get data
    let data = state.data.read();

    // 2+3. Construct the page's layout and widgets
    let (list, len) = match ui.current_page() {
        PageState::Browse { state: ui_state } => match ui_state {
            BrowsePageUIState::CategoryList { .. } => {
                let items = ui
                    .search_filtered_items(&data.browse.categories)
                    .into_iter()
                    .map(|c| (c.name.clone(), false))
                    .collect::<Vec<_>>();
                let count = items.len();
                rect = utils::render_panel(
                    frame,
                    &ui.theme,
                    rect,
                    "categories",
                    Some(Line::from(format!("{count} items"))),
                    is_active,
                );

                utils::construct_list_widget(&ui.theme, items, is_active)
            }
            BrowsePageUIState::CategoryPlaylistList { category, .. } => {
                let Some(playlists) = data.browse.category_playlists.get(&category.id) else {
                    let rect = utils::render_panel(
                        frame,
                        &ui.theme,
                        rect,
                        &format!("{} playlists", category.name),
                        None,
                        is_active,
                    );
                    frame.render_widget(Paragraph::new("Loading..."), rect);
                    return;
                };
                let items = ui
                    .search_filtered_items(playlists)
                    .into_iter()
                    .map(|c| (c.name.clone(), false))
                    .collect::<Vec<_>>();
                let count = items.len();
                let title = format!("{} playlists", category.name);
                rect = utils::render_panel(
                    frame,
                    &ui.theme,
                    rect,
                    &title,
                    Some(Line::from(format!("{count} items"))),
                    is_active,
                );

                utils::construct_list_widget(&ui.theme, items, is_active)
            }
        },
        _ => return,
    };

    // 4. Render the page's widget
    let Some(MutableWindowState::List(list_state)) = ui.current_page_mut().focus_window_state_mut()
    else {
        return;
    };
    utils::render_list_window(frame, list, rect, len, list_state);
}

pub fn render_lyrics_page(
    _is_active: bool,
    frame: &mut Frame,
    state: &SharedState,
    ui: &mut UIStateGuard,
    rect: Rect,
) {
    // 1. Get data
    let data = state.data.read();

    // 2. Construct the page's layout
    let rect = utils::render_panel(frame, &ui.theme, rect, "lyrics", None, true);
    let chunks = Layout::vertical([Constraint::Length(2), Constraint::Fill(0)]).split(rect);

    // 3. Construct the page's widgets
    let Some(progress) = state.player.read().playback_progress() else {
        frame.render_widget(Paragraph::new("No playback available"), rect);
        return;
    };

    let PageState::Lyrics {
        track_uri,
        track,
        artists,
    } = ui.current_page_mut()
    else {
        return;
    };

    let lyrics = match data.caches.lyrics.get(track_uri) {
        None => {
            frame.render_widget(Paragraph::new("Loading..."), rect);
            return;
        }
        Some(None) => {
            frame.render_widget(Paragraph::new("Lyrics not found"), rect);
            return;
        }
        Some(Some(lyrics)) => lyrics,
    };

    // 4. Render the page's widgets
    // render lyric page description text
    let bidi_track = to_bidi_string(track);
    let bidi_artists = to_bidi_string(artists);
    frame.render_widget(
        Paragraph::new(format!("{bidi_track} by {bidi_artists}")).style(ui.theme.page_desc()),
        chunks[0],
    );

    // render lyric text

    // the last played line id (1-based)
    // zero value indicates no line has been played yet
    let mut last_played_line_id = 0;
    for (id, (t, _)) in lyrics.lines.iter().enumerate() {
        if *t <= progress {
            last_played_line_id = id + 1;
        }
    }
    let lines = lyrics
        .lines
        .iter()
        .enumerate()
        .map(|(id, (_, line))| match (id + 1).cmp(&last_played_line_id) {
            std::cmp::Ordering::Less => Line::styled(line, ui.theme.lyrics_played()),
            std::cmp::Ordering::Equal => Line::styled(line, ui.theme.lyrics_playing()),
            std::cmp::Ordering::Greater => Line::raw(line),
        })
        .collect::<Vec<_>>();

    let mut paragraph = Paragraph::new(lines);
    // keep the currently playing line in the center if
    // the line goes pass the lower half of lyrics section
    let half_height = (chunks[1].height / 2) as usize;
    if let Some(offset) = last_played_line_id.checked_sub(half_height) {
        paragraph = paragraph.scroll((offset as u16, 0));
    }
    frame.render_widget(paragraph, chunks[1]);
}

pub fn render_commands_help_page(
    frame: &mut Frame,
    _state: &SharedState,
    ui: &mut UIStateGuard,
    rect: Rect,
) {
    // 1. Get data
    let configs = config::get_config();
    let mut map = BTreeMap::new();
    configs
        .keymap_config
        .keymaps
        .iter()
        .filter(|km| km.include_in_help_screen())
        .for_each(|km| {
            let v = map.entry(km.command);
            match v {
                Entry::Vacant(v) => {
                    v.insert(km.key_sequence.display_help());
                }
                Entry::Occupied(mut v) => {
                    let keys = format!("{}, {}", v.get(), km.key_sequence.display_help());
                    *v.get_mut() = keys;
                }
            }
        });

    let rows = global_help_rows()
        .into_iter()
        .map(|(shortcuts, description)| HelpRow {
            section: "First Keys",
            shortcuts,
            description,
        })
        .chain(map.into_iter().map(|(command, keys)| HelpRow {
            section: help_section(command),
            shortcuts: keys,
            description: command.desc().to_string(),
        }))
        .collect::<Vec<_>>();

    let rows = ui.search_filtered_items(&rows);
    let filtered_len = rows.len();
    let display_rows = build_help_display_rows(rows.into_iter().cloned().collect());

    let scroll_offset = match ui.current_page_mut() {
        PageState::CommandHelp {
            ref mut scroll_offset,
        } => {
            if !display_rows.is_empty() && *scroll_offset >= display_rows.len() {
                *scroll_offset = display_rows.len() - 1;
            }
            *scroll_offset
        }
        _ => return,
    };

    // 2. Construct the page's layout
    let rect = utils::render_panel(
        frame,
        &ui.theme,
        rect,
        "help",
        Some(Line::from(format!("{} items", filtered_len))),
        true,
    );

    // 3. Construct the page's widget
    let help_table = Table::new(
        display_rows
            .into_iter()
            .skip(scroll_offset)
            .map(|row| {
                if row.is_section_break {
                    Row::new(vec![Cell::from(""), Cell::from("")]).height(1)
                } else {
                    let key_style = if row.is_section {
                        ui.theme.page_desc().add_modifier(Modifier::BOLD)
                    } else {
                        ui.theme.page_desc().add_modifier(Modifier::BOLD)
                    };
                    let description_style = if row.is_section {
                        ui.theme.playlist_desc().add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };

                    Row::new(vec![
                        Cell::from(Text::from(row.shortcuts).alignment(Alignment::Left))
                            .style(key_style),
                        Cell::from(row.description).style(description_style),
                    ])
                }
            })
            .collect::<Vec<_>>(),
        COMMAND_TABLE_CONSTRAINTS,
    );

    // 4. Render the page's widget
    frame.render_widget(help_table, rect);
}

struct HelpDisplayRow {
    shortcuts: String,
    description: String,
    is_section: bool,
    is_section_break: bool,
}

fn global_help_rows() -> Vec<(String, String)> {
    vec![
        (
            format_shortcuts(&["a", "m", "r", "s", "g", "u"]),
            "a actions, m mode, r radio, s sorting, g go, u user".to_string(),
        ),
        (
            format_shortcuts(&["/", "?", "esc", "tab"]),
            "/ search, ? preview/help, esc back, tab focus".to_string(),
        ),
        (
            "!, @, $".to_string(),
            "! album, @ artist, $ song filters".to_string(),
        ),
        (
            "#, %".to_string(),
            "# and % reserved for future grammar".to_string(),
        ),
    ]
}

fn build_help_display_rows(rows: Vec<HelpRow>) -> Vec<HelpDisplayRow> {
    let mut grouped = BTreeMap::<&'static str, Vec<HelpRow>>::new();
    for row in rows {
        grouped.entry(row.section).or_default().push(row);
    }

    let ordered_sections = [
        "First Keys",
        "Navigation",
        "Views",
        "Playback",
        "Library",
        "Sorting",
        "System",
    ];

    let mut display_rows = Vec::new();
    for section in ordered_sections {
        let Some(mut section_rows) = grouped.remove(section) else {
            continue;
        };
        if !display_rows.is_empty() {
            display_rows.push(HelpDisplayRow {
                shortcuts: String::new(),
                description: String::new(),
                is_section: false,
                is_section_break: true,
            });
        }
        display_rows.push(HelpDisplayRow {
            shortcuts: section.to_lowercase(),
            description: String::new(),
            is_section: true,
            is_section_break: false,
        });
        section_rows.sort_by(|a, b| a.shortcuts.cmp(&b.shortcuts));
        display_rows.extend(section_rows.into_iter().map(|row| HelpDisplayRow {
            shortcuts: row.shortcuts,
            description: row.description,
            is_section: false,
            is_section_break: false,
        }));
    }

    for (_section, section_rows) in grouped {
        if !display_rows.is_empty() {
            display_rows.push(HelpDisplayRow {
                shortcuts: String::new(),
                description: String::new(),
                is_section: false,
                is_section_break: true,
            });
        }
        display_rows.extend(section_rows.into_iter().map(|row| HelpDisplayRow {
            shortcuts: row.shortcuts,
            description: row.description,
            is_section: false,
            is_section_break: false,
        }));
    }

    display_rows
}

fn help_section(command: Command) -> &'static str {
    match command {
        Command::SelectNextOrScrollDown
        | Command::SelectPreviousOrScrollUp
        | Command::PageSelectNextOrScrollDown
        | Command::PageSelectPreviousOrScrollUp
        | Command::SelectFirstOrScrollToTop
        | Command::SelectLastOrScrollToBottom
        | Command::ChooseSelected
        | Command::FocusNextWindow
        | Command::FocusPreviousWindow
        | Command::PreviousPage
        | Command::JumpToCurrentTrackInContext
        | Command::JumpToHighlightTrackInContext => "Navigation",
        Command::LibraryPage
        | Command::SearchPage
        | Command::SearchTuiHome
        | Command::BrowsePage
        | Command::Queue
        | Command::OpenCommandHelp
        | Command::OpenLogs
        | Command::CurrentlyPlayingContextPage
        | Command::TopTrackPage
        | Command::RecentlyPlayedTrackPage
        | Command::LikedTrackPage
        | Command::LyricsPage
        | Command::GoExternalGlow => "Views",
        Command::Search
        | Command::BrowseUserPlaylists
        | Command::BrowseUserFollowedArtists
        | Command::BrowseUserSavedAlbums => "Library",
        Command::NextTrack
        | Command::PreviousTrack
        | Command::ResumePause
        | Command::PlayRandom
        | Command::Repeat
        | Command::Shuffle
        | Command::VolumeChange { .. }
        | Command::Mute
        | Command::SeekStart
        | Command::SeekForward { .. }
        | Command::SeekBackward { .. }
        | Command::RefreshPlayback
        | Command::GoToRadioFromSelectedItem
        | Command::GoToRadioFromCurrentTrack
        | Command::GoToRadioFromCurrentContext
        | Command::SwitchDevice
        | Command::ShowActionsOnSelectedItem
        | Command::ShowActionsOnCurrentTrack
        | Command::ShowActionsOnCurrentContext
        | Command::AddSelectedItemToQueue => "Playback",
        Command::SortTrackByTitle
        | Command::SortTrackByArtists
        | Command::SortTrackByAlbum
        | Command::SortTrackByDuration
        | Command::SortTrackByAddedDate
        | Command::ReverseTrackOrder
        | Command::SortLibraryAlphabetically
        | Command::SortLibraryByRecent
        | Command::MovePlaylistItemUp
        | Command::MovePlaylistItemDown => "Sorting",
        Command::Quit
        | Command::ClosePopup
        | Command::SwitchTheme
        | Command::CreatePlaylist
        | Command::OpenSpotifyLinkFromClipboard => "System",
        Command::None => "System",
        #[cfg(feature = "streaming")]
        Command::RestartIntegratedClient => "System",
    }
}

pub(crate) fn context_help_rows(
    context_page_type: &ContextPageType,
    context_state: Option<&ContextPageUIState>,
) -> Vec<(String, String)> {
    let mut rows = vec![
        (
            format_shortcuts(&["up", "down", "j", "k"]),
            "move selection in this pane".to_string(),
        ),
        (
            format_shortcuts(&["enter"]),
            match context_page_type {
                ContextPageType::CurrentPlaying => {
                    "open or start playback from the selected item".to_string()
                }
                ContextPageType::Browsing(id) => match id {
                    crate::state::ContextId::Show(_) => "play the selected episode".to_string(),
                    crate::state::ContextId::Artist(_) => {
                        "open the selected item or drill into the next pane".to_string()
                    }
                    crate::state::ContextId::Album(_)
                    | crate::state::ContextId::Playlist(_)
                    | crate::state::ContextId::Tracks(_) => {
                        "start playback from the selected item".to_string()
                    }
                },
            },
        ),
        (format_shortcuts(&["/"]), "search this context".to_string()),
    ];

    if context_supports_track_sigils(context_page_type, context_state) {
        rows.push((
            "!, @, $".to_string(),
            "! album, @ artist, $ song filters".to_string(),
        ));
    }

    rows.extend([
        (
            format_shortcuts(&["esc"]),
            "go back or close this view".to_string(),
        ),
        (format_shortcuts(&["?"]), "close this help".to_string()),
    ]);

    rows.extend(context_page_type_rows(context_page_type, context_state));

    rows
}

fn context_supports_track_sigils(
    context_page_type: &ContextPageType,
    context_state: Option<&ContextPageUIState>,
) -> bool {
    match context_page_type {
        ContextPageType::CurrentPlaying => false,
        ContextPageType::Browsing(crate::state::ContextId::Album(_))
        | ContextPageType::Browsing(crate::state::ContextId::Playlist(_))
        | ContextPageType::Browsing(crate::state::ContextId::Tracks(_)) => true,
        ContextPageType::Browsing(crate::state::ContextId::Artist(_)) => matches!(
            context_state,
            Some(ContextPageUIState::Artist {
                focus: ArtistFocusState::TopTracks,
                ..
            })
        ),
        ContextPageType::Browsing(crate::state::ContextId::Show(_)) => false,
    }
}

fn context_page_type_rows(
    context_page_type: &ContextPageType,
    context_state: Option<&ContextPageUIState>,
) -> Vec<(String, String)> {
    match context_page_type {
        ContextPageType::CurrentPlaying => Vec::new(),
        ContextPageType::Browsing(id) => match id {
            crate::state::ContextId::Show(_) => vec![
                (
                    format_shortcuts(&["r x"]),
                    "radio from this show".to_string(),
                ),
                (
                    format_shortcuts(&["a x"]),
                    "actions for this show".to_string(),
                ),
            ],
            crate::state::ContextId::Playlist(_) => vec![
                (
                    format_shortcuts(&["r x"]),
                    "radio from this playlist".to_string(),
                ),
                (
                    format_shortcuts(&["a x"]),
                    "actions for this playlist".to_string(),
                ),
            ],
            crate::state::ContextId::Album(_) => vec![
                (
                    format_shortcuts(&["r x"]),
                    "radio from this album".to_string(),
                ),
                (
                    format_shortcuts(&["a x"]),
                    "actions for this album".to_string(),
                ),
            ],
            crate::state::ContextId::Tracks(_) => Vec::new(),
            crate::state::ContextId::Artist(_) => {
                let mut rows = vec![
                    (
                        format_shortcuts(&["r x"]),
                        "radio from this artist".to_string(),
                    ),
                    (
                        format_shortcuts(&["a x"]),
                        "actions for this artist".to_string(),
                    ),
                ];

                if matches!(context_state, Some(ContextPageUIState::Artist { .. })) {
                    rows.splice(
                        0..0,
                        [
                            (
                                format_shortcuts(&["tab", "backtab"]),
                                "switch focus between top tracks, albums, and related artists"
                                    .to_string(),
                            ),
                            (
                                format_shortcuts(&["a s"]),
                                "actions for the selected item".to_string(),
                            ),
                            (
                                format_shortcuts(&["r s"]),
                                "radio from the selected item".to_string(),
                            ),
                        ],
                    );
                }

                rows
            }
        },
    }
}

fn format_shortcuts(shortcuts: &[&str]) -> String {
    shortcuts
        .iter()
        .filter_map(|shortcut| KeySequence::from_str(shortcut).map(|keys| keys.display_help()))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::{
        search_tui_query_input_rect, search_tui_text_rect, split_row_right_layout, Rect,
        SplitRowRightLayout, SplitRowRightMeta, SEARCH_TUI_QUERY_SHELF_HEIGHT,
    };
    use crate::ui::utils;

    #[test]
    fn right_meta_layout_expands_when_row_has_room() {
        let values = vec![SplitRowRightMeta::pair("a very long album name", "5m 8s")];
        let layout = split_row_right_layout(
            &values,
            Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 1,
            },
        );

        assert_eq!(layout.total_width, values[0].display_width());
    }

    #[test]
    fn paired_right_meta_aligns_album_against_duration() {
        let layout = SplitRowRightLayout {
            total_width: 28,
            trailing_width: 5,
        };

        let short = SplitRowRightMeta::pair("an album name", "3m 2s").render(layout);
        let long = SplitRowRightMeta::pair("a very long album name", "5m 8s").render(layout);

        assert_eq!(short.find("3m 2s"), long.find("5m 8s"));
    }

    #[test]
    fn paired_right_meta_truncates_album_to_keep_duration_visible() {
        let layout = SplitRowRightLayout {
            total_width: 24,
            trailing_width: 5,
        };

        let rendered = SplitRowRightMeta::pair("a very long album name", "5m 8s").render(layout);

        assert_eq!(rendered.chars().count(), 24);
        assert!(rendered.ends_with("5m 8s"));
        assert!(rendered.contains("..."));
    }

    #[test]
    fn search_tui_text_and_query_shelf_follow_shell_rhythm() {
        let cases = [(100, 32, 2), (120, 36, 4), (140, 40, 4), (180, 48, 22)];

        for (width, height, expected_text_x) in cases {
            let shell = utils::content_shell_rect(Rect::new(0, 0, width, height));
            let query_rect = Rect::new(shell.x, 7, shell.width, SEARCH_TUI_QUERY_SHELF_HEIGHT);
            let text_rect = search_tui_text_rect(shell);
            let input_rect = search_tui_query_input_rect(query_rect);

            assert_eq!(
                text_rect.x, expected_text_x,
                "unexpected SearchTui header inset for {width}x{height}"
            );
            assert_eq!(
                input_rect.x, expected_text_x,
                "unexpected SearchTui query inset for {width}x{height}"
            );
            assert_eq!(
                input_rect.width,
                shell.width.saturating_sub(2),
                "unexpected SearchTui query width for {width}x{height}"
            );
            assert_eq!(
                input_rect.y,
                query_rect.y + 1,
                "unexpected SearchTui query y for {width}x{height}"
            );
        }
    }
}

pub fn render_queue_page(
    frame: &mut Frame,
    state: &SharedState,
    ui: &mut UIStateGuard,
    rect: Rect,
) {
    use rspotify::model::{FullEpisode, FullTrack, PlayableItem};
    fn get_playable_name(item: &PlayableItem) -> String {
        match item {
            PlayableItem::Track(FullTrack { ref name, .. })
            | PlayableItem::Episode(FullEpisode { ref name, .. }) => name.clone(),
            PlayableItem::Unknown(_) => String::new(),
        }
    }
    fn get_playable_artists(item: &PlayableItem) -> String {
        match item {
            PlayableItem::Track(FullTrack { ref artists, .. }) => artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            PlayableItem::Episode(FullEpisode { ref show, .. }) => show.publisher.clone(),
            PlayableItem::Unknown(_) => String::new(),
        }
    }
    fn get_playable_duration(item: &PlayableItem) -> String {
        match item {
            PlayableItem::Track(FullTrack { ref duration, .. })
            | PlayableItem::Episode(FullEpisode { ref duration, .. }) => {
                format_duration_hms(duration)
            }
            PlayableItem::Unknown(_) => String::new(),
        }
    }
    fn get_playable_album(item: &PlayableItem) -> String {
        match item {
            PlayableItem::Track(FullTrack { album, .. }) => album.name.clone(),
            PlayableItem::Episode(FullEpisode { show, .. }) => show.name.clone(),
            PlayableItem::Unknown(_) => String::new(),
        }
    }

    // 1. Get data
    let player = state.player.read();
    let queue = player.queue.as_ref().map(|q| &q.queue);
    let scroll_offset = match ui.current_page_mut() {
        PageState::Queue {
            ref mut scroll_offset,
        } => {
            if let Some(queue) = queue {
                if !queue.is_empty() && *scroll_offset >= queue.len() {
                    *scroll_offset = queue.len() - 1;
                }
            }
            *scroll_offset
        }
        _ => return,
    };

    let queue_len = queue.map_or(0, Vec::len);
    let rect = utils::render_panel(
        frame,
        &ui.theme,
        rect,
        "queue",
        Some(Line::from(format!("{queue_len} items"))),
        true,
    );

    let Some(queue) = queue else {
        frame.render_widget(
            Paragraph::new("Queue is empty").style(ui.theme.playback_metadata()),
            rect,
        );
        return;
    };

    // 3. Construct the page's widget
    let queue_items = queue.iter().skip(scroll_offset).collect::<Vec<_>>();
    let queue_right_meta = queue_items
        .iter()
        .map(|item| {
            let album = get_playable_album(item);
            let duration = get_playable_duration(item);
            if album.is_empty() {
                SplitRowRightMeta::plain(duration)
            } else {
                SplitRowRightMeta::pair(to_bidi_string(&album), duration)
            }
        })
        .collect::<Vec<_>>();
    let queue_right_layout = split_row_right_layout(&queue_right_meta, rect);
    let queue_table = Table::new(
        queue_items
            .into_iter()
            .zip(queue_right_meta)
            .enumerate()
            .map(|(i, (item, right_meta))| {
                let left = Line::from(vec![
                    Span::raw(to_bidi_string(&get_playable_name(item))),
                    Span::styled(" - ", ui.theme.playlist_desc()),
                    Span::styled(
                        to_bidi_string(&get_playable_artists(item)),
                        ui.theme.playlist_desc(),
                    ),
                ]);
                Row::new(vec![
                    Cell::from(left),
                    Cell::from(right_meta.render(queue_right_layout))
                        .style(ui.theme.playlist_desc()),
                ])
                .style(if (i + scroll_offset) % 2 == 0 {
                    ui.theme.secondary_row()
                } else {
                    Style::default()
                })
            })
            .collect::<Vec<_>>(),
        [
            Constraint::Fill(1),
            Constraint::Length(queue_right_layout.total_width),
        ],
    )
    .column_spacing(1);

    // 4. Render page's widget
    frame.render_widget(queue_table, rect);
}

/// Render windows for an artist context page, which includes
/// - A top track table
/// - An album table
/// - A related artist list
fn render_artist_context_page_windows(
    is_active: bool,
    frame: &mut Frame,
    state: &SharedState,
    ui: &mut UIStateGuard,
    data: &DataReadGuard,
    rect: Rect,
    artist_data: (&[Track], &[Album], &[Artist]),
) {
    // 1. Get data
    let (tracks, albums, artists) = (
        ui.search_filtered_tracks(artist_data.0),
        ui.search_filtered_items(artist_data.1),
        ui.search_filtered_items(artist_data.2),
    );

    let focus_state = match ui.current_page() {
        PageState::Context {
            state: Some(ContextPageUIState::Artist { focus, .. }),
            ..
        } => *focus,
        _ => return,
    };

    // 2. Construct the page's layout
    // top tracks window
    let chunks = Layout::vertical([Constraint::Fill(1), Constraint::Fill(1)]).split(rect);
    let top_tracks_rect = chunks[0];
    let is_albums_active = is_active && focus_state == ArtistFocusState::Albums;

    // albums and related artitsts windows
    let chunks = Layout::horizontal([Constraint::Ratio(1, 2); 2]).split(chunks[1]);
    let albums_rect = utils::render_panel(
        frame,
        &ui.theme,
        chunks[0],
        "albums",
        Some(Line::from(format!("{} items", albums.len()))),
        is_albums_active,
    );
    let related_artists_rect = utils::render_panel(
        frame,
        &ui.theme,
        chunks[1],
        "related artists",
        Some(Line::from(format!("{} items", artists.len()))),
        is_active && focus_state == ArtistFocusState::RelatedArtists,
    );

    // 3. Construct the page's widgets
    // album table
    let n_albums = albums.len();
    let album_rows = albums
        .into_iter()
        .map(|a| {
            Row::new(vec![
                Cell::from(a.release_date.clone()),
                Cell::from(a.album_type()),
                Cell::from(a.name.clone()),
            ])
            .style(Style::default())
        })
        .collect::<Vec<_>>();

    let albums_table = Table::new(
        album_rows,
        [
            Constraint::Length(10),
            Constraint::Length(6),
            Constraint::Fill(1),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("Date"),
            Cell::from("Type"),
            Cell::from("Name"),
        ])
        .style(ui.theme.table_header()),
    )
    .column_spacing(2)
    .highlight_symbol(utils::highlight_symbol(&ui.theme, is_albums_active))
    .row_highlight_style(ui.theme.selection(is_albums_active));

    // artist list widget
    let (artist_list, n_artists) = {
        let artist_items = artists
            .into_iter()
            .map(|a| (a.name.clone(), false))
            .collect::<Vec<_>>();

        utils::construct_list_widget(
            &ui.theme,
            artist_items,
            is_active && focus_state == ArtistFocusState::RelatedArtists,
        )
    };

    // 4. Render the page's widgets
    let top_tracks_rect = utils::render_panel(
        frame,
        &ui.theme,
        top_tracks_rect,
        "top tracks",
        Some(Line::from(format!("{} items", tracks.len()))),
        is_active && focus_state == ArtistFocusState::TopTracks,
    );
    render_track_table(
        frame,
        top_tracks_rect,
        is_active && focus_state == ArtistFocusState::TopTracks,
        state,
        tracks,
        ui,
        data,
    );

    let PageState::Context {
        state:
            Some(ContextPageUIState::Artist {
                album_table,
                related_artist_list,
                ..
            }),
        ..
    } = ui.current_page_mut()
    else {
        return;
    };

    utils::render_table_window(frame, albums_table, albums_rect, n_albums, album_table);
    utils::render_list_window(
        frame,
        artist_list,
        related_artists_rect,
        n_artists,
        related_artist_list,
    );
}

fn render_track_table(
    frame: &mut Frame,
    rect: Rect,
    is_active: bool,
    state: &SharedState,
    tracks: Vec<&Track>,
    ui: &mut UIStateGuard,
    data: &DataReadGuard,
) {
    let configs = config::get_config();
    let track_table_style = match ui.current_page() {
        PageState::Context {
            context_page_type: ContextPageType::Browsing(crate::state::ContextId::Tracks(id)),
            ..
        } if id.kind.starts_with("radio:") => TrackTableStyle::Radio,
        PageState::Context {
            context_page_type: ContextPageType::Browsing(crate::state::ContextId::Album(_)),
            ..
        } => TrackTableStyle::Album,
        _ => TrackTableStyle::Detailed,
    };

    // get the current playing track's URI to decorate such track (if exists) in the track table
    let mut playing_track_uri = String::new();
    if let Some(ref playback) = state.player.read().playback {
        if let Some(rspotify::model::PlayableItem::Track(ref track)) = playback.item {
            playing_track_uri = track
                .id
                .as_ref()
                .map(rspotify::prelude::Id::uri)
                .unwrap_or_default();
        }
    }

    let include_album_meta = !matches!(track_table_style, TrackTableStyle::Album);
    let right_meta = tracks
        .iter()
        .map(|track| track_right_meta(track, include_album_meta))
        .collect::<Vec<_>>();
    let right_layout = split_row_right_layout(&right_meta, rect);
    let rows = tracks
        .into_iter()
        .zip(right_meta)
        .map(|(track, right_meta)| {
            let is_current = playing_track_uri == track.id.uri();
            let row_style = if is_current {
                ui.theme.current_playing()
            } else {
                Style::default()
            };
            let right_style = if is_current {
                ui.theme.current_playing()
            } else {
                ui.theme.playlist_desc()
            };
            Row::new(vec![
                if data.user_data.is_liked_track(track) {
                    Cell::from(&configs.app_config.liked_icon as &str).style(ui.theme.like())
                } else {
                    Cell::from("")
                },
                Cell::from(track_left_line(track, ui, is_current)),
                Cell::from(right_meta.render(right_layout)).style(right_style),
            ])
            .style(row_style)
        })
        .collect::<Vec<_>>();
    let len = rows.len();
    let track_table = Table::new(
        rows,
        [
            Constraint::Length(configs.app_config.liked_icon.chars().count() as u16),
            Constraint::Fill(1),
            Constraint::Length(right_layout.total_width),
        ],
    )
    .column_spacing(1)
    .highlight_symbol(utils::highlight_symbol(&ui.theme, is_active))
    .row_highlight_style(ui.theme.selection(is_active));

    if let PageState::Context {
        state: Some(state), ..
    } = ui.current_page_mut()
    {
        let playable_table_state = match state {
            ContextPageUIState::Artist {
                top_track_table, ..
            } => top_track_table,
            ContextPageUIState::Playlist { track_table }
            | ContextPageUIState::Album { track_table }
            | ContextPageUIState::Tracks { track_table } => track_table,
            ContextPageUIState::Show { .. } => {
                unreachable!("show's episode table should be handled by render_episode_table")
            }
        };
        utils::render_table_window(frame, track_table, rect, len, playable_table_state);
    }
}

enum TrackTableStyle {
    Detailed,
    Album,
    Radio,
}

fn render_episode_table(
    frame: &mut Frame,
    rect: Rect,
    is_active: bool,
    state: &SharedState,
    episodes: Vec<&Episode>,
    ui: &mut UIStateGuard,
) {
    let configs = config::get_config();
    // get the current playing episode's URI to decorate such episode (if exists) in the episode table
    let mut playing_episode_uri = String::new();
    let mut playing_id = "";
    if let Some(ref playback) = state.player.read().playback {
        if let Some(rspotify::model::PlayableItem::Episode(ref episode)) = playback.item {
            playing_episode_uri = episode.id.uri();

            playing_id = if playback.is_playing {
                &configs.app_config.play_icon
            } else {
                &configs.app_config.pause_icon
            };
        }
    }

    let n_episodes = episodes.len();
    let rows = episodes
        .into_iter()
        .enumerate()
        .map(|(id, e)| {
            let (id, style) = if playing_episode_uri == e.id.uri() {
                (playing_id.to_string(), ui.theme.current_playing())
            } else {
                ((id + 1).to_string(), Style::default())
            };
            Row::new(vec![
                Cell::from(id),
                Cell::from(to_bidi_string(&e.name)),
                Cell::from(e.release_date.clone()),
                Cell::from(format_duration_hms(
                    &chrono::Duration::from_std(e.duration).unwrap_or_default(),
                )),
            ])
            .style(style)
        })
        .collect::<Vec<_>>();
    let episode_table = Table::new(
        rows,
        [
            Constraint::Length(4),
            Constraint::Fill(6),
            Constraint::Fill(2),
            Constraint::Fill(1),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("#"),
            Cell::from("Title"),
            Cell::from("Date"),
            Cell::from("Duration"),
        ])
        .style(ui.theme.table_header()),
    )
    .column_spacing(2)
    .highlight_symbol(utils::highlight_symbol(&ui.theme, is_active))
    .row_highlight_style(ui.theme.selection(is_active));

    if let PageState::Context {
        state: Some(state), ..
    } = ui.current_page_mut()
    {
        let playable_table_state = match state {
            ContextPageUIState::Show { episode_table } => episode_table,
            s => unreachable!("unexpected state: {s:?}"),
        };
        utils::render_table_window(frame, episode_table, rect, n_episodes, playable_table_state);
    }
}

pub fn render_logs_page(frame: &mut Frame, state: &SharedState, ui: &mut UIStateGuard, rect: Rect) {
    let rect = utils::render_panel(
        frame,
        &ui.theme,
        rect,
        "logs",
        Some(Line::from("most recent first visible by scroll")),
        true,
    );

    let logs = state.logs.lock();
    let scroll_offset = match ui.current_page_mut() {
        PageState::Logs { scroll_offset } => {
            if !logs.is_empty() && *scroll_offset >= logs.len() {
                *scroll_offset = logs.len() - 1;
            }
            *scroll_offset
        }
        _ => return,
    };

    let lines: Vec<Line> = logs
        .iter()
        .skip(scroll_offset)
        .map(|line| {
            let style = if line.contains("ERROR") {
                Style::default().fg(ratatui::style::Color::Red)
            } else if line.contains("WARN") {
                Style::default().fg(ratatui::style::Color::Yellow)
            } else {
                Style::default()
            };
            Line::styled(line, style)
        })
        .collect();

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, rect);
}
