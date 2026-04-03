use crate::modules::{
    map::{draw_map, Map, MapResource, MapTile, tile_to_world_coords, world_to_tile_coords, MAP_HEIGHT, MAP_WIDTH, TILE_SIZE},
    ui::LogMessage,
};
use bevy::prelude::*;

/// 플레이어 타일 이동 애니메이션 속도
const LERP_SPEED: f32 = 7.5; 

/// 플레이어 엔티티를 식별하기 위한 태그 컴포넌트입니다.
#[derive(Component)]
pub struct Player;

/// 플레이어가 이동 중일 때 타겟 좌표를 지정하는 컴포넌트입니다.
#[derive(Component)]
struct MovingTo {
    /// 이동할 월드 좌표
    target: Vec3,
}

/// 게임 시작 시 플레이어 엔티티를 스폰하고 첫 번째 방 중앙으로 배치합니다.
fn spawn_player(mut commands: Commands, asset_server: Res<AssetServer>, map_res: Res<MapResource>) {
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");
    // 첫 번째 방의 중앙 좌표를 플레이어 스폰 위치로 설정
    let (player_x, player_y) = if let Some(first_room) = map_res.map().rooms.first() {
        first_room.center()
    } else {
        warn!("No rooms found for player spawn. Spawning at map center as a fallback.");
        (MAP_WIDTH / 2, MAP_HEIGHT / 2)
    };
    let coord = tile_to_world_coords(player_x, player_y);
    let glyph = "@";
    
    commands.spawn((
        Text2dBundle {
            text: Text::from_section(
                glyph,
                TextStyle {
                    font: font.clone(),
                    font_size: TILE_SIZE,
                    color: Color::YELLOW,
                },
            ),
            transform: Transform::from_xyz(coord.x, coord.y, 1.0),
            ..default()
        },
        Player,
    ));
}

/// 키보드 입력을 감지하고, 유효한 이동인 경우 MovingTo 컴포넌트를 삽입하여 이동을 예약합니다.
fn player_movement(
    mut commands: Commands,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    // 이동 중이지 않은 플레이어 엔티티만 조회함
    player_query: Query<(Entity, &Transform), (With<Player>, Without<MovingTo>)>,
    map_res: Res<MapResource>,
    mut log_writer: EventWriter<LogMessage>,
) {
    if let Ok((player_entity, player_transform)) = player_query.get_single() {
        let mut delta = IVec2::ZERO;
        if keyboard_input.pressed(KeyCode::ArrowLeft) || keyboard_input.pressed(KeyCode::KeyA) { delta.x -= 1; }
        if keyboard_input.pressed(KeyCode::ArrowRight) || keyboard_input.pressed(KeyCode::KeyD) { delta.x += 1; }
        if keyboard_input.pressed(KeyCode::ArrowUp) || keyboard_input.pressed(KeyCode::KeyW) { delta.y += 1; }
        if keyboard_input.pressed(KeyCode::ArrowDown) || keyboard_input.pressed(KeyCode::KeyS) { delta.y -= 1; }
        
        if delta == IVec2::ZERO { return; }
        
        let (current_x, current_y) = world_to_tile_coords(player_transform.translation);
        let target_x = (current_x as i32 + delta.x) as usize;
        let target_y = (current_y as i32 + delta.y) as usize;
        
        // 이동 가능 여부(바닥 타일) 확인
        if map_res.map().get_tile(target_x, target_y) == MapTile::Floor {
            log_writer.send(LogMessage(format!("플레이어가 ({}, {}) 로 이동합니다.", target_x, target_y)));
            let target_pos = tile_to_world_coords(target_x, target_y);
            commands.entity(player_entity).insert(MovingTo {
                target: Vec3::new(target_pos.x, target_pos.y, 1.0),
            });
        }
    }
}

