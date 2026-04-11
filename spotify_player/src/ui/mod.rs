use crate::{
    config,
    state::{
        Album, Artist, ArtistFocusState, BrowsePageUIState, Context, ContextPageUIState,
        DataReadGuard, ExternalLaunchRequest, Id, LibraryFocusState, MutableWindowState, PageState,
        PageType, PlaybackMetadata, PlaylistCreateCurrentField, PlaylistFolderItem,
        PlaylistPopupAction, PopupState, SearchFocusState, SearchTuiMode, SharedState, Track,
        UIStateGuard,
    },
};
use anyhow::{Context as AnyhowContext, Result};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState, Wrap},
    Frame,
};

#[cfg(feature = "image")]
use crate::state::ImageRenderInfo;

type Terminal = ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>;

pub(crate) mod page;
mod playback;
mod popup;
pub mod single_line_input;
#[cfg(feature = "streaming")]
pub mod streaming;
pub mod utils;

const INTERACTION_GRACE_PERIOD: std::time::Duration = std::time::Duration::from_millis(400);
const SHORTCUT_PREFIX_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1200);
const LOADING_REFRESH_DURATION: std::time::Duration = std::time::Duration::from_millis(100);
const PLAYBACK_REFRESH_FLOOR: std::time::Duration = std::time::Duration::from_millis(250);
const IDLE_REFRESH_DURATION: std::time::Duration = std::time::Duration::from_millis(1000);

/// Run the application UI
pub fn run(state: &SharedState) -> Result<()> {
    let mut terminal = init_ui().context("failed to initialize the application's UI")?;

    let mut last_terminal_size = None;

    loop {
        let next_refresh_duration = {
            let mut ui = state.ui.lock();
            if !ui.is_running {
                let pending_external_launch = ui.pending_external_launch.take();
                drop(ui);
                clean_up(terminal).context("clean up UI resources")?;
                if let Some(request) = pending_external_launch {
                    launch_external_after_exit(request)?;
                }
                std::process::exit(0);
            }

            if !ui.input_key_sequence.keys.is_empty()
                && ui.last_interaction_at.elapsed() >= SHORTCUT_PREFIX_TIMEOUT
            {
                ui.input_key_sequence.keys.clear();
            }

            let terminal_size = terminal.size()?;
            if Some(terminal_size) != last_terminal_size {
                last_terminal_size = Some(terminal_size);
                #[cfg(feature = "image")]
                {
                    // redraw the cover image when the terminal's size changes
                    ui.last_cover_image_render_info = ImageRenderInfo::default();
                }
            }

            if let Err(err) = terminal.draw(|frame| {
                // set the background and foreground colors for the application
                let rect = frame.area();
                let block = Block::default().style(ui.theme.app());
                frame.render_widget(block, rect);

                render_application(frame, state, &mut ui, rect);
            }) {
                tracing::error!("Failed to render the application: {err:#}");
            }
            next_refresh_duration(state, &ui)
        };

        state.wait_for_redraw(next_refresh_duration);
    }
}

fn next_refresh_duration(state: &SharedState, ui: &UIStateGuard) -> std::time::Duration {
    let configured_refresh_duration = std::time::Duration::from_millis(
        config::get_config()
            .app_config
            .app_refresh_duration_in_ms
            .max(1),
    );

    if !ui.input_key_sequence.keys.is_empty() {
        return configured_refresh_duration;
    }

    if ui.last_interaction_at.elapsed() <= INTERACTION_GRACE_PERIOD {
        return configured_refresh_duration;
    }

    #[cfg(feature = "streaming")]
    if config::get_config().app_config.enable_audio_visualization
        && state.is_local_streaming_active()
    {
        return configured_refresh_duration;
    }

    let player = state.player.read();
    if player.playback_last_updated_time.is_none() {
        return configured_refresh_duration.max(LOADING_REFRESH_DURATION);
    }

    if player
        .buffered_playback
        .as_ref()
        .is_some_and(|playback| playback.is_playing)
    {
        return configured_refresh_duration.max(PLAYBACK_REFRESH_FLOOR);
    }

    IDLE_REFRESH_DURATION
}

// initialize the application's UI
fn init_ui() -> Result<Terminal> {
    let mut stdout = std::io::stdout();
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;
    terminal.clear()?;
    Ok(terminal)
}

