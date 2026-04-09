use super::*;
use crate::{command::construct_artist_actions, utils::filtered_items_from_query};
use anyhow::Context;

pub fn try_open_shortcut_family_popup(
    key: Key,
    state: &SharedState,
    ui: &mut UIStateGuard,
) -> bool {
    let prefix = KeySequence { keys: vec![key] };
    open_shortcut_family_popup(prefix, state, ui)
}

fn open_shortcut_family_popup(
    prefix: KeySequence,
    state: &SharedState,
    ui: &mut UIStateGuard,
) -> bool {
    let title = match shortcut_family_title(&prefix) {
        Some(title) => title,
        None => return false,
    };

    let items = build_shortcut_family_items(&prefix, state, ui);

    if items.is_empty() {
        return false;
    }

    ui.popup = Some(PopupState::ShortcutFamily {
        title,
        prefix,
        items,
        list_state: ListState::default(),
    });
    true
}

fn shortcut_family_title(prefix: &KeySequence) -> Option<String> {
    let first = *prefix.keys.first()?;
    let base = match first {
        Key::None(crossterm::event::KeyCode::Char('g')) => "go to",
        Key::None(crossterm::event::KeyCode::Char('r')) => "radio",
        Key::None(crossterm::event::KeyCode::Char('m')) => "mode",
        Key::None(crossterm::event::KeyCode::Char('a')) => "actions",
        Key::None(crossterm::event::KeyCode::Char('s')) => "sorting",
        Key::None(crossterm::event::KeyCode::Char('u')) => "user",
        _ => return None,
    };

    if prefix.keys.len() == 1 {
        return Some(base.to_string());
    }

    let mut suffix = Vec::new();
    for key in prefix.keys.iter().skip(1) {
        suffix.push(key.display_help());
    }
    Some(format!("{base} {}", suffix.join(" ")))
}

fn build_shortcut_family_items(
    prefix: &KeySequence,
    state: &SharedState,
    ui: &UIStateGuard,
) -> Vec<crate::state::ShortcutFamilyItem> {
    let mut items = Vec::new();
    let prefix_len = prefix.keys.len();

    for keymap in config::get_config()
        .keymap_config
        .find_matched_prefix_keymaps(prefix)
    {
        if !shortcut_family_command_available(keymap.command, state, ui) {
            continue;
        }

        if keymap.key_sequence.keys.len() <= prefix_len {
            continue;
        }

        let trigger_key = keymap.key_sequence.keys[prefix_len];
        let trigger = KeySequence {
            keys: vec![trigger_key],
        };
        let is_direct = keymap.key_sequence.keys.len() == prefix_len + 1;
        let child_prefix = KeySequence {
            keys: keymap.key_sequence.keys[..prefix_len + 1].to_vec(),
        };

        if let Some(existing) = items
            .iter_mut()
            .find(|item: &&mut crate::state::ShortcutFamilyItem| item.trigger == trigger)
        {
            if !is_direct {
                existing.has_children = true;
                if existing.command == Command::None {
                    existing.key_sequence = child_prefix;
                }
            } else if existing.command == Command::None {
                existing.command = keymap.command;
                existing.key_sequence = keymap.key_sequence.clone();
            }
            continue;
        }

        items.push(crate::state::ShortcutFamilyItem {
            trigger,
            key_sequence: if is_direct {
                keymap.key_sequence.clone()
            } else {
                child_prefix
            },
            command: if is_direct {
                keymap.command
            } else {
                Command::None
            },
            has_children: !is_direct,
        });
    }

    items
}

fn shortcut_family_command_available(
    command: Command,
    state: &SharedState,
    ui: &UIStateGuard,
) -> bool {
    let player = state.player.read();

    match command {
        Command::ShowActionsOnCurrentContext | Command::GoToRadioFromCurrentContext => {
            context_shortcut_available(ui)
        }
        Command::ShowActionsOnCurrentTrack => player.currently_playing().is_some(),
        Command::GoToRadioFromCurrentTrack => player.current_item_supports_radio(),
        _ => true,
    }
}

