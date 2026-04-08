use crate::{command::Command, utils::filtered_items_from_query};
use crossterm::event::KeyCode;

use super::{
    config, utils, Cell, Constraint, Frame, Layout, Line, Paragraph, PlaylistCreateCurrentField,
    PageState, PageType, PlaylistPopupAction, PopupState, Rect, Row, SearchTuiMode, SharedState,
    Span, Table, UIStateGuard,
};

const SHORTCUT_TABLE_N_COLUMNS: usize = 3;
const SHORTCUT_TABLE_CONSTRAINS: [Constraint; SHORTCUT_TABLE_N_COLUMNS] =
    [Constraint::Ratio(1, 3); 3];

/// Render a popup (if any) to handle a command or show additional information
/// depending on the current popup state.
///
/// The function returns a rectangle area to render the main layout and
/// a boolean value determining whether the focus should be placed in the main layout.
pub fn render_popup(
    frame: &mut Frame,
    state: &SharedState,
    ui: &mut UIStateGuard,
    rect: Rect,
) -> (Rect, bool) {
    match ui.popup {
        None => (rect, true),
        Some(ref popup) => match popup {
            PopupState::PlaylistCreate {
                name,
                desc,
                current_field,
            } => {
                let chunks =
                    Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).split(rect);

                let popup_chunks =
                    Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .split(chunks[1]);

                let name_input = utils::render_panel(
                    frame,
                    &ui.theme,
                    popup_chunks[0],
                    "playlist name",
                    None,
                    true,
                );

                let desc_input = utils::render_panel(
                    frame,
                    &ui.theme,
                    popup_chunks[1],
                    "description",
                    None,
                    true,
                );

                frame.render_widget(
                    name.widget(PlaylistCreateCurrentField::Name == *current_field),
                    name_input,
                );
                frame.render_widget(
                    desc.widget(PlaylistCreateCurrentField::Desc == *current_field),
                    desc_input,
                );
                (chunks[0], true)
            }
            PopupState::Search { query } => {
                let chunks =
                    Layout::vertical([Constraint::Fill(0), Constraint::Length(2)]).split(rect);
                let rect = utils::render_panel(frame, &ui.theme, chunks[1], "search", None, true);
                frame.render_widget(Paragraph::new(format!("/{query}")), rect);
                (chunks[0], true)
            }
            PopupState::SearchTuiHelp { scope, items } => {
                render_contextual_help_popup(frame, &ui.theme, rect, "search tui", scope, items)
            }
            PopupState::ContextHelp { scope, items } => {
                render_contextual_help_popup(frame, &ui.theme, rect, "context", scope, items)
            }
            PopupState::ShortcutFamily { title, items, .. } => {
                let title = title.clone();
                let display_items = items
                    .iter()
                    .map(|item| {
                        (
                            format!("{}  {}", item.trigger.display_help(), item.command.desc()),
                            false,
                        )
                    })
                    .collect();
                let rect = render_list_popup(
                    frame,
                    rect,
                    &title,
                    display_items,
                    items.len() as u16 + 2,
                    ui,
                );
                (rect, false)
            }
            PopupState::ActionList(item, _) => {
                let rect = render_list_popup(
                    frame,
                    rect,
                    &format!("Actions on {}", item.name()),
                    item.actions_desc()
                        .into_iter()
                        .enumerate()
                        .map(|(id, d)| (format!("[{id}] {d}"), false))
                        .collect(),
                    item.n_actions() as u16 + 2, // 2 for top/bot paddings
                    ui,
                );
                (rect, false)
            }
            PopupState::DeviceList { .. } => {
                let player = state.player.read();

                let current_device_id = match player.playback {
                    Some(ref playback) => playback.device.id.as_deref().unwrap_or_default(),
                    None => "",
                };
                let items = player
                    .devices
                    .iter()
                    .map(|d| (format!("{} | {}", d.name, d.id), current_device_id == d.id))
                    .collect();

                let rect = render_list_popup(frame, rect, "Devices", items, 5, ui);
                (rect, false)
            }
            PopupState::ThemeList(themes, ..) => {
                let items = themes.iter().map(|t| (t.name.clone(), false)).collect();

                let rect = render_list_popup(frame, rect, "Themes", items, 7, ui);
                (rect, false)
            }
            PopupState::UserPlaylistList(action, _) => {
                let data = state.data.read();
                let (items, search_query) = match action {
                    PlaylistPopupAction::Browse {
                        folder_id,
                        search_query,
                    } => (
                        data.user_data.folder_playlists_items(*folder_id),
                        search_query,
                    ),
                    PlaylistPopupAction::AddTrack {
                        folder_id,
                        search_query,
                        ..
                    }
                    | PlaylistPopupAction::AddEpisode {
                        folder_id,
                        search_query,
                        ..
                    } => (
                        data.user_data.modifiable_playlist_items(Some(*folder_id)),
                        search_query,
                    ),
                };

                // Filter items based on search query if present
                let filtered_items = filtered_items_from_query(search_query, &items);

                let display_items = filtered_items
                    .iter()
                    .map(|p| (p.to_string(), false))
                    .collect();

                let chunks = Layout::vertical([
                    Constraint::Length(2),
                    Constraint::Fill(0),
                    Constraint::Length(10),
                ])
                .split(rect);

                let search_rect = utils::render_panel(
                    frame,
                    &ui.theme,
                    chunks[0],
                    "playlist filter",
                    Some(Line::from("Backspace closes")),
                    true,
                );
                frame.render_widget(Paragraph::new(search_query.clone()), search_rect);

                let rect =
                    render_list_popup(frame, chunks[2], "User Playlists", display_items, 10, ui);
                (rect, false)
            }
            PopupState::UserFollowedArtistList { .. } => {
                let items = state
                    .data
                    .read()
                    .user_data
                    .followed_artists
                    .iter()
                    .map(|a| (a.to_string(), false))
                    .collect();

                let rect = render_list_popup(frame, rect, "User Followed Artists", items, 7, ui);
                (rect, false)
            }
            PopupState::UserSavedAlbumList { .. } => {
                let items = state
                    .data
                    .read()
                    .user_data
                    .saved_albums
                    .iter()
                    .map(|a| (a.to_string(), false))
                    .collect();

                let rect = render_list_popup(frame, rect, "User Saved Albums", items, 7, ui);
                (rect, false)
            }
            PopupState::ArtistList(_, artists, ..) => {
                let items = artists.iter().map(|a| (a.to_string(), false)).collect();

                let rect = render_list_popup(frame, rect, "Artists", items, 5, ui);
                (rect, false)
            }
        },
    }
}

