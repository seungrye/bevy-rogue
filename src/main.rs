use bevy::prelude::*;
use crate::modules::ui::{DIALOG_PANEL_HEIGHT_PX, STATS_PANEL_WIDTH_PX};
mod modules;

fn main() {
    let tile_size = modules::map::TILE_SIZE;

    App::new()
        .add_systems(Startup, modules::core::systems::spawn_2d_camera)
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Bevy Rogue Map".into(),
                resolution: (
                    40_f32 * tile_size + STATS_PANEL_WIDTH_PX,
                    25_f32 * tile_size + DIALOG_PANEL_HEIGHT_PX,
                ).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(modules::map::MapPlugin)
        .add_plugins(modules::player::PlayerPlugin)
        .add_plugins(modules::trigger::TriggerPlugin)
        .add_plugins(modules::ui::GameUiPlugin)
        .run();
}