fn context_shortcut_available(ui: &UIStateGuard) -> bool {
    matches!(
        ui.current_page(),
        PageState::Context {
            id: Some(_),
            context_page_type: ContextPageType::Browsing(
                ContextId::Playlist(_)
                    | ContextId::Album(_)
                    | ContextId::Artist(_)
                    | ContextId::Show(_)
            ),
            ..
        }
    )
}

pub fn handle_key_sequence_for_popup(
    key_sequence: &KeySequence,
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
    ui: &mut UIStateGuard,
) -> Result<bool> {
    // handle popups that need reading the raw key sequence instead of the matched command
    match ui.popup.as_ref().context("empty popup")? {
        PopupState::Search { .. } => {
            return handle_key_sequence_for_search_popup(key_sequence, client_pub, state, ui);
        }
        PopupState::ContextHelp { .. } => {
            return handle_key_sequence_for_context_help_popup(key_sequence, ui);
        }
        PopupState::PlaylistCreate { .. } => {
            return handle_key_sequence_for_create_playlist_popup(key_sequence, client_pub, ui);
        }
        PopupState::ShortcutFamily { .. } => {
            return handle_key_sequence_for_shortcut_family_popup(
                key_sequence,
                client_pub,
                state,
                ui,
            );
        }
        PopupState::ActionList(item, ..) => {
            return handle_key_sequence_for_action_list_popup(
                item.n_actions(),
                key_sequence,
                client_pub,
                state,
                ui,
            );
        }
        PopupState::UserPlaylistList(..) => {
            if handle_key_sequence_for_playlist_search_popup(key_sequence, ui) {
                return Ok(true);
            }
        }
        _ => {}
    }

    let Some(command) = config::get_config()
        .keymap_config
        .find_command_from_key_sequence(key_sequence)
    else {
        return Ok(false);
    };

    match ui.popup.as_ref().context("empty popup")? {
        PopupState::Search { .. } => anyhow::bail!("search popup should be handled before"),
        PopupState::ContextHelp { .. } => {
            anyhow::bail!("context help popup should be handled before")
        }
        PopupState::PlaylistCreate { .. } => {
            anyhow::bail!("create playlist popup should be handled before")
        }
        PopupState::ShortcutFamily { .. } => {
            anyhow::bail!("shortcut family popup should be handled before")
        }
        PopupState::ActionList(..) => {
            anyhow::bail!("action list popup should be handled before")
        }
        PopupState::ArtistList(_, artists, _) => {
            let n_items = artists.len();

            handle_command_for_list_popup(
                command,
                ui,
                n_items,
                |_, _| {},
                |ui: &mut UIStateGuard, id: usize| -> Result<()> {
                    let Some(PopupState::ArtistList(action, artists, _)) = &ui.popup else {
                        return Ok(());
                    };

                    match action {
                        ArtistPopupAction::Browse => {
                            let context_id = ContextId::Artist(artists[id].id.clone());
                            ui.new_page(PageState::Context {
                                id: None,
                                context_page_type: ContextPageType::Browsing(context_id),
                                state: None,
                            });
                        }
                        ArtistPopupAction::ShowActions => {
                            let actions = {
                                let data = state.data.read();
                                construct_artist_actions(&artists[id], &data)
                            };
                            ui.popup = Some(PopupState::ActionList(
                                Box::new(ActionListItem::Artist(artists[id].clone(), actions)),
                                ListState::default(),
                            ));
                        }
                    }

                    Ok(())
                },
                |ui: &mut UIStateGuard| {
                    ui.popup = None;
                },
            )
        }
        PopupState::UserPlaylistList(action, _) => match action {
            PlaylistPopupAction::Browse {
                folder_id,
                search_query,
            } => {
                let search_query = search_query.clone();
                let data = state.data.read();
                let items = data.user_data.folder_playlists_items(*folder_id);
                let filtered_items = filtered_items_from_query(&search_query, &items);

                handle_command_for_list_popup(
                    command,
                    ui,
                    filtered_items.len(),
                    |_, _| {},
                    |ui: &mut UIStateGuard, id: usize| -> Result<()> {
                        match filtered_items.get(id).expect("invalid index") {
                            PlaylistFolderItem::Folder(f) => {
                                ui.popup = Some(PopupState::UserPlaylistList(
                                    PlaylistPopupAction::Browse {
                                        folder_id: f.target_id,
                                        search_query: search_query.clone(),
                                    },
                                    ListState::default(),
                                ));
                            }
                            PlaylistFolderItem::Playlist(p) => {
                                let context_id = ContextId::Playlist(
                                    PlaylistId::from_uri(&crate::utils::parse_uri(&p.id.uri()))?
                                        .into_static(),
                                );
                                ui.new_page(PageState::Context {
                                    id: None,
                                    context_page_type: ContextPageType::Browsing(context_id),
                                    state: None,
                                });
                            }
                        }
                        Ok(())
                    },
                    |ui: &mut UIStateGuard| {
                        ui.popup = None;
                    },
                )
            }
            PlaylistPopupAction::AddTrack {
                folder_id,
                track_id,
                search_query,
            } => {
                let search_query = search_query.clone();
                let track_id = track_id.clone();
                let data = state.data.read();
                let items = data.user_data.modifiable_playlist_items(Some(*folder_id));
                let filtered_items = filtered_items_from_query(&search_query, &items);

                handle_command_for_list_popup(
                    command,
                    ui,
                    filtered_items.len(),
                    |_, _| {},
                    |ui: &mut UIStateGuard, id: usize| -> Result<()> {
                        ui.popup = match filtered_items.get(id).expect("invalid index") {
                            PlaylistFolderItem::Folder(f) => Some(PopupState::UserPlaylistList(
                                PlaylistPopupAction::AddTrack {
                                    folder_id: f.target_id,
                                    track_id,
                                    search_query: search_query.clone(),
                                },
                                ListState::default(),
                            )),
                            PlaylistFolderItem::Playlist(p) => {
                                client_pub.send(ClientRequest::AddPlayableToPlaylist(
                                    p.id.clone(),
                                    track_id.into(),
                                ))?;
                                None
                            }
                        };
                        Ok(())
                    },
                    |ui: &mut UIStateGuard| {
                        ui.popup = None;
                    },
                )
            }
            PlaylistPopupAction::AddEpisode {
                folder_id,
                episode_id,
                search_query,
            } => {
                let search_query = search_query.clone();
                let episode_id = episode_id.clone();
                let data = state.data.read();
                let items = data.user_data.modifiable_playlist_items(Some(*folder_id));
                let filtered_items = filtered_items_from_query(&search_query, &items);

                handle_command_for_list_popup(
                    command,
                    ui,
                    filtered_items.len(),
                    |_, _| {},
                    |ui: &mut UIStateGuard, id: usize| -> Result<()> {
                        ui.popup = match filtered_items.get(id).expect("invalid index") {
                            PlaylistFolderItem::Folder(f) => Some(PopupState::UserPlaylistList(
                                PlaylistPopupAction::AddEpisode {
                                    folder_id: f.target_id,
                                    episode_id,
                                    search_query: search_query.clone(),
                                },
                                ListState::default(),
                            )),
                            PlaylistFolderItem::Playlist(p) => {
                                client_pub.send(ClientRequest::AddPlayableToPlaylist(
                                    p.id.clone(),
                                    episode_id.into(),
                                ))?;
                                None
                            }
                        };
                        Ok(())
                    },
                    |ui: &mut UIStateGuard| {
                        ui.popup = None;
                    },
                )
            }
        },
        PopupState::UserFollowedArtistList(_) => {
            let artist_uris = state
                .data
                .read()
                .user_data
                .followed_artists
                .iter()
                .map(|a| a.id.uri())
                .collect::<Vec<_>>();

            handle_command_for_context_browsing_list_popup(
                command,
                ui,
                &artist_uris,
                &rspotify::model::Type::Artist,
            )
        }
        PopupState::UserSavedAlbumList(_) => {
            let album_uris = state
                .data
                .read()
                .user_data
                .saved_albums
                .iter()
                .map(|a| a.id.uri())
                .collect::<Vec<_>>();

            handle_command_for_context_browsing_list_popup(
                command,
                ui,
                &album_uris,
                &rspotify::model::Type::Album,
            )
        }
        PopupState::ThemeList(themes, _) => {
            let n_items = themes.len();

            handle_command_for_list_popup(
                command,
                ui,
                n_items,
                |ui: &mut UIStateGuard, id: usize| {
                    ui.theme = match ui.popup {
                        Some(PopupState::ThemeList(ref themes, _)) => themes[id].clone(),
                        _ => return,
                    };
                },
                |ui: &mut UIStateGuard, _| -> Result<()> {
                    ui.popup = None;
                    Ok(())
                },
                |ui: &mut UIStateGuard| {
                    ui.theme = match ui.popup {
                        Some(PopupState::ThemeList(ref themes, _)) => themes[0].clone(),
                        _ => return,
                    };
                    ui.popup = None;
                },
            )
        }
        PopupState::DeviceList(_) => {
            let player = state.player.read();

            handle_command_for_list_popup(
                command,
                ui,
                player.devices.len(),
                |_, _| {},
                |ui: &mut UIStateGuard, id: usize| -> Result<()> {
                    let is_playing = player.playback.as_ref().is_some_and(|p| p.is_playing);
                    client_pub.send(ClientRequest::Player(PlayerRequest::TransferPlayback(
                        player.devices[id].id.clone(),
                        is_playing,
                    )))?;
                    ui.popup = None;
                    Ok(())
                },
                |ui: &mut UIStateGuard| {
                    ui.popup = None;
                },
            )
        }
    }
}

