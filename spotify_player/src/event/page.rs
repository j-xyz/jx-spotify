use anyhow::Context as _;
use command::CommandOrAction;

use crate::command::{construct_album_actions, construct_playlist_actions, construct_show_actions};
use crate::state::{SearchTuiFocus, SearchTuiMode, SearchTuiPageUIState};
use crate::{search_tui, ui::single_line_input::LineInput};
use crossterm::event::KeyCode;
use rspotify::model::Offset;
use std::time::Instant;

use super::*;

pub fn handle_key_sequence_for_page(
    key_sequence: &KeySequence,
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
    ui: &mut UIStateGuard,
) -> Result<bool> {
    let page_type = ui.current_page().page_type();
    // handle search page separately as it needs access to the raw key sequence
    // as opposed to the matched command
    if page_type == PageType::Search {
        return handle_key_sequence_for_search_page(key_sequence, client_pub, state, ui);
    }
    if page_type == PageType::SearchTui {
        return handle_key_sequence_for_search_tui_page(key_sequence, client_pub, state, ui);
    }

    match config::get_config()
        .keymap_config
        .find_command_or_action_from_key_sequence(key_sequence)
    {
        Some(CommandOrAction::Command(command)) => match page_type {
            PageType::Search => anyhow::bail!("page search type should already be handled!"),
            PageType::SearchTui => anyhow::bail!("search tui should already be handled!"),
            PageType::Library => handle_command_for_library_page(command, client_pub, ui, state),
            PageType::Context => handle_command_for_context_page(command, client_pub, ui, state),
            PageType::Browse => handle_command_for_browse_page(command, client_pub, ui, state),
            // lyrics page doesn't support any commands
            PageType::Lyrics => Ok(false),
            PageType::Queue => Ok(handle_command_for_queue_page(command, ui)),
            PageType::CommandHelp => Ok(handle_command_for_command_help_page(command, ui)),
            PageType::Logs => Ok(handle_command_for_logs_page(command, ui)),
        },
        Some(CommandOrAction::Action(action, ActionTarget::SelectedItem)) => match page_type {
            PageType::Search => anyhow::bail!("page search type should already be handled!"),
            PageType::SearchTui => anyhow::bail!("search tui should already be handled!"),
            PageType::Library => handle_action_for_library_page(action, client_pub, ui, state),
            PageType::Context => {
                window::handle_action_for_focused_context_page(action, client_pub, ui, state)
            }
            PageType::Browse => handle_action_for_browse_page(action, client_pub, ui, state),
            _ => Ok(false),
        },
        _ => Ok(false),
    }
}

fn handle_key_sequence_for_search_tui_page(
    key_sequence: &KeySequence,
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
    ui: &mut UIStateGuard,
) -> Result<bool> {
    if key_sequence.keys.len() != 1 {
        return Ok(false);
    }

    let key = key_sequence.keys[0];
    match key {
        Key::Ctrl(KeyCode::Char('c')) => {
            ui.is_running = false;
            return Ok(true);
        }
        Key::None(KeyCode::Tab) => return toggle_search_tui_focus(ui, false),
        Key::None(KeyCode::BackTab) => return toggle_search_tui_focus(ui, true),
        Key::None(KeyCode::Esc) => return handle_search_tui_escape(ui),
        _ => {}
    }

    let focus = search_tui_focus(ui);

    if focus == SearchTuiFocus::Search {
        if matches!(key, Key::None(KeyCode::Char('?'))) {
            return Ok(false);
        }
        if matches!(key, Key::None(KeyCode::Enter)) {
            return toggle_search_tui_focus(ui, false);
        }

        if let Some(handled) = handle_search_tui_input(key, ui) {
            return Ok(handled);
        }
        return Ok(false);
    }

    match key {
        Key::None(KeyCode::Enter) => return choose_search_tui_item(client_pub, state, ui),
        Key::None(KeyCode::Char('p')) | Key::Alt(KeyCode::Char('p')) => {
            return play_search_tui_item(client_pub, state, ui);
        }
        Key::None(KeyCode::Char('r')) | Key::Alt(KeyCode::Char('r')) => {
            return radio_search_tui_item(client_pub, state, ui);
        }
        Key::None(KeyCode::Char('/')) => {
            return focus_search_tui_search(ui);
        }
        _ => {}
    }

    let len = search_tui_len(state, ui);
    let selected = search_tui_selected(ui);
    let command = match key {
        Key::None(KeyCode::Up) | Key::None(KeyCode::Char('k')) => {
            Some(Command::SelectPreviousOrScrollUp)
        }
        Key::None(KeyCode::Down) | Key::None(KeyCode::Char('j')) => {
            Some(Command::SelectNextOrScrollDown)
        }
        Key::None(KeyCode::PageUp) => Some(Command::PageSelectPreviousOrScrollUp),
        Key::None(KeyCode::PageDown) => Some(Command::PageSelectNextOrScrollDown),
        Key::None(KeyCode::Home) => Some(Command::SelectFirstOrScrollToTop),
        Key::None(KeyCode::End) | Key::None(KeyCode::Char('G')) => {
            Some(Command::SelectLastOrScrollToBottom)
        }
        _ => None,
    };

    if let Some(command) = command {
        Ok(handle_navigation_command(
            command,
            ui.current_page_mut(),
            selected,
            len,
            None,
        ))
    } else {
        Ok(false)
    }
}

