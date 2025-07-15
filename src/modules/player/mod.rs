use crate::modules::{
    map::{
        bsp::{draw_map, MapResource, MapTile},
        tile_to_world_coords, world_to_tile_coords, MAP_HEIGHT, MAP_WIDTH, TILE_SIZE,
    },
    ui::LogMessage,
};
use bevy::prelude::*;

const LERP_SPEED: f32 = 7.5; // 타일 이동 속도

// 플레이어 개체를 식별하기 위한 태그 컴포넌트입니다.
#[derive(Component)]
pub struct Player;

/// 플레이어가 특정 지점으로 이동 중임을 나타내는 컴포넌트입니다.
#[derive(Component)]
struct MovingTo {
    target: Vec3,
}

/// 게임 시작 시 플레이어 개체를 생성하고 월드에 추가하는 시스템입니다.
fn spawn_player(mut commands: Commands, asset_server: Res<AssetServer>, map_res: Res<MapResource>) {
    // 맵을 그릴 폰트를 로드합니다. `assets/fonts` 폴더에 폰트 파일이 있어야 합니다.
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");

    // 플레이어의 시작 위치를 첫 번째 방의 중앙으로 설정합니다.
    // 만약 생성된 방이 없다면, 경고를 출력하고 맵의 중앙에 배치합니다.
    let (player_x, player_y) = if let Some(first_room) = map_res.map().rooms.first() {
        first_room.center()
    } else {
        warn!("No rooms found for player spawn. Spawning at map center as a fallback.");
        (MAP_WIDTH / 2, MAP_HEIGHT / 2)
    };

    let coord = tile_to_world_coords(player_x, player_y);

    // `commands.spawn`을 사용하여 새로운 개체(Entity)를 생성합니다.
    // 이 개체는 Text2dBundle과 Player 컴포넌트를 가집니다.
    let glyph = "@";
    commands.spawn((
        Text2dBundle {
            // 표시할 텍스트와 스타일을 설정합니다.
            text: Text::from_section(
                glyph,
                TextStyle {
                    font: font.clone(),   // 로드한 폰트 핸들을 복제하여 사용합니다.
                    font_size: TILE_SIZE, // 타일 크기를 폰트 크기로 설정합니다.
                    color: Color::YELLOW, // 텍스트 색상을 노란색으로 설정합니다.
                },
            ),
            // 텍스트의 위치를 설정합니다.
            transform: Transform::from_xyz(
                // 타일 좌표를 실제 월드 좌표로 변환하고, 화면 중앙 정렬을 위한 오프셋을 적용합니다.
                coord.x, coord.y,
                1.0, // z-좌표를 1.0으로 설정하여 맵 타일(z=0.0)보다 위에 보이도록 합니다.
            ),
            ..default() // 나머지 필드는 기본값을 사용합니다.
        },
        // 이 개체가 플레이어임을 나타내는 `Player` 태그 컴포넌트를 추가합니다.
        Player,
    ));
}

