use crate::{auth::AuthConfig, client};

#[cfg(not(unix))]
use super::MAX_REQUEST_SIZE;
#[cfg(unix)]
use super::MAX_STREAM_MESSAGE_SIZE;
use super::{
    config, init_cli, start_socket, AlbumId, Command, ContextType, EditAction, GetRequest,
    IdOrName, ItemType, Key, PlaylistCommand, PlaylistId, Request, Response, SocketRequest,
    TrackId,
};
use anyhow::{Context, Result};
use clap::{ArgMatches, Id};
use clap_complete::{generate, Shell};
#[cfg(unix)]
use std::{
    io::{Read, Write},
    time::Duration,
};

#[cfg(not(unix))]
type ClientSocket = std::net::UdpSocket;

#[cfg(unix)]
type ClientSocket = std::os::unix::net::UnixStream;

#[cfg(not(unix))]
const SOCKET_HANDSHAKE_REQUEST: &[u8] = b"\0";
#[cfg(not(unix))]
const SOCKET_HANDSHAKE_RESPONSE: &[u8] = b"\x01";
#[cfg(not(unix))]
const RESPONSE_CHUNK_FINAL: u8 = 1;

#[cfg(unix)]
fn receive_response(socket: &mut ClientSocket) -> Result<Response> {
    let data = read_stream_frame(socket)?;
    Ok(serde_json::from_slice(&data)?)
}

#[cfg(not(unix))]
fn receive_response(socket: &ClientSocket) -> Result<Response> {
    // read response from the server's socket, which can be split into
    // smaller chunks of data
    let mut data = Vec::new();
    let mut buf = [0; 4096];
    loop {
        #[cfg(unix)]
        let n_bytes = socket.socket.recv(&mut buf)?;
        #[cfg(not(unix))]
        let (n_bytes, _) = socket.recv_from(&mut buf)?;
        if n_bytes == 0 {
            anyhow::bail!("received an empty socket response chunk");
        }
        let Some((&flags, payload)) = buf[..n_bytes].split_first() else {
            anyhow::bail!("received a malformed socket response chunk");
        };
        data.extend_from_slice(payload);
        if flags == RESPONSE_CHUNK_FINAL {
            break;
        }
    }

    Ok(serde_json::from_slice(&data)?)
}

#[cfg(unix)]
fn read_stream_frame(socket: &mut ClientSocket) -> Result<Vec<u8>> {
    let mut len_buf = [0; 4];
    socket.read_exact(&mut len_buf)?;

    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_STREAM_MESSAGE_SIZE {
        anyhow::bail!("socket frame exceeds the maximum allowed size");
    }

    let mut data = vec![0; len];
    socket.read_exact(&mut data)?;
    Ok(data)
}

#[cfg(unix)]
fn write_stream_frame(socket: &mut ClientSocket, data: &[u8]) -> Result<()> {
    if data.len() > MAX_STREAM_MESSAGE_SIZE {
        anyhow::bail!("socket frame exceeds the maximum allowed size");
    }

    let len = u32::try_from(data.len()).context("socket frame is too large to encode")?;
    socket.write_all(&len.to_be_bytes())?;
    socket.write_all(data)?;
    socket.flush()?;
    Ok(())
}

fn get_id_or_name(args: &ArgMatches) -> IdOrName {
    match args
        .get_one::<Id>("id_or_name")
        .expect("id_or_name group is required")
        .as_str()
    {
        "name" => IdOrName::Name(
            args.get_one::<String>("name")
                .expect("name should be specified")
                .to_owned(),
        ),
        "id" => IdOrName::Id(
            args.get_one::<String>("id")
                .expect("id should be specified")
                .to_owned(),
        ),
        id => panic!("unknown id: {id}"),
    }
}

fn handle_get_subcommand(args: &ArgMatches) -> Request {
    let (cmd, args) = args.subcommand().expect("playback subcommand is required");

    let request = match cmd {
        "key" => {
            let key = args
                .get_one::<Key>("key")
                .expect("key is required")
                .to_owned();
            Request::Get(GetRequest::Key(key))
        }
        "item" => {
            let item_type = args
                .get_one::<ItemType>("item_type")
                .expect("context_type is required")
                .to_owned();
            let id_or_name = get_id_or_name(args);
            Request::Get(GetRequest::Item(item_type, id_or_name))
        }
        _ => unreachable!(),
    };

    request
}