/// Clean up UI resources before quitting the application
fn clean_up(mut terminal: Terminal) -> Result<()> {
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn launch_external_after_exit(request: ExternalLaunchRequest) -> Result<()> {
    let mut command = std::process::Command::new(&request.command);
    command.args(&request.args);
    command.stdin(std::process::Stdio::inherit());
    command.stdout(std::process::Stdio::inherit());
    command.stderr(std::process::Stdio::inherit());
    for (key, value) in &request.env {
        command.env(key, value);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Replace jx-spotify with the target command so foreground terminal
        // ownership stays with the handoff process.
        let err = command.exec();
        return Err(err).with_context(|| {
            format!(
                "failed to exec external command after terminal cleanup: `{}`",
                request.command
            )
        });
    }

    #[cfg(not(unix))]
    {
        command.spawn().with_context(|| {
            format!(
                "failed to launch external command after terminal cleanup: `{}`",
                request.command
            )
        })?;
        Ok(())
    }
}

/// Render the application
fn render_application(frame: &mut Frame, state: &SharedState, ui: &mut UIStateGuard, rect: Rect) {
    // rendering order: footer chrome -> shortcut help popup -> other popups -> main layout

    let footer_rows: u16 = if ui.footer_help_preview_visible { 2 } else { 1 };
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(0),
        Constraint::Length(footer_rows),
    ])
    .split(rect);

    let top_shell = utils::content_shell_rect(chunks[0]);
    let body_shell = utils::content_shell_rect(chunks[1]);
    let footer_shell = utils::content_shell_rect(chunks[2]);

    render_app_chrome(frame, state, ui, top_shell, footer_shell);

    let rect = popup::render_shortcut_help_popup(frame, state, ui, body_shell);

    let (rect, is_active) = popup::render_popup(frame, state, ui, rect);

    render_main_layout(is_active, frame, state, ui, rect);
}

/// Render the application's main layout
fn render_main_layout(
    is_active: bool,
    frame: &mut Frame,
    state: &SharedState,
    ui: &mut UIStateGuard,
    rect: Rect,
) {
    let page_type = ui.current_page().page_type();
    match page_type {
        PageType::Library => page::render_library_page(is_active, frame, state, ui, rect),
        PageType::Search => page::render_search_page(is_active, frame, state, ui, rect),
        PageType::SearchTui => page::render_search_tui_page(is_active, frame, state, ui, rect),
        PageType::Context => page::render_context_page(is_active, frame, state, ui, rect),
        PageType::Browse => page::render_browse_page(is_active, frame, state, ui, rect),
        PageType::Lyrics => page::render_lyrics_page(is_active, frame, state, ui, rect),
        PageType::Queue => page::render_queue_page(frame, state, ui, rect),
        PageType::CommandHelp => page::render_commands_help_page(frame, state, ui, rect),
        PageType::Logs => page::render_logs_page(frame, state, ui, rect),
    }
}

fn render_app_chrome(
    frame: &mut Frame,
    state: &SharedState,
    ui: &mut UIStateGuard,
    top: Rect,
    footer: Rect,
) {
    let badge = " jx-spotify ";
    let badge_width = badge.chars().count() as u16;
    let top_rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(top);
    frame.render_widget(Paragraph::new(""), top_rows[1]);
    let header_text_rect = utils::shell_text_rect(top_rows[1]);
    let badge_rect = utils::app_badge_rect(top_rows[1], badge_width);
    let detail_x = badge_rect.x.saturating_add(badge_rect.width);
    let detail_width = header_text_rect
        .x
        .saturating_add(header_text_rect.width)
        .saturating_sub(detail_x);
    let detail_rect = Rect::new(
        detail_x,
        header_text_rect.y,
        detail_width,
        header_text_rect.height,
    );
    frame.render_widget(
        Paragraph::new(Span::styled(badge, ui.theme.app_title_badge())),
        badge_rect,
    );
    let header_line = app_header_line(ui, detail_rect.width as usize);
    frame.render_widget(
        Paragraph::new(Span::styled(header_line, ui.theme.playback_metadata())),
        detail_rect,
    );

    if ui.footer_help_preview_visible {
        let footer_chunks =
            Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(footer);
        frame.render_widget(
            Paragraph::new(footer_help_preview_spans(ui)),
            footer_chunks[0],
        );
        let bottom_chunks = Layout::horizontal([Constraint::Fill(1), Constraint::Length(28)])
            .split(footer_chunks[1]);
        if let Some(now_playing) = playback::footer_now_playing_line(state, ui) {
            frame.render_widget(Paragraph::new(now_playing), bottom_chunks[0]);
            ui.playback_progress_bar_rect = bottom_chunks[0];
        } else {
            frame.render_widget(Paragraph::new(""), bottom_chunks[0]);
            ui.playback_progress_bar_rect = Rect::default();
        }
        frame.render_widget(
            Paragraph::new(app_key_hint_spans(ui)).alignment(Alignment::Right),
            bottom_chunks[1],
        );
    } else {
        let bottom_chunks =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(28)]).split(footer);
        if let Some(now_playing) = playback::footer_now_playing_line(state, ui) {
            frame.render_widget(Paragraph::new(now_playing), bottom_chunks[0]);
            ui.playback_progress_bar_rect = bottom_chunks[0];
        } else {
            frame.render_widget(Paragraph::new(""), bottom_chunks[0]);
            ui.playback_progress_bar_rect = Rect::default();
        }
        frame.render_widget(
            Paragraph::new(app_key_hint_spans(ui)).alignment(Alignment::Right),
            bottom_chunks[1],
        );
    }
}

