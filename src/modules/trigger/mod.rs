//! 게임 내 특정 위치에서 발생하는 이벤트 트리거(Trigger) 관련 로직을 관리하는 모듈입니다.
//!
//! ## 주요 기능
//! - `TriggerPlugin`: 트리거 관련 시스템과 이벤트를 Bevy 앱에 등록합니다.
//! - `spawn_trigger`: 게임 시작 시 맵에 상자나 출구 같은 트리거를 배치합니다.
//! - `check_triggers`: 플레이어의 이동을 감지하여 트리거 발동 여부를 확인합니다.
//! - `handle_trigger_events`: `TriggerEvent`가 발생했을 때 상자 UI를 띄우는 등 실제 효과를 처리합니다.
//! - `close_ui_on_input`: 팝업된 UI를 사용자 입력으로 닫는 범용 시스템을 제공합니다.
use bevy::prelude::*;
use rand::{thread_rng, Rng};
// 필요한 내부 모듈들을 가져옵니다.
use crate::modules::map::bsp::{draw_map, MapResource};
use crate::modules::player::Player;
use crate::modules::map::{tile_to_world_coords, world_to_tile_coords, TILE_SIZE};
use crate::modules::ui::LogMessage;
use chest::spawn_chest_on_event;
mod chest;

/// 트리거 시스템을 Bevy 앱에 통합하는 플러그인입니다.
pub struct TriggerPlugin;

/// 트리거의 시각적 표현('?', '>')을 위한 Z-index 값입니다. 맵 타일 위에 그려지도록 설정합니다.
const Z_TRIGGER: f32 = 1.0;

/// 상자(Chest)와 같은 팝업 UI의 렌더링 순서를 위한 Z-index 값입니다. 최상단에 표시되도록 높은 값을 가집니다.
pub const Z_CHEST_UI: i32 = 100;

impl Plugin for TriggerPlugin {
    fn build(&self, app: &mut App) {
        app
            // TriggerEvent를 앱에 등록합니다.
            .add_event::<TriggerEvent>()
            // Startup 단계에서 맵이 그려진 후에 트리거를 스폰합니다.
            .add_systems(Startup, spawn_trigger
                .after(draw_map)
            )
            // 매 프레임마다 트리거 발동 여부를 확인하고, 이벤트를 처리하는 시스템을 등록합니다.
            .add_systems(Update, (
                close_ui_on_input,
                check_triggers,
                handle_trigger_events,
            ).chain()); // .chain()을 사용하여 check_triggers가 handle_trigger_events보다 먼저 실행되도록 보장합니다.
    }
}

/// 트리거가 발동했을 때 수행될 효과를 정의하는 열거형입니다.
///
/// 이 `enum`은 `Component`로도 사용됩니다. UI를 생성하는 효과(예: `OpenChest`)의 경우,
/// 생성된 UI 엔티티에 이 `enum` 값을 컴포넌트로 추가합니다. 이를 통해 `close_ui_on_input` 같은
/// 범용 시스템이 어떤 종류의 UI를 다루고 있는지 식별하고, 종류에 맞는 로직을 수행할 수 있습니다.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub enum TriggerEffect {
    /// 화면에 메시지를 표시합니다.
    ShowMessage(String),
    /// 상자를 엽니다.
    OpenChest,
    // 향후 다른 효과들을 추가할 수 있습니다. (예: NextLevel, SpawnEnemy)
}

/// 특정 위치에 도달했을 때 발동하는 트리거를 나타내는 컴포넌트입니다.
#[derive(Component)]
pub struct Trigger {
    /// 트리거의 x 좌표 (그리드 기준)
    pub x: usize,
    /// 트리거의 y 좌표 (그리드 기준)
    pub y: usize,
    /// 발동 시 수행될 효과
    pub effect: TriggerEffect,
}

/// `check_triggers` 시스템에서 트리거가 발동되었음을 알리기 위해 보내는 이벤트입니다.
#[derive(Event)]
pub struct TriggerEvent(pub TriggerEffect);