fn search_tui_focus(ui: &UIStateGuard) -> SearchTuiFocus {
    match ui.current_page() {
        PageState::SearchTui { state, .. } => state.focus,
        _ => SearchTuiFocus::Search,
    }
}

fn search_tui_selected(ui: &mut UIStateGuard) -> usize {
    match ui.current_page() {
        PageState::SearchTui { state, .. } => state.result_list.selected().unwrap_or_default(),
        _ => 0,
    }
}

fn toggle_search_tui_focus(ui: &mut UIStateGuard, reverse: bool) -> Result<bool> {
    let PageState::SearchTui { state, .. } = ui.current_page_mut() else {
        anyhow::bail!("expect a search tui page");
    };

    state.focus = match (state.focus, reverse) {
        (SearchTuiFocus::Search, false) | (SearchTuiFocus::Search, true) => SearchTuiFocus::Results,
        (SearchTuiFocus::Results, false) | (SearchTuiFocus::Results, true) => {
            SearchTuiFocus::Search
        }
    };

    Ok(true)
}

fn focus_search_tui_search(ui: &mut UIStateGuard) -> Result<bool> {
    let PageState::SearchTui { state, .. } = ui.current_page_mut() else {
        anyhow::bail!("expect a search tui page");
    };
    state.focus = SearchTuiFocus::Search;
    Ok(true)
}

fn handle_search_tui_input(key: Key, ui: &mut UIStateGuard) -> Option<bool> {
    let PageState::SearchTui { line_input, state } = ui.current_page_mut() else {
        return Some(false);
    };

    let effect = line_input.input(&key);
    if effect.is_some() {
        state.last_edited_at = Instant::now();
        if line_input.is_empty() {
            state.last_dispatched_query = None;
        }
        return Some(true);
    }

    None
}

fn handle_search_tui_escape(ui: &mut UIStateGuard) -> Result<bool> {
    let PageState::SearchTui { line_input, state } = ui.current_page_mut() else {
        anyhow::bail!("expect a search tui page");
    };

    if matches!(
        state.mode,
        SearchTuiMode::Playlist { .. } | SearchTuiMode::Album { .. } | SearchTuiMode::Artist { .. }
    ) {
        reset_search_tui_to_global(line_input, state);
    } else if !line_input.is_empty() {
        *line_input = LineInput::default();
        state.focus = SearchTuiFocus::Search;
        state.last_dispatched_query = None;
        state.last_edited_at = Instant::now();
        state.result_list = Default::default();
    } else {
        state.focus = match state.focus {
            SearchTuiFocus::Search => SearchTuiFocus::Results,
            SearchTuiFocus::Results => SearchTuiFocus::Search,
        };
    }

    Ok(true)
}

fn reset_search_tui_to_global(line_input: &mut LineInput, state: &mut SearchTuiPageUIState) {
    *line_input = LineInput::default();
    state.mode = SearchTuiMode::Global;
    state.focus = SearchTuiFocus::Search;
    state.last_dispatched_query = None;
    state.last_edited_at = Instant::now();
    state.result_list = Default::default();
}

