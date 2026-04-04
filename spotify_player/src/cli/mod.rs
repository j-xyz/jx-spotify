mod client;
mod commands;
mod handlers;

use crate::config;
use crate::utils::write_private_file;
use rspotify::model::{AlbumId, ArtistId, Id, PlaylistId, TrackId};
use serde::{Deserialize, Serialize};

#[cfg(unix)]
const MAX_STREAM_MESSAGE_SIZE: usize = 16 * 1024 * 1024;
#[cfg(not(unix))]
const MAX_REQUEST_SIZE: usize = 8192;
const SOCKET_AUTH_TOKEN_FILE: &str = "client_auth_token";
const SOCKET_FILE: &str = "client.sock";

pub use client::start_socket;
pub use handlers::handle_cli_subcommand;

#[derive(Debug, Serialize, Deserialize, clap::ValueEnum, Clone)]
pub enum Key {
    Playback,
    Devices,
    UserPlaylists,
    UserLikedTracks,
    UserSavedAlbums,
    UserFollowedArtists,
    UserTopTracks,
    Queue,
}

#[derive(Debug, Serialize, Deserialize, clap::ValueEnum, Clone)]
pub enum ContextType {
    Playlist,
    Album,
    Artist,
}

#[derive(Debug, Serialize, Deserialize, clap::ValueEnum, Clone)]
pub enum ItemType {
    Playlist,
    Album,
    Artist,
    Track,
}

/// Spotify item's ID
enum ItemId {
    Playlist(PlaylistId<'static>),
    Artist(ArtistId<'static>),
    Album(AlbumId<'static>),
    Track(TrackId<'static>),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum GetRequest {
    Key(Key),
    Item(ItemType, IdOrName),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum IdOrName {
    Id(String),
    Name(String),
}

#[derive(Debug, Serialize, Deserialize, clap::ValueEnum, Clone, Copy)]
pub enum EditAction {
    Add,
    Delete,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum PlaylistCommand {
    New {
        name: String,
        public: bool,
        collab: bool,
        description: String,
    },
    Delete {
        id: PlaylistId<'static>,
    },
    List,
    Import {
        from: PlaylistId<'static>,
        to: PlaylistId<'static>,
        delete: bool,
    },
    Fork {
        id: PlaylistId<'static>,
    },
    Sync {
        id: Option<PlaylistId<'static>>,
        delete: bool,
    },
    Edit {
        action: EditAction,
        playlist_id: PlaylistId<'static>,
        track_id: Option<TrackId<'static>>,
        album_id: Option<AlbumId<'static>>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Command {
    StartContext {
        context_type: ContextType,
        id_or_name: IdOrName,
        shuffle: bool,
    },
    StartTrack(IdOrName),
    StartLikedTracks {
        limit: usize,
        random: bool,
    },
    StartRadio(ItemType, IdOrName),
    PlayPause,
    Play,
    Pause,
    Next,
    Previous,
    Shuffle,
    Repeat,
    Volume {
        percent: i8,
        is_offset: bool,
    },
    Seek(i64),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Get(GetRequest),
    Playback(Command),
    Connect(IdOrName),
    Like { unlike: bool },
    Playlist(PlaylistCommand),
    Search { query: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SocketRequest {
    pub auth_token: String,
    pub request: Request,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Ok(Vec<u8>),
    Err(Vec<u8>),
}

impl From<ContextType> for ItemType {
    fn from(value: ContextType) -> Self {
        match value {
            ContextType::Playlist => Self::Playlist,
            ContextType::Album => Self::Album,
            ContextType::Artist => Self::Artist,
        }
    }
}

impl ItemId {
    pub fn uri(&self) -> String {
        match self {
            ItemId::Playlist(id) => id.uri(),
            ItemId::Artist(id) => id.uri(),
            ItemId::Album(id) => id.uri(),
            ItemId::Track(id) => id.uri(),
        }
    }
}

pub fn load_or_create_socket_auth_token() -> anyhow::Result<String> {
    let path = config::get_config()
        .cache_folder
        .join(SOCKET_AUTH_TOKEN_FILE);

    match std::fs::read_to_string(&path) {
        Ok(token) => {
            let token = token.trim().to_string();
            if !token.is_empty() {
                return Ok(token);
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }

    let token = format!(
        "{:032x}{:032x}",
        rand::random::<u128>(),
        rand::random::<u128>()
    );
    write_private_file(&path, &token)?;
    Ok(token)
}

pub fn socket_path() -> std::path::PathBuf {
    config::get_config().cache_folder.join(SOCKET_FILE)
}

pub fn init_cli() -> anyhow::Result<clap::Command> {
    let default_cache_folder = config::get_cache_folder_path()?;
    let default_config_folder = config::get_config_folder_path()?;

    let cmd = clap::Command::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .subcommand(commands::init_get_subcommand())
        .subcommand(commands::init_playback_subcommand())
        .subcommand(commands::init_connect_subcommand())
        .subcommand(commands::init_like_command())
        .subcommand(commands::init_authenticate_command())
        .subcommand(commands::init_playlist_subcommand())
        .subcommand(commands::init_generate_command())
        .subcommand(commands::init_search_command())
        .subcommand(commands::init_print_features_command())
        .arg(
            clap::Arg::new("theme")
                .short('t')
                .long("theme")
                .value_name("THEME")
                .help("Application theme"),
        )
        .arg(
            clap::Arg::new("config-folder")
                .short('c')
                .long("config-folder")
                .value_name("FOLDER")
                .default_value(default_config_folder.into_os_string())
                .help("Path to the application's config folder"),
        )
        .arg(
            clap::Arg::new("cache-folder")
                .short('C')
                .long("cache-folder")
                .value_name("FOLDER")
                .default_value(default_cache_folder.into_os_string())
                .help("Path to the application's cache folder"),
        );

    #[cfg(feature = "daemon")]
    let cmd = cmd.arg(
        clap::Arg::new("daemon")
            .short('d')
            .long("daemon")
            .action(clap::ArgAction::SetTrue)
            .help("Running the application as a daemon"),
    );

    Ok(cmd)
}
