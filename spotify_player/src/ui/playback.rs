use super::{
    config, utils, Alignment, Constraint, Frame, Layout, Line, Modifier, Paragraph,
    PlaybackMetadata, Rect, SharedState, Span, Text, UIStateGuard, Wrap,
};
#[cfg(feature = "image")]
use crate::state::ImageRenderInfo;
use crate::{
    state::Track,
    ui::utils::{format_genres, to_bidi_string},
};
#[cfg(feature = "image")]
use anyhow::{Context, Result};
use rspotify::model::Id;
use std::collections::BTreeSet;

/// Render a playback window showing information about the current playback, which includes
/// - track title, artists, album
/// - playback metadata (playing state, repeat state, shuffle state, volume, device, etc)
/// - cover image (if `image` feature is enabled)
pub fn render_playback_window(
    frame: &mut Frame,
    state: &SharedState,
    ui: &mut UIStateGuard,
    rect: Rect,
) -> Rect {
    let (rect, other_rect) = split_rect_for_playback_window(state, ui, rect);
    let player = state.player.read();

    let rect = utils::render_panel(
        frame,
        &ui.theme,
        rect,
        "playback",
        Some(playback_meta_line(ui, &player)),
        true,
    );
    if let Some(ref playback) = player.playback {
        if let Some(item) = &playback.item {
            // Carve off the visualization rows here, inside the active-playback
            // branch, so the full rect is used when there is nothing playing.
            #[cfg(feature = "streaming")]
            let (rect, vis_rect) = {
                let configs = config::get_config();
                if configs.app_config.enable_audio_visualization
                    && state.is_local_streaming_active()
                {
                    let chunks = Layout::vertical([
                        Constraint::Fill(0),
                        Constraint::Length(super::streaming::VIS_HEIGHT),
                    ])
                    .split(rect);
                    (chunks[0], Some(chunks[1]))
                } else {
                    (rect, None)
                }
            };

            let metadata_rect = {
                // Render the track's cover image if `image` feature is enabled
                #[cfg(feature = "image")]
                {
                    let (cover_img_rect, metadata_rect) = split_rect_for_cover_img(rect);

                    let url = match item {
                        rspotify::model::PlayableItem::Track(track) => {
                            crate::utils::get_track_album_image_url(track).map(String::from)
                        }
                        rspotify::model::PlayableItem::Episode(episode) => {
                            crate::utils::get_episode_show_image_url(episode).map(String::from)
                        }
                        rspotify::model::PlayableItem::Unknown(_) => None,
                    };
                    if let Some(url) = url {
                        let needs_clear = if ui.last_cover_image_render_info.url != url
                            || ui.last_cover_image_render_info.render_area != cover_img_rect
                        {
                            ui.last_cover_image_render_info = ImageRenderInfo {
                                url,
                                render_area: cover_img_rect,
                                rendered: false,
                            };
                            true
                        } else {
                            false
                        };

                        if needs_clear {
                            // clear the image's both new and old areas to ensure no remaining artifacts before rendering the image
                            // See: https://github.com/aome510/spotify-player/issues/389
                            clear_area(
                                frame,
                                ui.last_cover_image_render_info.render_area,
                                &ui.theme,
                            );
                            clear_area(frame, cover_img_rect, &ui.theme);
                        } else {
                            if !ui.last_cover_image_render_info.rendered {
                                if let Err(err) = render_playback_cover_image(state, ui) {
                                    tracing::error!(
                                        "Failed to render playback's cover image: {err:#}"
                                    );
                                }
                            }

                            // set the `skip` state of cells in the cover image area
                            // to prevent buffer from overwriting the image's rendered area
                            // NOTE: `skip` should not be set when clearing the render area.
                            // Otherwise, nothing will be clear as the buffer doesn't handle cells with `skip=true`.
                            for x in cover_img_rect.left()..cover_img_rect.right() {
                                for y in cover_img_rect.top()..cover_img_rect.bottom() {
                                    frame
                                        .buffer_mut()
                                        .cell_mut((x, y))
                                        .expect("invalid cell")
                                        .set_skip(true);
                                }
                            }
                        }
                    }
                    metadata_rect
                }

                #[cfg(not(feature = "image"))]
                {
                    rect
                }
            };

            if let Some(ref playback) = player.buffered_playback {
                let playback_progress = player.playback_progress();
                let playback_text =
                    construct_playback_text(ui, state, item, playback, playback_progress);
                let playback_desc = Paragraph::new(playback_text);
                frame.render_widget(playback_desc, metadata_rect);
            }
            ui.playback_progress_bar_rect = Rect::default();
            #[cfg(feature = "streaming")]
            if let Some(vis_r) = vis_rect {
                super::streaming::render_audio_visualization(frame, state, vis_r);
            }
            return other_rect;
        }
    }

    // Previously rendered image can result in a weird rendering text,
    // clear the previous widget's area before rendering the text.
    #[cfg(feature = "image")]
    {
        if ui.last_cover_image_render_info.rendered {
            clear_area(
                frame,
                ui.last_cover_image_render_info.render_area,
                &ui.theme,
            );
            ui.last_cover_image_render_info = ImageRenderInfo::default();
        }
    }

    if player.playback_last_updated_time.is_none() {
        // Still waiting for the first successful playback fetch — show animated loading indicator
        const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let frame_idx = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            / 100) as usize
            % SPINNER_FRAMES.len();
        let vertical_chunks = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .split(rect);
        frame.render_widget(
            Paragraph::new(format!("{} Loading...", SPINNER_FRAMES[frame_idx]))
                .style(ui.theme.playback_metadata())
                .alignment(Alignment::Center),
            vertical_chunks[1],
        );
    } else {
        frame.render_widget(
            Paragraph::new(
                "No playback found. Please start a new playback.\n \
                 Make sure there is a running Spotify device and try to connect to one using the `SwitchDevice` command."
            )
            .wrap(Wrap { trim: true }),
            rect,
        );
    }

    other_rect
}