type PlayerQueryWithoutMovingTo<'w, 's> =
    Query<'w, 's, (Entity, &'static Transform), (With<Player>, Without<MovingTo>)>;

/// 키보드 입력을 받아 플레이어의 이동 목표를 설정하는 시스템입니다.
fn player_movement(
    mut commands: Commands,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    // `Without<MovingTo>` 필터: 플레이어가 이미 이동 중일 때는 새로운 입력을 받지 않습니다.
    player_query: PlayerQueryWithoutMovingTo,
    map_res: Res<MapResource>,
    mut log_writer: EventWriter<LogMessage>,
) {
    if let Ok((player_entity, player_transform)) = player_query.get_single() {
        // 1. 키 입력에 따라 이동 방향(delta)을 결정합니다.
        // `pressed`를 사용하여 키를 누르고 있는 동안 계속 반응합니다.
        let mut delta = IVec2::ZERO;
        if keyboard_input.pressed(KeyCode::ArrowLeft) || keyboard_input.pressed(KeyCode::KeyA) {
            delta.x -= 1;
        }
        if keyboard_input.pressed(KeyCode::ArrowRight) || keyboard_input.pressed(KeyCode::KeyD) {
            delta.x += 1;
        }
        if keyboard_input.pressed(KeyCode::ArrowUp) || keyboard_input.pressed(KeyCode::KeyW) {
            delta.y += 1;
        }
        if keyboard_input.pressed(KeyCode::ArrowDown) || keyboard_input.pressed(KeyCode::KeyS) {
            delta.y -= 1;
        }

        // 이동 입력이 없으면 아무것도 하지 않습니다.
        if delta == IVec2::ZERO {
            return;
        }

        // 2. 현재 위치와 목표 위치를 계산합니다.
        let (current_x, current_y) = world_to_tile_coords(player_transform.translation);
        let target_x = (current_x as i32 + delta.x) as usize;
        let target_y = (current_y as i32 + delta.y) as usize;

        // 3. 목표 지점이 이동 가능한 '바닥' 타일인지 확인합니다.
        if map_res.map().get_tile(target_x, target_y) == MapTile::Floor {
            //println!("player_movement called with delta: {:?}", delta);

            log_writer.send(LogMessage(format!(
                "플레이어가 ({}, {}) 로 이동합니다.",
                target_x, target_y
            )));

            // 4. `Transform`을 직접 바꾸는 대신, `MovingTo` 컴포넌트를 추가하여
            //    `smooth_player_lerp` 시스템이 처리하도록 합니다.
            let target_pos = tile_to_world_coords(target_x, target_y);
            commands.entity(player_entity).insert(MovingTo {
                target: Vec3::new(target_pos.x, target_pos.y, 1.0),
            });
        }
    }
}

/// `MovingTo` 컴포넌트를 가진 플레이어를 목표 지점까지 부드럽게 이동시키는(Lerp) 시스템입니다.
fn smooth_player_lerp(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut Transform, &MovingTo), With<Player>>,
) {
    for (entity, mut transform, moving) in query.iter_mut() {
        let current_pos = transform.translation;
        let target_pos = moving.target;

        // 현재 위치와 목표 위치 사이의 거리를 계산합니다.
        let distance = current_pos.distance(target_pos);
        //println!("smooth_player_lerp called with distance: {:?}", distance);

        // 이번 프레임에 이동할 거리를 계산합니다.
        // 타일 크기에 비례하여 속도를 조절하면 일관된 느낌을 줄 수 있습니다.
        let move_amount = LERP_SPEED * TILE_SIZE * time.delta_seconds();

        // 목표에 거의 도달했거나 지나쳤다면, 목표 위치로 즉시 이동시키고 `MovingTo` 컴포넌트를 제거합니다.
        if distance < move_amount {
            transform.translation = target_pos;
            commands.entity(entity).remove::<MovingTo>();
        } else {
            // 목표 방향으로 조금씩 이동시킵니다.
            let direction = (target_pos - current_pos).normalize();
            transform.translation += direction * move_amount;
        }
    }
}

// 플레이어 관련 로직(컴포넌트, 시스템)을 하나로 묶는 플러그인 구조체입니다.
// 이렇게 모듈화하면 `main.rs`에서 `.add_plugins(PlayerPlugin)` 한 줄로 간단하게 추가할 수 있습니다.
pub struct PlayerPlugin;

// `Plugin` 트레이트를 구현하여 Bevy 앱에 시스템과 리소스를 등록하는 방법을 정의합니다.
impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        // `app.add_systems`를 사용하여 시스템을 스케줄에 추가합니다.
        app
            // `Startup` 스케줄에 `spawn_player` 시스템을 추가합니다.
            // 이 시스템은 앱이 시작될 때 단 한 번만 실행됩니다.
            .add_systems(Startup, spawn_player.after(draw_map))
            // `Update` 스케줄에 `player_movement` 시스템을 추가합니다.
            // 이 시스템은 매 프레임마다 실행됩니다.
            .add_systems(
                Update,
                (
                    // player_movement는 이동 중이 아닐 때만 입력을 받습니다.
                    player_movement,
                    // smooth_player_lerp가 항상 player_movement 뒤에 실행되도록 보장합니다.
                    smooth_player_lerp.after(player_movement),
                    // fov와 카메라는 모든 이동이 끝난 후에 업데이트합니다.
                    update_fov.after(smooth_player_lerp),
                    camera_follow_player.after(update_fov),
                ),
            );
    }
}

