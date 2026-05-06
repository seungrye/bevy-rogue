use crate::modules::{
    map::{
        draw_map, MapResource, TileKind, OccupiedTiles, MonsterTiles,
        tile_to_world_coords, world_to_tile_coords, is_line_of_sight_clear,
        MAP_HEIGHT, MAP_WIDTH, TILE_SIZE,
        MapSystemSet, PlayerRespawnEvent, PlayerActedEvent, BumpTileEvent, AttackMonsterEvent,
    },
    combat::{CombatStats, Defeated, Speed},
    item::EquipmentPanelOpen,
    ui::{LogMessage, shop::ShopPanelOpen},
};
use bevy::prelude::*;
use bevy::sprite::Anchor;
use std::collections::VecDeque;

pub mod pathfinding;

#[derive(Component, Default)]
pub struct MoveQueue(pub VecDeque<Vec3>);

pub const PLAYER_HP: i32 = 30;
pub const PLAYER_MP: i32 = 20;
pub const PLAYER_ATK: i32 = 5;
pub const PLAYER_DEF: i32 = 1;

const BAR_WIDTH: f32 = 14.0;
const BAR_HEIGHT: f32 = 2.0;
const BAR_X: f32 = -BAR_WIDTH / 2.0;
const HP_BAR_Y: f32 = 11.0;
const MP_BAR_Y: f32 = HP_BAR_Y + BAR_HEIGHT; // 간격 없이 바로 위에 붙임
const BAR_ALPHA: f32 = 0.7;

const HP_BG_COLOR: Color = Color::rgba(0.6, 0.0, 0.0, BAR_ALPHA);
const MP_BG_COLOR: Color = Color::rgba(0.35, 0.35, 0.35, BAR_ALPHA);
const MP_FG_COLOR: Color = Color::rgba(0.2, 0.5, 1.0, BAR_ALPHA);

#[derive(Component)] struct HpBarFill;
#[derive(Component)] struct MpBarFill;

pub fn hp_color(ratio: f32) -> Color {
    if ratio > 0.5 { Color::rgba(0.0, 0.8, 0.0, BAR_ALPHA) }
    else if ratio > 0.25 { Color::rgba(0.9, 0.8, 0.0, BAR_ALPHA) }
    else { Color::rgba(0.9, 0.1, 0.1, BAR_ALPHA) }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlayerSystemSet {
    Movement,
    /// smooth_player_lerp 완료 후 실행되는 세트.
    /// 이동 완료 시 PlayerActedEvent 가 여기서 발행되므로,
    /// 픽업·몬스터·주민 등 턴 로직은 이 세트 이후에 실행해야 한다.
    MovementComplete,
}

pub const LERP_SPEED: f32 = 7.5;
const INITIAL_HOLD_DELAY: f32 = 0.12;

#[derive(Resource, Default)]
pub struct MoveHoldState {
    pub dir: IVec2,
    pub elapsed: f32,
}

#[derive(Resource, Default)]
pub struct PlayerPath(pub VecDeque<(usize, usize)>);

pub fn tick_hold(state: &mut MoveHoldState, dir: IVec2, just_pressed: bool, dt: f32) -> bool {
    if dir == IVec2::ZERO {
        state.dir = IVec2::ZERO;
        state.elapsed = 0.0;
        return false;
    }
    if dir != state.dir {
        let was_continuous = state.elapsed >= INITIAL_HOLD_DELAY;
        let from_stopped = state.dir == IVec2::ZERO;
        state.dir = dir;
        state.elapsed = if was_continuous { INITIAL_HOLD_DELAY } else { 0.0 };
        return was_continuous || (from_stopped && just_pressed);
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
        CombatStats {
            hp: PLAYER_HP, max_hp: PLAYER_HP,
            mp: PLAYER_MP, max_mp: PLAYER_MP,
            attack: PLAYER_ATK, defense: PLAYER_DEF,
        },
        Speed::new(1.0),
    )).with_children(|parent| {
        // HP 바 배경 (어두운 빨간색)
        parent.spawn(SpriteBundle {
            sprite: Sprite { custom_size: Some(Vec2::new(BAR_WIDTH, BAR_HEIGHT)), color: HP_BG_COLOR, anchor: Anchor::CenterLeft, ..default() },
            transform: Transform::from_xyz(BAR_X, HP_BAR_Y, 0.1),
            ..default()
        });
        // HP 바 전경
        parent.spawn((
            SpriteBundle {
                sprite: Sprite { custom_size: Some(Vec2::new(BAR_WIDTH, BAR_HEIGHT)), color: hp_color(1.0), anchor: Anchor::CenterLeft, ..default() },
                transform: Transform::from_xyz(BAR_X, HP_BAR_Y, 0.2),
                ..default()
            },
            HpBarFill,
        ));
        // MP 바 배경 (회색)
        parent.spawn(SpriteBundle {
            sprite: Sprite { custom_size: Some(Vec2::new(BAR_WIDTH, BAR_HEIGHT)), color: MP_BG_COLOR, anchor: Anchor::CenterLeft, ..default() },
            transform: Transform::from_xyz(BAR_X, MP_BAR_Y, 0.1),
            ..default()
        });
        // MP 바 전경 (파란색)
        parent.spawn((
            SpriteBundle {
                sprite: Sprite { custom_size: Some(Vec2::new(BAR_WIDTH, BAR_HEIGHT)), color: MP_FG_COLOR, anchor: Anchor::CenterLeft, ..default() },
                transform: Transform::from_xyz(BAR_X, MP_BAR_Y, 0.2),
                ..default()
            },
            MpBarFill,
        ));
    });
}