fn render_contextual_help_popup(
    frame: &mut Frame,
    theme: &config::Theme,
    rect: Rect,
    title: &str,
    scope: &str,
    items: &[(String, String)],
) -> (Rect, bool) {
    let height = (items.len() as u16).min(8) + 4;
    let chunks = Layout::vertical([Constraint::Fill(0), Constraint::Length(height)]).split(rect);
    let rect = utils::render_panel(
        frame,
        theme,
        chunks[1],
        title,
        Some(Line::from(scope.to_string())),
        true,
    );
    let rows = items
        .iter()
        .map(|(shortcuts, description)| {
            Row::new(vec![
                Cell::from(shortcuts.clone()),
                Cell::from(description.clone()),
            ])
        })
        .collect::<Vec<_>>();
    let table = Table::new(rows, [Constraint::Length(16), Constraint::Fill(0)]).column_spacing(2);
    frame.render_widget(table, rect);
    (chunks[0], true)
}

/// A helper function to render a list popup
fn render_list_popup(
    frame: &mut Frame,
    rect: Rect,
    title: &str,
    items: Vec<(String, bool)>,
    length: u16,
    ui: &mut UIStateGuard,
) -> Rect {
    let chunks = Layout::vertical([Constraint::Fill(0), Constraint::Length(length)]).split(rect);

    let rect = utils::render_panel(frame, &ui.theme, chunks[1], title, None, true);
    let (list, len) = utils::construct_list_widget(&ui.theme, items, true);

    utils::render_list_window(
        frame,
        list,
        rect,
        len,
        ui.popup.as_mut().unwrap().list_state_mut().unwrap(),
    );

    chunks[0]
}