/// `Update` 시스템: 카메라가 플레이어를 따라다니도록 위치를 업데이트합니다.
fn camera_follow_player(
    // With<Player> 필터로 플레이어 엔티티의 Transform을 가져옵니다.
    player_query: Query<&Transform, With<Player>>,
    // With<Camera> 필터로 카메라 엔티티의 Transform을 변경 가능하게 가져옵니다.
    // Without<Player> 필터는 플레이어와 카메라가 같은 엔티티일 경우를 방지합니다.
    mut camera_query: Query<&mut Transform, (With<Camera>, Without<Player>)>,
) {
    // 플레이어의 Transform을 성공적으로 가져왔을 경우
    if let Ok(player_transform) = player_query.get_single() {
        // 카메라의 Transform을 성공적으로 가져왔을 경우
        if let Ok(mut camera_transform) = camera_query.get_single_mut() {
            // 카메라의 x, y 위치를 플레이어의 위치와 일치시킵니다.
            // z-좌표는 변경하지 않아 카메라의 깊이를 유지합니다.
            camera_transform.translation.x = player_transform.translation.x;
            camera_transform.translation.y = player_transform.translation.y;
        }
    }
}

/// 두 점 사이의 선을 따라 타일을 체크하는 함수입니다.
/// 벽에 막히면 false를 반환하고, 목표 타일까지 도달하면 true를 반환합니다.
fn is_line_of_sight_clear(
    map: &crate::modules::map::bsp::Map,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
) -> bool {
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;
    let mut x = x0;
    let mut y = y0;

    loop {
        // 맵 범위를 벗어나면 false 반환
        if x < 0 || x >= map.width as i32 || y < 0 || y >= map.height as i32 {
            return false;
        }

        // 목표 타일에 도달했으면 true 반환
        if x == x1 && y == y1 {
            return true;
        }

        // 벽에 막히면 false 반환 (목표 타일 자체는 체크하지 않음)
        if x != x0 || y != y0 {
            let idx = map.index(x as usize, y as usize);
            if map.tiles[idx] == crate::modules::map::bsp::MapTile::Wall {
                return false;
            }
        }

        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x += sx;
        }
        if e2 < dx {
            err += dx;
            y += sy;
        }
    }
}

/// 플레이어의 위치를 기반으로 시야(Field of View)를 계산하고 맵 데이터를 업데이트합니다.
fn update_fov(player_query: Query<&Transform, With<Player>>, mut map_res: ResMut<MapResource>) {
    if let Ok(player_transform) = player_query.get_single() {
        let map = map_res.map_mut();
        // 현재 프레임의 가시성 정보를 초기화합니다.
        map.visible_tiles.iter_mut().for_each(|v| *v = false);

        let (player_x, player_y) = world_to_tile_coords(player_transform.translation);
        let player_x = player_x as i32;
        let player_y = player_y as i32;

        // 시야 반경 설정
        let radius = 8;

        // 플레이어 주변의 모든 타일을 체크합니다.
        for y in (player_y - radius)..=(player_y + radius) {
            for x in (player_x - radius)..=(player_x + radius) {
                // 맵 범위 체크
                if x < 0 || x >= map.width as i32 || y < 0 || y >= map.height as i32 {
                    continue;
                }

                // 거리 체크
                let dx = x - player_x;
                let dy = y - player_y;
                let distance_squared = dx * dx + dy * dy;
                if distance_squared > radius * radius {
                    continue;
                }

                // 선형 시야 체크 (벽에 막히지 않았는지 확인)
                if is_line_of_sight_clear(map, player_x, player_y, x, y) {
                    let idx = map.index(x as usize, y as usize);
                    map.visible_tiles[idx] = true;
                    map.revealed_tiles[idx] = true; // 한 번 본 타일은 계속해서 revealed 상태로 유지
                }
            }
        }
    }
}

