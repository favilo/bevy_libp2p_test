use crate::loading::FontAssets;
use crate::network::{GameAdminEvent, GameEvent, NetworkManager};
use crate::GameState;
use async_std::task;
use bevy::prelude::*;

pub struct MenuPlugin;

/// This plugin is responsible for the game menu (containing only one button...)
/// The menu is only drawn during the State `GameState::Menu` and is removed when that state is exited
impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ButtonColors>()
            .add_systems(OnEnter(GameState::Menu), setup_menu)
            .add_systems(OnEnter(GameState::HostMenu), setup_host_menu)
            .add_systems(
                Update,
                hover_button.run_if(
                    in_state(GameState::Menu)
                        .or_else(in_state(GameState::HostMenu))
                        .or_else(in_state(GameState::JoinMenu)),
                ),
            )
            .add_systems(Update, click_host_button.run_if(in_state(GameState::Menu)))
            .add_systems(OnExit(GameState::Menu), cleanup_menu)
            .add_systems(OnExit(GameState::HostMenu), cleanup_menu);
    }
}

#[derive(Resource)]
struct ButtonColors {
    normal: Color,
    hovered: Color,
}

impl Default for ButtonColors {
    fn default() -> Self {
        ButtonColors {
            normal: Color::rgb(0.15, 0.15, 0.15),
            hovered: Color::rgb(0.25, 0.25, 0.25),
        }
    }
}

#[derive(Component)]
struct Menu;

#[derive(Component)]
struct HostMenu;

#[derive(Component)]
struct HostButton;

#[derive(Component)]
struct JoinButton;

fn setup_menu(
    mut commands: Commands,
    font_assets: Res<FontAssets>,
    button_colors: Res<ButtonColors>,
) {
    commands.spawn(Camera2dBundle::default());
    commands
        .spawn((
            ButtonBundle {
                style: Style {
                    width: Val::Px(120.0),
                    height: Val::Px(50.0),
                    margin: UiRect::all(Val::Auto),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..Default::default()
                },
                background_color: button_colors.normal.into(),
                ..Default::default()
            },
            HostButton,
            Menu,
        ))
        .with_children(|parent| {
            parent.spawn(TextBundle::from_section(
                "Host",
                TextStyle {
                    font: font_assets.fira_sans.clone(),
                    font_size: 40.0,
                    color: Color::rgb(0.9, 0.9, 0.9),
                },
            ));
        });
    commands
        .spawn((
            ButtonBundle {
                style: Style {
                    width: Val::Px(120.0),
                    height: Val::Px(50.0),
                    margin: UiRect::all(Val::Auto),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..Default::default()
                },
                background_color: button_colors.normal.into(),
                ..Default::default()
            },
            JoinButton,
            Menu,
        ))
        .with_children(|parent| {
            parent.spawn(TextBundle::from_section(
                "Join",
                TextStyle {
                    font: font_assets.fira_sans.clone(),
                    font_size: 40.0,
                    color: Color::rgb(0.9, 0.9, 0.9),
                },
            ));
        });
}

fn click_host_button(
    mut state: ResMut<NextState<GameState>>,
    mut interaction_query: Query<(&Interaction), (Changed<Interaction>, With<HostButton>)>,
) {
    for interaction in &mut interaction_query {
        match *interaction {
            Interaction::Pressed => {
                state.set(GameState::HostMenu);
            }
            _ => {}
        }
    }
}

fn hover_button(
    button_colors: Res<ButtonColors>,
    mut interaction_query: Query<
        (&Interaction, &mut BackgroundColor),
        (Changed<Interaction>, With<Button>),
    >,
) {
    for (interaction, mut color) in &mut interaction_query {
        match *interaction {
            Interaction::Hovered => {
                *color = button_colors.hovered.into();
            }
            Interaction::None => {
                *color = button_colors.normal.into();
            }
            _ => {}
        }
    }
}

fn cleanup_menu(mut commands: Commands, buttons: Query<Entity, (With<Button>, With<Menu>)>) {
    for button in &buttons {
        commands.entity(button).despawn_recursive();
    }
}

fn setup_host_menu(
    mut commands: Commands,
    font_assets: Res<FontAssets>,
    button_colors: Res<ButtonColors>,
    mut manager: ResMut<NetworkManager<(), ()>>,
) {
    // TODO: Add textbox for setting options eventually.
    use rand::Rng;
    let code_1: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(3)
        .map(char::from)
        .map(char::to_uppercase)
        .flatten()
        .collect();
    let code_2: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(3)
        .map(char::from)
        .map(char::to_uppercase)
        .flatten()
        .collect();
    let room_code = format!("{}-{}", code_1, code_2);
    let room_code_text = format!("Room Code: {}", room_code);
    task::block_on(
        manager
            .as_mut()
            .send_to_network(GameEvent::Admin(GameAdminEvent::Host { room_code })),
    )
    .expect("send worked");
    commands
        .spawn((
            NodeBundle {
                style: Style {
                    width: Val::Px(150.0),
                    height: Val::Px(50.0),
                    margin: UiRect::all(Val::Auto),
                    justify_content: JustifyContent::FlexStart,
                    align_items: AlignItems::Center,
                    ..Default::default()
                },
                ..Default::default()
            },
            HostMenu,
        ))
        .with_children(|parent| {
            parent.spawn(TextBundle::from_section(
                &room_code_text,
                TextStyle {
                    font: font_assets.fira_sans.clone(),
                    font_size: 40.0,
                    color: Color::rgb(0.9, 0.9, 0.9),
                },
            ));
        });
    // For now we'll just show the room code, and start hosting.

    commands
        .spawn((
            ButtonBundle {
                style: Style {
                    width: Val::Px(120.0),
                    height: Val::Px(50.0),
                    margin: UiRect::all(Val::Auto),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..Default::default()
                },
                background_color: button_colors.normal.into(),
                ..Default::default()
            },
            HostButton,
        ))
        .with_children(|parent| {
            parent.spawn(TextBundle::from_section(
                "Host",
                TextStyle {
                    font: font_assets.fira_sans.clone(),
                    font_size: 40.0,
                    color: Color::rgb(0.9, 0.9, 0.9),
                },
            ));
        });
}
