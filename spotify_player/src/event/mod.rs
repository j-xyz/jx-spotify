use crate::{
    client::{ClientRequest, PlayerRequest},
    command::{
        self, construct_artist_actions, Action, ActionContext, ActionTarget, Command,
        CommandOrAction,
    },
    config,
    key::{Key, KeySequence},
    state::{
        ActionListItem, Album, AlbumId, Artist, ArtistFocusState, ArtistId, ArtistPopupAction,
        BrowsePageUIState, Context, ContextId, ContextPageType, ContextPageUIState, DataReadGuard,
        Focusable, Id, Item, ItemId, LibraryFocusState, LibraryPageUIState, PageState, PageType,
        PlayableId, Playback, PlaylistCreateCurrentField, PlaylistFolderItem, PlaylistId,
        PlaylistPopupAction, PopupState, SearchFocusState, SearchPageUIState, SearchTuiFocus,
        SharedState, ShowId, Track, TrackId, TrackOrder, TracksId, UIStateGuard,
        USER_LIKED_TRACKS_ID, USER_RECENTLY_PLAYED_TRACKS_ID, USER_TOP_TRACKS_ID,
    },
    ui::{single_line_input::LineInput, Orientation},
    utils::parse_uri,
};

use crate::utils::map_join;
use anyhow::{Context as _, Result};
use crossterm::event::KeyCode;
use serde::Serialize;
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use clipboard::{execute_copy_command, get_clipboard_content};
use ratatui::widgets::ListState;

mod clipboard;
mod page;
mod popup;
mod window;