fn handle_key_sequence_for_context_help_popup(
    key_sequence: &KeySequence,
    ui: &mut UIStateGuard,
) -> Result<bool> {
    if key_sequence.keys.len() != 1 {
        return Ok(true);
    }

    match key_sequence.keys[0] {
        Key::None(crossterm::event::KeyCode::Esc)
        | Key::None(crossterm::event::KeyCode::Char('?')) => {
            ui.popup = None;
            Ok(true)
        }
        _ => Ok(true),
    }
}

fn handle_key_sequence_for_shortcut_family_popup(
    key_sequence: &KeySequence,
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
    ui: &mut UIStateGuard,
) -> Result<bool> {
    if key_sequence.keys.len() == 1 {
        if let Some(PopupState::ShortcutFamily { prefix, .. }) = ui.popup.as_ref() {
            if prefix.keys.first() == key_sequence.keys.first() {
                ui.popup = None;
                return Ok(true);
            }
        }
    }

    if let Some(entry) = shortcut_family_entry_from_key_sequence(key_sequence, ui) {
        if entry.has_children {
            return Ok(open_shortcut_family_popup(entry.key_sequence, state, ui));
        }
        ui.popup = None;
        return super::dispatch_key_sequence(&entry.key_sequence, client_pub, state, ui);
    }

    if key_sequence.keys.len() == 1 {
        let nested_prefix = match ui.popup.as_ref() {
            Some(PopupState::ShortcutFamily { prefix, .. }) => {
                let mut combined = prefix.clone();
                combined.keys.push(key_sequence.keys[0]);
                combined
            }
            _ => return Ok(false),
        };

        if open_shortcut_family_popup(nested_prefix, state, ui) {
            return Ok(true);
        }
    }

    let keymap_config = &config::get_config().keymap_config;
    if keymap_config
        .find_command_or_action_from_key_sequence(key_sequence)
        .is_some()
        || keymap_config.has_matched_prefix(key_sequence)
    {
        ui.popup = None;
        return super::dispatch_key_sequence(key_sequence, client_pub, state, ui);
    }

    let Some(command) = keymap_config.find_command_from_key_sequence(key_sequence) else {
        ui.popup = None;
        return Ok(true);
    };

    let n_items = match ui.popup.as_ref() {
        Some(PopupState::ShortcutFamily { items, .. }) => items.len(),
        _ => return Ok(false),
    };

    if handle_command_for_list_popup(
        command,
        ui,
        n_items,
        |_, _| {},
        |ui: &mut UIStateGuard, id: usize| -> Result<()> {
            let entry = match ui.popup.as_ref() {
                Some(PopupState::ShortcutFamily { items, .. }) => items[id].clone(),
                _ => return Ok(()),
            };
            ui.popup = None;
            super::dispatch_key_sequence(&entry.key_sequence, client_pub, state, ui)?;
            Ok(())
        },
        |ui: &mut UIStateGuard| {
            ui.popup = None;
        },
    )? {
        return Ok(true);
    }

    ui.popup = None;
    super::dispatch_key_sequence(key_sequence, client_pub, state, ui)
}