fn app_header_line(ui: &UIStateGuard, max_chars: usize) -> String {
    let (context, detail) = app_header_context_detail(ui);
    compose_header_line(&context, &detail, max_chars)
}

fn app_header_context_detail(ui: &UIStateGuard) -> (String, String) {
    match ui.current_page() {
        PageState::Library { .. } => ("Library".to_string(), "playlists".to_string()),
        PageState::Search { current_query, .. } => {
            let detail = if current_query.is_empty() {
                "results".to_string()
            } else {
                format!("results/{current_query}")
            };
            ("Search".to_string(), detail)
        }
        PageState::SearchTui {
            state, line_input, ..
        } => {
            let mode = match &state.mode {
                SearchTuiMode::Global => "global".to_string(),
                SearchTuiMode::Playlist { title, .. } => format!("playlist/{title}"),
                SearchTuiMode::Album { title, .. } => format!("album/{title}"),
                SearchTuiMode::Artist { title, .. } => format!("artist/{title}"),
            };
            let query = line_input.get_text();
            let detail = if query.is_empty() {
                mode
            } else {
                format!("{mode}/{query}")
            };
            ("Search".to_string(), detail)
        }
        PageState::Context {
            context_page_type, ..
        } => ("Context".to_string(), context_page_type.title()),
        PageState::Browse { .. } => ("Browse".to_string(), "catalog".to_string()),
        PageState::Lyrics { track, artists, .. } => {
            ("Lyrics".to_string(), format!("{track}/{artists}"))
        }
        PageState::Queue { .. } => ("Queue".to_string(), "up next".to_string()),
        PageState::CommandHelp { .. } => ("Commands".to_string(), "families".to_string()),
        PageState::Logs { .. } => ("Logs".to_string(), "session".to_string()),
    }
}

fn compose_header_line(context: &str, detail: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let context = truncate_with_ellipsis(context, max_chars);
    if detail.is_empty() || context.chars().count() + 1 >= max_chars {
        return context;
    }

    let detail_chars = max_chars.saturating_sub(context.chars().count() + 1);
    let detail = truncate_with_ellipsis(detail, detail_chars);
    if detail.is_empty() {
        return context;
    }

    format!("{context} {detail}")
}

fn truncate_with_ellipsis(value: &str, max_chars: usize) -> String {
    let length = value.chars().count();
    if length <= max_chars {
        return value.to_string();
    }

    if max_chars == 0 {
        return String::new();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let keep = max_chars - 3;
    let prefix = value.chars().take(keep).collect::<String>();
    format!("{prefix}...")
}

fn footer_help_preview_spans(ui: &UIStateGuard) -> Line<'static> {
    let key = ui.theme.page_desc().add_modifier(Modifier::BOLD);
    let label = ui.theme.playback_metadata();
    Line::from(vec![
        Span::styled("a", key),
        Span::styled(" actions, ", label),
        Span::styled("g", key),
        Span::styled(" go, ", label),
        Span::styled("m", key),
        Span::styled(" mode, ", label),
        Span::styled("r", key),
        Span::styled(" radio, ", label),
        Span::styled("s", key),
        Span::styled(" sorting, ", label),
        Span::styled("u", key),
        Span::styled(" user, ", label),
        Span::styled("/", key),
        Span::styled(" search, ", label),
        Span::styled("esc", key),
        Span::styled(" back, ", label),
        Span::styled("tab", key),
        Span::styled(" focus, ", label),
        Span::styled("?", key),
        Span::styled(" full help", label),
    ])
}

