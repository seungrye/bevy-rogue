use crate::modules::{
    map::{
        draw_map, Map, MapResource, MapTile, OccupiedTiles,
        tile_to_world_coords, world_to_tile_coords,
        MAP_HEIGHT, MAP_WIDTH, TILE_SIZE,
        MapSystemSet, PlayerRespawnEvent, PlayerActedEvent, BumpTileEvent,
    },
    ui::LogMessage,
};
use bevy::prelude::*;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlayerSystemSet {
    Movement,
}

const LERP_SPEED: f32 = 7.5;
const INITIAL_HOLD_DELAY: f32 = 0.12;

#[derive(Resource, Default)]
pub struct MoveHoldState {
    pub dir: IVec2,
    pub elapsed: f32,
}

/// 방향키 홀드 상태를 관리하고 이동 가능 여부를 반환한다.
/// just_pressed=true이고 정지 상태에서 처음 누른 경우 즉시 true, 방향 전환 시 false.
pub fn tick_hold(state: &mut MoveHoldState, dir: IVec2, just_pressed: bool, dt: f32) -> bool {
    if dir == IVec2::ZERO {
        state.dir = IVec2::ZERO;
        state.elapsed = 0.0;
        return false;
    }
    if dir != state.dir {
        let from_stopped = state.dir == IVec2::ZERO;
        state.dir = dir;
        state.elapsed = 0.0;
        return from_stopped && just_pressed;
    }
    state.elapsed += dt;
    state.elapsed >= INITIAL_HOLD_DELAY
}

#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct MovingTo {
    pub target: Vec3,
}

fn spawn_player(mut commands: Commands, asset_server: Res<AssetServer>, map_res: Res<MapResource>) {
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");
    let (px, py) = if let Some(r) = map_res.map().rooms.first() {
        r.center()
    } else {
        warn!("방이 없어 맵 중앙에 스폰합니다.");
        (MAP_WIDTH / 2, MAP_HEIGHT / 2)
    };
    let coord = tile_to_world_coords(px, py);
    commands.spawn((
        Text2dBundle {
            text: Text::from_section("@", TextStyle {
                font,
                font_size: TILE_SIZE,
                color: Color::YELLOW,
            }),
            transform: Transform::from_xyz(coord.x, coord.y, 1.0),
            ..default()
        },
        Player,
    ));
}

fn player_movement(
    mut commands: Commands,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut hold_state: ResMut<MoveHoldState>,
    player_query: Query<(Entity, &Transform), (With<Player>, Without<MovingTo>)>,
    map_res: Res<MapResource>,
    occupied: Res<OccupiedTiles>,
    mut acted: EventWriter<PlayerActedEvent>,
    mut bump: EventWriter<BumpTileEvent>,
    mut log_writer: EventWriter<LogMessage>,
) {
    let Ok((entity, transform)) = player_query.get_single() else { return };

    let mut dir = IVec2::ZERO;
    if keyboard_input.pressed(KeyCode::ArrowLeft)  || keyboard_input.pressed(KeyCode::KeyA) { dir.x -= 1; }
    if keyboard_input.pressed(KeyCode::ArrowRight) || keyboard_input.pressed(KeyCode::KeyD) { dir.x += 1; }
    if keyboard_input.pressed(KeyCode::ArrowUp)    || keyboard_input.pressed(KeyCode::KeyW) { dir.y += 1; }
    if keyboard_input.pressed(KeyCode::ArrowDown)  || keyboard_input.pressed(KeyCode::KeyS) { dir.y -= 1; }

    let just_pressed = keyboard_input.just_pressed(KeyCode::ArrowLeft) || keyboard_input.just_pressed(KeyCode::KeyA)
        || keyboard_input.just_pressed(KeyCode::ArrowRight) || keyboard_input.just_pressed(KeyCode::KeyD)
        || keyboard_input.just_pressed(KeyCode::ArrowUp) || keyboard_input.just_pressed(KeyCode::KeyW)
        || keyboard_input.just_pressed(KeyCode::ArrowDown) || keyboard_input.just_pressed(KeyCode::KeyS);

    if !tick_hold(&mut hold_state, dir, just_pressed, time.delta_seconds()) { return; }
    let delta = hold_state.dir;
    if delta == IVec2::ZERO { return; }

    let (cx, cy) = world_to_tile_coords(transform.translation);
    let tx = (cx as i32 + delta.x) as usize;
    let ty = (cy as i32 + delta.y) as usize;

    if map_res.map().get_tile(tx, ty) != MapTile::Floor { return; }

    if occupied.0.contains(&(tx, ty)) {
        // 주민과 충돌: 대사 트리거 + 턴 소비
        bump.send(BumpTileEvent(tx, ty));
        acted.send(PlayerActedEvent);
    } else {
        log_writer.send(LogMessage(format!("({}, {}) 로 이동합니다.", tx, ty)));
        let wp = tile_to_world_coords(tx, ty);
        commands.entity(entity).insert(MovingTo { target: Vec3::new(wp.x, wp.y, 1.0) });
        acted.send(PlayerActedEvent);
    }
}

fn smooth_player_lerp(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut Transform, &MovingTo), With<Player>>,
) {
    for (entity, mut transform, moving) in query.iter_mut() {
        let dist = transform.translation.distance(moving.target);
        let step = LERP_SPEED * TILE_SIZE * time.delta_seconds();
        if dist < step {
            transform.translation = moving.target;
            commands.entity(entity).remove::<MovingTo>();
        } else {
            let dir = (moving.target - transform.translation).normalize();
            transform.translation += dir * step;
        }
    }
}

