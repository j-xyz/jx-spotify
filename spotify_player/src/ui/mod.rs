use crate::{
    config,
    state::{
        Album, Artist, ArtistFocusState, BrowsePageUIState, Context, ContextPageUIState,
        DataReadGuard, Id, LibraryFocusState, MutableWindowState, PageState, PageType,
        PlaybackMetadata, PlaylistCreateCurrentField, PlaylistFolderItem, PlaylistPopupAction,
        PopupState, SearchFocusState, SearchTuiMode, SharedState, Track, UIStateGuard,
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
                clean_up(terminal).context("clean up UI resources")?;
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

/// Render the application
fn render_application(frame: &mut Frame, state: &SharedState, ui: &mut UIStateGuard, rect: Rect) {
    // rendering order: footer chrome -> shortcut help popup -> other popups -> main layout

    let footer_rows: u16 = if ui.footer_help_preview_visible { 2 } else { 1 };
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(0),
        Constraint::Length(footer_rows),
    ])
    .split(rect);

    render_app_chrome(frame, state, ui, chunks[0], chunks[2]);

    let rect = popup::render_shortcut_help_popup(frame, state, ui, chunks[1]);

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
    let top_chunks = Layout::horizontal([Constraint::Length(11), Constraint::Fill(0)]).split(top);
    let badge_style = Style::default()
        .fg(ratatui::style::Color::Rgb(0x0f, 0x14, 0x19))
        .bg(ratatui::style::Color::Rgb(0xff, 0x79, 0xc6))
        .add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new(Span::styled("jx-spotify", badge_style)),
        top_chunks[0],
    );
    frame.render_widget(Paragraph::new(""), top_chunks[1]);

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

fn footer_help_preview_spans(ui: &UIStateGuard) -> Line<'static> {
    let key = ui.theme.page_desc().add_modifier(Modifier::BOLD);
    let label = ui.theme.playback_metadata();
    Line::from(vec![
        Span::styled("a", key),
        Span::styled(" action, ", label),
        Span::styled("g", key),
        Span::styled(" go, ", label),
        Span::styled("m", key),
        Span::styled(" mode, ", label),
        Span::styled("r", key),
        Span::styled(" radio, ", label),
        Span::styled("s", key),
        Span::styled(" sort, ", label),
        Span::styled("u", key),
        Span::styled(" user, ", label),
        Span::styled("/", key),
        Span::styled(" search, ", label),
        Span::styled("esc", key),
        Span::styled(" clear/back, ", label),
        Span::styled("tab", key),
        Span::styled(" move focus, ", label),
        Span::styled("?", key),
        Span::styled(" help", label),
    ])
}

fn app_key_hint_spans(ui: &UIStateGuard) -> Line<'static> {
    let key = ui.theme.page_desc().add_modifier(Modifier::BOLD);
    let label = ui.theme.playback_metadata();
    Line::from(vec![
        Span::styled("/", key),
        Span::styled(" search ", label),
        Span::styled("?", key),
        Span::styled(" help", label),
    ])
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