fn playback_meta_line(ui: &UIStateGuard, player: &crate::state::PlayerState) -> Line<'static> {
    let device_name = player
        .playback
        .as_ref()
        .map(|playback| playback.device.name.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "no active device".to_string());

    Line::from(vec![Span::styled(device_name, ui.theme.playlist_desc())])
}

#[cfg(feature = "image")]
fn split_rect_for_cover_img(rect: Rect) -> (Rect, Rect) {
    let configs = config::get_config();
    let hor_chunks = Layout::horizontal([
        Constraint::Length(configs.app_config.cover_img_length as u16),
        Constraint::Fill(0), // metadata_rect
    ])
    .spacing(1)
    .split(rect);
    let ver_chunks = Layout::vertical([
        Constraint::Length(configs.app_config.cover_img_width as u16), // cover_img_rect
    ])
    .split(hor_chunks[0]);

    (ver_chunks[0], hor_chunks[1])
}

#[cfg(feature = "image")]
fn clear_area(frame: &mut Frame, rect: Rect, theme: &config::Theme) {
    for x in rect.left()..rect.right() {
        for y in rect.top()..rect.bottom() {
            frame
                .buffer_mut()
                .cell_mut((x, y))
                .expect("invalid cell")
                .set_char(' ')
                .set_style(theme.app());
        }
    }
}