fn handle_playback_subcommand(args: &ArgMatches) -> Result<Request> {
    let (cmd, args) = args.subcommand().expect("playback subcommand is required");
    let command = match cmd {
        "start" => match args.subcommand() {
            Some(("track", args)) => Command::StartTrack(get_id_or_name(args)),
            Some(("context", args)) => {
                let context_type = args
                    .get_one::<ContextType>("context_type")
                    .expect("context_type is required")
                    .to_owned();
                let shuffle = args.get_flag("shuffle");

                let id_or_name = get_id_or_name(args);
                Command::StartContext {
                    context_type,
                    id_or_name,
                    shuffle,
                }
            }
            Some(("liked", args)) => {
                let limit = *args
                    .get_one::<usize>("limit")
                    .expect("limit should have a default value");
                let random = args.get_flag("random");
                Command::StartLikedTracks { limit, random }
            }
            Some(("radio", args)) => {
                let item_type = args
                    .get_one::<ItemType>("item_type")
                    .expect("item_type is required")
                    .to_owned();
                let id_or_name = get_id_or_name(args);
                Command::StartRadio(item_type, id_or_name)
            }
            _ => {
                anyhow::bail!("invalid command!");
            }
        },
        "play-pause" => Command::PlayPause,
        "play" => Command::Play,
        "pause" => Command::Pause,
        "next" => Command::Next,
        "previous" => Command::Previous,
        "shuffle" => Command::Shuffle,
        "repeat" => Command::Repeat,
        "volume" => {
            let percent = args
                .get_one::<i8>("percent")
                .expect("percent arg is required");
            let offset = args.get_flag("offset");
            Command::Volume {
                percent: *percent,
                is_offset: offset,
            }
        }
        "seek" => {
            let position_offset_ms = args
                .get_one::<i64>("position_offset_ms")
                .expect("position_offset_ms is required");
            Command::Seek(*position_offset_ms)
        }
        _ => unreachable!(),
    };

    Ok(Request::Playback(command))
}

#[cfg(not(unix))]
/// Tries to connect to a running client, if exists, by sending a connection request
/// to the client via a UDP socket.
/// If no running client found, create a new client running in a separate thread to
/// handle the socket request.
fn try_connect_to_client(socket: &ClientSocket, configs: &config::Configs) -> Result<()> {
    // send a handshake request to confirm the server is alive
    let perform_handshake = || -> std::io::Result<(usize, [u8; 1])> {
        socket.send(SOCKET_HANDSHAKE_REQUEST)?;
        let mut buf = [0; 1];
        Ok((socket.recv(&mut buf)?, buf))
    };

    let needs_spawn = false;

    if needs_spawn {
        spawn_client_socket_server(configs)?;
    }

    match perform_handshake() {
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
            ) =>
        {
            spawn_client_socket_server(configs)?;
            let (n_bytes, buf) = perform_handshake()?;
            if n_bytes != SOCKET_HANDSHAKE_RESPONSE.len()
                || &buf[..n_bytes] != SOCKET_HANDSHAKE_RESPONSE
            {
                anyhow::bail!("received an invalid socket handshake response");
            }
        }
        Err(err) => return Err(err.into()),
        Ok((n_bytes, buf))
            if n_bytes != SOCKET_HANDSHAKE_RESPONSE.len()
                || &buf[..n_bytes] != SOCKET_HANDSHAKE_RESPONSE =>
        {
            anyhow::bail!("received an invalid socket handshake response");
        }
        Ok(_) => {}
    }

    Ok(())
}

#[cfg(unix)]
fn connect_to_client(configs: &config::Configs) -> Result<ClientSocket> {
    let socket_path = super::socket_path();

    match connect_client_stream(&socket_path) {
        Ok(stream) => Ok(stream),
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            spawn_client_socket_server(configs)?;
            connect_client_stream_with_retry(&socket_path)
        }
        Err(err) => Err(err.into()),
    }
}

