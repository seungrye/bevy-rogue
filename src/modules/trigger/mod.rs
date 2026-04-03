use bevy::prelude::*;
use rand::{thread_rng, Rng};
use crate::modules::map::{draw_map, MapResource, tile_to_world_coords, world_to_tile_coords, TILE_SIZE};
use crate::modules::player::Player;
use crate::modules::ui::LogMessage;

mod chest;
use chest::spawn_chest_on_event;

/// 특정 지점 도달 시 발생하는 이벤트 트리거 시스템을 관리하는 플러그인입니다.
pub struct TriggerPlugin;

/// 트리거 시각적 표시의 Z-좌표 (타일보다 위에 위치)
const Z_TRIGGER: f32 = 1.0;
/// 상자 UI 등 팝업의 Z-좌표 (최상단)
pub const Z_CHEST_UI: i32 = 100;

impl Plugin for TriggerPlugin {
    /// 플러그인을 초기화하고 필요한 시스템과 이벤트를 등록합니다.
    fn build(&self, app: &mut App) {
        app.add_event::<TriggerEvent>()
            // 맵 생성 후에 트리거를 배치함
            .add_systems(Startup, spawn_trigger.after(draw_map))
            // 입력 확인, 트리거 체크, 이벤트 처리를 체인으로 연결하여 순서대로 실행
            .add_systems(Update, (close_ui_on_input, check_triggers, handle_trigger_events).chain());
    }
}

/// 트리거가 발동했을 때 나타나는 효과의 종류입니다.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub enum TriggerEffect { 
    /// 로그 메시지 표시
    ShowMessage(String), 
    /// 상자 열기 UI 표시
    OpenChest 
}

/// 맵 위의 트리거 객체를 나타내는 컴포넌트입니다.
#[derive(Component)]
pub struct Trigger { 
    /// 그리드 x 좌표
    pub x: usize, 
    /// 그리드 y 좌표
    pub y: usize, 
    /// 발동 시 효과
    pub effect: TriggerEffect 
}

/// 트리거가 발동되었음을 알리는 이벤트 구조체입니다.
#[derive(Event)]
pub struct TriggerEvent(pub TriggerEffect);

/// 게임 시작 시 맵의 각 방 중앙에 무작위로 트리거(상자, 출구 등)를 스폰합니다.
fn spawn_trigger(mut commands: Commands, map_res: Res<MapResource>, asset_server: Res<AssetServer>) {
    let map = map_res.map();
    if map.rooms.len() < 2 { return; }
    let mut rng = thread_rng();
    
    // 첫 번째 방들에 무작위로 상자 배치
    let chest_room_idx = rng.gen_range(0..map.rooms.len() - 1);
    if let Some(chest_room) = map.rooms.get(chest_room_idx) {
        let (x, y) = chest_room.center();
        let font = asset_server.load("fonts/FiraMono-Medium.ttf");
        let coord = tile_to_world_coords(x, y);
        
        commands.spawn((
            Trigger { x, y, effect: TriggerEffect::OpenChest }, 
            Text2dBundle {
                text: Text::from_section("?", TextStyle { font, font_size: TILE_SIZE, color: Color::YELLOW }),
                transform: Transform::from_xyz(coord.x, coord.y, Z_TRIGGER), 
                ..default()
            }
        ));
    }
    
    // 마지막 방에 출구 배치
    if let Some(last_room) = map.rooms.last() {
        let (x, y) = last_room.center();
        let font = asset_server.load("fonts/FiraMono-Medium.ttf");
        let coord = tile_to_world_coords(x, y);
        
        commands.spawn((
            Trigger { x, y, effect: TriggerEffect::ShowMessage("You've reached the exit! Well done!".to_string()) }, 
            Text2dBundle {
                text: Text::from_section(">", TextStyle { font, font_size: TILE_SIZE, color: Color::RED }),
                transform: Transform::from_xyz(coord.x, coord.y, Z_TRIGGER), 
                ..default()
            }
        ));
    }
}

/// 플레이어의 현재 위치와 트리거의 위치를 대조하여 이벤트를 발생시킵니다.
fn check_triggers(
    mut commands: Commands, 
    player_query: Query<&Transform, With<Player>>, 
    trigger_query: Query<(Entity, &Trigger)>, 
    mut trigger_events: EventWriter<TriggerEvent>
) {
    if let Ok(player_transform) = player_query.get_single() {
        let (player_x, player_y) = world_to_tile_coords(player_transform.translation);
        for (trigger_entity, trigger) in trigger_query.iter() {
            if player_x == trigger.x && player_y == trigger.y {
                // 이벤트 발생 및 트리거 엔티티 제거
                trigger_events.send(TriggerEvent(trigger.effect.clone()));
                commands.entity(trigger_entity).despawn();
            }
        }
    }
}

/// 키보드 입력(이동 키 또는 ESC) 감지 시 활성화된 UI 팝업을 닫습니다.
fn close_ui_on_input(
    mut commands: Commands, 
    keyboard_input: Res<ButtonInput<KeyCode>>, 
    q_ui_to_close: Query<(Entity, &TriggerEffect)>
) {
    if let Ok((ui_entity, _effect)) = q_ui_to_close.get_single() {
        let close_keys = [
            KeyCode::ArrowUp, KeyCode::ArrowDown, KeyCode::ArrowLeft, KeyCode::ArrowRight, 
            KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD, KeyCode::Escape
        ];
        
        let close_input_pressed = close_keys.iter().any(|&key| keyboard_input.just_pressed(key));
        if close_input_pressed { 
            commands.entity(ui_entity).despawn_recursive(); 
        }
    }
}

/// 발생한 트리거 이벤트를 수신하여 실제 효과(로그 출력, 상자 UI 등)를 실행합니다.
fn handle_trigger_events(
    mut commands: Commands, 
    mut events: EventReader<TriggerEvent>, 
    asset_server: Res<AssetServer>, 
    q_trigger_effect: Query<Entity, With<TriggerEffect>>, 
    mut log_writer: EventWriter<LogMessage>
) {
    for TriggerEvent(effect) in events.read() {
        match effect {
            TriggerEffect::ShowMessage(message) => { 
                log_writer.send(LogMessage(message.clone())); 
            }
            TriggerEffect::OpenChest => { 
                // 다른 UI가 이미 떠 있는지 확인하여 전달
                spawn_chest_on_event(&mut commands, &*asset_server, !q_trigger_effect.is_empty()) 
            }
        }
    }
}