fn construct_playback_text(
    ui: &UIStateGuard,
    state: &SharedState,
    playable: &rspotify::model::PlayableItem,
    playback: &PlaybackMetadata,
    playback_progress: Option<chrono::Duration>,
) -> Text<'static> {
    // Construct a "styled" text (`playback_text`) from playback's data
    // based on a user-configurable format string (app_config.playback_format)
    let configs = config::get_config();
    let format_str = &configs.app_config.playback_format;
    let data = state.data.read();

    let mut playback_text = Text::default();
    let mut spans = vec![];

    // this regex is to handle a format argument or a newline
    let re = regex::Regex::new(r"\{.*?\}|\n").unwrap();

    let mut ptr = 0;
    for m in re.find_iter(format_str) {
        let s = m.start();
        let e = m.end();
        if ptr < s {
            spans.push(Span::raw(format_str[ptr..s].to_string()));
        }
        ptr = e;

        let (text, style) = match m.as_str() {
            // upon encountering a newline, create a new `Spans`
            "\n" => {
                let mut tmp = vec![];
                std::mem::swap(&mut tmp, &mut spans);
                playback_text.lines.push(Line::from(tmp));
                continue;
            }
            "{status}" => (
                if playback.is_playing {
                    &configs.app_config.play_icon
                } else {
                    &configs.app_config.pause_icon
                }
                .to_owned(),
                ui.theme.playback_status(),
            ),
            "{liked}" => match playable {
                rspotify::model::PlayableItem::Track(track) => match &track.id {
                    Some(id) => {
                        if data.user_data.saved_tracks.contains_key(&id.uri()) {
                            (configs.app_config.liked_icon.clone(), ui.theme.like())
                        } else {
                            continue;
                        }
                    }
                    None => continue,
                },
                rspotify::model::PlayableItem::Episode(_)
                | rspotify::model::PlayableItem::Unknown(_) => continue,
            },
            "{track}" => match playable {
                rspotify::model::PlayableItem::Track(track) => (
                    {
                        let track = Track::try_from_full_track(track.clone()).unwrap();
                        to_bidi_string(&track.display_name())
                    },
                    ui.theme.playback_track(),
                ),
                rspotify::model::PlayableItem::Episode(episode) => (
                    {
                        let bidi_string = to_bidi_string(&episode.name);
                        if episode.explicit {
                            format!("{bidi_string} (E)")
                        } else {
                            bidi_string
                        }
                    },
                    ui.theme.playback_track(),
                ),
                rspotify::model::PlayableItem::Unknown(_) => {
                    continue;
                }
            },
            "{track_number}" => match playable {
                rspotify::model::PlayableItem::Track(track) => (
                    { to_bidi_string(&track.track_number.to_string()) },
                    ui.theme.playback_track(),
                ),
                rspotify::model::PlayableItem::Episode(_)
                | rspotify::model::PlayableItem::Unknown(_) => {
                    continue;
                }
            },
            "{artists}" => match playable {
                rspotify::model::PlayableItem::Track(track) => (
                    to_bidi_string(&crate::utils::map_join(&track.artists, |a| &a.name, ", ")),
                    ui.theme.playback_artists(),
                ),
                rspotify::model::PlayableItem::Episode(episode) => {
                    (episode.show.publisher.clone(), ui.theme.playback_artists())
                }
                rspotify::model::PlayableItem::Unknown(_) => {
                    continue;
                }
            },
            "{album}" => match playable {
                rspotify::model::PlayableItem::Track(track) => {
                    (to_bidi_string(&track.album.name), ui.theme.playback_album())
                }
                rspotify::model::PlayableItem::Episode(episode) => (
                    to_bidi_string(&episode.show.name),
                    ui.theme.playback_album(),
                ),
                rspotify::model::PlayableItem::Unknown(_) => {
                    continue;
                }
            },
            "{genres}" => match playable {
                rspotify::model::PlayableItem::Track(full_track) => {
                    let genre = match data.caches.genres.get(&full_track.artists[0].name) {
                        Some(genres) => &format_genres(genres, configs.app_config.genre_num),
                        None => "no genre",
                    };
                    (to_bidi_string(genre), ui.theme.playback_genres())
                }
                rspotify::model::PlayableItem::Episode(_) => {
                    (to_bidi_string("no genre"), ui.theme.playback_genres())
                }
                rspotify::model::PlayableItem::Unknown(_) => {
                    continue;
                }
            },
            "{metadata}" => {
                let volume_value = if let Some(volume) = playback.mute_state {
                    format!("{volume}% (muted)")
                } else {
                    format!("{}%", playback.volume.unwrap_or_default())
                };
                let duration = match playable {
                    rspotify::model::PlayableItem::Track(track) => track.duration,
                    rspotify::model::PlayableItem::Episode(episode) => episode.duration,
                    rspotify::model::PlayableItem::Unknown(_) => chrono::Duration::zero(),
                };
                let progress = playback_progress
                    .map(|progress| std::cmp::min(progress, duration))
                    .unwrap_or_default();
                let time_value = format!(
                    "{}/{}",
                    crate::utils::format_duration(&progress),
                    crate::utils::format_duration(&duration),
                );
                let active_value_style = ui.theme.page_desc().add_modifier(Modifier::BOLD);
                let muted_value_style = ui.theme.playback_metadata();
                let label_style = ui.theme.playback_metadata();

                let mut metadata_spans = vec![Span::styled(time_value, active_value_style)];
                let mut first = false;
                let mut mode_fields = Vec::new();
                let mut seen_fields = BTreeSet::new();

                for field in &configs.app_config.playback_metadata_fields {
                    if !seen_fields.insert(field.as_str()) {
                        continue;
                    }

                    match field.as_str() {
                        "repeat" => mode_fields.push((
                            "repeat".to_string(),
                            <&'static str>::from(playback.repeat_state).to_string(),
                            playback.repeat_state != rspotify::model::RepeatState::Off,
                        )),
                        "shuffle" => mode_fields.push((
                            "shuffle".to_string(),
                            if playback.shuffle_state { "on" } else { "off" }.to_string(),
                            playback.shuffle_state,
                        )),
                        "volume" => {
                            if !first {
                                metadata_spans.push(Span::styled(" | ", label_style));
                            }
                            first = false;
                            metadata_spans.push(Span::styled("volume: ", label_style));
                            metadata_spans
                                .push(Span::styled(volume_value.clone(), active_value_style));
                        }
                        "device" => continue,
                        _ => continue,
                    }
                }

                if !mode_fields.is_empty() {
                    if !first {
                        metadata_spans.push(Span::styled(" | ", label_style));
                    }
                    metadata_spans.push(Span::styled("mode: ", label_style));
                    for (idx, (label, value, enabled)) in mode_fields.into_iter().enumerate() {
                        if idx > 0 {
                            metadata_spans.push(Span::styled(" · ", label_style));
                        }
                        metadata_spans.push(Span::styled(format!("{label} "), label_style));
                        metadata_spans.push(Span::styled(
                            value,
                            if enabled {
                                muted_value_style
                            } else {
                                muted_value_style.add_modifier(Modifier::DIM)
                            },
                        ));
                    }
                }

                spans.extend(metadata_spans);
                continue;
            }
            _ => continue,
        };

        spans.push(Span::styled(text, style));
    }
    if ptr < format_str.len() {
        spans.push(Span::raw(format_str[ptr..].to_string()));
    }
    if !spans.is_empty() {
        playback_text.lines.push(Line::from(spans));
    }

    playback_text
}