fn player_movement(
    mut commands: Commands,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut hold_state: ResMut<MoveHoldState>,
    mut player_path: ResMut<PlayerPath>,
    player_query: Query<(Entity, &Transform), (With<Player>, Without<MovingTo>, Without<Defeated>)>,
    map_res: Res<MapResource>,
    occupied: Res<OccupiedTiles>,
    monster_tiles: Res<MonsterTiles>,
    mut acted: EventWriter<PlayerActedEvent>,
    mut bump: EventWriter<BumpTileEvent>,
    mut attack: EventWriter<AttackMonsterEvent>,
    _log_writer: EventWriter<LogMessage>,
    equipment_open: Res<EquipmentPanelOpen>,
    shop_open: Res<ShopPanelOpen>,
) {
    if equipment_open.0 || shop_open.0 { return; }
    let Ok((entity, transform)) = player_query.get_single() else { return };

    // 스페이스바: 제자리 대기 — hold state 초기화 후 턴 소비
    if keyboard_input.just_pressed(KeyCode::Space) {
        hold_state.dir = IVec2::ZERO;
        hold_state.elapsed = 0.0;
        player_path.0.clear();
        acted.send(PlayerActedEvent);
        return;
    }

    let mut dir = IVec2::ZERO;
    if keyboard_input.pressed(KeyCode::ArrowLeft)  || keyboard_input.pressed(KeyCode::KeyA) { dir.x -= 1; }
    if keyboard_input.pressed(KeyCode::ArrowRight) || keyboard_input.pressed(KeyCode::KeyD) { dir.x += 1; }
    if keyboard_input.pressed(KeyCode::ArrowUp)    || keyboard_input.pressed(KeyCode::KeyW) { dir.y += 1; }
    if keyboard_input.pressed(KeyCode::ArrowDown)  || keyboard_input.pressed(KeyCode::KeyS) { dir.y -= 1; }

    let just_pressed = keyboard_input.just_pressed(KeyCode::ArrowLeft) || keyboard_input.just_pressed(KeyCode::KeyA)
        || keyboard_input.just_pressed(KeyCode::ArrowRight) || keyboard_input.just_pressed(KeyCode::KeyD)
        || keyboard_input.just_pressed(KeyCode::ArrowUp) || keyboard_input.just_pressed(KeyCode::KeyW)
        || keyboard_input.just_pressed(KeyCode::ArrowDown) || keyboard_input.just_pressed(KeyCode::KeyS);

    // 키 입력이 있으면 자동 이동 경로 취소
    if dir != IVec2::ZERO || just_pressed {
        player_path.0.clear();
    }

    // 자동 이동 경로 소비 (키 입력 없을 때)
    if dir == IVec2::ZERO && !player_path.0.is_empty() {
        if !tick_hold(&mut hold_state, IVec2::ONE, false, time.delta_seconds()) { return; }

        let (tx, ty) = player_path.0.front().copied().unwrap();

        if monster_tiles.0.contains(&(tx, ty)) {
            player_path.0.clear();
            attack.send(AttackMonsterEvent(tx, ty));
            acted.send(PlayerActedEvent);
        } else if occupied.0.contains(&(tx, ty)) {
            player_path.0.clear();
            bump.send(BumpTileEvent(tx, ty));
            acted.send(PlayerActedEvent);
        } else {
            player_path.0.pop_front();
            let wp = tile_to_world_coords(tx, ty);
            commands.entity(entity).insert(MovingTo { target: Vec3::new(wp.x, wp.y, 1.0) });
            // PlayerActedEvent 는 smooth_player_lerp 가 이동 완료 시 발행
        }
        return;
    }

    if !tick_hold(&mut hold_state, dir, just_pressed, time.delta_seconds()) { return; }
    let delta = hold_state.dir;
    if delta == IVec2::ZERO { return; }

    let (cx, cy) = world_to_tile_coords(transform.translation);
    let tx = (cx as i32 + delta.x) as usize;
    let ty = (cy as i32 + delta.y) as usize;

    if map_res.map().get_tile(tx, ty) != TileKind::Floor { return; }

    if monster_tiles.0.contains(&(tx, ty)) {
        attack.send(AttackMonsterEvent(tx, ty));
        acted.send(PlayerActedEvent);
    } else if occupied.0.contains(&(tx, ty)) {
        bump.send(BumpTileEvent(tx, ty));
        acted.send(PlayerActedEvent);
    } else {
        let wp = tile_to_world_coords(tx, ty);
        commands.entity(entity).insert(MovingTo { target: Vec3::new(wp.x, wp.y, 1.0) });
        // PlayerActedEvent 는 smooth_player_lerp 가 이동 완료 시 발행
    }
}