/// Render a shortcut help popup to show the available shortcuts based on user's inputs
pub fn render_shortcut_help_popup(
    frame: &mut Frame,
    state: &SharedState,
    ui: &mut UIStateGuard,
    rect: Rect,
) -> Rect {
    let input = &ui.input_key_sequence;

    // get the matches (keymaps) from the current key sequence input,
    // if there is at lease one match, render the shortcut help popup
    let matches = {
        if input.keys.is_empty() {
            vec![]
        } else {
            config::get_config()
                .keymap_config
                .find_matched_prefix_keymaps(input)
                .into_iter()
                .map(|keymap| {
                    let mut keymap = keymap.clone();
                    keymap.key_sequence.keys.drain(0..input.keys.len());
                    keymap
                })
                .filter(|keymap| {
                    !keymap.key_sequence.keys.is_empty() && keymap.command != Command::None
                })
                .collect::<Vec<_>>()
        }
    };

    let page_type = ui.current_page().page_type();
    let has_playback = state.player.read().current_playback().is_some();
    let matches = matches
        .into_iter()
        .filter(|km| match km.command {
            Command::ShowActionsOnCurrentContext | Command::GoToRadioFromCurrentContext => {
                page_type == PageType::Context
            }
            Command::ShowActionsOnCurrentTrack | Command::GoToRadioFromCurrentTrack => {
                has_playback
            }
            _ => true,
        })
        .collect::<Vec<_>>();

    if matches.is_empty() {
        rect
    } else {
        let popup_height = (matches.len().min(8) as u16) + 2;
        let chunks =
            Layout::vertical([Constraint::Fill(0), Constraint::Length(popup_height)]).split(rect);

        let mut meta = vec![
            super::Span::styled(
                input.display_help(),
                ui.theme.page_desc().add_modifier(super::Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("press second key", ui.theme.playback_metadata()),
            Span::raw("  "),
            Span::styled("esc cancels", ui.theme.playlist_desc()),
            Span::raw("  "),
            Span::styled(
                shortcut_family_context_label(ui),
                ui.theme.playback_metadata(),
            ),
        ];
        if let Some(playback) = state.player.read().current_playback() {
            let mode_text = format!(
                "repeat {:?} shuffle {}",
                playback.repeat_state,
                if playback.shuffle_state { "on" } else { "off" }
            );
            meta.extend([
                Span::raw("  "),
                Span::styled(mode_text, ui.theme.playback_metadata()),
            ]);
        }
        let meta = Line::from(meta);
        let rect = utils::render_panel(
            frame,
            &ui.theme,
            chunks[1],
            shortcut_family_title(input),
            Some(meta),
            true,
        );

        let help_table = Table::new(
            matches
                .into_iter()
                .map(|km| {
                    Row::new(vec![
                        Cell::from(km.key_sequence.display_help())
                            .style(ui.theme.page_desc().add_modifier(super::Modifier::BOLD)),
                        Cell::from(km.command.desc()),
                        Cell::from(format!("{:?}", km.command)).style(ui.theme.playback_metadata()),
                    ])
                })
                .collect::<Vec<_>>(),
            SHORTCUT_TABLE_CONSTRAINS,
        )
        .column_spacing(2);

        frame.render_widget(help_table, rect);
        chunks[0]
    }
}

fn shortcut_family_title(input: &crate::key::KeySequence) -> &str {
    match input.keys.as_slice() {
        [crate::key::Key::None(KeyCode::Char('g'))] => "go to",
        [crate::key::Key::None(KeyCode::Char('r'))] => "radio",
        [crate::key::Key::None(KeyCode::Char('m'))] => "mode",
        [crate::key::Key::None(KeyCode::Char('a'))] => "actions",
        _ => "shortcuts",
    }
}

fn shortcut_family_context_label(ui: &UIStateGuard) -> String {
    match ui.current_page() {
        PageState::Library { .. } => "library".to_string(),
        PageState::Search { .. } => "search".to_string(),
        PageState::SearchTui { state, .. } => match state.mode {
            SearchTuiMode::Global => "search tui / global".to_string(),
            SearchTuiMode::Playlist { .. } => "search tui / playlist".to_string(),
            SearchTuiMode::Album { .. } => "search tui / album".to_string(),
            SearchTuiMode::Artist { .. } => "search tui / artist".to_string(),
        },
        PageState::Context { context_page_type, .. } => {
            context_page_type.title().to_lowercase()
        }
        PageState::Browse { .. } => "browse".to_string(),
        PageState::Lyrics { .. } => "lyrics".to_string(),
        PageState::Queue { .. } => "queue".to_string(),
        PageState::CommandHelp { .. } => "help".to_string(),
        PageState::Logs { .. } => "logs".to_string(),
    }
}