#[cfg(feature = "image")]
fn render_playback_cover_image(state: &SharedState, ui: &mut UIStateGuard) -> Result<()> {
    let data = state.data.read();
    if let Some(image) = data.caches.images.get(&ui.last_cover_image_render_info.url) {
        let rect = ui.last_cover_image_render_info.render_area;

        // `viuer` renders image using `sixel` in a different scale compared to other methods.
        // Scale the image to make the rendered image more fit if needed.
        // This scaling factor is user configurable as the scale works differently
        // with different fonts and terminals.
        // For more context, see https://github.com/aome510/spotify-player/issues/122.
        let scale = config::get_config().app_config.cover_img_scale;
        let width = (f32::from(rect.width) * scale).round() as u32;
        let height = (f32::from(rect.height) * scale).round() as u32;

        viuer::print(
            image,
            &viuer::Config {
                x: rect.x,
                y: rect.y as i16,
                width: Some(width),
                height: Some(height),
                restore_cursor: true,
                transparent: true,
                ..Default::default()
            },
        )
        .context("print image to the terminal")?;

        ui.last_cover_image_render_info.rendered = true;
    }

    Ok(())
}

/// Split the given area into two, the first one for the playback window
/// and the second one for the main application's layout (popup, page, etc).
#[allow(unused_variables)]
fn split_rect_for_playback_window(
    state: &SharedState,
    ui: &UIStateGuard,
    rect: Rect,
) -> (Rect, Rect) {
    let configs = config::get_config();
    let playback_width = estimated_playback_content_height(state, ui)
        .unwrap_or(configs.app_config.layout.playback_window_height)
        .min(configs.app_config.layout.playback_window_height);
    // the playback window's width should not be smaller than the cover image's width + 1
    #[cfg(feature = "image")]
    let playback_width = std::cmp::max(configs.app_config.cover_img_width + 1, playback_width);

    // When visualization is enabled *and* librespot is actively streaming,
    // reserve extra rows for the bar chart. When the local player is idle
    // (e.g. playback on an external Spotify Connect device) the rows are not
    // reserved so the space is available to the rest of the layout.
    #[cfg(feature = "streaming")]
    let playback_width = playback_width
        + if configs.app_config.enable_audio_visualization && state.is_local_streaming_active() {
            super::streaming::VIS_HEIGHT as usize
        } else {
            0
        };

    let playback_width = (playback_width + 1) as u16;

    let chunks =
        Layout::vertical([Constraint::Fill(0), Constraint::Length(playback_width)]).split(rect);

    (chunks[1], chunks[0])
}

fn estimated_playback_content_height(state: &SharedState, ui: &UIStateGuard) -> Option<usize> {
    let _ = (state, ui);
    let playback_format_lines = config::get_config()
        .app_config
        .playback_format
        .lines()
        .count()
        .max(1);

    Some(playback_format_lines.max(2))
}