fn smooth_player_lerp(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut Transform, &MovingTo), With<Player>>,
    mut acted: EventWriter<PlayerActedEvent>,
) {
    for (entity, mut transform, moving) in query.iter_mut() {
        let dist = transform.translation.distance(moving.target);
        let step = LERP_SPEED * TILE_SIZE * time.delta_seconds();
        if dist < step {
            transform.translation = moving.target;
            commands.entity(entity).remove::<MovingTo>();
            acted.send(PlayerActedEvent);
        } else {
            let dir = (moving.target - transform.translation).normalize();
            transform.translation += dir * step;
        }
    }
}

fn on_mouse_click(
    mouse_input: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<Camera>>,
    player_query: Query<&Transform, With<Player>>,
    map_res: Res<MapResource>,
    mut player_path: ResMut<PlayerPath>,
    equipment_open: Res<EquipmentPanelOpen>,
    shop_open: Res<ShopPanelOpen>,
) {
    if !mouse_input.just_pressed(MouseButton::Left) { return; }
    if equipment_open.0 || shop_open.0 { return; }

    let Ok(window) = windows.get_single() else { return };
    let Ok((camera, cam_transform)) = camera_q.get_single() else { return };
    let Ok(player_transform) = player_query.get_single() else { return };

    let Some(cursor_pos) = window.cursor_position() else { return };
    let Some(world_pos) = camera.viewport_to_world_2d(cam_transform, cursor_pos) else { return };

    let world_vec3 = Vec3::new(world_pos.x, world_pos.y, 0.0);
    let (tx, ty) = world_to_tile_coords(world_vec3);
    let map = map_res.map();
    if map.get_tile(tx, ty) != TileKind::Floor { return; }

    let (px, py) = world_to_tile_coords(player_transform.translation);
    let path = pathfinding::find_path(map, (px, py), (tx, ty));
    player_path.0 = VecDeque::from(path);
}

