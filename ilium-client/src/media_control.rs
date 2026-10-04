//! MPRIS media-player pause/resume adapter over the D-Bus session bus --
//! `lib.rs`'s `reconcile_voice_runtime` calls [`pause_playing_players`] when
//! voice mode actually starts and [`resume_players`] with its return value
//! when voice mode stops (see `VoiceSettings::pause_media_while_active`).
//! This sends the same `org.mpris.MediaPlayer2.Player.Pause`/`Play` D-Bus
//! calls a desktop's physical media keys trigger -- no shelled-out command.

use std::time::Duration;

use zbus::zvariant::OwnedValue;
use zbus::Connection;

mod owner;
pub(crate) use owner::{MediaLease, MediaOwner};

const MPRIS_BUS_NAME_PREFIX: &str = "org.mpris.MediaPlayer2.";
const PLAYER_OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const PLAYING_STATUS: &str = "Playing";

// Upper bound on any single D-Bus method call made through this module.
// zbus applies no reply timeout by default, and the bus daemon never
// answers on a peer's behalf, so without this a wedged bus-name owner
// (e.g. a stopped player process that never replies to `Get` or `Pause`)
// would hang the supervised owner indefinitely and delay restoration.
const DBUS_METHOD_TIMEOUT: Duration = Duration::from_secs(2);

/// Session-bus connection whose method calls all carry
/// [`DBUS_METHOD_TIMEOUT`], or `None` when no session bus is reachable
/// (headless environments, some containers) -- a normal, expected
/// condition for this module, not a bug.
async fn session_connection() -> Option<Connection> {
    zbus::connection::Builder::session()
        .ok()?
        .method_timeout(DBUS_METHOD_TIMEOUT)
        .max_queued(1)
        .build()
        .await
        .ok()
}

/// Pauses every MPRIS player currently reporting `PlaybackStatus: Playing`
/// and returns their bus names, so a later [`resume_players`] call resumes
/// only the players this call actually paused -- one already paused, or
/// with no active playback, is left untouched. Never propagates a failure:
/// no session bus (headless environments, some containers) or no MPRIS
/// player running is a normal, expected condition, not a bug -- it simply
/// pauses nothing.
pub async fn pause_playing_players() -> Vec<String> {
    pause_playing_players_until(&|| false).await
}

/// Stop admitting new player effects when the owner no longer wants Pause.
/// A Pause already sent is awaited and retained if positively acknowledged.
async fn pause_playing_players_until(should_stop: &(dyn Fn() -> bool + Sync)) -> Vec<String> {
    if should_stop() {
        return Vec::new();
    }
    let Some(connection) = session_connection().await else {
        return Vec::new();
    };
    let mut paused = Vec::new();
    for player in mpris_player_names(&connection).await {
        if should_stop() {
            break;
        }
        if playback_status(&connection, &player).await.as_deref() == Some(PLAYING_STATUS)
            && !should_stop()
            && call_player_method(&connection, &player, "Pause").await
        {
            paused.push(player);
        }
    }
    paused
}

/// Resumes exactly the players a prior [`pause_playing_players`] call
/// paused. Never propagates a failure: a player that quit, or a session bus
/// that's gone by the time voice mode stops, is a normal, expected
/// condition here, not a bug.
pub async fn resume_players(players: Vec<String>) {
    if players.is_empty() {
        return;
    }
    let Some(connection) = session_connection().await else {
        return;
    };
    for player in players {
        call_player_method(&connection, &player, "Play").await;
    }
}

/// Every session-bus name owned by a running MPRIS player.
async fn mpris_player_names(connection: &Connection) -> Vec<String> {
    let Ok(reply) = connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "ListNames",
            &(),
        )
        .await
    else {
        return Vec::new();
    };
    if reply.body().len() > 512 * 1024 {
        tracing::error!("media ListNames response exceeds 512 KiB admission");
        return Vec::new();
    }
    let names: Vec<String> = reply
        .body()
        .deserialize::<Vec<String>>()
        .unwrap_or_default()
        .into_iter()
        .filter(|name| name.starts_with(MPRIS_BUS_NAME_PREFIX))
        .collect();
    if names.len() > 256 {
        tracing::error!("media player inventory exceeds 256-player admission");
        return Vec::new();
    }
    names
}

/// `player`'s current `org.mpris.MediaPlayer2.Player.PlaybackStatus`
/// property (`"Playing"`, `"Paused"`, or `"Stopped"`), or `None` if the
/// player didn't answer.
async fn playback_status(connection: &Connection, player: &str) -> Option<String> {
    let reply = connection
        .call_method(
            Some(player),
            PLAYER_OBJECT_PATH,
            Some(PROPERTIES_INTERFACE),
            "Get",
            &(PLAYER_INTERFACE, "PlaybackStatus"),
        )
        .await
        .ok()?;
    if reply.body().len() > 1024 {
        tracing::error!("media PlaybackStatus response exceeds 1 KiB admission");
        return None;
    }
    let value: OwnedValue = reply.body().deserialize().ok()?;
    String::try_from(value).ok()
}

/// Invokes a no-argument `org.mpris.MediaPlayer2.Player` method on `player`,
/// returning whether it succeeded.
async fn call_player_method(connection: &Connection, player: &str, method: &str) -> bool {
    connection
        .call_method(
            Some(player),
            PLAYER_OBJECT_PATH,
            Some(PLAYER_INTERFACE),
            method,
            &(),
        )
        .await
        .is_ok()
}