#[cfg(unix)]
fn connect_client_stream(path: &std::path::Path) -> std::io::Result<ClientSocket> {
    let stream = std::os::unix::net::UnixStream::connect(path)?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    Ok(stream)
}

#[cfg(unix)]
fn connect_client_stream_with_retry(path: &std::path::Path) -> Result<ClientSocket> {
    let mut last_err = None;

    for _ in 0..20 {
        match connect_client_stream(path) {
            Ok(stream) => return Ok(stream),
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) =>
            {
                last_err = Some(err);
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(err) => return Err(err.into()),
        }
    }

    Err(last_err
        .unwrap_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "timed out waiting for the client socket server",
            )
        })
        .into())
}

fn spawn_client_socket_server(_configs: &config::Configs) -> Result<()> {
    // no running `spotify_player` instance found,
    // initialize a new client to handle the current CLI command

    let rt = tokio::runtime::Runtime::new()?;

    // create a Spotify API client
    let client = rt
        .block_on(client::AppClient::new())
        .context("construct app client")?;
    rt.block_on(client.new_session(None, false))
        .context("new session")?;

    // create a client socket for handling CLI commands
    // NOTE: the socket must be bound *before* spawning the thread to avoid a
    // race condition where the caller sends a request before the socket is ready.
    #[cfg(unix)]
    let client_socket = {
        let socket_path = super::socket_path();
        if let Some(parent) = socket_path.parent() {
            crate::utils::ensure_private_dir(parent)?;
        }
        match std::fs::remove_file(&socket_path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
        let socket = tokio::net::UnixListener::bind(&socket_path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;
        }
        socket
    };

    #[cfg(not(unix))]
    let client_socket = rt.block_on(tokio::net::UdpSocket::bind((
        "127.0.0.1",
        _configs.app_config.client_port,
    )))?;

    // spawn a thread to handle the CLI request
    std::thread::spawn(move || {
        rt.block_on(start_socket(&client, None, Some(client_socket)));
    });

    Ok(())
}

#[cfg(not(unix))]
fn create_client_socket(_configs: &config::Configs) -> Result<ClientSocket> {
    Ok(std::net::UdpSocket::bind("127.0.0.1:0")?)
}

pub fn handle_cli_subcommand(cmd: &str, args: &ArgMatches) -> Result<()> {
    let configs = config::get_config();

    // handle commands that don't require a client separately
    match cmd {
        "authenticate" => {
            let auth_config = AuthConfig::new(configs)?;
            crate::auth::get_creds(&auth_config, true, false)?;
            std::process::exit(0);
        }
        "generate" => {
            let gen = *args
                .get_one::<Shell>("shell")
                .expect("shell argument is required");
            let mut cmd = init_cli()?;
            let name = cmd.get_name().to_string();
            generate(gen, &mut cmd, name, &mut std::io::stdout());
            std::process::exit(0);
        }
        "features" => {
            print_features();
            std::process::exit(0);
        }
        _ => {}
    }

    // construct a socket request based on the CLI command and its arguments
    let request = match cmd {
        "get" => handle_get_subcommand(args),
        "playback" => handle_playback_subcommand(args)?,
        "playlist" => handle_playlist_subcommand(args)?,
        "connect" => Request::Connect(get_id_or_name(args)),
        "like" => Request::Like {
            unlike: args.get_flag("unlike"),
        },
        "search" => Request::Search {
            query: args
                .get_one::<String>("query")
                .expect("query is required")
                .to_owned(),
        },
        _ => unreachable!(),
    };

    // send the request to the client's socket
    let request_buf = serde_json::to_vec(&SocketRequest {
        auth_token: super::load_or_create_socket_auth_token()?,
        request,
    })?;

    #[cfg(unix)]
    let response = {
        let mut socket = connect_to_client(configs).context("try to connect to a client")?;
        write_stream_frame(&mut socket, &request_buf)?;
        receive_response(&mut socket)?
    };

    #[cfg(not(unix))]
    let response = {
        let socket = create_client_socket(configs)?;
        try_connect_to_client(&socket, configs).context("try to connect to a client")?;
        assert!(request_buf.len() <= MAX_REQUEST_SIZE);
        socket.send(&request_buf)?;
        receive_response(&socket)?
    };

    match response {
        Response::Err(err) => {
            eprintln!("{}", String::from_utf8_lossy(&err));
            std::process::exit(1);
        }
        Response::Ok(data) => {
            println!("{}", String::from_utf8_lossy(&data).replace("\\n", "\n"));
            std::process::exit(0);
        }
    }
}

fn handle_playlist_subcommand(args: &ArgMatches) -> Result<Request> {
    let (cmd, args) = args.subcommand().expect("playlist subcommand is required");
    let command = match cmd {
        "new" => {
            let name = args
                .get_one::<String>("name")
                .expect("name arg is required")
                .to_owned();

            let description = args
                .get_one::<String>("description")
                .map(std::borrow::ToOwned::to_owned)
                .unwrap_or_default();

            let public = args.get_flag("public");
            let collab = args.get_flag("collab");

            PlaylistCommand::New {
                name,
                public,
                collab,
                description,
            }
        }
        "delete" => {
            let id = args
                .get_one::<String>("id")
                .expect("id arg is required")
                .to_owned();

            let pid = PlaylistId::from_id(id)?;

            PlaylistCommand::Delete { id: pid }
        }
        "list" => PlaylistCommand::List,
        "import" => {
            let from_s = args
                .get_one::<String>("from")
                .expect("'from' PlaylistID is required.")
                .to_owned();

            let to_s = args
                .get_one::<String>("to")
                .expect("'to' PlaylistID is required.")
                .to_owned();

            let delete = args.get_flag("delete");

            let from = PlaylistId::from_id(from_s.clone())?;
            let to = PlaylistId::from_id(to_s.clone())?;

            println!("Importing '{from_s}' into '{to_s}'...\n");
            PlaylistCommand::Import { from, to, delete }
        }
        "fork" => {
            let id_s = args
                .get_one::<String>("id")
                .expect("Playlist id is required.")
                .to_owned();

            let id = PlaylistId::from_id(id_s.clone())?;

            println!("Forking '{id_s}'...\n");
            PlaylistCommand::Fork { id }
        }
        "sync" => {
            let id_s = args.get_one::<String>("id");
            let delete = args.get_flag("delete");

            let pid = if let Some(id_s) = id_s {
                println!("Syncing imports for playlist '{id_s}'...\n");
                Some(PlaylistId::from_id(id_s.to_owned())?)
            } else {
                println!("Syncing imports for all playlists...\n");
                None
            };

            PlaylistCommand::Sync { id: pid, delete }
        }
        "edit" => {
            let playlist_id = PlaylistId::from_id(
                args.get_one::<String>("playlist_id")
                    .expect("playlist_id arg is required")
                    .to_owned(),
            )?;

            let action = *args
                .get_one::<EditAction>("action")
                .expect("action arg is required");

            let track_id = args
                .get_one::<String>("track_id")
                .map(|s| TrackId::from_id(s.to_owned()))
                .transpose()?;

            let album_id = args
                .get_one::<String>("album_id")
                .map(|s| AlbumId::from_id(s.to_owned()))
                .transpose()?;

            PlaylistCommand::Edit {
                playlist_id,
                action,
                track_id,
                album_id,
            }
        }
        _ => unreachable!(),
    };

    Ok(Request::Playlist(command))
}

macro_rules! print_feature {
    ($feature:literal) => {
        #[cfg(feature = $feature)]
        println!("  ✓ {}", $feature);
        #[cfg(not(feature = $feature))]
        println!("  ✗ {}", $feature);
    };
}

fn print_features() {
    println!("Compile-time features:");

    print_feature!("daemon");
    print_feature!("streaming");
    print_feature!("media-control");
    print_feature!("image");
    print_feature!("viuer");
    print_feature!("sixel");
    print_feature!("pixelate");
    print_feature!("notify");
    print_feature!("fzf");

    // Audio backends
    print_feature!("pulseaudio-backend");
    print_feature!("alsa-backend");
    print_feature!("rodio-backend");
    print_feature!("jackaudio-backend");
    print_feature!("sdl-backend");
    print_feature!("gstreamer-backend");
}
