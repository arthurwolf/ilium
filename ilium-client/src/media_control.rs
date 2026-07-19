//! MPRIS media-player pause/resume adapter over the D-Bus session bus --
//! `lib.rs`'s `reconcile_voice_runtime` calls [`pause_playing_players`] when
//! voice mode actually starts and [`resume_players`] with its return value
//! when voice mode stops (see `VoiceSettings::pause_media_while_active`).
//! This sends the same `org.mpris.MediaPlayer2.Player.Pause`/`Play` D-Bus
//! calls a desktop's physical media keys trigger -- no shelled-out command.

use zbus::zvariant::OwnedValue;
use zbus::Connection;

const MPRIS_BUS_NAME_PREFIX: &str = "org.mpris.MediaPlayer2.";
const PLAYER_OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const PLAYING_STATUS: &str = "Playing";

/// Pauses every MPRIS player currently reporting `PlaybackStatus: Playing`
/// and returns their bus names, so a later [`resume_players`] call resumes
/// only the players this call actually paused -- one already paused, or
/// with no active playback, is left untouched. Never propagates a failure:
/// no session bus (headless environments, some containers) or no MPRIS
/// player running is a normal, expected condition, not a bug -- it simply
/// pauses nothing.
pub async fn pause_playing_players() -> Vec<String> {
    let Ok(connection) = Connection::session().await else {
        return Vec::new();
    };
    let mut paused = Vec::new();
    for player in mpris_player_names(&connection).await {
        if playback_status(&connection, &player).await.as_deref() == Some(PLAYING_STATUS)
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
    let Ok(connection) = Connection::session().await else {
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
    reply
        .body()
        .deserialize::<Vec<String>>()
        .unwrap_or_default()
        .into_iter()
        .filter(|name| name.starts_with(MPRIS_BUS_NAME_PREFIX))
        .collect()
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
