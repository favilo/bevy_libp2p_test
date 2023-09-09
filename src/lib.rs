#![allow(clippy::type_complexity)]

mod actions;
mod audio;
pub mod crypto;
mod loading;
mod menu;
pub mod network;
mod peer;
mod player;

use crate::actions::ActionsPlugin;
use crate::audio::InternalAudioPlugin;
use crate::loading::LoadingPlugin;
use crate::menu::MenuPlugin;
use crate::network::{GameAdminEvent, GameEvent, NetworkManager, NetworkPlugin};
use crate::peer::PeerPlugin;
use crate::player::PlayerPlugin;

use async_std::task;
#[cfg(debug_assertions)]
use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::{app::App, window::WindowCloseRequested};
use bevy_inspector_egui::quick::WorldInspectorPlugin;

// This example game uses States to separate logic
// See https://bevy-cheatbook.github.io/programming/states.html
// Or https://github.com/bevyengine/bevy/blob/main/examples/ecs/state.rs
#[derive(States, Default, Clone, Eq, PartialEq, Debug, Hash)]
enum GameState {
    // During the loading State the LoadingPlugin will load our assets
    #[default]
    Loading,
    // During this State the actual game logic is executed
    Playing,
    // Here the menu is drawn and waiting for player interaction
    Menu,

    // Here the hosting menu is drawn
    HostMenu,

    // Here the join menu is drawn
    JoinMenu,
}

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_state::<GameState>()
            .add_plugins((
                LoadingPlugin,
                MenuPlugin,
                ActionsPlugin,
                InternalAudioPlugin,
                PlayerPlugin,
                NetworkPlugin,
                PeerPlugin,
            ))
            .add_plugins(WorldInspectorPlugin::new())
            .add_systems(Update, send_quit_on_close);

        #[cfg(debug_assertions)]
        {
            app.add_plugins(
                (
                    // FrameTimeDiagnosticsPlugin,
                    LogDiagnosticsPlugin::default()
                ),
            );
        }
    }
}

fn send_quit_on_close(
    mut commands: Commands,
    mut manager: ResMut<NetworkManager<(), ()>>,
    mut events: EventReader<WindowCloseRequested>,
) {
    for event in events.iter() {
        log::info!("Window Closing");
        task::block_on(
            manager
                .as_mut()
                .send_to_network(GameEvent::Admin(GameAdminEvent::Quit)),
        )
        .expect("Send to open channel should succeed");
        commands.entity(event.window).despawn_recursive();
    }
}