/// `Startup` 시점에 맵에 트리거들을 생성하는 시스템입니다.
/// 현재는 상자 트리거와 출구 트리거, 두 종류를 생성합니다.
fn spawn_trigger(
    mut commands: Commands,
    map_res: Res<MapResource>,
    asset_server: Res<AssetServer>,
) {
    let map = map_res.map();
    if map.rooms.len() < 2 {
        // 트리거를 생성하기에 방이 충분하지 않은 경우, 함수를 조기 종료합니다.
        warn!("Not enough rooms to spawn triggers. Need at least 2.");
        return;
    }

    let mut rng = thread_rng();
    // 마지막 방을 제외한 방 중 하나를 랜덤하게 선택하여 상자를 배치합니다.
    let chest_room_idx = rng.gen_range(0..map.rooms.len() - 1);
    if let Some(chest_room) = map.rooms.get(chest_room_idx) {
        let (x, y) = chest_room.center();
        let font = asset_server.load("fonts/FiraMono-Medium.ttf");
        let coord = tile_to_world_coords(x, y);

        // 상자 열기 트리거 엔티티를 생성합니다.
        // 이 엔티티는 논리적인 Trigger 컴포넌트와 시각적인 Text2dBundle을 모두 가집니다.
        commands.spawn((
            Trigger {
                x,
                y,
                // effect: TriggerEffect::ShowMessage("You found a mysterious spot...".to_string()),
                effect: TriggerEffect::OpenChest,
            },
            Text2dBundle {
                text: Text::from_section(
                    "?", // 상자 트리거의 시각적 표현
                    TextStyle {
                        font,
                        font_size: TILE_SIZE,
                        color: Color::YELLOW, // 눈에 띄는 색상
                    },
                ),
                transform: Transform::from_xyz(
                    coord.x,
                    coord.y,
                    Z_TRIGGER, // 정의된 z-index 상수를 사용합니다.
                ),
                ..default()
            }
        ));

        info!("Trigger spawned at grid coordinates: ({}, {})", x, y);
    }

    // 마지막으로 생성된 방의 중앙에 출구 트리거를 배치합니다.
    if let Some(last_room) = map.rooms.last() {
        let (x, y) = last_room.center();
        let font = asset_server.load("fonts/FiraMono-Medium.ttf");
        let coord = tile_to_world_coords(x, y);

        // 출구 트리거 엔티티를 생성합니다.
        // 이 엔티티는 논리적인 Trigger 컴포넌트와 시각적인 Text2dBundle을 모두 가집니다.
        commands.spawn((
            Trigger {
                x,
                y,
                effect: TriggerEffect::ShowMessage(
                    "You've reached the exit! Well done!".to_string(),
                ),
            },
            Text2dBundle {
                text: Text::from_section(
                    ">", // 출구 트리거의 시각적 표현
                    TextStyle {
                        font,
                        font_size: TILE_SIZE,
                        color: Color::RED, // 눈에 띄는 색상
                    },
                ),
                transform: Transform::from_xyz(
                    coord.x,
                    coord.y,
                    Z_TRIGGER, // 정의된 z-index 상수를 사용합니다.
                ),
                ..default()
            }
        ));

        info!("Trigger spawned at grid coordinates: ({}, {})", x, y);
    }
}