fn app_key_hint_spans(ui: &UIStateGuard) -> Line<'static> {
    if let Some((family, children)) = popup::pending_shortcut_family_hint(ui) {
        return pending_family_key_hint_spans(ui, &family, &children);
    }

    let key = ui.theme.page_desc().add_modifier(Modifier::BOLD);
    let label = ui.theme.playback_metadata();
    Line::from(vec![
        Span::styled("a", key),
        Span::styled(" actions ", label),
        Span::styled("g", key),
        Span::styled(" go ", label),
        Span::styled("m", key),
        Span::styled(" mode ", label),
        Span::styled("?", key),
        Span::styled(" help", label),
    ])
}

fn pending_family_key_hint_spans(ui: &UIStateGuard, family: &str, children: &[String]) -> Line<'static> {
    const MAX_CHILDREN: usize = 4;

    let key = ui.theme.page_desc().add_modifier(Modifier::BOLD);
    let label = ui.theme.playback_metadata();
    let mut spans = vec![Span::styled(format!("{family}:"), key)];

    for child in children.iter().take(MAX_CHILDREN) {
        spans.push(Span::styled(" ", label));
        spans.push(Span::styled(child.clone(), key));
    }

    let hidden = children.len().saturating_sub(MAX_CHILDREN);
    if hidden > 0 {
        spans.push(Span::styled(format!(" +{hidden}"), label));
    }

    spans.push(Span::styled(" ", label));
    spans.push(Span::styled("esc", key));
    spans.push(Span::styled(" cancel", label));
    Line::from(spans)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Orientation {
    Vertical,
    #[default]
    Horizontal,
}

impl Orientation {
    /// Construct screen orientation based on the terminal's size
    pub fn from_size(columns: u16, rows: u16) -> Self {
        let ratio = f64::from(columns) / f64::from(rows);

        // a larger ratio has to be used since terminal cells aren't square
        if ratio > 2.3 {
            Self::Horizontal
        } else {
            Self::Vertical
        }
    }

    pub fn layout<I>(self, constraints: I) -> Layout
    where
        I: IntoIterator,
        I::Item: Into<Constraint>,
    {
        match self {
            Self::Vertical => Layout::vertical(constraints),
            Self::Horizontal => Layout::horizontal(constraints),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{app_key_hint_spans, compose_header_line};
    use crate::{
        command::Command,
        key::{Key, KeySequence},
        state::{PopupState, ShortcutFamilyItem, UIState},
    };
    use crossterm::event::KeyCode;
    use parking_lot::Mutex;
    use ratatui::{text::Line, widgets::ListState};

    #[test]
    fn header_line_truncates_detail_before_context() {
        let line = compose_header_line("Search", "playlist/a-very-very-long-name", 20);
        assert!(line.starts_with("Search "));
        assert_eq!(line.chars().count(), 20);
        assert!(line.ends_with("..."));
    }

    #[test]
    fn header_line_truncates_context_when_width_is_tight() {
        let line = compose_header_line("Commands", "families", 6);
        assert_eq!(line, "Com...");
    }

    #[test]
    fn header_line_omits_separator_when_detail_is_empty() {
        let line = compose_header_line("Library", "", 20);
        assert_eq!(line, "Library");
    }

    #[test]
    fn app_key_hint_defaults_to_family_first_idle_prompt() {
        let ui = Mutex::new(UIState::default());
        let ui = ui.lock();
        let line = app_key_hint_spans(&ui);
        assert_eq!(line_plain(&line), "a actions g go m mode ? help");
    }

    #[test]
    fn app_key_hint_shows_pending_family_children_when_popup_is_open() {
        let ui = Mutex::new(UIState::default());
        let mut ui = ui.lock();
        ui.popup = Some(PopupState::ShortcutFamily {
            title: "go to".to_string(),
            prefix: KeySequence {
                keys: vec![Key::None(KeyCode::Char('g'))],
            },
            items: vec![
                shortcut_item('c', "g c", Command::CurrentlyPlayingContextPage),
                shortcut_item('t', "g t", Command::TopTrackPage),
                shortcut_item('r', "g r", Command::RecentlyPlayedTrackPage),
                shortcut_item('y', "g y", Command::LikedTrackPage),
                shortcut_item('l', "g l", Command::LibraryPage),
            ],
            list_state: ListState::default(),
        });

        let line = app_key_hint_spans(&ui);
        assert_eq!(line_plain(&line), "g: c t r y +1 esc cancel");
    }

    fn shortcut_item(key: char, key_sequence: &str, command: Command) -> ShortcutFamilyItem {
        ShortcutFamilyItem {
            trigger: KeySequence {
                keys: vec![Key::None(KeyCode::Char(key))],
            },
            key_sequence: key_sequence.into(),
            command,
            has_children: false,
        }
    }

    fn line_plain(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }
}
