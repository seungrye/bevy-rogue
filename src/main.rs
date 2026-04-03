//! Bevy Rogue-like 게임의 메인 애플리케이션 진입점입니다.
//!
//! 이 파일에서는 Bevy 앱을 설정하고, 필요한 플러그인들을 추가하며,
//! 게임 윈도우를 생성하고, 게임 루프를 시작합니다.

use bevy::prelude::*;

use crate::modules::ui::{DIALOG_PANEL_HEIGHT_PX, STATS_PANEL_WIDTH_PX};
mod modules;

fn main() {
    // 맵 크기와 타일 크기 설정을 모듈에서 가져옵니다.
    // let map_width = modules::map::MAP_WIDTH;
    // let map_height = modules::map::MAP_HEIGHT;
    let tile_size = modules::map::TILE_SIZE;

    App::new()
        // 공통 시스템 추가: 2D 카메라를 스폰합니다.
        .add_systems(Startup, modules::core::systems::spawn_2d_camera)
        // Bevy의 핵심 기능들을 담은 기본 플러그인 그룹을 추가합니다.
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Bevy Rogue Map".into(),
                // 창의 해상도를 맵의 전체 크기에 맞게 설정합니다.
                resolution: (
                    40_f32 * tile_size + STATS_PANEL_WIDTH_PX,
                    25_f32 * tile_size + DIALOG_PANEL_HEIGHT_PX,
                ).into(),
                ..default()
            }),
            ..default()
        }))
        // 게임 관련 플러그인들을 추가합니다.
        .add_plugins(modules::map::MapPlugin { algorithm: modules::map::MapAlgorithm::Bsp })
        .add_plugins(modules::player::PlayerPlugin)
        .add_plugins(modules::trigger::TriggerPlugin)
        .add_plugins(modules::ui::GameUiPlugin)
        .run();
}