/// 매 프레임 플레이어의 위치를 감지하여 트리거 발동 여부를 검사하는 시스템입니다.
fn check_triggers(
    mut commands: Commands,
    player_query: Query<&Transform, With<Player>>,
    trigger_query: Query<(Entity, &Trigger)>,
    mut trigger_events: EventWriter<TriggerEvent>,
) {
    if let Ok(player_transform) = player_query.get_single() {
        // 플레이어의 월드 좌표를 그리드 좌표로 변환합니다.
        let (player_x, player_y) = world_to_tile_coords(player_transform.translation);

        for (trigger_entity, trigger) in trigger_query.iter() {
            // 플레이어의 그리드 좌표와 트리거의 그리드 좌표가 일치하는지 확인합니다.
            if player_x == trigger.x && player_y == trigger.y {
                // 플레이어가 트리거 위치에 도달했습니다.
                info!("Player entered trigger at ({}, {})", trigger.x, trigger.y);
                trigger_events.send(TriggerEvent(trigger.effect.clone()));
                // 트리거는 일회성이므로, 발동 후에는 해당 엔티티를 즉시 제거하여
                // 중복 발동을 방지하고 시스템 자원을 절약합니다.
                commands.entity(trigger_entity).despawn();
            }
        }
    }
}

/// 사용자 입력이 있을 때 특정 UI를 화면에서 제거하는 범용 시스템입니다.
///
/// `TriggerEffect` 컴포넌트가 부착된 UI 엔티티(예: 상자 팝업)가 있을 때,
/// 플레이어가 이동 키나 `Escape` 키를 누르면 해당 UI를 화면에서 제거합니다.
fn close_ui_on_input(
    mut commands: Commands,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    // `TriggerEffect` 컴포넌트를 가진 UI 엔티티를 쿼리합니다.
    // 이 컴포넌트는 UI를 생성한 원본 이벤트를 식별하는 데 사용됩니다.
    q_ui_to_close: Query<(Entity, &TriggerEffect)>,
) {
    // `get_single`을 사용하여 UI가 하나만 존재하는지 확인하고, 엔티티를 가져옵니다.
    // 현재 설계에서는 한 번에 하나의 팝업만 뜬다고 가정합니다.
    if let Ok((ui_entity, effect)) = q_ui_to_close.get_single() {
        // 이동 키 또는 ESC 키가 눌렸는지 확인합니다.
        let close_input_pressed = [
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::KeyW,
            KeyCode::KeyA,
            KeyCode::KeyS,
            KeyCode::KeyD,
            KeyCode::Escape,
        ]
        .iter()
        .any(|&key| keyboard_input.just_pressed(key));

        if close_input_pressed {
            info!("Input detected, despawning UI for effect: {:?}", effect);

            // UI 종류(effect)에 따라 닫을 때 다른 추가 동작을 수행할 수 있습니다.
            match effect {
                TriggerEffect::OpenChest => {
                    // 향후 상자를 닫을 때만 실행할 로직을 여기에 추가할 수 있습니다. (예: 닫는 소리 재생)
                }
                TriggerEffect::ShowMessage(_) => {
                    // 향후 메시지 창을 닫을 때만 실행할 로직을 여기에 추가할 수 있습니다.
                }
            }

            // `despawn_recursive`를 호출하여 UI와 그 자식들을 모두 제거합니다.
            commands.entity(ui_entity).despawn_recursive();
        }
    }
}

/// `TriggerEvent`를 수신하여 그에 맞는 효과를 실행하는 시스템입니다.
fn handle_trigger_events(
    mut commands: Commands,
    mut events: EventReader<TriggerEvent>,
    asset_server: Res<AssetServer>,
    // 다른 UI가 이미 화면에 떠 있는지 확인하기 위한 쿼리입니다.
    q_trigger_effect: Query<Entity, With<TriggerEffect>>,
    mut log_writer: EventWriter<LogMessage>,
) {
    // 발생한 모든 TriggerEvent를 순회합니다.
    for TriggerEvent(effect) in events.read() {
        match effect {
            TriggerEffect::ShowMessage(message) => {
                info!("[EVENT] {}", message);
                log_writer.send(LogMessage(message.clone()));
            }
            TriggerEffect::OpenChest => {
                spawn_chest_on_event(&mut commands, &asset_server, &q_trigger_effect)
            }
            // TODO: 향후 다른 TriggerEffect variant에 대한 처리 로직을 여기에 추가할 수 있습니다.
            // 예: TriggerEffect::NextLevel => { ... }
        }
    }
}