fn search_tui_len(state: &SharedState, ui: &mut UIStateGuard) -> usize {
    let data = state.data.read();
    let (mode, query) = match ui.current_page() {
        PageState::SearchTui { line_input, state } => (&state.mode, line_input.get_text()),
        _ => return 0,
    };

    match mode {
        SearchTuiMode::Global => search_tui::build_items(&data, mode, &query).len(),
        SearchTuiMode::Playlist { .. }
        | SearchTuiMode::Album { .. }
        | SearchTuiMode::Artist { .. } => {
            search_tui::build_context_tracks(&data, mode, &query).len()
        }
    }
}

fn choose_search_tui_item(
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
    ui: &mut UIStateGuard,
) -> Result<bool> {
    let focus = search_tui_focus(ui);
    if focus == SearchTuiFocus::Search {
        return toggle_search_tui_focus(ui, false);
    }

    let selected = search_tui_selected(ui);
    let (mode, query) = match ui.current_page() {
        PageState::SearchTui { line_input, state } => (state.mode.clone(), line_input.get_text()),
        _ => anyhow::bail!("expect a search tui page"),
    };

    let data = state.data.read();
    match mode {
        SearchTuiMode::Global => {
            let items = search_tui::build_items(&data, &SearchTuiMode::Global, &query);
            if selected >= items.len() {
                return Ok(false);
            }

            let item = items[selected].clone();
            drop(data);
            play_or_open_search_tui_item(item, client_pub, state, ui, false)
        }
        SearchTuiMode::Playlist { .. }
        | SearchTuiMode::Album { .. }
        | SearchTuiMode::Artist { .. } => {
            let tracks = search_tui::build_context_tracks(&data, &mode, &query);
            if selected >= tracks.len() {
                return Ok(false);
            }
            let track = tracks[selected].clone();
            drop(data);
            state.player.write().currently_playing_tracks_id = None;
            let playback = match mode {
                SearchTuiMode::Playlist {
                    ref playlist_id, ..
                } => Playback::Context(
                    ContextId::Playlist(playlist_id.clone_static()),
                    Some(Offset::Uri(track.id.uri())),
                ),
                SearchTuiMode::Album { ref album_id, .. } => Playback::Context(
                    ContextId::Album(album_id.clone_static()),
                    Some(Offset::Uri(track.id.uri())),
                ),
                SearchTuiMode::Artist { .. } => Playback::URIs(vec![track.id.into()], None),
                SearchTuiMode::Global => unreachable!("handled above"),
            };
            client_pub.send(ClientRequest::Player(PlayerRequest::StartPlayback(
                playback, None,
            )))?;
            Ok(true)
        }
    }
}

fn play_search_tui_item(
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
    ui: &mut UIStateGuard,
) -> Result<bool> {
    let selected = search_tui_selected(ui);
    let (mode, query) = match ui.current_page() {
        PageState::SearchTui { line_input, state } => (state.mode.clone(), line_input.get_text()),
        _ => anyhow::bail!("expect a search tui page"),
    };

    let data = state.data.read();
    match mode {
        SearchTuiMode::Global => {
            let items = search_tui::build_items(&data, &SearchTuiMode::Global, &query);
            if selected >= items.len() {
                return Ok(false);
            }

            let item = items[selected].clone();
            drop(data);
            play_or_open_search_tui_item(item, client_pub, state, ui, true)
        }
        SearchTuiMode::Playlist { .. }
        | SearchTuiMode::Album { .. }
        | SearchTuiMode::Artist { .. } => {
            drop(data);
            choose_search_tui_item(client_pub, state, ui)
        }
    }
}