/// Start a terminal event handler (key pressed, mouse clicked, etc)
pub fn start_event_handler(state: &SharedState, client_pub: &flume::Sender<ClientRequest>) {
    while let Ok(event) = crossterm::event::read() {
        let _enter = tracing::info_span!("terminal_event", event = ?event).entered();
        if let Err(err) = match event {
            crossterm::event::Event::Mouse(event) => {
                state.ui.lock().last_interaction_at = Instant::now();
                state.request_redraw();
                handle_mouse_event(event, client_pub, state)
            }
            crossterm::event::Event::Resize(columns, rows) => {
                let mut ui = state.ui.lock();
                ui.orientation = Orientation::from_size(columns, rows);
                ui.last_interaction_at = Instant::now();
                drop(ui);
                state.request_redraw();
                Ok(())
            }
            crossterm::event::Event::Key(event) => {
                if should_handle_key_event(&event) {
                    state.ui.lock().last_interaction_at = Instant::now();
                    state.request_redraw();
                    handle_key_event(event, client_pub, state)
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        } {
            tracing::error!("Failed to handle terminal event: {err:#}");
        }
    }
}

fn should_handle_key_event(event: &crossterm::event::KeyEvent) -> bool {
    match event.kind {
        crossterm::event::KeyEventKind::Press => true,
        // Accept held-key repeats for directional navigation without
        // re-enabling duplicate handling for every shortcut.
        crossterm::event::KeyEventKind::Repeat => matches!(
            event.code,
            crossterm::event::KeyCode::Up
                | crossterm::event::KeyCode::Down
                | crossterm::event::KeyCode::Left
                | crossterm::event::KeyCode::Right
                | crossterm::event::KeyCode::PageUp
                | crossterm::event::KeyCode::PageDown
                | crossterm::event::KeyCode::Char('j')
                | crossterm::event::KeyCode::Char('k')
        ),
        crossterm::event::KeyEventKind::Release => false,
    }
}

// Handle a terminal mouse event
fn handle_mouse_event(
    event: crossterm::event::MouseEvent,
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
) -> Result<()> {
    tracing::debug!("Handling mouse event: {event:?}");

    match event.kind {
        // a left click event
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
            let rect = state.ui.lock().playback_progress_bar_rect;
            if event.row == rect.y && event.column >= rect.x && event.column < rect.x + rect.width {
                // calculate the seek position (in ms) based on the mouse click position,
                // the progress bar's width and the track's duration (in ms)
                let player = state.player.read();
                let duration = match player.currently_playing() {
                    Some(rspotify::model::PlayableItem::Track(track)) => Some(track.duration),
                    Some(rspotify::model::PlayableItem::Episode(episode)) => Some(episode.duration),
                    Some(rspotify::model::PlayableItem::Unknown(_)) | None => None,
                };
                if let Some(duration) = duration {
                    let position_ms = (duration.num_milliseconds())
                        * i64::from(event.column - rect.x)
                        / i64::from(rect.width);
                    client_pub.send(ClientRequest::Player(PlayerRequest::SeekTrack(
                        chrono::Duration::try_milliseconds(position_ms).unwrap(),
                    )))?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

// Handle a terminal key pressed event
fn handle_key_event(
    event: crossterm::event::KeyEvent,
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
) -> Result<()> {
    let key: Key = event.into();
    let mut ui = state.ui.lock();
    let is_question_mark = matches!(key, Key::None(KeyCode::Char('?')));

    if is_question_mark && !page_accepts_text_input(&ui) {
        ui.toggle_footer_help_preview();
        state.request_redraw();
        if ui.footer_help_preview_visible {
            return Ok(());
        }
    } else if matches!(key, Key::None(KeyCode::Esc)) && ui.footer_help_preview_visible {
        ui.hide_footer_help_preview();
        state.request_redraw();
    } else if ui.footer_help_preview_visible && !matches!(key, Key::None(KeyCode::Esc)) {
        ui.hide_footer_help_preview();
        state.request_redraw();
    }

    if !ui.input_key_sequence.keys.is_empty() && key == Key::None(KeyCode::Esc) {
        ui.input_key_sequence.keys.clear();
        ui.count_prefix = None;
        return Ok(());
    }

    if ui.popup.is_none()
        && ui.input_key_sequence.keys.is_empty()
        && !page_accepts_text_input(&ui)
        && popup::try_open_shortcut_family_popup(key, &mut ui)
    {
        ui.count_prefix = None;
        return Ok(());
    }

    let mut key_sequence = ui.input_key_sequence.clone();
    key_sequence.keys.push(key);

    // check if the current key sequence matches any keymap's prefix
    // if not, reset the key sequence
    let keymap_config = &config::get_config().keymap_config;
    if !keymap_config.has_matched_prefix(&key_sequence) {
        key_sequence = KeySequence { keys: vec![key] };
    }

    tracing::debug!(
        "Handling key event: {event:?}, current key sequence: {key_sequence:?}, count prefix: {:?}",
        ui.count_prefix
    );
    let handled = dispatch_key_sequence(&key_sequence, client_pub, state, &mut ui)?;

    // if handled, clear the key sequence and count prefix
    // otherwise, the current key sequence can be a prefix of a command's shortcut
    if handled {
        ui.input_key_sequence.keys = vec![];
        ui.count_prefix = None;
    } else {
        // update the count prefix if the key is a digit
        match key {
            Key::None(KeyCode::Char(c)) if c.is_ascii_digit() => {
                let digit = c.to_digit(10).unwrap() as usize;
                ui.input_key_sequence.keys = vec![];
                ui.count_prefix = match ui.count_prefix {
                    Some(count) => Some(count * 10 + digit),
                    None => {
                        if digit > 0 {
                            Some(digit)
                        } else {
                            None
                        }
                    }
                };
            }
            _ => {
                ui.input_key_sequence = key_sequence;
                ui.count_prefix = None;
            }
        }
    }
    Ok(())
}

fn page_accepts_text_input(ui: &UIStateGuard) -> bool {
    match ui.current_page() {
        PageState::Search { state, .. } => state.focus == SearchFocusState::Input,
        PageState::SearchTui { state, .. } => state.focus == SearchTuiFocus::Search,
        _ => false,
    }
}

pub(super) fn dispatch_key_sequence(
    key_sequence: &KeySequence,
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
    ui: &mut UIStateGuard,
) -> Result<bool> {
    let handled = {
        if ui.popup.is_none() {
            page::handle_key_sequence_for_page(key_sequence, client_pub, state, ui)?
        } else {
            popup::handle_key_sequence_for_popup(key_sequence, client_pub, state, ui)?
        }
    };

    if handled {
        return Ok(true);
    }

    match config::get_config()
        .keymap_config
        .find_command_or_action_from_key_sequence(key_sequence)
    {
        Some(CommandOrAction::Action(action, target)) => {
            handle_global_action(action, target, client_pub, state, ui)
        }
        Some(CommandOrAction::Command(command)) => {
            handle_global_command(command, client_pub, state, ui)
        }
        None => Ok(false),
    }
}

pub fn handle_action_in_context(
    action: Action,
    context: ActionContext,
    client_pub: &flume::Sender<ClientRequest>,
    data: &DataReadGuard,
    ui: &mut UIStateGuard,
) -> Result<bool> {
    match context {
        ActionContext::Track(track) => match action {
            Action::GoToAlbum => {
                if let Some(album) = track.album {
                    let context_id = ContextId::Album(
                        AlbumId::from_uri(&parse_uri(&album.id.uri()))?.into_static(),
                    );
                    ui.new_page(PageState::Context {
                        id: None,
                        context_page_type: ContextPageType::Browsing(context_id),
                        state: None,
                    });
                    return Ok(true);
                }
                Ok(false)
            }
            Action::GoToArtist => {
                handle_go_to_artist(track.artists, ui);
                Ok(true)
            }
            Action::AddToQueue => {
                client_pub.send(ClientRequest::AddPlayableToQueue(track.id.into()))?;
                ui.popup = None;
                Ok(true)
            }
            Action::CopyLink => {
                let track_url = format!("https://open.spotify.com/track/{}", track.id.id());
                execute_copy_command(track_url)?;
                ui.popup = None;
                Ok(true)
            }
            Action::AddToPlaylist => {
                client_pub.send(ClientRequest::GetUserPlaylists)?;
                ui.popup = Some(PopupState::UserPlaylistList(
                    PlaylistPopupAction::AddTrack {
                        folder_id: 0,
                        track_id: track.id,
                        search_query: String::new(),
                    },
                    ListState::default(),
                ));
                Ok(true)
            }
            Action::ToggleLiked => {
                if data.user_data.is_liked_track(&track) {
                    client_pub.send(ClientRequest::DeleteFromLibrary(ItemId::Track(track.id)))?;
                } else {
                    client_pub.send(ClientRequest::AddToLibrary(Item::Track(track)))?;
                }
                ui.popup = None;
                Ok(true)
            }
            Action::AddToLiked => {
                client_pub.send(ClientRequest::AddToLibrary(Item::Track(track)))?;
                ui.popup = None;
                Ok(true)
            }
            Action::DeleteFromLiked => {
                client_pub.send(ClientRequest::DeleteFromLibrary(ItemId::Track(track.id)))?;
                ui.popup = None;
                Ok(true)
            }
            Action::GoToRadio => {
                handle_go_to_radio(&track.id.uri(), &track.name, ui, client_pub)?;
                Ok(true)
            }
            Action::ShowActionsOnArtist => {
                handle_show_actions_on_artist(track.artists, data, ui);
                Ok(true)
            }
            Action::ShowActionsOnAlbum => {
                if let Some(album) = track.album {
                    let context = ActionContext::Album(album.clone());
                    ui.popup = Some(PopupState::ActionList(
                        Box::new(ActionListItem::Album(
                            album,
                            context.get_available_actions(data),
                        )),
                        ListState::default(),
                    ));
                    return Ok(true);
                }
                Ok(false)
            }
            Action::DeleteFromPlaylist => {
                if let PageState::Context {
                    id: Some(ContextId::Playlist(playlist_id)),
                    ..
                } = ui.current_page()
                {
                    client_pub.send(ClientRequest::DeleteTrackFromPlaylist(
                        playlist_id.clone_static(),
                        track.id,
                    ))?;
                }
                ui.popup = None;
                Ok(true)
            }
            _ => Ok(false),
        },
        ActionContext::Album(album) => match action {
            Action::GoToArtist => {
                handle_go_to_artist(album.artists, ui);
                Ok(true)
            }
            Action::GoToRadio => {
                handle_go_to_radio(&album.id.uri(), &album.name, ui, client_pub)?;
                Ok(true)
            }
            Action::ShowActionsOnArtist => {
                handle_show_actions_on_artist(album.artists, data, ui);
                Ok(true)
            }
            Action::AddToLibrary => {
                client_pub.send(ClientRequest::AddToLibrary(Item::Album(album)))?;
                ui.popup = None;
                Ok(true)
            }
            Action::DeleteFromLibrary => {
                client_pub.send(ClientRequest::DeleteFromLibrary(ItemId::Album(album.id)))?;
                ui.popup = None;
                Ok(true)
            }
            Action::CopyLink => {
                let album_url = format!("https://open.spotify.com/album/{}", album.id.id());
                execute_copy_command(album_url)?;
                ui.popup = None;
                Ok(true)
            }
            Action::AddToQueue => {
                client_pub.send(ClientRequest::AddAlbumToQueue(album.id))?;
                ui.popup = None;
                Ok(true)
            }
            _ => Ok(false),
        },
        ActionContext::Artist(artist) => match action {
            Action::Follow => {
                client_pub.send(ClientRequest::AddToLibrary(Item::Artist(artist)))?;
                ui.popup = None;
                Ok(true)
            }
            Action::Unfollow => {
                client_pub.send(ClientRequest::DeleteFromLibrary(ItemId::Artist(artist.id)))?;
                ui.popup = None;
                Ok(true)
            }
            Action::CopyLink => {
                let artist_url = format!("https://open.spotify.com/artist/{}", artist.id.id());
                execute_copy_command(artist_url)?;
                ui.popup = None;
                Ok(true)
            }
            Action::GoToRadio => {
                handle_go_to_radio(&artist.id.uri(), &artist.name, ui, client_pub)?;
                Ok(true)
            }
            _ => Ok(false),
        },
        ActionContext::Playlist(playlist) => match action {
            Action::AddToLibrary => {
                client_pub.send(ClientRequest::AddToLibrary(Item::Playlist(playlist)))?;
                ui.popup = None;
                Ok(true)
            }
            Action::GoToRadio => {
                handle_go_to_radio(&playlist.id.uri(), &playlist.name, ui, client_pub)?;
                Ok(true)
            }
            Action::CopyLink => {
                let playlist_url =
                    format!("https://open.spotify.com/playlist/{}", playlist.id.id());
                execute_copy_command(playlist_url)?;
                ui.popup = None;
                Ok(true)
            }
            Action::DeleteFromLibrary => {
                client_pub.send(ClientRequest::DeleteFromLibrary(ItemId::Playlist(
                    playlist.id,
                )))?;
                ui.popup = None;
                Ok(true)
            }
            _ => Ok(false),
        },
        ActionContext::Show(show) => match action {
            Action::CopyLink => {
                let show_url = format!("https://open.spotify.com/show/{}", show.id.id());
                execute_copy_command(show_url)?;
                ui.popup = None;
                Ok(true)
            }
            Action::AddToLibrary => {
                client_pub.send(ClientRequest::AddToLibrary(Item::Show(show)))?;
                ui.popup = None;
                Ok(true)
            }
            Action::DeleteFromLibrary => {
                client_pub.send(ClientRequest::DeleteFromLibrary(ItemId::Show(show.id)))?;
                ui.popup = None;
                Ok(true)
            }
            _ => Ok(false),
        },
        ActionContext::Episode(episode) => match action {
            Action::GoToShow => {
                if let Some(show) = episode.show {
                    let context_id = ContextId::Show(
                        ShowId::from_uri(&parse_uri(&show.id.uri()))?.into_static(),
                    );
                    ui.new_page(PageState::Context {
                        id: None,
                        context_page_type: ContextPageType::Browsing(context_id),
                        state: None,
                    });
                    return Ok(true);
                }
                Ok(false)
            }
            Action::AddToQueue => {
                client_pub.send(ClientRequest::AddPlayableToQueue(episode.id.into()))?;
                ui.popup = None;
                Ok(true)
            }
            Action::CopyLink => {
                let episode_url = format!("https://open.spotify.com/episode/{}", episode.id.id());
                execute_copy_command(episode_url)?;
                ui.popup = None;
                Ok(true)
            }
            Action::AddToPlaylist => {
                client_pub.send(ClientRequest::GetUserPlaylists)?;
                ui.popup = Some(PopupState::UserPlaylistList(
                    PlaylistPopupAction::AddEpisode {
                        folder_id: 0,
                        episode_id: episode.id,
                        search_query: String::new(),
                    },
                    ListState::default(),
                ));
                Ok(true)
            }
            Action::ShowActionsOnShow => {
                if let Some(show) = episode.show {
                    let context = ActionContext::Show(show.clone());
                    ui.popup = Some(PopupState::ActionList(
                        Box::new(ActionListItem::Show(
                            show,
                            context.get_available_actions(data),
                        )),
                        ListState::default(),
                    ));
                    return Ok(true);
                }
                Ok(false)
            }
            _ => Ok(false),
        },
        // TODO: support actions for playlist folders
        ActionContext::PlaylistFolder(_) => Ok(false),
    }
}

fn handle_go_to_radio(
    seed_uri: &str,
    seed_name: &str,
    ui: &mut UIStateGuard,
    client_pub: &flume::Sender<ClientRequest>,
) -> anyhow::Result<()> {
    let radio_id = TracksId::new(format!("radio:{seed_uri}"), format!("{seed_name} Radio"));
    ui.new_page(PageState::Context {
        id: None,
        context_page_type: ContextPageType::Browsing(ContextId::Tracks(radio_id.clone())),
        state: None,
    });
    client_pub.send(ClientRequest::GetContext(ContextId::Tracks(radio_id)))?;
    Ok(())
}

fn handle_go_to_artist(artists: Vec<Artist>, ui: &mut UIStateGuard) {
    if artists.len() == 1 {
        let context_id = ContextId::Artist(artists[0].id.clone());
        ui.new_page(PageState::Context {
            id: None,
            context_page_type: ContextPageType::Browsing(context_id),
            state: None,
        });
    } else {
        ui.popup = Some(PopupState::ArtistList(
            ArtistPopupAction::Browse,
            artists,
            ListState::default(),
        ));
    }
}

fn handle_show_actions_on_artist(
    artists: Vec<Artist>,
    data: &DataReadGuard,
    ui: &mut UIStateGuard,
) {
    if artists.len() == 1 {
        let actions = construct_artist_actions(&artists[0], data);
        ui.popup = Some(PopupState::ActionList(
            Box::new(ActionListItem::Artist(artists[0].clone(), actions)),
            ListState::default(),
        ));
    } else {
        ui.popup = Some(PopupState::ArtistList(
            ArtistPopupAction::ShowActions,
            artists,
            ListState::default(),
        ));
    }
}

/// Handle a global action, currently this is only used to target
/// the currently playing item instead of the selection.
fn handle_global_action(
    action: Action,
    target: ActionTarget,
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
    ui: &mut UIStateGuard,
) -> Result<bool> {
    if target == ActionTarget::PlayingTrack {
        let player = state.player.read();
        let data = state.data.read();

        if let Some(currently_playing) = player.currently_playing() {
            match currently_playing {
                rspotify::model::PlayableItem::Track(track) => {
                    if let Some(track) = Track::try_from_full_track(track.clone()) {
                        return handle_action_in_context(
                            action,
                            ActionContext::Track(track),
                            client_pub,
                            &data,
                            ui,
                        );
                    }
                }
                rspotify::model::PlayableItem::Episode(episode) => {
                    return handle_action_in_context(
                        action,
                        ActionContext::Episode(episode.clone().into()),
                        client_pub,
                        &data,
                        ui,
                    );
                }
                rspotify::model::PlayableItem::Unknown(_) => {
                    return Ok(false);
                }
            }
        }
    }

    Ok(false)
}

/// Handle a global command that is not specific to any page/popup
fn handle_global_command(
    command: Command,
    client_pub: &flume::Sender<ClientRequest>,
    state: &SharedState,
    ui: &mut UIStateGuard,
) -> Result<bool> {
    match command {
        Command::Quit => {
            ui.is_running = false;
        }
        Command::NextTrack => {
            client_pub.send(ClientRequest::Player(PlayerRequest::NextTrack))?;
        }
        Command::PreviousTrack => {
            client_pub.send(ClientRequest::Player(PlayerRequest::PreviousTrack))?;
        }
        Command::ResumePause => {
            client_pub.send(ClientRequest::Player(PlayerRequest::ResumePause))?;
        }
        Command::Repeat => {
            client_pub.send(ClientRequest::Player(PlayerRequest::Repeat))?;
        }
        Command::Shuffle => {
            client_pub.send(ClientRequest::Player(PlayerRequest::Shuffle))?;
        }
        Command::VolumeChange { offset } => {
            if let Some(ref playback) = state.player.read().buffered_playback {
                if let Some(volume) = playback.volume {
                    let volume = std::cmp::min(volume as i32 + offset, 100_i32);
                    client_pub.send(ClientRequest::Player(PlayerRequest::Volume(volume as u8)))?;
                }
            }
        }
        Command::Mute => {
            client_pub.send(ClientRequest::Player(PlayerRequest::ToggleMute))?;
        }
        Command::SeekStart => {
            client_pub.send(ClientRequest::Player(PlayerRequest::SeekTrack(
                chrono::TimeDelta::try_seconds(0).unwrap(),
            )))?;
        }
        Command::SeekForward { duration } => {
            if let Some(progress) = state.player.read().playback_progress() {
                let duration =
                    duration.unwrap_or(config::get_config().app_config.seek_duration_secs);
                client_pub.send(ClientRequest::Player(PlayerRequest::SeekTrack(
                    progress + chrono::Duration::try_seconds(i64::from(duration)).unwrap(),
                )))?;
            }
        }
        Command::SeekBackward { duration } => {
            if let Some(progress) = state.player.read().playback_progress() {
                let duration =
                    duration.unwrap_or(config::get_config().app_config.seek_duration_secs);
                client_pub.send(ClientRequest::Player(PlayerRequest::SeekTrack(
                    std::cmp::max(
                        chrono::Duration::zero(),
                        progress - chrono::Duration::try_seconds(i64::from(duration)).unwrap(),
                    ),
                )))?;
            }
        }
        Command::OpenCommandHelp => {
            ui.new_page(PageState::CommandHelp { scroll_offset: 0 });
        }
        Command::OpenLogs => {
            ui.new_page(PageState::Logs { scroll_offset: 0 });
        }
        Command::GoExternalGlow => {
            launch_external_glow(state)?;
        }
        Command::RefreshPlayback => {
            client_pub.send(ClientRequest::GetCurrentPlayback)?;
        }
        Command::GoToRadioFromCurrentTrack => {
            let player = state.player.read();
            let data = state.data.read();

            if let Some(currently_playing) = player.currently_playing() {
                match currently_playing {
                    rspotify::model::PlayableItem::Track(track) => {
                        if let Some(track) = Track::try_from_full_track(track.clone()) {
                            handle_action_in_context(
                                Action::GoToRadio,
                                ActionContext::Track(track),
                                client_pub,
                                &data,
                                ui,
                            )?;
                        }
                    }
                    rspotify::model::PlayableItem::Episode(episode) => {
                        handle_action_in_context(
                            Action::GoToRadio,
                            ActionContext::Episode(episode.clone().into()),
                            client_pub,
                            &data,
                            ui,
                        )?;
                    }
                    rspotify::model::PlayableItem::Unknown(_) => {}
                }
            }
        }
        Command::ShowActionsOnCurrentTrack => {
            if let Some(currently_playing) = state.player.read().currently_playing() {
                match currently_playing {
                    rspotify::model::PlayableItem::Track(track) => {
                        if let Some(track) = Track::try_from_full_track(track.clone()) {
                            let data = state.data.read();
                            let actions = command::construct_track_actions(&track, &data);
                            ui.popup = Some(PopupState::ActionList(
                                Box::new(ActionListItem::Track(track, actions)),
                                ListState::default(),
                            ));
                        }
                    }
                    rspotify::model::PlayableItem::Episode(episode) => {
                        let episode = episode.clone().into();
                        let data = state.data.read();
                        let actions = command::construct_episode_actions(&episode, &data);
                        ui.popup = Some(PopupState::ActionList(
                            Box::new(ActionListItem::Episode(episode, actions)),
                            ListState::default(),
                        ));
                    }
                    rspotify::model::PlayableItem::Unknown(_) => {}
                }
            }
        }
        Command::CurrentlyPlayingContextPage => {
            ui.new_page(PageState::Context {
                id: None,
                context_page_type: ContextPageType::CurrentPlaying,
                state: None,
            });
        }
        Command::BrowseUserPlaylists => {
            client_pub.send(ClientRequest::GetUserPlaylists)?;
            ui.popup = Some(PopupState::UserPlaylistList(
                PlaylistPopupAction::Browse {
                    folder_id: 0,
                    search_query: String::new(),
                },
                ListState::default(),
            ));
        }
        Command::BrowseUserFollowedArtists => {
            client_pub.send(ClientRequest::GetUserFollowedArtists)?;
            ui.popup = Some(PopupState::UserFollowedArtistList(ListState::default()));
        }
        Command::BrowseUserSavedAlbums => {
            client_pub.send(ClientRequest::GetUserSavedAlbums)?;
            ui.popup = Some(PopupState::UserSavedAlbumList(ListState::default()));
        }
        Command::TopTrackPage => {
            ui.new_page(PageState::Context {
                id: None,
                context_page_type: ContextPageType::Browsing(ContextId::Tracks(
                    USER_TOP_TRACKS_ID.to_owned(),
                )),
                state: None,
            });
            client_pub.send(ClientRequest::GetContext(ContextId::Tracks(
                USER_TOP_TRACKS_ID.to_owned(),
            )))?;
        }
        Command::RecentlyPlayedTrackPage => {
            ui.new_page(PageState::Context {
                id: None,
                context_page_type: ContextPageType::Browsing(ContextId::Tracks(
                    USER_RECENTLY_PLAYED_TRACKS_ID.to_owned(),
                )),
                state: None,
            });
            client_pub.send(ClientRequest::GetContext(ContextId::Tracks(
                USER_RECENTLY_PLAYED_TRACKS_ID.to_owned(),
            )))?;
        }
        Command::LikedTrackPage => {
            ui.new_page(PageState::Context {
                id: None,
                context_page_type: ContextPageType::Browsing(ContextId::Tracks(
                    USER_LIKED_TRACKS_ID.to_owned(),
                )),
                state: None,
            });
            client_pub.send(ClientRequest::GetContext(ContextId::Tracks(
                USER_LIKED_TRACKS_ID.to_owned(),
            )))?;
        }
        Command::LibraryPage => {
            ui.new_page(PageState::Library {
                state: LibraryPageUIState::new(),
            });
        }
        Command::SearchPage => {
            ui.new_page(PageState::Search {
                line_input: LineInput::default(),
                current_query: String::new(),
                state: SearchPageUIState::new(),
            });
        }
        Command::SearchTuiHome => {
            ui.open_or_reset_search_tui_home();
        }
        Command::BrowsePage => {
            ui.new_page(PageState::Browse {
                state: BrowsePageUIState::CategoryList {
                    state: ListState::default(),
                },
            });
            client_pub.send(ClientRequest::GetBrowseCategories)?;
        }
        Command::PreviousPage => {
            if ui.popup.is_some() {
                ui.popup = None;
            } else if ui.history.len() > 1 {
                ui.history.pop();
            }
        }
        Command::OpenSpotifyLinkFromClipboard => {
            let content = get_clipboard_content().context("get clipboard's content")?;
            let re = regex::Regex::new(
                r"https://open.spotify.com/(?P<type>.*?)/(?P<id>[[:alnum:]]*).*",
            )?;
            if let Some(cap) = re.captures(&content) {
                let typ = cap.name("type").expect("valid capture").as_str();
                let id = cap.name("id").expect("valid capture").as_str();
                match typ {
                    // for track link, play the song
                    "track" => {
                        let id = TrackId::from_id(id)?.into_static();

                        // Clear Tracks context when playing from clipboard link
                        state.player.write().currently_playing_tracks_id = None;

                        client_pub.send(ClientRequest::Player(PlayerRequest::StartPlayback(
                            Playback::URIs(vec![id.into()], None),
                            None,
                        )))?;
                    }
                    // for playlist/artist/album link, go to the corresponding context page
                    "playlist" => {
                        let id = PlaylistId::from_id(id)?.into_static();
                        ui.new_page(PageState::Context {
                            id: None,
                            context_page_type: ContextPageType::Browsing(ContextId::Playlist(id)),
                            state: None,
                        });
                    }
                    "artist" => {
                        let id = ArtistId::from_id(id)?.into_static();
                        ui.new_page(PageState::Context {
                            id: None,
                            context_page_type: ContextPageType::Browsing(ContextId::Artist(id)),
                            state: None,
                        });
                    }
                    "album" => {
                        let id = AlbumId::from_id(id)?.into_static();
                        ui.new_page(PageState::Context {
                            id: None,
                            context_page_type: ContextPageType::Browsing(ContextId::Album(id)),
                            state: None,
                        });
                    }
                    e => anyhow::bail!("unsupported Spotify type {e}!"),
                }
            } else {
                tracing::warn!("clipboard's content ({content}) is not a valid Spotify link!");
            }
        }
        Command::LyricsPage => {
            if let Some(rspotify::model::PlayableItem::Track(track)) =
                state.player.read().currently_playing()
            {
                if let Some(id) = &track.id {
                    let artists = map_join(&track.artists, |a| &a.name, ", ");
                    ui.new_page(PageState::Lyrics {
                        track_uri: id.uri(),
                        track: track.name.clone(),
                        artists,
                    });

                    client_pub.send(ClientRequest::GetLyrics {
                        track_id: id.clone_static(),
                    })?;
                }
            }
        }
        Command::SwitchDevice => {
            ui.popup = Some(PopupState::DeviceList(ListState::default()));
            client_pub.send(ClientRequest::GetDevices)?;
        }
        Command::SwitchTheme => {
            // get the available themes with the current theme moved to the first position
            let mut themes = config::get_config().theme_config.themes.clone();
            let id = themes.iter().position(|t| t.name == ui.theme.name);
            if let Some(id) = id {
                let theme = themes.remove(id);
                themes.insert(0, theme);
            }

            ui.popup = Some(PopupState::ThemeList(themes, ListState::default()));
        }
        #[cfg(feature = "streaming")]
        Command::RestartIntegratedClient => {
            client_pub.send(ClientRequest::RestartIntegratedClient)?;
        }
        Command::FocusNextWindow => {
            if !ui.has_focused_popup() {
                ui.current_page_mut().next();
            }
        }
        Command::FocusPreviousWindow => {
            if !ui.has_focused_popup() {
                ui.current_page_mut().previous();
            }
        }
        Command::Queue => {
            ui.new_page(PageState::Queue { scroll_offset: 0 });
            client_pub.send(ClientRequest::GetCurrentUserQueue)?;
        }
        Command::CreatePlaylist => {
            ui.popup = Some(PopupState::PlaylistCreate {
                name: LineInput::default(),
                desc: LineInput::default(),
                current_field: PlaylistCreateCurrentField::Name,
            });
        }
        Command::JumpToCurrentTrackInContext => {
            let track_id = match state.player.read().currently_playing() {
                Some(rspotify::model::PlayableItem::Track(track)) => {
                    PlayableId::Track(track.id.clone().expect("all non-local tracks have ids"))
                }
                Some(rspotify::model::PlayableItem::Episode(episode)) => {
                    PlayableId::Episode(episode.id.clone())
                }
                Some(rspotify::model::PlayableItem::Unknown(_)) | None => return Ok(false),
            };

            if let PageState::Context {
                id: Some(context_id),
                ..
            } = ui.current_page()
            {
                let context_track_pos = state
                    .data
                    .read()
                    .context_tracks(context_id)
                    .and_then(|tracks| tracks.iter().position(|t| t.id.uri() == track_id.uri()));

                if let Some(p) = context_track_pos {
                    ui.current_page_mut().select(p);
                }
            }
        }
        Command::ClosePopup => {
            ui.popup = None;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

#[derive(Serialize)]
struct ExternalHandoffEnvelope {
    version: u8,
    from: &'static str,
    to: &'static str,
    intent: &'static str,
    created_at: String,
    return_token: String,
    payload: ExternalHandoffPayload,
}

#[derive(Serialize)]
struct ExternalHandoffPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    now_playing: Option<ExternalNowPlayingPayload>,
}

#[derive(Serialize)]
struct ExternalNowPlayingPayload {
    track_name: String,
    artist_names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    album_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    progress_ms: Option<i64>,
}

fn launch_external_glow(state: &SharedState) -> Result<()> {
    let cache_folder = config::get_config().cache_folder.clone();
    let handoff_path = write_external_handoff_file(state, &cache_folder)?;
    let external_command = resolve_external_glow_command()?;

    let mut command = std::process::Command::new(&external_command.command);
    command.args(&external_command.args);
    command.arg("--handoff-file");
    command.arg(&handoff_path);
    command.spawn().with_context(|| {
        format!(
            "failed to launch external jx-glow command `{}`",
            external_command.command
        )
    })?;

    Ok(())
}

fn resolve_external_glow_command() -> Result<config::Command> {
    if let Some(command) = config::get_config()
        .app_config
        .external_glow_command
        .clone()
    {
        return Ok(command);
    }

    let glow = which::which("jx-glow")
        .context("no external glow command configured and `jx-glow` is not installed on PATH")?;
    Ok(config::Command {
        command: glow.to_string_lossy().into_owned(),
        args: vec![],
    })
}

fn write_external_handoff_file(state: &SharedState, cache_folder: &Path) -> Result<PathBuf> {
    let handoff_dir = cache_folder.join("handoffs");
    fs::create_dir_all(&handoff_dir).with_context(|| {
        format!(
            "failed to create handoff directory `{}`",
            handoff_dir.display()
        )
    })?;
    prune_stale_handoff_files(&handoff_dir);

    let envelope = ExternalHandoffEnvelope {
        version: 1,
        from: "jx-spotify",
        to: "jx-glow",
        intent: "resume-work",
        created_at: chrono::Utc::now().to_rfc3339(),
        return_token: generate_return_token(),
        payload: ExternalHandoffPayload {
            now_playing: build_now_playing_payload(state),
        },
    };

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    for attempt in 0..8 {
        let filename = format!(
            "jx-worksuite-handoff-{}-{}-{}.json",
            seed,
            std::process::id(),
            attempt
        );
        let path = handoff_dir.join(filename);
        let mut file = create_private_handoff_file(&path)?;
        serde_json::to_writer_pretty(&mut file, &envelope).with_context(|| {
            format!(
                "failed to serialize handoff envelope to `{}`",
                path.display()
            )
        })?;
        file.write_all(b"\n")
            .context("failed to finalize handoff envelope")?;
        return Ok(path);
    }

    anyhow::bail!(
        "failed to allocate a unique handoff envelope file in `{}`",
        handoff_dir.display()
    )
}

#[cfg(unix)]
fn create_private_handoff_file(path: &Path) -> Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("failed to create handoff file `{}`", path.display()))
}

#[cfg(not(unix))]
fn create_private_handoff_file(path: &Path) -> Result<File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("failed to create handoff file `{}`", path.display()))
}

fn prune_stale_handoff_files(dir: &Path) {
    let cutoff = match SystemTime::now().checked_sub(Duration::from_secs(24 * 60 * 60)) {
        Some(cutoff) => cutoff,
        None => return,
    };

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
            continue;
        };
        if !name.starts_with("jx-worksuite-handoff-") || !name.ends_with(".json") {
            continue;
        }

        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if modified < cutoff {
            let _ = fs::remove_file(path);
        }
    }
}

fn build_now_playing_payload(state: &SharedState) -> Option<ExternalNowPlayingPayload> {
    let player = state.player.read();
    let progress_ms = player
        .playback_progress()
        .map(|progress| progress.num_milliseconds());

    match player.currently_playing() {
        Some(rspotify::model::PlayableItem::Track(track)) => Some(ExternalNowPlayingPayload {
            track_name: track.name.clone(),
            artist_names: track
                .artists
                .iter()
                .map(|artist| artist.name.clone())
                .collect(),
            album_name: Some(track.album.name.clone()),
            uri: track.id.as_ref().map(|id| id.uri()),
            progress_ms,
        }),
        Some(rspotify::model::PlayableItem::Episode(episode)) => Some(ExternalNowPlayingPayload {
            track_name: episode.name.clone(),
            artist_names: vec![episode.show.publisher.clone()],
            album_name: Some(episode.show.name.clone()),
            uri: Some(episode.id.uri()),
            progress_ms,
        }),
        Some(rspotify::model::PlayableItem::Unknown(_)) | None => None,
    }
}

fn generate_return_token() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("{}-{millis}", std::process::id())
}