/// MovingTo 타겟 좌표까지 부드럽게 위치를 보간(Lerp)하며 이동을 처리합니다.
fn smooth_player_lerp(mut commands: Commands, time: Res<Time>, mut query: Query<(Entity, &mut Transform, &MovingTo), With<Player>>) {
    for (entity, mut transform, moving) in query.iter_mut() {
        let current_pos = transform.translation;
        let target_pos = moving.target;
        let distance = current_pos.distance(target_pos);
        let move_amount = LERP_SPEED * TILE_SIZE * time.delta_seconds();
        
        // 목표 근처 도착 시 처리
        if distance < move_amount {
            transform.translation = target_pos;
            commands.entity(entity).remove::<MovingTo>();
        } else {
            // 위치 업데이트
            let direction = (target_pos - current_pos).normalize();
            transform.translation += direction * move_amount;
        }
    }
}

/// 플레이어 관리 기능을 제공하는 플러그인입니다.
pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    /// 플레이어 스폰, 이동, 시야 갱신 및 카메라 추적 시스템을 구성합니다.
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_player.after(draw_map))
           .add_systems(Update, (
               player_movement,
               smooth_player_lerp.after(player_movement),
               update_fov.after(smooth_player_lerp),
               camera_follow_player.after(update_fov),
           ));
    }
}

/// 카메라 위치가 플레이어를 중앙에 유지하도록 지속적으로 업데이트합니다.
fn camera_follow_player(player_query: Query<&Transform, With<Player>>, mut camera_query: Query<&mut Transform, (With<Camera>, Without<Player>)>) {
    if let Ok(player_transform) = player_query.get_single() {
        if let Ok(mut camera_transform) = camera_query.get_single_mut() {
            camera_transform.translation.x = player_transform.translation.x;
            camera_transform.translation.y = player_transform.translation.y;
        }
    }
}

/// 두 지점 사이에 시야를 가로막는 벽이 있는지 보간법(Bresenham's)으로 확인합니다.
///
/// # Returns
/// 시야가 확보되면 true, 벽에 가려지면 false
fn is_line_of_sight_clear(map: &Map, x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
    let dx = (x1 - x0).abs(); let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;
    let mut x = x0; let mut y = y0;
    loop {
        if x < 0 || x >= map.width as i32 || y < 0 || y >= map.height as i32 { return false; }
        if x == x1 && y == y1 { return true; }
        // 이동 중인 타일 자체를 벽으로 인식하지 않도록 검사
        if (x != x0 || y != y0) && map.tiles[map.index(x as usize, y as usize)] == MapTile::Wall { return false; }
        let e2 = 2 * err;
        if e2 > -dy { err -= dy; x += sx; }
        if e2 < dx { err += dx; y += sy; }
    }
}

/// 플레이어 위치 변화 시 시야각(FOV) 정보를 다시 계산하고 revealed_tiles를 갱신합니다.
fn update_fov(player_query: Query<&Transform, With<Player>>, mut map_res: ResMut<MapResource>, mut last_pos: Local<Option<IVec2>>) {
    if let Ok(player_transform) = player_query.get_single() {
        let (p_x, p_y) = world_to_tile_coords(player_transform.translation);
        let current_pos = IVec2::new(p_x as i32, p_y as i32);
        
        // 타일 기반 위치 변화 감지 (최적화)
        if Some(current_pos) == *last_pos { return; }
        *last_pos = Some(current_pos);
        
        let start = std::time::Instant::now();
        let map = map_res.map_mut();
        // 가시성 리셋
        map.visible_tiles.iter_mut().for_each(|v| *v = false);
        
        // 시야 반경 내 타일 검사
        let radius = 8;
        for y in (current_pos.y - radius)..=(current_pos.y + radius) {
            for x in (current_pos.x - radius)..=(current_pos.x + radius) {
                if x < 0 || x >= map.width as i32 || y < 0 || y >= map.height as i32 { continue; }
                let dx = x - current_pos.x; let dy = y - current_pos.y;
                if dx * dx + dy * dy > radius * radius { continue; }
                
                if is_line_of_sight_clear(map, current_pos.x, current_pos.y, x, y) {
                    let idx = map.index(x as usize, y as usize);
                    map.visible_tiles[idx] = true;
                    map.revealed_tiles[idx] = true;
                }
            }
        }
        
        let elapsed = start.elapsed();
        if elapsed.as_micros() > 0 { info!("FOV update took: {:?}", elapsed); }
    }
}