fn play_or_open_search_tui_item(
    item: search_tui::SearchTuiItem,
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
    ui: &mut UIStateGuard,
    force_play_context: bool,
) -> Result<bool> {
    match item {
        search_tui::SearchTuiItem::Track { track, .. } => {
            state.player.write().currently_playing_tracks_id = None;
            client_pub.send(ClientRequest::Player(PlayerRequest::StartPlayback(
                Playback::URIs(vec![track.id.into()], None),
                None,
            )))?;
        }
        search_tui::SearchTuiItem::Artist { artist, .. } if force_play_context => {
            state.player.write().currently_playing_tracks_id = None;
            client_pub.send(ClientRequest::Player(PlayerRequest::StartPlayback(
                Playback::Context(ContextId::Artist(artist.id), None),
                None,
            )))?;
        }
        search_tui::SearchTuiItem::Artist { artist, .. } => {
            client_pub.send(ClientRequest::GetContext(ContextId::Artist(
                artist.id.clone_static(),
            )))?;

            let PageState::SearchTui { line_input, state } = ui.current_page_mut() else {
                anyhow::bail!("expect a search tui page");
            };
            *line_input = LineInput::default();
            state.mode = SearchTuiMode::Artist {
                artist_id: artist.id.clone_static(),
                title: artist.name,
            };
            state.focus = SearchTuiFocus::Results;
            state.result_list = Default::default();
            state.last_dispatched_query = None;
            state.last_edited_at = Instant::now();
        }
        search_tui::SearchTuiItem::Album { album, .. } if force_play_context => {
            state.player.write().currently_playing_tracks_id = None;
            client_pub.send(ClientRequest::Player(PlayerRequest::StartPlayback(
                Playback::Context(ContextId::Album(album.id), None),
                None,
            )))?;
        }
        search_tui::SearchTuiItem::Album { album, .. } => {
            client_pub.send(ClientRequest::GetContext(ContextId::Album(
                album.id.clone_static(),
            )))?;

            let PageState::SearchTui { line_input, state } = ui.current_page_mut() else {
                anyhow::bail!("expect a search tui page");
            };
            *line_input = LineInput::default();
            state.mode = SearchTuiMode::Album {
                album_id: album.id.clone_static(),
                title: album.name,
            };
            state.focus = SearchTuiFocus::Results;
            state.result_list = Default::default();
            state.last_dispatched_query = None;
            state.last_edited_at = Instant::now();
        }
        search_tui::SearchTuiItem::Playlist { playlist, .. } if force_play_context => {
            state.player.write().currently_playing_tracks_id = None;
            client_pub.send(ClientRequest::Player(PlayerRequest::StartPlayback(
                Playback::Context(ContextId::Playlist(playlist.id), None),
                None,
            )))?;
        }
        search_tui::SearchTuiItem::Playlist { playlist, .. } => {
            client_pub.send(ClientRequest::GetContext(ContextId::Playlist(
                playlist.id.clone_static(),
            )))?;

            let PageState::SearchTui { line_input, state } = ui.current_page_mut() else {
                anyhow::bail!("expect a search tui page");
            };
            *line_input = LineInput::default();
            state.mode = SearchTuiMode::Playlist {
                playlist_id: playlist.id.clone_static(),
                title: playlist.name,
            };
            state.focus = SearchTuiFocus::Results;
            state.result_list = Default::default();
            state.last_dispatched_query = None;
            state.last_edited_at = Instant::now();
        }
    }

    Ok(true)
}

fn radio_search_tui_item(
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
    ui: &mut UIStateGuard,
) -> Result<bool> {
    let selected = search_tui_selected(ui);
    let (mode, query) = match ui.current_page() {
        PageState::SearchTui { line_input, state } => (state.mode.clone(), line_input.get_text()),
        _ => anyhow::bail!("expect a search tui page"),
    };

    let (seed_uri, seed_name) = {
        let data = state.data.read();
        match mode {
            SearchTuiMode::Global => {
                let items = search_tui::build_items(&data, &SearchTuiMode::Global, &query);
                if selected >= items.len() {
                    return Ok(false);
                }
                let item = items[selected].clone();
                match item {
                    search_tui::SearchTuiItem::Track { track, .. } => (track.id.uri(), track.name),
                    search_tui::SearchTuiItem::Artist { artist, .. } => {
                        (artist.id.uri(), artist.name)
                    }
                    search_tui::SearchTuiItem::Album { album, .. } => (album.id.uri(), album.name),
                    search_tui::SearchTuiItem::Playlist { playlist, .. } => {
                        (playlist.id.uri(), playlist.name)
                    }
                }
            }
            SearchTuiMode::Playlist { .. }
            | SearchTuiMode::Album { .. }
            | SearchTuiMode::Artist { .. } => {
                let tracks = search_tui::build_context_tracks(&data, &mode, &query);
                if selected >= tracks.len() {
                    return Ok(false);
                }
                let track = tracks[selected].clone();
                (track.id.uri(), track.name)
            }
        }
    };

    super::handle_go_to_radio(&seed_uri, &seed_name, ui, client_pub)?;
    Ok(true)
}