fn respawn_player_on_regen(
    mut commands: Commands,
    mut events: EventReader<PlayerRespawnEvent>,
    mut player_query: Query<(Entity, &mut Transform), With<Player>>,
    mut player_path: ResMut<PlayerPath>,
) {
    for PlayerRespawnEvent(x, y) in events.read() {
        if let Ok((entity, mut transform)) = player_query.get_single_mut() {
            let wp = tile_to_world_coords(*x, *y);
            transform.translation = Vec3::new(wp.x, wp.y, 1.0);
            commands.entity(entity).remove::<MovingTo>();
            player_path.0.clear();
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
    map.tiles.iter_mut().for_each(|t| t.visible = false);

    let radius = 8i32;
    for y in (cur.y - radius)..=(cur.y + radius) {
        for x in (cur.x - radius)..=(cur.x + radius) {
            if x < 0 || x >= map.width as i32 || y < 0 || y >= map.height as i32 { continue; }
            let (dx, dy) = (x - cur.x, y - cur.y);
            if dx * dx + dy * dy > radius * radius { continue; }
            if is_line_of_sight_clear(map, cur.x, cur.y, x, y) {
                let idx = map.index(x as usize, y as usize);
                map.tiles[idx].visible = true;
                map.tiles[idx].revealed = true;
            }
        }
    }
    let elapsed = start.elapsed();
    if elapsed.as_micros() > 0 { info!("FOV: {:?}", elapsed); }
}

fn update_player_bars(
    player_query: Query<&CombatStats, (With<Player>, Changed<CombatStats>)>,
    mut hp_query: Query<&mut Sprite, (With<HpBarFill>, Without<MpBarFill>)>,
    mut mp_query: Query<&mut Sprite, (With<MpBarFill>, Without<HpBarFill>)>,
) {
    let Ok(stats) = player_query.get_single() else { return };

    if let Ok(mut sprite) = hp_query.get_single_mut() {
        let ratio = (stats.hp as f32 / stats.max_hp as f32).clamp(0.0, 1.0);
        sprite.custom_size = Some(Vec2::new(BAR_WIDTH * ratio, BAR_HEIGHT));
        sprite.color = hp_color(ratio);
    }
    if let Ok(mut sprite) = mp_query.get_single_mut() {
        let ratio = if stats.max_mp > 0 {
            (stats.mp as f32 / stats.max_mp as f32).clamp(0.0, 1.0)
        } else { 0.0 };
        sprite.custom_size = Some(Vec2::new(BAR_WIDTH * ratio, BAR_HEIGHT));
    }
}

pub struct PlayerPlugin;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hp_color_green_above_half() {
        assert_eq!(hp_color(1.0),  Color::rgba(0.0, 0.8, 0.0, BAR_ALPHA));
        assert_eq!(hp_color(0.51), Color::rgba(0.0, 0.8, 0.0, BAR_ALPHA));
    }

    #[test]
    fn hp_color_yellow_quarter_to_half() {
        assert_eq!(hp_color(0.5),  Color::rgba(0.9, 0.8, 0.0, BAR_ALPHA));
        assert_eq!(hp_color(0.26), Color::rgba(0.9, 0.8, 0.0, BAR_ALPHA));
    }

    #[test]
    fn hp_color_red_at_or_below_quarter() {
        assert_eq!(hp_color(0.25), Color::rgba(0.9, 0.1, 0.1, BAR_ALPHA));
        assert_eq!(hp_color(0.0),  Color::rgba(0.9, 0.1, 0.1, BAR_ALPHA));
    }

    #[test]
    fn hp_color_alpha_is_bar_alpha() {
        for ratio in [0.0, 0.25, 0.26, 0.5, 0.51, 1.0] {
            assert_eq!(hp_color(ratio).a(), BAR_ALPHA, "ratio={ratio} 의 alpha 가 BAR_ALPHA 여야 한다");
        }
    }

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
    fn tick_hold_resets_on_direction_change_during_delay() {
        let mut state = MoveHoldState::default();
        let dir = IVec2::new(-1, 0);
        tick_hold(&mut state, dir, true, 0.0);
        // 2 frames (0.032s) — still in initial delay (< 0.12s)
        tick_hold(&mut state, dir, false, 0.016);
        tick_hold(&mut state, dir, false, 0.016);
        let result = tick_hold(&mut state, IVec2::new(1, 0), false, 0.016);
        assert!(!result, "초기 지연 중 방향 전환 직후에는 이동하지 않아야 한다");
        assert_eq!(state.elapsed, 0.0, "초기 지연 중 방향 전환 시 타이머가 리셋돼야 한다");
    }

    #[test]
    fn tick_hold_zero_direction_never_triggers_move() {
        // wait(스페이스) 후 ZERO 방향은 elapsed 가 아무리 쌓여도 이동을 유발하지 않는다
        let mut state = MoveHoldState { dir: IVec2::new(1, 0), elapsed: 1.0 };
        assert!(!tick_hold(&mut state, IVec2::ZERO, false, 0.0));
        assert_eq!(state.dir, IVec2::ZERO);
        assert_eq!(state.elapsed, 0.0);
    }

    #[test]
    fn tick_hold_direction_change_in_continuous_is_immediate() {
        let mut state = MoveHoldState::default();
        let dir = IVec2::new(-1, 0);
        tick_hold(&mut state, dir, true, 0.0);
        // 10 frames (0.16s) — past INITIAL_HOLD_DELAY (0.12s) → continuous mode
        for _ in 0..10 { tick_hold(&mut state, dir, false, 0.016); }
        let result = tick_hold(&mut state, IVec2::new(1, 0), false, 0.016);
        assert!(result, "연속 이동 중 방향 전환 시 즉시 이동해야 한다");
        assert_eq!(state.elapsed, INITIAL_HOLD_DELAY, "연속 이동 중 방향 전환 시 타이머는 INITIAL_HOLD_DELAY여야 한다");
    }
}

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MoveHoldState>()
            .init_resource::<PlayerPath>()
            .configure_sets(Update, PlayerSystemSet::MovementComplete.after(PlayerSystemSet::Movement))
            .add_systems(Startup, spawn_player.after(draw_map))
            .add_systems(Update, (
                on_mouse_click.before(PlayerSystemSet::Movement),
                player_movement.in_set(PlayerSystemSet::Movement),
                smooth_player_lerp.in_set(PlayerSystemSet::MovementComplete),
                update_fov.after(PlayerSystemSet::MovementComplete),
                camera_follow_player.after(update_fov),
                update_player_bars,
                respawn_player_on_regen.after(MapSystemSet::ExecuteRegen),
            ));
    }
}