fn shortcut_family_entry_from_key_sequence(
    key_sequence: &KeySequence,
    ui: &UIStateGuard,
) -> Option<crate::state::ShortcutFamilyItem> {
    let PopupState::ShortcutFamily { items, .. } = ui.popup.as_ref()? else {
        return None;
    };

    items
        .iter()
        .find(|item| item.trigger == *key_sequence)
        .cloned()
}

fn handle_key_sequence_for_create_playlist_popup(
    key_sequence: &KeySequence,
    client_pub: &flume::Sender<ClientRequest>,
    ui: &mut UIStateGuard,
) -> Result<bool> {
    let Some(PopupState::PlaylistCreate {
        name,
        desc,
        current_field,
    }) = &mut ui.popup
    else {
        return Ok(false);
    };
    if key_sequence.keys.len() == 1 {
        match &key_sequence.keys[0] {
            Key::None(crossterm::event::KeyCode::Enter) => {
                client_pub.send(ClientRequest::CreatePlaylist {
                    playlist_name: name.get_text(),
                    public: false,
                    collab: false,
                    desc: desc.get_text(),
                })?;
                ui.popup = None;
                return Ok(true);
            }
            Key::None(crossterm::event::KeyCode::Tab | crossterm::event::KeyCode::BackTab) => {
                *current_field = match &current_field {
                    PlaylistCreateCurrentField::Name => PlaylistCreateCurrentField::Desc,
                    PlaylistCreateCurrentField::Desc => PlaylistCreateCurrentField::Name,
                };
                return Ok(true);
            }
            k => {
                let line_input = match current_field {
                    PlaylistCreateCurrentField::Name => name,
                    PlaylistCreateCurrentField::Desc => desc,
                };
                if line_input.input(k).is_some() {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn handle_key_sequence_for_search_popup(
    key_sequence: &KeySequence,
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
    ui: &mut UIStateGuard,
) -> Result<bool> {
    // handle user's input that updates the search query
    let Some(PopupState::Search { ref mut query }) = &mut ui.popup else {
        return Ok(false);
    };
    if key_sequence.keys.len() == 1 {
        if let Key::None(c) = key_sequence.keys[0] {
            match c {
                crossterm::event::KeyCode::Char(c) => {
                    query.push(c);
                    ui.current_page_mut().select(0);
                    return Ok(true);
                }
                crossterm::event::KeyCode::Backspace => {
                    if query.is_empty() {
                        return Ok(true);
                    } else {
                        query.pop().unwrap();
                        ui.current_page_mut().select(0);
                    }
                    return Ok(true);
                }
                _ => {}
            }
        }
    }

    // key sequence not handle by the popup should be moved to the current page's event handler
    page::handle_key_sequence_for_page(key_sequence, client_pub, state, ui)
}

/// Handle a command for a context list popup in which each item represents a context
///
/// # Arguments
/// In addition to application's states and the key sequence,
/// the function requires to specify:
/// - `uris`: a list of context URIs
/// - `uri_type`: an enum represents the type of a context in the list (`playlist`, `artist`, etc)
fn handle_command_for_context_browsing_list_popup(
    command: Command,
    ui: &mut UIStateGuard,
    uris: &[String],
    context_type: &rspotify::model::Type,
) -> Result<bool> {
    handle_command_for_list_popup(
        command,
        ui,
        uris.len(),
        |_, _| {},
        |ui: &mut UIStateGuard, id: usize| -> Result<()> {
            let uri = crate::utils::parse_uri(&uris[id]);
            let context_id = match context_type {
                rspotify::model::Type::Playlist => {
                    ContextId::Playlist(PlaylistId::from_uri(&uri)?.into_static())
                }
                rspotify::model::Type::Artist => {
                    ContextId::Artist(ArtistId::from_uri(&uri)?.into_static())
                }
                rspotify::model::Type::Album => {
                    ContextId::Album(AlbumId::from_uri(&uri)?.into_static())
                }
                _ => {
                    return Ok(());
                }
            };

            ui.new_page(PageState::Context {
                id: None,
                context_page_type: ContextPageType::Browsing(context_id),
                state: None,
            });

            Ok(())
        },
        |ui: &mut UIStateGuard| {
            ui.popup = None;
        },
    )
}

/// Handle a command for a generic list popup.
///
/// # Arguments
/// - `n_items`: the number of items in the list
/// - `on_select_func`: the callback when selecting an item
/// - `on_choose_func`: the callback when choosing an item
/// - `on_close_func`: the callback when closing the popup
fn handle_command_for_list_popup(
    command: Command,
    ui: &mut UIStateGuard,
    n_items: usize,
    on_select_func: impl FnOnce(&mut UIStateGuard, usize),
    on_choose_func: impl FnOnce(&mut UIStateGuard, usize) -> anyhow::Result<()>,
    on_close_func: impl FnOnce(&mut UIStateGuard),
) -> anyhow::Result<bool> {
    let popup = ui.popup.as_mut().with_context(|| "expect a popup")?;
    let current_id = popup.list_selected().unwrap_or_default();

    match command {
        Command::SelectPreviousOrScrollUp => {
            if n_items > 0 {
                let next_id = if current_id == 0 {
                    n_items - 1
                } else {
                    current_id - 1
                };
                popup.list_select(Some(next_id));
                on_select_func(ui, next_id);
            }
        }
        Command::SelectNextOrScrollDown => {
            if n_items > 0 {
                let next_id = (current_id + 1) % n_items;
                popup.list_select(Some(next_id));
                on_select_func(ui, next_id);
            }
        }
        Command::ChooseSelected => {
            if current_id < n_items {
                on_choose_func(ui, current_id)?;
            }
        }
        Command::ClosePopup | Command::PreviousPage => {
            on_close_func(ui);
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn handle_key_sequence_for_action_list_popup(
    n_actions: usize,
    key_sequence: &KeySequence,
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
    ui: &mut UIStateGuard,
) -> Result<bool> {
    if let Some(Key::None(crossterm::event::KeyCode::Char(c))) = key_sequence.keys.first() {
        if let Some(id) = c.to_digit(10) {
            let id = id as usize;
            if id < n_actions {
                handle_item_action(id, client_pub, state, ui)?;
                return Ok(true);
            }
        }
    }

    let Some(command) = config::get_config()
        .keymap_config
        .find_command_from_key_sequence(key_sequence)
    else {
        return Ok(false);
    };

    handle_command_for_list_popup(
        command,
        ui,
        n_actions,
        |_, _| {},
        |ui: &mut UIStateGuard, id: usize| -> Result<()> {
            handle_item_action(id, client_pub, state, ui)?;
            Ok(())
        },
        |ui: &mut UIStateGuard| {
            ui.popup = None;
        },
    )
}

/// Handle the `n`-th action in an action list popup
pub fn handle_item_action(
    n: usize,
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
    ui: &mut UIStateGuard,
) -> Result<bool> {
    let item = match ui.popup {
        Some(PopupState::ActionList(ref item, ..)) => *item.clone(),
        _ => return Ok(false),
    };

    let data = state.data.read();

    match item {
        ActionListItem::Track(track, actions) => {
            handle_action_in_context(actions[n], track.into(), client_pub, state, &data, ui)
        }
        ActionListItem::Album(album, actions) => {
            handle_action_in_context(actions[n], album.into(), client_pub, state, &data, ui)
        }
        ActionListItem::Artist(artist, actions) => {
            handle_action_in_context(actions[n], artist.into(), client_pub, state, &data, ui)
        }
        ActionListItem::Playlist(playlist, actions) => {
            handle_action_in_context(actions[n], playlist.into(), client_pub, state, &data, ui)
        }
        ActionListItem::Show(show, actions) => {
            handle_action_in_context(actions[n], show.into(), client_pub, state, &data, ui)
        }
        ActionListItem::Episode(episode, actions) => {
            handle_action_in_context(actions[n], episode.into(), client_pub, state, &data, ui)
        }
    }
}

/// Handle key sequence for playlist search popup (AddTrack/AddEpisode)
fn handle_key_sequence_for_playlist_search_popup(
    key_sequence: &KeySequence,
    ui: &mut UIStateGuard,
) -> bool {
    // Handle user's input that updates the search query
    let Some(PopupState::UserPlaylistList(ref mut action, _)) = &mut ui.popup else {
        return false;
    };

    let search_query = match action {
        PlaylistPopupAction::AddTrack { search_query, .. }
        | PlaylistPopupAction::AddEpisode { search_query, .. }
        | PlaylistPopupAction::Browse { search_query, .. } => search_query,
    };

    if key_sequence.keys.len() == 1 {
        if let Key::None(c) = key_sequence.keys[0] {
            match c {
                crossterm::event::KeyCode::Char(c) => {
                    search_query.push(c);
                    // Reset selection to first item when search query changes
                    if let Some(popup) = &mut ui.popup {
                        popup.list_select(Some(0));
                    }
                    return true;
                }
                crossterm::event::KeyCode::Backspace => {
                    if search_query.is_empty() {
                        return true;
                    } else {
                        search_query.pop();
                        // Reset selection to first item when search query changes
                        if let Some(popup) = &mut ui.popup {
                            popup.list_select(Some(0));
                        }
                    }
                    return true;
                }
                _ => {}
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{self, Configs},
        state::{Mutex, State},
    };
    use std::{
        collections::VecDeque,
        path::PathBuf,
        sync::{Arc, Once},
    };

    static TEST_CONFIG: Once = Once::new();

    fn init_test_config() {
        TEST_CONFIG.call_once(|| {
            let root =
                std::env::temp_dir().join(format!("jx-spotify-popup-tests-{}", std::process::id()));
            let config_dir = root.join("config");
            let cache_dir = root.join("cache");

            std::fs::create_dir_all(&config_dir).expect("create test config dir");
            std::fs::create_dir_all(&cache_dir).expect("create test cache dir");

            config::set_config(
                Configs::new(&PathBuf::from(&config_dir), &PathBuf::from(&cache_dir))
                    .expect("initialize test config"),
            );
        });
    }

    fn test_state() -> SharedState {
        init_test_config();
        Arc::new(State::new(false, Arc::new(Mutex::new(VecDeque::new()))))
    }

    #[test]
    fn shortcut_family_popup_closes_on_unknown_single_key() {
        let state = test_state();
        let (client_pub, _client_sub) = flume::unbounded();
        let mut ui = state.ui.lock();

        assert!(open_shortcut_family_popup(
            KeySequence {
                keys: vec![Key::None(crossterm::event::KeyCode::Char('r'))],
            },
            &state,
            &mut ui,
        ));

        let handled = handle_key_sequence_for_shortcut_family_popup(
            &KeySequence {
                keys: vec![Key::None(crossterm::event::KeyCode::Char('q'))],
            },
            &client_pub,
            &state,
            &mut ui,
        )
        .expect("handle unknown shortcut-family key");

        assert!(handled);
        assert!(ui.popup.is_none());
    }

    #[test]
    fn shortcut_family_popup_falls_through_incomplete_prefix() {
        let state = test_state();
        let (client_pub, _client_sub) = flume::unbounded();
        let mut ui = state.ui.lock();

        assert!(open_shortcut_family_popup(
            KeySequence {
                keys: vec![Key::None(crossterm::event::KeyCode::Char('g'))],
            },
            &state,
            &mut ui,
        ));

        let handled = handle_key_sequence_for_shortcut_family_popup(
            &KeySequence {
                keys: vec![Key::None(crossterm::event::KeyCode::Char('m'))],
            },
            &client_pub,
            &state,
            &mut ui,
        )
        .expect("handle incomplete prefix after shortcut popup");

        assert!(!handled);
        assert!(ui.popup.is_none());
    }

    #[test]
    fn shortcut_family_popup_consumes_hidden_invalid_key() {
        let state = test_state();
        let (client_pub, _client_sub) = flume::unbounded();
        let mut ui = state.ui.lock();

        assert!(open_shortcut_family_popup(
            KeySequence {
                keys: vec![Key::None(crossterm::event::KeyCode::Char('r'))],
            },
            &state,
            &mut ui,
        ));

        let handled = handle_key_sequence_for_shortcut_family_popup(
            &KeySequence {
                keys: vec![Key::None(crossterm::event::KeyCode::Char('x'))],
            },
            &client_pub,
            &state,
            &mut ui,
        )
        .expect("consume hidden invalid radio-family key");

        assert!(handled);
        assert!(ui.popup.is_none());
    }

    #[test]
    fn shortcut_family_popup_repeated_family_key_cancels_popup() {
        let state = test_state();
        let (client_pub, _client_sub) = flume::unbounded();
        let mut ui = state.ui.lock();

        assert!(open_shortcut_family_popup(
            KeySequence {
                keys: vec![Key::None(crossterm::event::KeyCode::Char('r'))],
            },
            &state,
            &mut ui,
        ));

        let handled = handle_key_sequence_for_shortcut_family_popup(
            &KeySequence {
                keys: vec![Key::None(crossterm::event::KeyCode::Char('r'))],
            },
            &client_pub,
            &state,
            &mut ui,
        )
        .expect("repeat family key should cancel popup");

        assert!(handled);
        assert!(ui.popup.is_none());
    }

    #[test]
    fn shortcut_family_popup_dispatches_global_single_key_command() {
        let state = test_state();
        let (client_pub, client_sub) = flume::unbounded();
        let mut ui = state.ui.lock();

        assert!(open_shortcut_family_popup(
            KeySequence {
                keys: vec![Key::None(crossterm::event::KeyCode::Char('g'))],
            },
            &state,
            &mut ui,
        ));

        let handled = handle_key_sequence_for_shortcut_family_popup(
            &KeySequence {
                keys: vec![Key::None(crossterm::event::KeyCode::Char(' '))],
            },
            &client_pub,
            &state,
            &mut ui,
        )
        .expect("handle global single-key command after shortcut popup");

        assert!(handled);
        assert!(ui.popup.is_none());
        assert!(matches!(
            client_sub.try_recv().expect("resume/pause request"),
            ClientRequest::Player(PlayerRequest::ResumePause)
        ));
    }

    #[test]
    fn shortcut_family_popup_dispatches_leaked_full_sequence() {
        let state = test_state();
        let (client_pub, _client_sub) = flume::unbounded();
        let mut ui = state.ui.lock();
        let history_len = ui.history.len();

        assert!(open_shortcut_family_popup(
            KeySequence {
                keys: vec![Key::None(crossterm::event::KeyCode::Char('g'))],
            },
            &state,
            &mut ui,
        ));

        let handled = handle_key_sequence_for_shortcut_family_popup(
            &KeySequence {
                keys: vec![
                    Key::None(crossterm::event::KeyCode::Char('g')),
                    Key::None(crossterm::event::KeyCode::Char('l')),
                ],
            },
            &client_pub,
            &state,
            &mut ui,
        )
        .expect("handle leaked full shortcut-family sequence");

        assert!(handled);
        assert!(ui.popup.is_none());
        assert_eq!(ui.history.len(), history_len + 1);
    }

    #[test]
    fn radio_shortcut_family_hides_current_track_without_playback() {
        let state = test_state();
        let mut ui = state.ui.lock();

        assert!(open_shortcut_family_popup(
            KeySequence {
                keys: vec![Key::None(crossterm::event::KeyCode::Char('r'))],
            },
            &state,
            &mut ui,
        ));

        let Some(PopupState::ShortcutFamily { items, .. }) = ui.popup.as_ref() else {
            panic!("expected shortcut family popup");
        };

        assert!(!items.iter().any(|item| {
            item.trigger
                == KeySequence {
                    keys: vec![Key::None(crossterm::event::KeyCode::Char('c'))],
                }
        }));
    }
}