fn handle_action_for_library_page(
    action: Action,
    client_pub: &flume::Sender<ClientRequest>,
    ui: &mut UIStateGuard,
    state: &SharedState,
) -> Result<bool> {
    let data = state.data.read();
    let (focus_state, folder_id) = match ui.current_page() {
        PageState::Library { state } => (state.focus, state.playlist_folder_id),
        _ => anyhow::bail!("expect a library page state"),
    };
    match focus_state {
        LibraryFocusState::Playlists => window::handle_action_for_selected_item(
            action,
            &ui.search_filtered_items(&data.user_data.folder_playlists_items(folder_id))
                .into_iter()
                .copied()
                .collect::<Vec<_>>(),
            &data,
            ui,
            client_pub,
        ),
        LibraryFocusState::SavedAlbums => window::handle_action_for_selected_item(
            action,
            &ui.search_filtered_items(&data.user_data.saved_albums),
            &data,
            ui,
            client_pub,
        ),
        LibraryFocusState::FollowedArtists => window::handle_action_for_selected_item(
            action,
            &ui.search_filtered_items(&data.user_data.followed_artists),
            &data,
            ui,
            client_pub,
        ),
    }
}

fn handle_command_for_library_page(
    command: Command,
    client_pub: &flume::Sender<ClientRequest>,
    ui: &mut UIStateGuard,
    state: &SharedState,
) -> Result<bool> {
    if command == Command::Search {
        ui.new_search_popup();
        return Ok(true);
    }

    let (focus_state, folder_id) = match ui.current_page() {
        PageState::Library { state } => (state.focus, state.playlist_folder_id),
        _ => anyhow::bail!("expect a library page state"),
    };

    if command == Command::SortLibraryAlphabetically {
        let mut data = state.data.write();

        // Sort playlists alphabetically, keeping folders on top
        data.user_data.playlists.sort_by(|a, b| match (a, b) {
            (PlaylistFolderItem::Folder(_), PlaylistFolderItem::Playlist(_)) => {
                std::cmp::Ordering::Less
            }
            (PlaylistFolderItem::Playlist(_), PlaylistFolderItem::Folder(_)) => {
                std::cmp::Ordering::Greater
            }
            _ => a
                .to_string()
                .to_lowercase()
                .cmp(&b.to_string().to_lowercase()),
        });

        // Sort albums alphabetically
        data.user_data
            .saved_albums
            .sort_by(|x, y| x.name.to_lowercase().cmp(&y.name.to_lowercase()));

        // Sort artists alphabetically
        data.user_data
            .followed_artists
            .sort_by(|x, y| x.name.to_lowercase().cmp(&y.name.to_lowercase()));
    }

    if command == Command::SortLibraryByRecent {
        let mut data = state.data.write();

        // Sort playlists by `current_folder_id` and then by `snapshot_id`
        data.user_data.playlists.sort_by(|a, b| {
            match (a, b) {
                (PlaylistFolderItem::Playlist(p1), PlaylistFolderItem::Playlist(p2)) => {
                    if p1.current_folder_id == p2.current_folder_id {
                        p1.snapshot_id.cmp(&p2.snapshot_id)
                    } else {
                        p1.current_folder_id.cmp(&p2.current_folder_id)
                    }
                }
                (PlaylistFolderItem::Folder(_), PlaylistFolderItem::Playlist(_)) => {
                    std::cmp::Ordering::Less
                }
                (PlaylistFolderItem::Playlist(_), PlaylistFolderItem::Folder(_)) => {
                    std::cmp::Ordering::Greater
                }
                _ => std::cmp::Ordering::Equal, // Keep folders in place
            }
        });

        // Sort albums by recent addition
        data.user_data
            .saved_albums
            .sort_by(|a, b| b.added_at.cmp(&a.added_at));
    }

    match focus_state {
        LibraryFocusState::Playlists => {
            let data = state.data.read();
            Ok(window::handle_command_for_playlist_list_window(
                command,
                &ui.search_filtered_items(&data.user_data.folder_playlists_items(folder_id))
                    .into_iter()
                    .copied()
                    .collect::<Vec<_>>(),
                &data,
                ui,
            ))
        }
        LibraryFocusState::SavedAlbums => {
            // Use a read lock for the function call
            let data = state.data.read();
            window::handle_command_for_album_list_window(
                command,
                &ui.search_filtered_items(&data.user_data.saved_albums),
                &data,
                ui,
                client_pub,
            )
        }
        LibraryFocusState::FollowedArtists => {
            // Handle artist-specific commands
            let data = state.data.read();
            Ok(window::handle_command_for_artist_list_window(
                command,
                &ui.search_filtered_items(&data.user_data.followed_artists),
                &data,
                ui,
            ))
        }
    }
}