fn respawn_player_on_regen(
    mut commands: Commands,
    mut events: EventReader<PlayerRespawnEvent>,
    mut player_query: Query<(Entity, &mut Transform), With<Player>>,
) {
    for PlayerRespawnEvent(x, y) in events.read() {
        if let Ok((entity, mut transform)) = player_query.get_single_mut() {
            let wp = tile_to_world_coords(*x, *y);
            transform.translation = Vec3::new(wp.x, wp.y, 1.0);
            commands.entity(entity).remove::<MovingTo>();
        }
    }
}

fn camera_follow_player(
    player_query: Query<&Transform, With<Player>>,
    mut camera_query: Query<&mut Transform, (With<Camera>, Without<Player>)>,
) {
    let Ok(pt) = player_query.get_single() else { return };
    let Ok(mut ct) = camera_query.get_single_mut() else { return };
    ct.translation.x = pt.translation.x;
    ct.translation.y = pt.translation.y;
}

fn is_line_of_sight_clear(map: &Map, x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
    let (dx, dy) = ((x1 - x0).abs(), (y1 - y0).abs());
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let mut err = dx - dy;
    let (mut x, mut y) = (x0, y0);
    loop {
        if x < 0 || x >= map.width as i32 || y < 0 || y >= map.height as i32 { return false; }
        if x == x1 && y == y1 { return true; }
        if (x != x0 || y != y0) && map.tiles[map.index(x as usize, y as usize)] == MapTile::Wall {
            return false;
        }
        let e2 = 2 * err;
        if e2 > -dy { err -= dy; x += sx; }
        if e2 < dx  { err += dx; y += sy; }
    }
}

fn update_fov(
    player_query: Query<&Transform, With<Player>>,
    mut map_res: ResMut<MapResource>,
    mut last_pos: Local<Option<IVec2>>,
) {
    // 맵이 교체되면 강제 재계산
    if map_res.is_changed() {
        *last_pos = None;
    }

    let Ok(transform) = player_query.get_single() else { return };
    let (px, py) = world_to_tile_coords(transform.translation);
    let cur = IVec2::new(px as i32, py as i32);
    if Some(cur) == *last_pos { return; }
    *last_pos = Some(cur);

    let start = std::time::Instant::now();
    let map = map_res.map_mut();
    map.visible_tiles.iter_mut().for_each(|v| *v = false);

    let radius = 8i32;
    for y in (cur.y - radius)..=(cur.y + radius) {
        for x in (cur.x - radius)..=(cur.x + radius) {
            if x < 0 || x >= map.width as i32 || y < 0 || y >= map.height as i32 { continue; }
            let (dx, dy) = (x - cur.x, y - cur.y);
            if dx * dx + dy * dy > radius * radius { continue; }
            if is_line_of_sight_clear(map, cur.x, cur.y, x, y) {
                let idx = map.index(x as usize, y as usize);
                map.visible_tiles[idx] = true;
                map.revealed_tiles[idx] = true;
            }
        }
    }
    let elapsed = start.elapsed();
    if elapsed.as_micros() > 0 { info!("FOV: {:?}", elapsed); }
}

pub struct PlayerPlugin;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_hold_immediate_on_just_pressed() {
        let mut state = MoveHoldState::default();
        assert!(tick_hold(&mut state, IVec2::new(-1, 0), true, 0.016));
    }

    #[test]
    fn tick_hold_no_move_before_delay() {
        let mut state = MoveHoldState::default();
        let dir = IVec2::new(-1, 0);
        tick_hold(&mut state, dir, true, 0.0);
        assert!(!tick_hold(&mut state, dir, false, 0.016));
    }

    #[test]
    fn tick_hold_triggers_after_delay() {
        let mut state = MoveHoldState::default();
        let dir = IVec2::new(-1, 0);
        tick_hold(&mut state, dir, true, 0.0);
        let triggered = (0..20).any(|_| tick_hold(&mut state, dir, false, 0.016));
        assert!(triggered, "INITIAL_HOLD_DELAY 이후 연속 이동이 시작돼야 한다");
    }

    #[test]
    fn tick_hold_resets_on_key_release() {
        let mut state = MoveHoldState::default();
        let dir = IVec2::new(-1, 0);
        tick_hold(&mut state, dir, true, 0.0);
        tick_hold(&mut state, IVec2::ZERO, false, 0.016);
        assert_eq!(state.dir, IVec2::ZERO);
        assert_eq!(state.elapsed, 0.0);
    }

    #[test]
    fn tick_hold_resets_on_direction_change() {
        let mut state = MoveHoldState::default();
        let dir = IVec2::new(-1, 0);
        tick_hold(&mut state, dir, true, 0.0);
        for _ in 0..10 { tick_hold(&mut state, dir, false, 0.016); }
        let result = tick_hold(&mut state, IVec2::new(1, 0), false, 0.016);
        assert!(!result, "방향 전환 직후에는 이동하지 않아야 한다");
        assert_eq!(state.elapsed, 0.0, "방향 전환 시 타이머가 리셋돼야 한다");
    }
}

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MoveHoldState>()
            .add_systems(Startup, spawn_player.after(draw_map))
            .add_systems(Update, (
                player_movement.in_set(PlayerSystemSet::Movement),
                smooth_player_lerp.after(PlayerSystemSet::Movement),
                update_fov.after(smooth_player_lerp),
                camera_follow_player.after(update_fov),
                respawn_player_on_regen.after(MapSystemSet::ExecuteRegen),
            ));
    }
}