fn handle_key_sequence_for_search_page(
    key_sequence: &KeySequence,
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
    ui: &mut UIStateGuard,
) -> Result<bool> {
    let (focus_state, current_query, line_input) = match ui.current_page_mut() {
        PageState::Search {
            state,
            line_input,
            current_query,
        } => (state.focus, current_query, line_input),
        _ => anyhow::bail!("expect a search page"),
    };

    // handle user's input
    if let SearchFocusState::Input = focus_state {
        if key_sequence.keys.len() == 1 {
            return match &key_sequence.keys[0] {
                Key::None(crossterm::event::KeyCode::Enter) => {
                    if !line_input.is_empty() {
                        *current_query = line_input.get_text();
                        client_pub.send(ClientRequest::Search(line_input.get_text()))?;
                    }
                    Ok(true)
                }
                k => match line_input.input(k) {
                    None => Ok(false),
                    _ => Ok(true),
                },
            };
        }
    }

    let Some(found_keymap) = config::get_config()
        .keymap_config
        .find_command_or_action_from_key_sequence(key_sequence)
    else {
        return Ok(false);
    };

    let data = state.data.read();
    let search_results = data.caches.search.get(current_query);

    match focus_state {
        SearchFocusState::Input => anyhow::bail!("user's search input should be handled before"),
        SearchFocusState::Tracks => {
            let tracks = search_results
                .map(|s| s.tracks.iter().collect::<Vec<_>>())
                .unwrap_or_default();

            match found_keymap {
                CommandOrAction::Command(command) => window::handle_command_for_track_list_window(
                    command, client_pub, &tracks, &data, ui, state,
                ),
                CommandOrAction::Action(action, ActionTarget::SelectedItem) => {
                    window::handle_action_for_selected_item(action, &tracks, &data, ui, client_pub)
                }
                CommandOrAction::Action(..) => Ok(false),
            }
        }
        SearchFocusState::Artists => {
            let artists = search_results
                .map(|s| s.artists.iter().collect::<Vec<_>>())
                .unwrap_or_default();

            match found_keymap {
                CommandOrAction::Command(command) => Ok(
                    window::handle_command_for_artist_list_window(command, &artists, &data, ui),
                ),
                CommandOrAction::Action(action, ActionTarget::SelectedItem) => {
                    window::handle_action_for_selected_item(action, &artists, &data, ui, client_pub)
                }
                CommandOrAction::Action(..) => Ok(false),
            }
        }
        SearchFocusState::Albums => {
            let albums = search_results
                .map(|s| s.albums.iter().collect::<Vec<_>>())
                .unwrap_or_default();

            match found_keymap {
                CommandOrAction::Command(command) => window::handle_command_for_album_list_window(
                    command, &albums, &data, ui, client_pub,
                ),
                CommandOrAction::Action(action, ActionTarget::SelectedItem) => {
                    window::handle_action_for_selected_item(action, &albums, &data, ui, client_pub)
                }
                CommandOrAction::Action(..) => Ok(false),
            }
        }
        SearchFocusState::Playlists => {
            let playlists = search_results
                .map(|s| {
                    s.playlists
                        .iter()
                        .map(|p| PlaylistFolderItem::Playlist(p.clone()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let playlist_refs = playlists.iter().collect::<Vec<_>>();

            match found_keymap {
                CommandOrAction::Command(command) => {
                    Ok(window::handle_command_for_playlist_list_window(
                        command,
                        &playlist_refs,
                        &data,
                        ui,
                    ))
                }
                CommandOrAction::Action(action, ActionTarget::SelectedItem) => {
                    window::handle_action_for_selected_item(
                        action,
                        &playlist_refs,
                        &data,
                        ui,
                        client_pub,
                    )
                }
                CommandOrAction::Action(..) => Ok(false),
            }
        }
        SearchFocusState::Shows => {
            let shows = search_results
                .map(|s| s.shows.iter().collect::<Vec<_>>())
                .unwrap_or_default();

            match found_keymap {
                CommandOrAction::Command(command) => Ok(
                    window::handle_command_for_show_list_window(command, &shows, &data, ui),
                ),
                CommandOrAction::Action(action, ActionTarget::SelectedItem) => {
                    window::handle_action_for_selected_item(action, &shows, &data, ui, client_pub)
                }
                CommandOrAction::Action(..) => Ok(false),
            }
        }
        SearchFocusState::Episodes => {
            let episodes = match search_results {
                Some(s) => s.episodes.iter().collect(),
                None => Vec::new(),
            };

            match found_keymap {
                CommandOrAction::Command(command) => {
                    window::handle_command_for_episode_list_window(
                        command, client_pub, &episodes, &data, ui, state,
                    )
                }
                CommandOrAction::Action(action, ActionTarget::SelectedItem) => {
                    window::handle_action_for_selected_item(
                        action, &episodes, &data, ui, client_pub,
                    )
                }
                CommandOrAction::Action(..) => Ok(false),
            }
        }
    }
}

fn handle_command_for_context_page(
    command: Command,
    client_pub: &flume::Sender<ClientRequest>,
    ui: &mut UIStateGuard,
    state: &SharedState,
) -> Result<bool> {
    match command {
        Command::Search => {
            ui.new_search_popup();
            Ok(true)
        }
        Command::ShowActionsOnCurrentContext => {
            let context_id = match ui.current_page() {
                PageState::Context { id, .. } => match id {
                    None => return Ok(false),
                    Some(id) => id,
                },
                _ => anyhow::bail!("expect a context page"),
            };
            let data = state.data.read();

            match data.caches.context.get(&context_id.uri()) {
                Some(context) => match context {
                    Context::Playlist { playlist, .. } => {
                        let actions = construct_playlist_actions(playlist, &data);
                        ui.popup = Some(PopupState::ActionList(
                            Box::new(ActionListItem::Playlist(playlist.clone(), actions)),
                            ListState::default(),
                        ));
                        Ok(true)
                    }
                    Context::Album { album, .. } => {
                        let actions = construct_album_actions(album, &data);
                        ui.popup = Some(PopupState::ActionList(
                            Box::new(ActionListItem::Album(album.clone(), actions)),
                            ListState::default(),
                        ));
                        Ok(true)
                    }
                    Context::Artist { artist, .. } => {
                        let actions = construct_artist_actions(artist, &data);
                        ui.popup = Some(PopupState::ActionList(
                            Box::new(ActionListItem::Artist(artist.clone(), actions)),
                            ListState::default(),
                        ));
                        Ok(true)
                    }
                    Context::Show { show, .. } => {
                        let actions = construct_show_actions(show, &data);
                        ui.popup = Some(PopupState::ActionList(
                            Box::new(ActionListItem::Show(show.clone(), actions)),
                            ListState::default(),
                        ));
                        Ok(true)
                    }
                    Context::Tracks { tracks: _, desc: _ } => Ok(false),
                },
                None => Ok(false),
            }
        }
        _ => window::handle_command_for_focused_context_window(command, client_pub, ui, state),
    }
}

fn handle_action_for_browse_page(
    action: Action,
    client_pub: &flume::Sender<ClientRequest>,
    ui: &mut UIStateGuard,
    state: &SharedState,
) -> Result<bool> {
    let data = state.data.read();

    match ui.current_page() {
        PageState::Browse { state } => match state {
            BrowsePageUIState::CategoryPlaylistList { category, .. } => {
                let Some(playlists) = data.browse.category_playlists.get(&category.id) else {
                    return Ok(false);
                };

                let page_state = ui.current_page_mut();
                let selected = page_state.selected().unwrap_or_default();
                if selected >= playlists.len() {
                    return Ok(false);
                }

                handle_action_in_context(
                    action,
                    playlists[selected].clone().into(),
                    client_pub,
                    &data,
                    ui,
                )?;

                Ok(true)
            }
            BrowsePageUIState::CategoryList { .. } => Ok(false),
        },
        _ => anyhow::bail!("expect a browse page state"),
    }
}

fn handle_command_for_browse_page(
    command: Command,
    client_pub: &flume::Sender<ClientRequest>,
    ui: &mut UIStateGuard,
    state: &SharedState,
) -> Result<bool> {
    let data = state.data.read();

    let len = match ui.current_page() {
        PageState::Browse { state } => match state {
            BrowsePageUIState::CategoryList { .. } => {
                ui.search_filtered_items(&data.browse.categories).len()
            }
            BrowsePageUIState::CategoryPlaylistList { category, .. } => data
                .browse
                .category_playlists
                .get(&category.id)
                .map(|v| ui.search_filtered_items(v).len())
                .unwrap_or_default(),
        },
        _ => anyhow::bail!("expect a browse page state"),
    };

    let count = ui.count_prefix;
    let page_state = ui.current_page_mut();
    let selected = page_state.selected().unwrap_or_default();
    if selected >= len {
        return Ok(false);
    }

    if handle_navigation_command(command, page_state, selected, len, count) {
        return Ok(true);
    }
    match command {
        Command::ChooseSelected => match page_state {
            PageState::Browse { state } => match state {
                BrowsePageUIState::CategoryList { .. } => {
                    let categories = ui.search_filtered_items(&data.browse.categories);
                    client_pub.send(ClientRequest::GetBrowseCategoryPlaylists(
                        categories[selected].clone(),
                    ))?;
                    ui.new_page(PageState::Browse {
                        state: BrowsePageUIState::CategoryPlaylistList {
                            category: categories[selected].clone(),
                            state: ListState::default(),
                        },
                    });
                }
                BrowsePageUIState::CategoryPlaylistList { category, .. } => {
                    let playlists =
                        data.browse
                            .category_playlists
                            .get(&category.id)
                            .context(format!(
                                "expect to have playlists data for {category} category"
                            ))?;
                    let context_id = ContextId::Playlist(
                        ui.search_filtered_items(playlists)[selected].id.clone(),
                    );
                    ui.new_page(PageState::Context {
                        id: None,
                        context_page_type: ContextPageType::Browsing(context_id),
                        state: None,
                    });
                }
            },
            _ => anyhow::bail!("expect a browse page state"),
        },
        Command::Search => {
            ui.new_search_popup();
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn handle_command_for_queue_page(command: Command, ui: &mut UIStateGuard) -> bool {
    let scroll_offset = match ui.current_page() {
        PageState::Queue { scroll_offset } => *scroll_offset,
        _ => return false,
    };
    let count = ui.count_prefix;
    handle_navigation_command(command, ui.current_page_mut(), scroll_offset, 10000, count)
}

fn handle_command_for_command_help_page(command: Command, ui: &mut UIStateGuard) -> bool {
    let scroll_offset = match ui.current_page() {
        PageState::CommandHelp { scroll_offset } => *scroll_offset,
        _ => return false,
    };
    if matches!(command, Command::ClosePopup | Command::PreviousPage) {
        if ui.history.len() > 1 {
            ui.history.pop();
            ui.popup = None;
        }
        return true;
    }
    if command == Command::Search {
        ui.new_search_popup();
        return true;
    }
    let count = ui.count_prefix;
    handle_navigation_command(command, ui.current_page_mut(), scroll_offset, 10000, count)
}

fn handle_command_for_logs_page(command: Command, ui: &mut UIStateGuard) -> bool {
    let scroll_offset = match ui.current_page() {
        PageState::Logs { scroll_offset } => *scroll_offset,
        _ => return false,
    };
    let count = ui.count_prefix;
    handle_navigation_command(command, ui.current_page_mut(), scroll_offset, 10000, count)
}

pub fn handle_navigation_command(
    command: Command,
    page: &mut PageState,
    id: usize,
    len: usize,
    count: Option<usize>,
) -> bool {
    if len == 0 {
        return false;
    }

    let configs = config::get_config();
    match command {
        Command::SelectNextOrScrollDown => {
            let offset = count.unwrap_or(1);
            page.select(std::cmp::min(id + offset, len - 1));
            true
        }
        Command::SelectPreviousOrScrollUp => {
            let offset = count.unwrap_or(1);
            page.select(id.saturating_sub(offset));
            true
        }
        Command::PageSelectNextOrScrollDown => {
            let page_size = configs.app_config.page_size_in_rows;
            let offset = count.unwrap_or(1) * page_size;
            page.select(std::cmp::min(id + offset, len - 1));
            true
        }
        Command::PageSelectPreviousOrScrollUp => {
            let page_size = configs.app_config.page_size_in_rows;
            let offset = count.unwrap_or(1) * page_size;
            page.select(id.saturating_sub(offset));
            true
        }
        Command::SelectLastOrScrollToBottom => {
            page.select(len - 1);
            true
        }
        Command::SelectFirstOrScrollToTop => {
            page.select(0);
            true
        }
        _ => false,
    }
}
