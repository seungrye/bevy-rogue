use crate::modules::{
    map::{
        draw_map, Map, MapResource, OccupiedTiles, MonsterTiles,
        tile_to_world_coords, world_to_tile_coords, is_in_view, FOV_FRONT, FOV_BACK,
        MAP_HEIGHT, MAP_WIDTH, TILE_SIZE,
        MapSystemSet, PlayerRespawnEvent, PlayerActedEvent, BumpTileEvent, AttackMonsterEvent,
    },
    combat::{CombatStats, Defeated, Speed},
    item::EquipmentPanelOpen,
    ui::{help::HelpPanelOpen, shop::ShopPanelOpen},
    elemental::ElementalStatus,
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


#[derive(Resource, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PlayerProgress {
    pub level: u32,
    pub xp: u32,
    pub next_level_xp: u32,
    pub kills: u32,
}

impl Default for PlayerProgress {
    fn default() -> Self {
        Self { level: 1, xp: 0, next_level_xp: xp_to_next_level(1), kills: 0 }
    }
}

/// 현재 레벨에서 다음 레벨로 오르는 데 필요한 XP를 반환한다.
///
/// 몬스터 밀도를 조정하는 단계라 곡선은 작고 읽기 쉽게 유지한다.
/// 초반 처치는 빠른 피드백을 주고, 레벨이 오를수록 조금 더 많은 전투를 요구한다.
pub fn xp_to_next_level(level: u32) -> u32 {
    20 + level.saturating_sub(1) * 15
}

/// 몬스터 표시 이름에 따라 처치 보상 XP를 반환한다.
///
/// 보상 값을 한 함수에 모아 첫 밸런스 기준을 명확히 하고, 테스트가 의존할 안정적인
/// 계약을 제공한다. 내부 몬스터 스폰 테이블은 노출하지 않는다.
pub fn xp_reward_for_monster(name: &str) -> u32 {
    match name {
        "고블린" => 8,
        "오크" => 14,
        "트롤" => 24,
        _ => 10,
    }
}

/// XP를 더하고 처치 수를 기록한 뒤, 레벨업 보너스를 플레이어 스탯에 적용한다.
///
/// 현재 레벨업은 생존력만 올린다. 최대 HP/MP가 증가하고 두 자원이 모두
/// 회복된다. 공격/방어는 장비 갱신 흐름에 맡겨 두 시스템이 서로 값을 덮어쓰지
/// 않게 한다. 별도의 기본 스탯 모델이 생기기 전까지 이 경계를 유지한다.
pub fn grant_xp(
    progress: &mut PlayerProgress,
    stats: &mut CombatStats,
    amount: u32,
) -> u32 {
    progress.kills += 1;
    progress.xp += amount;

    let mut gained_levels = 0;
    while progress.xp >= progress.next_level_xp {
        progress.xp -= progress.next_level_xp;
        progress.level += 1;
        gained_levels += 1;
        progress.next_level_xp = xp_to_next_level(progress.level);

        stats.max_hp += 5;
        stats.max_mp += 2;
        stats.hp = stats.max_hp;
        stats.mp = stats.max_mp;
    }

    gained_levels
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

pub fn offset_tile_in_bounds(map: &Map, x: usize, y: usize, delta: IVec2) -> Option<(usize, usize)> {
    let tx = x as i32 + delta.x;
    let ty = y as i32 + delta.y;
    if tx < 0 || ty < 0 || tx >= map.width as i32 || ty >= map.height as i32 {
        return None;
    }
    Some((tx as usize, ty as usize))
}

#[derive(Component)]
pub struct Player;

/// 시야 주체가 마지막으로 향한 방향(이동 방향). 이동할 때마다 갱신되고,
/// 정지 시에는 마지막 방향을 유지한다. 방향 시야(facing-based FOV)가
/// "정면은 멀리, 등 뒤는 가깝게" 보도록 하는 기준이다.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Facing(pub IVec2);

impl Default for Facing {
    /// 초기 기본값은 아래(`(0,-1)`)를 향한다.
    fn default() -> Self { Facing(IVec2::new(0, -1)) }
}

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
        ElementalStatus::default(),
        Facing::default(),
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

/// 키보드 이동과 대기 입력을 읽어 플레이어 행동 하나로 변환한다.
///
/// 모달 UI 패널이 열려 있으면 이동 처리를 의도적으로 멈춘다. 패널 조작 명령이
/// 던전에서 실수 이동이나 대기 턴으로 새어 나가지 않게 하기 위해서다.
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
    equipment_open: Res<EquipmentPanelOpen>,
    shop_open: Res<ShopPanelOpen>,
    help_open: Res<HelpPanelOpen>,
    ranged: Res<crate::modules::ranged::RangedTargeting>,
) {
    if equipment_open.0 || shop_open.0 || help_open.0 { return; }
    if ranged.active { return; }
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
            let (cx, cy) = world_to_tile_coords(transform.translation);
            let face = IVec2::new(tx as i32 - cx as i32, ty as i32 - cy as i32);
            if face != IVec2::ZERO { commands.entity(entity).insert(Facing(face)); }
            let wp = tile_to_world_coords(tx, ty);
            commands.entity(entity).insert(MovingTo { target: Vec3::new(wp.x, wp.y, 1.0) });
            // PlayerActedEvent 는 smooth_player_lerp 가 이동 완료 시 발행
        }
        return;
    }

    if !tick_hold(&mut hold_state, dir, just_pressed, time.delta_seconds()) { return; }
    let delta = hold_state.dir;
    // tick_hold 는 dir 이 ZERO 면 항상 false 를 돌려주므로, 여기까지 왔다면
    // hold_state.dir 은 결코 ZERO 가 아니다. 방어적 가드. // 도달 불가 방어코드
    if delta == IVec2::ZERO { return; }

    let map = map_res.map();
    let (cx, cy) = world_to_tile_coords(transform.translation);
    let Some((tx, ty)) = offset_tile_in_bounds(map, cx, cy, delta) else { return; };

    if !map.get_tile(tx, ty).is_walkable() { return; }

    if monster_tiles.0.contains(&(tx, ty)) {
        attack.send(AttackMonsterEvent(tx, ty));
        acted.send(PlayerActedEvent);
    } else if occupied.0.contains(&(tx, ty)) {
        bump.send(BumpTileEvent(tx, ty));
        acted.send(PlayerActedEvent);
    } else {
        commands.entity(entity).insert(Facing(delta));
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

/// 클릭한 화면 좌표(`target_world`)에서 자동 이동 경로를 계산하는 순수 결정 로직.
///
/// 윈도우/카메라 viewport 변환은 헤드리스 테스트로 재현하기 어려워 시스템 쪽에 남기고,
/// 변환 결과(`target_world: Option<Vec2>`)만 받아 분기를 모두 여기서 다룬다.
/// 목적지가 이동 불가(`Wall`/`Water`)거나 viewport 변환이 실패하면 `None` 을 돌려
/// 호출자가 경로를 건드리지 않게 한다.
fn plan_click_path(
    target_world: Option<Vec2>,
    map: &Map,
    player_world: Vec3,
) -> Option<VecDeque<(usize, usize)>> {
    let world_pos = target_world?;
    let world_vec3 = Vec3::new(world_pos.x, world_pos.y, 0.0);
    let (tx, ty) = world_to_tile_coords(world_vec3);
    if !map.get_tile(tx, ty).is_walkable() { return None; }

    let (px, py) = world_to_tile_coords(player_world);
    let path = pathfinding::find_path(map, (px, py), (tx, ty));
    Some(VecDeque::from(path))
}

/// 플레이어 위치에서 클릭한 바닥 타일까지 자동 이동 경로를 만든다.
///
/// 모달 패널이 열려 있을 때는 마우스 경로 이동도 무시한다. 키보드 이동과 같은
/// 기준을 적용해 오버레이가 단순한 투명 UI가 아니라 상호작용 경계로 동작하게 한다.
fn on_mouse_click(
    mouse_input: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<Camera>>,
    player_query: Query<&Transform, (With<Player>, Without<Defeated>)>,
    map_res: Res<MapResource>,
    mut player_path: ResMut<PlayerPath>,
    equipment_open: Res<EquipmentPanelOpen>,
    shop_open: Res<ShopPanelOpen>,
    help_open: Res<HelpPanelOpen>,
    ranged: Res<crate::modules::ranged::RangedTargeting>,
) {
    if !mouse_input.just_pressed(MouseButton::Left) { return; }
    if equipment_open.0 || shop_open.0 || help_open.0 { return; }
    if ranged.active { return; }  // 원격 모드 중에는 ranged 시스템이 마우스 처리

    let Ok(window) = windows.get_single() else { return };
    let Ok((camera, cam_transform)) = camera_q.get_single() else { return };
    let Ok(player_transform) = player_query.get_single() else { return };

    let Some(cursor_pos) = window.cursor_position() else { return };
    let world_pos = camera.viewport_to_world_2d(cam_transform, cursor_pos);

    // viewport 변환 성공 시에만 Some — 헤드리스 테스트는 viewport_to_world_2d 가
    // 항상 None 이라 이 if 의 Some 분기는 직접 검증 불가(plan_click_path 를 순수
    // 함수로 분리해 결정 분기는 단위 테스트로 모두 커버). // 도달 불가 방어코드
    if let Some(path) = plan_click_path(world_pos, map_res.map(), player_transform.translation) {
        player_path.0 = path;
    }
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
    mut camera_query: Query<(&mut Transform, &OrthographicProjection), (With<Camera>, Without<Player>)>,
) {
    use crate::modules::map::{MAP_WIDTH, MAP_HEIGHT, TILE_SIZE};
    let Ok(pt) = player_query.get_single() else { return };
    let Ok((mut ct, proj)) = camera_query.get_single_mut() else { return };

    // 카메라 viewport 가 윈도우 크기에 따라 변하므로 동적으로 clamp.
    // viewport 가 맵보다 크면 외부 노출이 불가피하므로 중앙 (0, 0) 에 고정.
    let map_w = MAP_WIDTH as f32 * TILE_SIZE;
    let map_h = MAP_HEIGHT as f32 * TILE_SIZE;
    let half_w = proj.area.width() / 2.0;
    let half_h = proj.area.height() / 2.0;

    let cx = if half_w * 2.0 >= map_w {
        0.0
    } else {
        pt.translation.x.clamp(half_w - map_w / 2.0, map_w / 2.0 - half_w)
    };
    let cy = if half_h * 2.0 >= map_h {
        0.0
    } else {
        pt.translation.y.clamp(half_h - map_h / 2.0, map_h / 2.0 - half_h)
    };

    ct.translation.x = cx;
    ct.translation.y = cy;
}

fn update_fov(
    player_query: Query<(&Transform, &Facing), With<Player>>,
    mut map_res: ResMut<MapResource>,
    mut last_pos: Local<Option<(IVec2, IVec2)>>,
) {
    // 맵이 교체되면 강제 재계산
    if map_res.is_changed() {
        *last_pos = None;
    }

    let Ok((transform, facing)) = player_query.get_single() else { return };
    let (px, py) = world_to_tile_coords(transform.translation);
    let cur = IVec2::new(px as i32, py as i32);
    // 위치뿐 아니라 facing 이 바뀌어도(제자리 회전) 시야가 달라지므로 함께 추적한다.
    let key = (cur, facing.0);
    if Some(key) == *last_pos { return; }
    *last_pos = Some(key);

    let start = std::time::Instant::now();
    let map = map_res.map_mut();
    map.tiles.iter_mut().for_each(|t| t.visible = false);

    // 두-반원의 최대 탐색 범위는 max(front, back).
    let radius = FOV_FRONT.max(FOV_BACK);
    for y in (cur.y - radius)..=(cur.y + radius) {
        for x in (cur.x - radius)..=(cur.x + radius) {
            if x < 0 || x >= map.width as i32 || y < 0 || y >= map.height as i32 { continue; }
            if is_in_view(cur.x, cur.y, facing.0, x, y, FOV_FRONT, FOV_BACK, map) {
                let idx = map.index(x as usize, y as usize);
                map.tiles[idx].visible = true;
                map.tiles[idx].revealed = true;
            }
        }
    }
    let elapsed = start.elapsed();
    // FOV 계산이 5ms 이상 걸릴 때만 찍는 성능 로그. 테스트 맵은 작아 항상 즉시
    // 끝나므로 이 분기의 True 쪽은 결정론적으로 도달 불가. // 도달 불가 방어코드
    if elapsed.as_millis() >= 5 { debug!("FOV: {:?}", elapsed); }
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
    #![allow(non_snake_case)]
    use super::*;
    use crate::modules::map::TileKind;

    #[test]
    fn 레벨이_오르면_다음_레벨_필요xp가_증가한다() {
        assert_eq!(xp_to_next_level(1), 20);
        assert_eq!(xp_to_next_level(3), 50);
    }

    #[test]
    fn 몬스터별_처치보상xp는_첫_밸런스값과_일치한다() {
        assert_eq!(xp_reward_for_monster("고블린"), 8);
        assert_eq!(xp_reward_for_monster("오크"), 14);
        assert_eq!(xp_reward_for_monster("트롤"), 24);
        assert_eq!(xp_reward_for_monster("알 수 없음"), 10);
    }

    #[test]
    fn xp를_획득해_레벨업하면_자원이_최대치로_회복된다() {
        let mut progress = PlayerProgress::default();
        let mut stats = CombatStats {
            hp: 3,
            max_hp: PLAYER_HP,
            mp: 1,
            max_mp: PLAYER_MP,
            attack: PLAYER_ATK,
            defense: PLAYER_DEF,
        };

        let levels = grant_xp(&mut progress, &mut stats, 20);

        assert_eq!(levels, 1);
        assert_eq!(progress.level, 2);
        assert_eq!(progress.kills, 1);
        assert_eq!(stats.max_hp, PLAYER_HP + 5);
        assert_eq!(stats.hp, stats.max_hp);
        assert_eq!(stats.max_mp, PLAYER_MP + 2);
        assert_eq!(stats.mp, stats.max_mp);
    }

    #[test]
    fn hp비율이_절반_초과면_색은_녹색이다() {
        assert_eq!(hp_color(1.0),  Color::rgba(0.0, 0.8, 0.0, BAR_ALPHA));
        assert_eq!(hp_color(0.51), Color::rgba(0.0, 0.8, 0.0, BAR_ALPHA));
    }

    #[test]
    fn hp비율이_사분의일에서_절반_사이면_색은_노랑이다() {
        assert_eq!(hp_color(0.5),  Color::rgba(0.9, 0.8, 0.0, BAR_ALPHA));
        assert_eq!(hp_color(0.26), Color::rgba(0.9, 0.8, 0.0, BAR_ALPHA));
    }

    #[test]
    fn hp비율이_사분의일_이하면_색은_빨강이다() {
        assert_eq!(hp_color(0.25), Color::rgba(0.9, 0.1, 0.1, BAR_ALPHA));
        assert_eq!(hp_color(0.0),  Color::rgba(0.9, 0.1, 0.1, BAR_ALPHA));
    }

    #[test]
    fn hp색상의_투명도는_항상_바_투명도와_같다() {
        for ratio in [0.0, 0.25, 0.26, 0.5, 0.51, 1.0] {
            assert_eq!(hp_color(ratio).a(), BAR_ALPHA, "ratio={ratio} 의 alpha 가 BAR_ALPHA 여야 한다");
        }
    }

    #[test]
    fn 경계밖_음수_좌표로의_이동은_거부된다() {
        let map = Map::new(10, 10);
        assert_eq!(offset_tile_in_bounds(&map, 0, 0, IVec2::new(-1, 0)), None);
    }

    #[test]
    fn 경계_안_유효좌표로의_이동은_허용된다() {
        let map = Map::new(10, 10);
        assert_eq!(offset_tile_in_bounds(&map, 3, 4, IVec2::new(1, -1)), Some((4, 3)));
    }

    #[test]
    fn 경계밖_위쪽_음수_y좌표로의_이동은_거부된다() {
        // ty < 0 분기: x 는 유효하지만 y 가 음수.
        let map = Map::new(10, 10);
        assert_eq!(offset_tile_in_bounds(&map, 3, 0, IVec2::new(0, -1)), None);
    }

    #[test]
    fn 경계밖_오른쪽_x좌표로의_이동은_거부된다() {
        // tx >= width 분기.
        let map = Map::new(10, 10);
        assert_eq!(offset_tile_in_bounds(&map, 9, 3, IVec2::new(1, 0)), None);
    }

    #[test]
    fn 경계밖_아래쪽_y좌표로의_이동은_거부된다() {
        // ty >= height 분기.
        let map = Map::new(10, 10);
        assert_eq!(offset_tile_in_bounds(&map, 3, 9, IVec2::new(0, 1)), None);
    }

    // --- 플레이어 키보드 이동: Water 차단 / Sand 통과 (App 하네스) ---

    /// (5,5) 에 플레이어를 두고 오른쪽 한 칸 (6,5) 의 타일 종류를 바꿔
    /// player_movement 시스템이 MovingTo 를 삽입하는지로 이동 가부를 검증한다.
    fn run_player_move_right(target_kind: TileKind) -> bool {
        use crate::modules::map::{MapResource, OccupiedTiles, MonsterTiles, MapType};
        let mut app = App::new();
        app.init_resource::<Time>();
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app.init_resource::<MoveHoldState>();
        app.init_resource::<PlayerPath>();
        app.init_resource::<OccupiedTiles>();
        app.init_resource::<MonsterTiles>();
        app.insert_resource(EquipmentPanelOpen(false));
        app.insert_resource(ShopPanelOpen(false));
        app.insert_resource(HelpPanelOpen(false));
        app.init_resource::<crate::modules::ranged::RangedTargeting>();
        app.add_event::<PlayerActedEvent>();
        app.add_event::<BumpTileEvent>();
        app.add_event::<AttackMonsterEvent>();

        // 맵: 전부 Wall, (5,5)=Floor, (6,5)=대상 타일
        let mut map = Map::new(20, 20);
        map.map_type = MapType::Dungeon;
        map.set_tile(5, 5, TileKind::Floor);
        map.set_tile(6, 5, target_kind);
        app.insert_resource(MapResource(map));

        let pos = tile_to_world_coords(5, 5);
        let player = app.world.spawn((
            Player,
            Transform::from_xyz(pos.x, pos.y, 1.0),
        )).id();

        app.add_systems(Update, player_movement);

        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowRight);
        app.update();

        app.world.entity(player).contains::<MovingTo>()
    }

    #[test]
    fn 플레이어는_물타일로_이동하지_못한다() {
        assert!(!run_player_move_right(TileKind::Water),
            "물 타일로는 이동(MovingTo)이 발생하면 안 된다");
    }

    #[test]
    fn 플레이어는_모래타일로_이동할_수_있다() {
        assert!(run_player_move_right(TileKind::Sand),
            "모래 타일로는 이동(MovingTo)이 발생해야 한다");
    }

    #[test]
    fn 플레이어는_벽타일로_이동하지_못한다() {
        // 기존 동작 불변 회귀: Wall 은 여전히 막혀야 한다.
        assert!(!run_player_move_right(TileKind::Wall),
            "벽 타일로는 이동이 발생하면 안 된다");
    }

    #[test]
    fn 플레이어는_바닥타일로_이동할_수_있다() {
        // 기존 동작 불변 회귀: Floor 는 여전히 통과해야 한다.
        assert!(run_player_move_right(TileKind::Floor),
            "바닥 타일로는 이동이 발생해야 한다");
    }

    #[test]
    fn 키를_막_누르면_즉시_이동이_허용된다() {
        let mut state = MoveHoldState::default();
        assert!(tick_hold(&mut state, IVec2::new(-1, 0), true, 0.016));
    }

    #[test]
    fn 초기지연_이전에는_이동하지_않는다() {
        let mut state = MoveHoldState::default();
        let dir = IVec2::new(-1, 0);
        tick_hold(&mut state, dir, true, 0.0);
        assert!(!tick_hold(&mut state, dir, false, 0.016));
    }

    #[test]
    fn 초기지연_이후에는_연속이동이_시작된다() {
        let mut state = MoveHoldState::default();
        let dir = IVec2::new(-1, 0);
        tick_hold(&mut state, dir, true, 0.0);
        let triggered = (0..20).any(|_| tick_hold(&mut state, dir, false, 0.016));
        assert!(triggered, "INITIAL_HOLD_DELAY 이후 연속 이동이 시작돼야 한다");
    }

    #[test]
    fn 키를_떼면_홀드상태가_초기화된다() {
        let mut state = MoveHoldState::default();
        let dir = IVec2::new(-1, 0);
        tick_hold(&mut state, dir, true, 0.0);
        tick_hold(&mut state, IVec2::ZERO, false, 0.016);
        assert_eq!(state.dir, IVec2::ZERO);
        assert_eq!(state.elapsed, 0.0);
    }

    #[test]
    fn 초기지연_중_방향전환하면_타이머가_리셋되고_이동하지_않는다() {
        let mut state = MoveHoldState::default();
        let dir = IVec2::new(-1, 0);
        tick_hold(&mut state, dir, true, 0.0);
        // 2프레임(0.032초) — 아직 초기 지연 시간 안쪽이다(< 0.12초)
        tick_hold(&mut state, dir, false, 0.016);
        tick_hold(&mut state, dir, false, 0.016);
        let result = tick_hold(&mut state, IVec2::new(1, 0), false, 0.016);
        assert!(!result, "초기 지연 중 방향 전환 직후에는 이동하지 않아야 한다");
        assert_eq!(state.elapsed, 0.0, "초기 지연 중 방향 전환 시 타이머가 리셋돼야 한다");
    }

    #[test]
    fn 방향이_영벡터면_누적시간과_무관하게_이동하지_않는다() {
        // wait(스페이스) 후 ZERO 방향은 elapsed 가 아무리 쌓여도 이동을 유발하지 않는다
        let mut state = MoveHoldState { dir: IVec2::new(1, 0), elapsed: 1.0 };
        assert!(!tick_hold(&mut state, IVec2::ZERO, false, 0.0));
        assert_eq!(state.dir, IVec2::ZERO);
        assert_eq!(state.elapsed, 0.0);
    }

    #[test]
    fn 연속이동_중_방향전환은_즉시_이동한다() {
        let mut state = MoveHoldState::default();
        let dir = IVec2::new(-1, 0);
        tick_hold(&mut state, dir, true, 0.0);
        // 10프레임(0.16초) — INITIAL_HOLD_DELAY(0.12초)를 지나 연속 이동 상태가 된다
        for _ in 0..10 { tick_hold(&mut state, dir, false, 0.016); }
        let result = tick_hold(&mut state, IVec2::new(1, 0), false, 0.016);
        assert!(result, "연속 이동 중 방향 전환 시 즉시 이동해야 한다");
        assert_eq!(state.elapsed, INITIAL_HOLD_DELAY, "연속 이동 중 방향 전환 시 타이머는 INITIAL_HOLD_DELAY여야 한다");
    }

    // ===================================================================
    // 시스템 App 하네스 — player_movement / 마우스 / lerp / FOV / 카메라 /
    // 상태바 / 리스폰 / 스폰 / 플러그인 빌드
    // ===================================================================

    use crate::modules::map::{MapResource, OccupiedTiles, MonsterTiles, MapType, Rect};
    use crate::modules::combat::{CombatStats, Defeated};
    use crate::modules::ranged::RangedTargeting;

    /// player_movement 시스템 단독을 돌리기 위한 공통 하네스.
    /// 필요한 리소스를 모두 기본값으로 깔고, 빈 던전 맵 하나만 둔 App 을 만든다.
    /// (5,5) 만 Floor 로 열어두고 플레이어를 그 위에 둔다.
    struct MoveHarness {
        app: App,
        player: Entity,
    }

    impl MoveHarness {
        fn new(map_w: usize, map_h: usize) -> Self {
            let mut app = App::new();
            app.init_resource::<Time>();
            app.insert_resource(ButtonInput::<KeyCode>::default());
            app.init_resource::<MoveHoldState>();
            app.init_resource::<PlayerPath>();
            app.init_resource::<OccupiedTiles>();
            app.init_resource::<MonsterTiles>();
            app.insert_resource(EquipmentPanelOpen(false));
            app.insert_resource(ShopPanelOpen(false));
            app.insert_resource(HelpPanelOpen(false));
            app.init_resource::<RangedTargeting>();
            app.add_event::<PlayerActedEvent>();
            app.add_event::<BumpTileEvent>();
            app.add_event::<AttackMonsterEvent>();

            let mut map = Map::new(map_w, map_h);
            map.map_type = MapType::Dungeon;
            map.set_tile(5, 5, TileKind::Floor);
            app.insert_resource(MapResource(map));

            let pos = tile_to_world_coords(5, 5);
            let player = app.world.spawn((
                Player,
                Transform::from_xyz(pos.x, pos.y, 1.0),
            )).id();

            app.add_systems(Update, player_movement);
            Self { app, player }
        }

        fn set_tile(&mut self, x: usize, y: usize, kind: TileKind) {
            self.app.world.resource_mut::<MapResource>().map_mut().set_tile(x, y, kind);
        }
        fn press(&mut self, key: KeyCode) {
            self.app.world.resource_mut::<ButtonInput<KeyCode>>().press(key);
        }
        fn advance(&mut self, secs: f32) {
            self.app.world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(secs));
        }
        fn update(&mut self) { self.app.update(); }
        fn moving(&self) -> bool { self.app.world.entity(self.player).contains::<MovingTo>() }
        fn acted_count(&mut self) -> usize {
            let events = self.app.world.resource::<Events<PlayerActedEvent>>();
            let mut r = events.get_reader();
            r.read(events).count()
        }
        fn attack_targets(&mut self) -> Vec<(usize, usize)> {
            let events = self.app.world.resource::<Events<AttackMonsterEvent>>();
            let mut r = events.get_reader();
            r.read(events).map(|e| (e.0, e.1)).collect()
        }
        fn bump_targets(&mut self) -> Vec<(usize, usize)> {
            let events = self.app.world.resource::<Events<BumpTileEvent>>();
            let mut r = events.get_reader();
            r.read(events).map(|e| (e.0, e.1)).collect()
        }
        fn path_len(&self) -> usize { self.app.world.resource::<PlayerPath>().0.len() }
        fn facing(&self) -> Option<IVec2> {
            self.app.world.get::<Facing>(self.player).map(|f| f.0)
        }
    }

    // --- player_movement: 조기 종료 분기 ---

    #[test]
    fn 장비패널이_열려있으면_이동입력을_무시한다() {
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(6, 5, TileKind::Floor);
        h.app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        h.press(KeyCode::ArrowRight);
        h.update();
        assert!(!h.moving(), "장비 패널이 열려 있으면 이동하지 않아야 한다");
    }

    #[test]
    fn 상점패널이_열려있으면_이동입력을_무시한다() {
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(6, 5, TileKind::Floor);
        h.app.world.resource_mut::<ShopPanelOpen>().0 = true;
        h.press(KeyCode::ArrowRight);
        h.update();
        assert!(!h.moving());
    }

    #[test]
    fn 도움말패널이_열려있으면_이동입력을_무시한다() {
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(6, 5, TileKind::Floor);
        h.app.world.resource_mut::<HelpPanelOpen>().0 = true;
        h.press(KeyCode::ArrowRight);
        h.update();
        assert!(!h.moving());
    }

    #[test]
    fn 원격모드가_활성화면_이동입력을_무시한다() {
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(6, 5, TileKind::Floor);
        h.app.world.resource_mut::<RangedTargeting>().active = true;
        h.press(KeyCode::ArrowRight);
        h.update();
        assert!(!h.moving());
    }

    #[test]
    fn 플레이어가_사망상태면_이동입력을_무시한다() {
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(6, 5, TileKind::Floor);
        h.app.world.entity_mut(h.player).insert(Defeated);
        h.press(KeyCode::ArrowRight);
        h.update();
        assert!(!h.moving(), "Defeated 상태면 query 가 비어 이동하지 않아야 한다");
    }

    #[test]
    fn 이미_이동중이면_새_이동입력을_받지_않는다() {
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(6, 5, TileKind::Floor);
        // 이미 MovingTo 가 붙어 있으면 Without<MovingTo> 필터로 query 가 비어 무시.
        let target = tile_to_world_coords(6, 5);
        h.app.world.entity_mut(h.player).insert(MovingTo { target: target.extend(1.0) });
        h.press(KeyCode::ArrowRight);
        h.update();
        assert_eq!(h.acted_count(), 0, "이동 중에는 행동 이벤트가 발생하지 않아야 한다");
    }

    // --- player_movement: 스페이스 대기 ---

    #[test]
    fn 스페이스바를_누르면_제자리_대기로_턴을_소비한다() {
        let mut h = MoveHarness::new(20, 20);
        // 홀드 상태와 경로를 채워두고 스페이스가 둘 다 비우는지 확인
        h.app.world.resource_mut::<MoveHoldState>().dir = IVec2::new(1, 0);
        h.app.world.resource_mut::<MoveHoldState>().elapsed = 0.5;
        h.app.world.resource_mut::<PlayerPath>().0.push_back((6, 5));
        h.press(KeyCode::Space);
        h.update();
        assert!(!h.moving(), "대기는 이동을 일으키지 않는다");
        assert_eq!(h.app.world.resource::<MoveHoldState>().dir, IVec2::ZERO);
        assert_eq!(h.path_len(), 0, "대기 시 자동 경로가 비워져야 한다");
        assert_eq!(h.acted_count(), 1, "대기는 턴 한 번을 소비한다");
    }

    // --- player_movement: 키보드 8방향(|| 분기 양쪽) ---

    /// 단일 키를 막 눌러 한 칸 이동(또는 차단)을 검증하는 헬퍼.
    /// `target` 타일을 Floor 로 열어두고, 그 방향 키를 눌렀을 때 이동이
    /// 발생하는지 반환한다.
    fn move_once_with_key(key: KeyCode, target: (usize, usize)) -> bool {
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(target.0, target.1, TileKind::Floor);
        h.press(key);
        h.update();
        h.moving()
    }

    #[test]
    fn 왼쪽화살표로_왼쪽으로_이동한다() {
        assert!(move_once_with_key(KeyCode::ArrowLeft, (4, 5)));
    }
    #[test]
    fn a키로_왼쪽으로_이동한다() {
        assert!(move_once_with_key(KeyCode::KeyA, (4, 5)));
    }
    #[test]
    fn 오른쪽화살표로_오른쪽으로_이동한다() {
        assert!(move_once_with_key(KeyCode::ArrowRight, (6, 5)));
    }
    #[test]
    fn d키로_오른쪽으로_이동한다() {
        assert!(move_once_with_key(KeyCode::KeyD, (6, 5)));
    }
    #[test]
    fn 위쪽화살표로_위로_이동한다() {
        assert!(move_once_with_key(KeyCode::ArrowUp, (5, 6)));
    }
    #[test]
    fn w키로_위로_이동한다() {
        assert!(move_once_with_key(KeyCode::KeyW, (5, 6)));
    }
    #[test]
    fn 아래쪽화살표로_아래로_이동한다() {
        assert!(move_once_with_key(KeyCode::ArrowDown, (5, 4)));
    }
    #[test]
    fn s키로_아래로_이동한다() {
        assert!(move_once_with_key(KeyCode::KeyS, (5, 4)));
    }

    // --- player_movement: 이동 시 Facing 갱신 ---

    #[test]
    fn 키보드로_오른쪽으로_이동하면_facing이_오른쪽으로_갱신된다() {
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(6, 5, TileKind::Floor);
        h.press(KeyCode::ArrowRight);
        h.update();
        assert_eq!(h.facing(), Some(IVec2::new(1, 0)), "이동 방향으로 facing 갱신");
    }

    #[test]
    fn 키보드로_위로_이동하면_facing이_위로_갱신된다() {
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(5, 6, TileKind::Floor);
        h.press(KeyCode::ArrowUp);
        h.update();
        assert_eq!(h.facing(), Some(IVec2::new(0, 1)), "위로 이동 시 facing 갱신");
    }

    #[test]
    fn 이동이_차단되면_facing은_갱신되지_않는다() {
        // 벽으로 막혀 이동(MovingTo)이 없으면 Facing 컴포넌트도 붙지 않는다.
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(6, 5, TileKind::Wall);
        h.press(KeyCode::ArrowRight);
        h.update();
        assert!(!h.moving(), "벽이라 이동 없음");
        assert_eq!(h.facing(), None, "이동하지 않으면 facing 도 갱신되지 않는다");
    }

    #[test]
    fn 자동경로로_이동하면_그_방향으로_facing이_갱신된다() {
        let h = run_path_step(|h| {
            h.set_tile(6, 5, TileKind::Floor);
            h.app.world.resource_mut::<PlayerPath>().0.push_back((6, 5));
        });
        assert!(h.moving(), "자동 경로로 이동");
        assert_eq!(h.facing(), Some(IVec2::new(1, 0)), "자동 경로 이동 방향으로 facing 갱신");
    }

    // --- player_movement: 키보드 이동 시 몬스터/주민/경계/홀드 분기 ---

    #[test]
    fn 키보드로_몬스터타일에_부딪치면_공격이벤트를_보낸다() {
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(6, 5, TileKind::Floor);
        h.app.world.resource_mut::<MonsterTiles>().0.insert((6, 5));
        h.press(KeyCode::ArrowRight);
        h.update();
        assert!(!h.moving(), "공격은 이동을 만들지 않는다");
        assert_eq!(h.attack_targets(), vec![(6, 5)]);
        assert_eq!(h.acted_count(), 1);
    }

    #[test]
    fn 키보드로_장애물타일에_부딪치면_범프이벤트를_보낸다() {
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(6, 5, TileKind::Floor);
        h.app.world.resource_mut::<OccupiedTiles>().0.insert((6, 5));
        h.press(KeyCode::ArrowRight);
        h.update();
        assert!(!h.moving());
        assert_eq!(h.bump_targets(), vec![(6, 5)]);
        assert_eq!(h.acted_count(), 1);
    }

    #[test]
    fn 키보드로_맵_경계_밖으로는_이동하지_않는다() {
        // 플레이어를 (0,5) 에 두고 왼쪽으로 누르면 경계 밖이라 무시.
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(0, 5, TileKind::Floor);
        let pos = tile_to_world_coords(0, 5);
        h.app.world.entity_mut(h.player).get_mut::<Transform>().unwrap().translation
            = pos.extend(1.0);
        h.press(KeyCode::ArrowLeft);
        h.update();
        assert!(!h.moving(), "경계 밖으로는 이동하지 않아야 한다");
    }

    #[test]
    fn 대각입력이_상쇄되어_영벡터가_되면_이동하지_않는다() {
        // 좌/우를 동시에 누르면 dir.x 가 0 으로 상쇄되고 dir.y 도 0 → delta ZERO.
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(6, 5, TileKind::Floor);
        h.set_tile(4, 5, TileKind::Floor);
        h.press(KeyCode::ArrowLeft);
        h.press(KeyCode::ArrowRight);
        h.update();
        assert!(!h.moving(), "좌우 동시 입력은 상쇄되어 이동하지 않아야 한다");
    }

    #[test]
    fn 연속입력_홀드_지연_전에는_두번째_프레임에_이동하지_않는다() {
        // 첫 프레임에 just_pressed 로 한 칸 이동(MovingTo) → 두 번째 프레임은
        // MovingTo 가 붙어 query 가 비므로, 대신 hold 게이트를 직접 검증한다.
        // 여기서는 just_pressed 없이 pressed 만 유지된 두 번째 프레임을 만든다.
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(6, 5, TileKind::Floor);
        // 첫 프레임: 막 누름 → 이동
        h.press(KeyCode::ArrowRight);
        h.update();
        assert!(h.moving(), "첫 탭은 즉시 이동");
        // MovingTo 를 제거해 두 번째 프레임에서 query 가 살아있게 한다.
        h.app.world.entity_mut(h.player).remove::<MovingTo>();
        // 키 상태를 clear→press 하지 않고 그대로 두면 just_pressed=false, pressed=true
        h.app.world.resource_mut::<ButtonInput<KeyCode>>().clear();
        h.app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowRight);
        h.app.world.resource_mut::<ButtonInput<KeyCode>>().clear_just_pressed(KeyCode::ArrowRight);
        h.advance(0.0);
        h.update();
        assert!(!h.moving(), "초기 지연 안쪽 두 번째 프레임에는 이동하지 않아야 한다");
    }

    // --- player_movement: 자동 경로(마우스) 소비 분기 ---

    /// 자동 경로를 채우고 키 입력 없이 한 턴 진행하기 위한 헬퍼.
    /// hold 게이트를 통과시키려고 충분한 시간을 미리 흘려둔다.
    fn run_path_step(setup: impl FnOnce(&mut MoveHarness)) -> MoveHarness {
        let mut h = MoveHarness::new(20, 20);
        // 경로 자동 소비는 tick_hold(ONE,...) 게이트를 지나야 하므로 elapsed 를 채운다.
        h.app.world.resource_mut::<MoveHoldState>().dir = IVec2::ONE;
        h.app.world.resource_mut::<MoveHoldState>().elapsed = 1.0;
        setup(&mut h);
        h.advance(0.5);
        h.update();
        h
    }

    #[test]
    fn 자동경로가_비어있고_키입력도_없으면_아무것도_하지_않는다() {
        let mut h = MoveHarness::new(20, 20);
        h.advance(0.5);
        h.update();
        assert!(!h.moving());
        assert_eq!(h.acted_count(), 0);
    }

    #[test]
    fn 자동경로의_다음칸이_빈_바닥이면_그칸으로_이동한다() {
        let h = run_path_step(|h| {
            h.set_tile(6, 5, TileKind::Floor);
            h.app.world.resource_mut::<PlayerPath>().0.push_back((6, 5));
        });
        assert!(h.moving(), "자동 경로 다음 칸으로 이동해야 한다");
        assert_eq!(h.path_len(), 0, "소비된 칸은 경로에서 제거된다");
    }

    #[test]
    fn 자동경로의_다음칸에_몬스터가_있으면_공격하고_경로를_비운다() {
        let mut h = run_path_step(|h| {
            h.set_tile(6, 5, TileKind::Floor);
            h.app.world.resource_mut::<MonsterTiles>().0.insert((6, 5));
            h.app.world.resource_mut::<PlayerPath>().0.push_back((6, 5));
        });
        assert!(!h.moving());
        assert_eq!(h.attack_targets(), vec![(6, 5)]);
        assert_eq!(h.path_len(), 0, "공격 시 경로가 비워져야 한다");
        assert_eq!(h.acted_count(), 1);
    }

    #[test]
    fn 자동경로의_다음칸에_장애물이_있으면_범프하고_경로를_비운다() {
        let mut h = run_path_step(|h| {
            h.set_tile(6, 5, TileKind::Floor);
            h.app.world.resource_mut::<OccupiedTiles>().0.insert((6, 5));
            h.app.world.resource_mut::<PlayerPath>().0.push_back((6, 5));
        });
        assert!(!h.moving());
        assert_eq!(h.bump_targets(), vec![(6, 5)]);
        assert_eq!(h.path_len(), 0);
        assert_eq!(h.acted_count(), 1);
    }

    #[test]
    fn 자동경로_소비는_홀드_게이트를_지나기_전에는_대기한다() {
        // elapsed 를 0 으로 두면 tick_hold(ONE) 게이트를 통과하지 못해 대기.
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(6, 5, TileKind::Floor);
        h.app.world.resource_mut::<PlayerPath>().0.push_back((6, 5));
        h.advance(0.0);
        h.update();
        assert!(!h.moving(), "홀드 게이트 전에는 자동 이동하지 않아야 한다");
        assert_eq!(h.path_len(), 1, "경로가 그대로 남아 있어야 한다");
    }

    #[test]
    fn 자동경로_진행중_키를_누르면_경로가_취소된다() {
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(6, 5, TileKind::Floor);
        h.set_tile(4, 5, TileKind::Floor);
        h.app.world.resource_mut::<PlayerPath>().0.push_back((6, 5));
        h.press(KeyCode::ArrowLeft);
        h.update();
        assert_eq!(h.path_len(), 0, "키 입력 시 자동 경로가 취소돼야 한다");
    }

    // --- smooth_player_lerp ---

    fn lerp_harness() -> App {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.add_event::<PlayerActedEvent>();
        app.add_systems(Update, smooth_player_lerp);
        app
    }

    #[test]
    fn lerp는_목표에_충분히_가까우면_스냅하고_moving을_해제한다() {
        let mut app = lerp_harness();
        let target = Vec3::new(100.0, 0.0, 1.0);
        // 목표 바로 근처에서 시작 → 한 스텝 안에 도달
        let e = app.world.spawn((
            Player,
            Transform::from_xyz(99.99, 0.0, 1.0),
            MovingTo { target },
        )).id();
        app.world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.5));
        app.update();
        assert_eq!(app.world.get::<Transform>(e).unwrap().translation, target, "목표로 스냅");
        assert!(!app.world.entity(e).contains::<MovingTo>(), "도착 시 MovingTo 제거");
        let events = app.world.resource::<Events<PlayerActedEvent>>();
        assert_eq!(events.get_reader().read(events).count(), 1, "도착 시 행동 이벤트 1회");
    }

    #[test]
    fn lerp는_목표가_멀면_방향으로_조금씩_나아간다() {
        let mut app = lerp_harness();
        let start = Vec3::new(0.0, 0.0, 1.0);
        let target = Vec3::new(1000.0, 0.0, 1.0);
        let e = app.world.spawn((Player, Transform::from_translation(start), MovingTo { target })).id();
        app.world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.016));
        app.update();
        let t = app.world.get::<Transform>(e).unwrap().translation;
        assert!(t.x > 0.0, "목표를 향해 전진해야 한다: {}", t.x);
        assert!(t.x < 1000.0, "한 프레임에 목표를 넘지 않아야 한다: {}", t.x);
        assert!(app.world.entity(e).contains::<MovingTo>(), "아직 도착 전이라 MovingTo 유지");
    }

    // --- respawn_player_on_regen ---

    #[test]
    fn 리스폰이벤트를_받으면_플레이어를_옮기고_상태를_정리한다() {
        let mut app = App::new();
        app.add_event::<PlayerRespawnEvent>();
        app.init_resource::<PlayerPath>();
        app.add_systems(Update, respawn_player_on_regen);
        let e = app.world.spawn((
            Player,
            Transform::from_xyz(0.0, 0.0, 1.0),
            MovingTo { target: Vec3::ZERO },
        )).id();
        app.world.resource_mut::<PlayerPath>().0.push_back((1, 1));
        app.world.send_event(PlayerRespawnEvent(7, 8));
        app.update();
        let expected = tile_to_world_coords(7, 8);
        let t = app.world.get::<Transform>(e).unwrap().translation;
        assert_eq!(t, Vec3::new(expected.x, expected.y, 1.0), "리스폰 위치로 이동");
        assert!(!app.world.entity(e).contains::<MovingTo>(), "MovingTo 제거");
        assert_eq!(app.world.resource::<PlayerPath>().0.len(), 0, "자동 경로 정리");
    }

    #[test]
    fn 리스폰이벤트가_없으면_플레이어를_옮기지_않는다() {
        let mut app = App::new();
        app.add_event::<PlayerRespawnEvent>();
        app.init_resource::<PlayerPath>();
        app.add_systems(Update, respawn_player_on_regen);
        let e = app.world.spawn((Player, Transform::from_xyz(3.0, 4.0, 1.0))).id();
        app.update();
        assert_eq!(app.world.get::<Transform>(e).unwrap().translation, Vec3::new(3.0, 4.0, 1.0));
    }

    #[test]
    fn 리스폰_대상_플레이어가_없으면_이벤트를_조용히_무시한다() {
        // 플레이어 엔티티가 없으면 get_single_mut 가 Err → if let 의 else 경로.
        let mut app = App::new();
        app.add_event::<PlayerRespawnEvent>();
        app.init_resource::<PlayerPath>();
        app.add_systems(Update, respawn_player_on_regen);
        app.world.send_event(PlayerRespawnEvent(1, 1));
        app.update(); // 패닉하지 않으면 통과
    }

    // --- camera_follow_player ---

    /// 카메라 추적 하네스. 플레이어 위치와 카메라 viewport 영역을 지정한다.
    fn run_camera_follow(player_world: Vec3, half_w: f32, half_h: f32) -> (f32, f32) {
        let mut app = App::new();
        app.add_systems(Update, camera_follow_player);
        app.world.spawn((Player, Transform::from_translation(player_world)));
        let mut proj = OrthographicProjection::default();
        proj.area = Rect2d_area(half_w, half_h);
        let cam = app.world.spawn((
            Camera::default(),
            Transform::default(),
            proj,
        )).id();
        app.update();
        let t = app.world.get::<Transform>(cam).unwrap().translation;
        (t.x, t.y)
    }

    fn Rect2d_area(half_w: f32, half_h: f32) -> bevy::math::Rect {
        bevy::math::Rect {
            min: Vec2::new(-half_w, -half_h),
            max: Vec2::new(half_w, half_h),
        }
    }

    #[test]
    fn 카메라는_맵보다_좁은_뷰포트면_플레이어를_따라_클램프한다() {
        use crate::modules::map::{MAP_WIDTH, MAP_HEIGHT, TILE_SIZE};
        let map_w = MAP_WIDTH as f32 * TILE_SIZE;
        let map_h = MAP_HEIGHT as f32 * TILE_SIZE;
        // 뷰포트가 맵보다 작음(half < map/2). 플레이어를 맵 중앙 근처에 두면
        // 클램프 안쪽이라 플레이어 좌표를 그대로 따라간다.
        let half_w = map_w / 4.0;
        let half_h = map_h / 4.0;
        let (cx, cy) = run_camera_follow(Vec3::new(50.0, 30.0, 0.0), half_w, half_h);
        assert!((cx - 50.0).abs() < 1e-3, "x 는 플레이어를 따라가야 한다: {cx}");
        assert!((cy - 30.0).abs() < 1e-3, "y 는 플레이어를 따라가야 한다: {cy}");
    }

    #[test]
    fn 카메라는_맵_가장자리에서_뷰포트가_맵밖으로_나가지_않게_클램프한다() {
        use crate::modules::map::{MAP_WIDTH, MAP_HEIGHT, TILE_SIZE};
        let map_w = MAP_WIDTH as f32 * TILE_SIZE;
        let map_h = MAP_HEIGHT as f32 * TILE_SIZE;
        let half_w = map_w / 4.0;
        let half_h = map_h / 4.0;
        // 플레이어를 맵 오른쪽 끝 너머로 두면 카메라 x 는 최대 클램프 값에 고정.
        let (cx, _cy) = run_camera_follow(Vec3::new(map_w, map_h, 0.0), half_w, half_h);
        let max_x = map_w / 2.0 - half_w;
        assert!((cx - max_x).abs() < 1e-3, "오른쪽 경계에서 클램프되어야 한다: {cx} vs {max_x}");
    }

    #[test]
    fn 카메라는_뷰포트가_맵보다_크면_중앙_0_0에_고정된다() {
        use crate::modules::map::{MAP_WIDTH, MAP_HEIGHT, TILE_SIZE};
        let map_w = MAP_WIDTH as f32 * TILE_SIZE;
        let map_h = MAP_HEIGHT as f32 * TILE_SIZE;
        // 뷰포트가 맵보다 큼(half*2 >= map) → x, y 모두 0 고정.
        let (cx, cy) = run_camera_follow(Vec3::new(123.0, 456.0, 0.0), map_w, map_h);
        assert_eq!((cx, cy), (0.0, 0.0), "뷰포트가 맵보다 크면 (0,0) 고정");
    }

    #[test]
    fn 카메라_추적은_플레이어가_없으면_아무것도_하지_않는다() {
        let mut app = App::new();
        app.add_systems(Update, camera_follow_player);
        let cam = app.world.spawn((Camera::default(), Transform::from_xyz(9.0, 9.0, 0.0), OrthographicProjection::default())).id();
        app.update();
        assert_eq!(app.world.get::<Transform>(cam).unwrap().translation, Vec3::new(9.0, 9.0, 0.0));
    }

    #[test]
    fn 카메라_추적은_카메라가_없으면_아무것도_하지_않는다() {
        let mut app = App::new();
        app.add_systems(Update, camera_follow_player);
        app.world.spawn((Player, Transform::from_xyz(1.0, 2.0, 0.0)));
        app.update(); // 패닉하지 않으면 통과
    }

    // --- update_fov ---

    /// FOV 하네스: 맵·플레이어·facing 을 두고 update_fov 한 번 돌린 뒤 맵을 돌려준다.
    fn run_fov_facing(map: Map, player_tile: (usize, usize), facing: IVec2) -> Map {
        let mut app = App::new();
        app.insert_resource(MapResource(map));
        let pos = tile_to_world_coords(player_tile.0, player_tile.1);
        app.world.spawn((Player, Transform::from_xyz(pos.x, pos.y, 1.0), Facing(facing)));
        app.add_systems(Update, update_fov);
        app.update();
        app.world.remove_resource::<MapResource>().unwrap().0
    }

    /// 방향이 결과에 영향을 주지 않는 회귀 테스트용 — facing 0(전방향 원형) 으로 돈다.
    fn run_fov(map: Map, player_tile: (usize, usize)) -> Map {
        run_fov_facing(map, player_tile, IVec2::ZERO)
    }

    #[test]
    fn fov는_플레이어_주변_바닥을_가시화하고_드러낸다() {
        let mut map = Map::new(30, 30);
        for y in 5..15 { for x in 5..15 { map.set_tile(x, y, TileKind::Floor); } }
        let out = run_fov(map, (10, 10));
        let idx = out.index(10, 10);
        assert!(out.tiles[idx].visible, "플레이어 위치는 보여야 한다");
        assert!(out.tiles[idx].revealed, "플레이어 위치는 드러나야 한다");
        let near = out.index(12, 10);
        assert!(out.tiles[near].visible, "가까운 바닥은 보여야 한다");
    }

    #[test]
    fn fov는_물타일_너머도_시야가_통과한다() {
        // 플레이어와 목표 사이에 Water 한 칸을 두어도 시야가 통과해야 한다.
        let mut map = Map::new(30, 30);
        for x in 5..15 { map.set_tile(x, 10, TileKind::Floor); }
        map.set_tile(10, 10, TileKind::Floor); // 플레이어
        map.set_tile(11, 10, TileKind::Water); // 물
        map.set_tile(12, 10, TileKind::Floor); // 물 너머
        // 오른쪽(+x)을 보게 해 (12,10) 이 정면 반경 안에 들도록.
        let out = run_fov_facing(map, (10, 10), IVec2::new(1, 0));
        let beyond = out.index(12, 10);
        assert!(out.tiles[beyond].visible, "물 너머 타일이 보여야 한다(물은 시야를 막지 않음)");
    }

    #[test]
    fn fov는_벽_너머는_시야를_차단한다() {
        let mut map = Map::new(30, 30);
        for x in 5..15 { map.set_tile(x, 10, TileKind::Floor); }
        map.set_tile(10, 10, TileKind::Floor);
        map.set_tile(11, 10, TileKind::Wall);  // 벽
        map.set_tile(12, 10, TileKind::Floor); // 벽 너머
        let out = run_fov_facing(map, (10, 10), IVec2::new(1, 0));
        let beyond = out.index(12, 10);
        assert!(!out.tiles[beyond].visible, "벽 너머 타일은 보이지 않아야 한다");
    }

    #[test]
    fn fov는_바라보는_정면은_멀리_보이고_등_뒤는_가깝게만_보인다() {
        // 플레이어(15,15)가 오른쪽(+x)을 본다. 정면 FOV_FRONT(8)칸 타일은 보이지만,
        // 등 뒤로 FOV_BACK(3) 을 넘는 타일(왼쪽 5칸)은 보이지 않아야 한다.
        let mut map = Map::new(40, 40);
        for y in 0..40 { for x in 0..40 { map.set_tile(x, y, TileKind::Floor); } }
        let out = run_fov_facing(map, (15, 15), IVec2::new(1, 0));
        let front_far = out.index(15 + FOV_FRONT as usize, 15); // 정면 8칸
        assert!(out.tiles[front_far].visible, "정면 FOV_FRONT 거리 타일은 보여야 한다");
        let back_far = out.index(15 - (FOV_BACK as usize + 2), 15); // 등 뒤 5칸
        assert!(!out.tiles[back_far].visible, "등 뒤 FOV_BACK 초과 타일은 보이지 않아야 한다");
        let back_near = out.index(15 - FOV_BACK as usize, 15); // 등 뒤 3칸
        assert!(out.tiles[back_near].visible, "등 뒤라도 FOV_BACK 이내는 보여야 한다");
    }

    #[test]
    fn fov는_같은_위치에서_다시_돌리면_재계산을_건너뛴다() {
        // last_pos 가 같으면 early return → 맵을 건드리지 않는다.
        // 시스템이 재계산하면 map_mut() 로 MapResource 가 changed 되므로,
        // is_resource_changed 로 "재계산을 건너뛰었는지" 를 관측한다.
        let mut map = Map::new(30, 30);
        for y in 5..15 { for x in 5..15 { map.set_tile(x, y, TileKind::Floor); } }
        let mut app = App::new();
        app.insert_resource(MapResource(map));
        let pos = tile_to_world_coords(10, 10);
        app.world.spawn((Player, Transform::from_xyz(pos.x, pos.y, 1.0), Facing::default()));
        app.add_systems(Update, update_fov);
        app.update(); // 1회차: 계산 + last_pos=((10,10), facing)
        app.world.clear_trackers(); // 변경 추적 리셋
        app.update(); // 2회차: 같은 위치+같은 facing → early return (맵 미변경)
        assert!(!app.world.is_resource_changed::<MapResource>(),
            "같은 위치·방향이면 재계산을 건너뛰어 맵을 건드리지 않아야 한다");
    }

    #[test]
    fn fov는_제자리에서_방향만_바뀌어도_시야를_재계산한다() {
        // 위치는 그대로지만 facing 이 바뀌면 last_pos 키가 달라져 재계산한다.
        let mut map = Map::new(40, 40);
        for y in 0..40 { for x in 0..40 { map.set_tile(x, y, TileKind::Floor); } }
        let mut app = App::new();
        app.insert_resource(MapResource(map));
        let pos = tile_to_world_coords(15, 15);
        let player = app.world.spawn((Player, Transform::from_xyz(pos.x, pos.y, 1.0), Facing(IVec2::new(1, 0)))).id();
        app.add_systems(Update, update_fov);
        app.update(); // 오른쪽을 봄 → 오른쪽 먼 타일이 보임
        let right_far = app.world.resource::<MapResource>().map().index(15 + FOV_FRONT as usize, 15);
        assert!(app.world.resource::<MapResource>().map().tiles[right_far].visible, "처음엔 오른쪽 정면이 보임");
        // 왼쪽으로 돌면 오른쪽 먼 타일은 등 뒤가 되어 안 보여야 한다.
        app.world.get_mut::<Facing>(player).unwrap().0 = IVec2::new(-1, 0);
        app.update();
        assert!(!app.world.resource::<MapResource>().map().tiles[right_far].visible,
            "방향을 반대로 돌리면 이전 정면(이제 등 뒤 먼 곳)은 보이지 않아야 한다");
    }

    #[test]
    fn fov는_맵이_바뀌면_같은_위치여도_재계산을_강제한다() {
        // is_changed() 분기: MapResource 를 외부에서 변경하면 last_pos 가
        // None 으로 리셋되어 같은 위치여도 재계산한다.
        // 재계산은 모든 tile.visible 을 먼저 false 로 만들므로, 외부에서 켜둔
        // 먼 타일의 visible 이 false 로 바뀌는지로 재계산을 확인한다.
        let mut map = Map::new(30, 30);
        for y in 5..15 { for x in 5..15 { map.set_tile(x, y, TileKind::Floor); } }
        let mut app = App::new();
        app.insert_resource(MapResource(map));
        let pos = tile_to_world_coords(10, 10);
        app.world.spawn((Player, Transform::from_xyz(pos.x, pos.y, 1.0), Facing::default()));
        app.add_systems(Update, update_fov);
        app.update();
        let far = {
            // 최대 반경 밖 먼 타일을 visible=true 로 켜두고 MapResource 를 changed 로 만든다.
            let mut mr = app.world.resource_mut::<MapResource>();
            let idx = mr.map().index(29, 29);
            mr.map_mut().tiles[idx].visible = true;
            idx
        };
        app.update(); // is_changed → last_pos=None → 재계산 → 먼 타일 visible=false
        let mr = app.world.resource::<MapResource>();
        assert!(!mr.map().tiles[far].visible,
            "맵 변경 시 재계산되어 반경 밖 타일의 visible 이 꺼져야 한다");
    }

    #[test]
    fn fov는_플레이어가_없으면_아무것도_하지_않는다() {
        let mut app = App::new();
        let mut map = Map::new(30, 30);
        map.set_tile(10, 10, TileKind::Floor);
        app.insert_resource(MapResource(map));
        app.add_systems(Update, update_fov);
        app.update(); // 패닉하지 않으면 통과
    }

    #[test]
    fn fov는_맵_경계_근처에서_범위_밖_좌표를_건너뛴다() {
        // 플레이어를 (0,0) 에 두면 반경 8 스캔이 음수 좌표를 포함해
        // x<0||y<0 분기를 타며, 작은 맵이라 width 초과 분기도 탄다.
        let mut map = Map::new(5, 5);
        for y in 0..5 { for x in 0..5 { map.set_tile(x, y, TileKind::Floor); } }
        let out = run_fov(map, (0, 0));
        let idx = out.index(0, 0);
        assert!(out.tiles[idx].visible, "경계 코너에서도 자기 위치는 보여야 한다");
    }

    // --- update_player_bars ---

    /// 상태바 하네스: 플레이어(CombatStats)와 HP/MP 자식 스프라이트를 만든다.
    fn run_player_bars(stats: CombatStats) -> (Vec2, Vec2) {
        let mut app = App::new();
        app.add_systems(Update, update_player_bars);
        let hp = app.world.spawn((
            Sprite { custom_size: Some(Vec2::new(BAR_WIDTH, BAR_HEIGHT)), ..default() },
            HpBarFill,
        )).id();
        let mp = app.world.spawn((
            Sprite { custom_size: Some(Vec2::new(BAR_WIDTH, BAR_HEIGHT)), ..default() },
            MpBarFill,
        )).id();
        app.world.spawn((Player, stats));
        app.update();
        let hp_size = app.world.get::<Sprite>(hp).unwrap().custom_size.unwrap();
        let mp_size = app.world.get::<Sprite>(mp).unwrap().custom_size.unwrap();
        (hp_size, mp_size)
    }

    #[test]
    fn 상태바는_hp_mp_비율에_맞게_너비를_조정한다() {
        let (hp, mp) = run_player_bars(CombatStats {
            hp: 15, max_hp: 30, mp: 10, max_mp: 20, attack: 5, defense: 1,
        });
        assert!((hp.x - BAR_WIDTH * 0.5).abs() < 1e-3, "HP 50% 너비: {}", hp.x);
        assert!((mp.x - BAR_WIDTH * 0.5).abs() < 1e-3, "MP 50% 너비: {}", mp.x);
    }

    #[test]
    fn 상태바는_max_mp가_0이면_mp_너비를_0으로_둔다() {
        let (_hp, mp) = run_player_bars(CombatStats {
            hp: 30, max_hp: 30, mp: 0, max_mp: 0, attack: 5, defense: 1,
        });
        assert_eq!(mp.x, 0.0, "max_mp 가 0 이면 MP 바 너비는 0");
    }

    #[test]
    fn 상태바는_스탯이_변하지_않은_플레이어가_없으면_갱신하지_않는다() {
        // Changed<CombatStats> 가 없는(=두 번째 프레임) 상태에서는 early return.
        let mut app = App::new();
        app.add_systems(Update, update_player_bars);
        app.world.spawn((Player, CombatStats {
            hp: 10, max_hp: 30, mp: 5, max_mp: 20, attack: 5, defense: 1,
        }));
        app.update(); // 1회차: Changed 감지 → 갱신
        // HP/MP 바 엔티티 없이 두 번째 프레임: get_single 은 Changed 없어 Err → early return
        app.update();
        // 패닉 없이 통과하면 OK
    }

    #[test]
    fn 상태바는_hp바만_있고_mp바가_없어도_안전하다() {
        let mut app = App::new();
        app.add_systems(Update, update_player_bars);
        app.world.spawn((
            Sprite { custom_size: Some(Vec2::new(BAR_WIDTH, BAR_HEIGHT)), ..default() },
            HpBarFill,
        ));
        app.world.spawn((Player, CombatStats {
            hp: 10, max_hp: 30, mp: 5, max_mp: 20, attack: 5, defense: 1,
        }));
        app.update(); // mp_query.get_single_mut() 이 Err 인 경로
    }

    // --- plan_click_path (마우스 결정 로직 순수 함수) ---

    fn click_map() -> Map {
        let mut map = Map::new(20, 20);
        for y in 4..8 { for x in 4..8 { map.set_tile(x, y, TileKind::Floor); } }
        map
    }

    #[test]
    fn 클릭경로계산은_viewport_변환_실패시_경로를_만들지_않는다() {
        let map = click_map();
        let player_world = tile_to_world_coords(5, 5).extend(1.0);
        assert!(plan_click_path(None, &map, player_world).is_none(),
            "viewport 변환 실패(None)면 경로를 만들지 않아야 한다");
    }

    #[test]
    fn 클릭경로계산은_목적지가_바닥이면_경로를_반환한다() {
        let map = click_map();
        let player_world = tile_to_world_coords(5, 5).extend(1.0);
        let target = tile_to_world_coords(7, 5); // Floor
        let path = plan_click_path(Some(target), &map, player_world).expect("바닥 클릭은 경로를 만든다");
        assert!(!path.is_empty(), "바닥 클릭이면 경로가 비어있지 않아야 한다");
        assert_eq!(*path.back().unwrap(), (7, 5), "경로 끝은 클릭한 타일");
    }

    #[test]
    fn 클릭경로계산은_목적지가_벽이면_경로를_만들지_않는다() {
        let map = click_map(); // (0,0) 등은 기본 Wall
        let player_world = tile_to_world_coords(5, 5).extend(1.0);
        let target = tile_to_world_coords(0, 0); // Wall
        assert!(plan_click_path(Some(target), &map, player_world).is_none(),
            "벽 클릭은 경로를 만들지 않아야 한다");
    }

    #[test]
    fn 클릭경로계산은_목적지가_물이면_경로를_만들지_않는다() {
        let mut map = click_map();
        map.set_tile(7, 5, TileKind::Water);
        let player_world = tile_to_world_coords(5, 5).extend(1.0);
        let target = tile_to_world_coords(7, 5); // Water
        assert!(plan_click_path(Some(target), &map, player_world).is_none(),
            "물 클릭은 이동 불가라 경로를 만들지 않아야 한다");
    }

    #[test]
    fn 클릭경로계산은_목적지가_모래면_경로를_반환한다() {
        let mut map = click_map();
        map.set_tile(7, 5, TileKind::Sand);
        let player_world = tile_to_world_coords(5, 5).extend(1.0);
        let target = tile_to_world_coords(7, 5); // Sand
        let path = plan_click_path(Some(target), &map, player_world).expect("모래 클릭은 경로를 만든다");
        assert_eq!(*path.back().unwrap(), (7, 5));
    }

    // --- on_mouse_click 시스템: 가드 분기 (App 하네스) ---

    /// on_mouse_click 하네스. 윈도우/카메라/플레이어를 선택적으로 둔다.
    /// viewport_to_world_2d 는 헤드리스에서 항상 None 이라 경로가 채워지지는
    /// 않지만, 클릭 이전의 모든 가드 분기를 양방향으로 통과시킨다.
    struct ClickHarness { app: App }
    impl ClickHarness {
        fn new() -> Self {
            let mut app = App::new();
            app.insert_resource(ButtonInput::<MouseButton>::default());
            app.insert_resource(MapResource(click_map()));
            app.init_resource::<PlayerPath>();
            app.insert_resource(EquipmentPanelOpen(false));
            app.insert_resource(ShopPanelOpen(false));
            app.insert_resource(HelpPanelOpen(false));
            app.init_resource::<RangedTargeting>();
            app.add_systems(Update, on_mouse_click);
            Self { app }
        }
        fn spawn_window_with_cursor(&mut self, cursor: Option<Vec2>) {
            let mut window = Window::default();
            window.resolution.set(800.0, 600.0);
            if let Some(c) = cursor {
                window.set_physical_cursor_position(Some(bevy::math::DVec2::new(c.x as f64, c.y as f64)));
            }
            self.app.world.spawn(window);
        }
        fn spawn_camera(&mut self) {
            self.app.world.spawn((Camera::default(), GlobalTransform::default()));
        }
        fn spawn_player(&mut self) {
            let pos = tile_to_world_coords(5, 5);
            self.app.world.spawn((Player, Transform::from_xyz(pos.x, pos.y, 1.0)));
        }
        fn click(&mut self) {
            self.app.world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        }
        fn update(&mut self) { self.app.update(); }
    }

    #[test]
    fn 마우스_클릭이_없으면_경로처리를_하지_않는다() {
        let mut h = ClickHarness::new();
        h.spawn_window_with_cursor(Some(Vec2::new(400.0, 300.0)));
        h.spawn_camera();
        h.spawn_player();
        h.update(); // 클릭 안 함
        assert_eq!(h.app.world.resource::<PlayerPath>().0.len(), 0);
    }

    #[test]
    fn 마우스클릭은_장비패널이_열려있으면_무시된다() {
        let mut h = ClickHarness::new();
        h.app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        h.spawn_window_with_cursor(Some(Vec2::new(400.0, 300.0)));
        h.spawn_camera();
        h.spawn_player();
        h.click();
        h.update();
        assert_eq!(h.app.world.resource::<PlayerPath>().0.len(), 0);
    }

    #[test]
    fn 마우스클릭은_상점패널이_열려있으면_무시된다() {
        let mut h = ClickHarness::new();
        h.app.world.resource_mut::<ShopPanelOpen>().0 = true;
        h.spawn_window_with_cursor(Some(Vec2::new(400.0, 300.0)));
        h.spawn_camera();
        h.spawn_player();
        h.click();
        h.update();
        assert_eq!(h.app.world.resource::<PlayerPath>().0.len(), 0);
    }

    #[test]
    fn 마우스클릭은_도움말패널이_열려있으면_무시된다() {
        let mut h = ClickHarness::new();
        h.app.world.resource_mut::<HelpPanelOpen>().0 = true;
        h.spawn_window_with_cursor(Some(Vec2::new(400.0, 300.0)));
        h.spawn_camera();
        h.spawn_player();
        h.click();
        h.update();
        assert_eq!(h.app.world.resource::<PlayerPath>().0.len(), 0);
    }

    #[test]
    fn 마우스클릭은_원격모드_활성중이면_무시된다() {
        let mut h = ClickHarness::new();
        h.app.world.resource_mut::<RangedTargeting>().active = true;
        h.spawn_window_with_cursor(Some(Vec2::new(400.0, 300.0)));
        h.spawn_camera();
        h.spawn_player();
        h.click();
        h.update();
        assert_eq!(h.app.world.resource::<PlayerPath>().0.len(), 0);
    }

    #[test]
    fn 마우스클릭은_윈도우가_없으면_무시된다() {
        let mut h = ClickHarness::new();
        h.spawn_camera();
        h.spawn_player();
        h.click();
        h.update(); // 윈도우 없음 → get_single Err
        assert_eq!(h.app.world.resource::<PlayerPath>().0.len(), 0);
    }

    #[test]
    fn 마우스클릭은_카메라가_없으면_무시된다() {
        let mut h = ClickHarness::new();
        h.spawn_window_with_cursor(Some(Vec2::new(400.0, 300.0)));
        h.spawn_player();
        h.click();
        h.update(); // 카메라 없음
        assert_eq!(h.app.world.resource::<PlayerPath>().0.len(), 0);
    }

    #[test]
    fn 마우스클릭은_플레이어가_없으면_무시된다() {
        let mut h = ClickHarness::new();
        h.spawn_window_with_cursor(Some(Vec2::new(400.0, 300.0)));
        h.spawn_camera();
        h.click();
        h.update(); // 플레이어 없음
        assert_eq!(h.app.world.resource::<PlayerPath>().0.len(), 0);
    }

    #[test]
    fn 마우스클릭은_커서_위치가_없으면_무시된다() {
        let mut h = ClickHarness::new();
        h.spawn_window_with_cursor(None); // 커서 위치 없음
        h.spawn_camera();
        h.spawn_player();
        h.click();
        h.update(); // cursor_position None → 무시
        assert_eq!(h.app.world.resource::<PlayerPath>().0.len(), 0);
    }

    #[test]
    fn 마우스클릭은_모든_가드를_통과해도_헤드리스에서는_viewport변환이_실패한다() {
        // 윈도우/카메라/플레이어/커서가 모두 있어 가드를 통과하지만,
        // viewport_to_world_2d 는 헤드리스에서 None 이라 경로가 채워지지 않는다.
        let mut h = ClickHarness::new();
        h.spawn_window_with_cursor(Some(Vec2::new(400.0, 300.0)));
        h.spawn_camera();
        h.spawn_player();
        h.click();
        h.update();
        assert_eq!(h.app.world.resource::<PlayerPath>().0.len(), 0,
            "헤드리스에서는 viewport 변환이 실패해 경로가 비어 있다");
    }

    // --- spawn_player ---

    /// AssetServer 가 필요한 spawn_player 를 돌리기 위한 하네스.
    fn run_spawn_player(with_room: bool) -> (App, usize) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app.init_asset::<Image>();

        let mut map = Map::new(40, 40);
        for y in 0..40 { for x in 0..40 { map.set_tile(x, y, TileKind::Floor); } }
        if with_room {
            map.rooms.push(Rect::new(5, 5, 6, 6));
        }
        app.insert_resource(MapResource(map));
        app.add_systems(Update, spawn_player);
        app.update();
        let count = app.world.query_filtered::<Entity, With<Player>>().iter(&app.world).count();
        (app, count)
    }

    #[test]
    fn 스폰은_첫_방의_중앙에_플레이어를_만든다() {
        let (mut app, count) = run_spawn_player(true);
        assert_eq!(count, 1, "플레이어 한 명이 스폰돼야 한다");
        // 첫 방 (5,5)~(11,11) 중앙 = (8,8)
        let expected = tile_to_world_coords(8, 8);
        let t = app.world.query_filtered::<&Transform, With<Player>>()
            .iter(&app.world).next().unwrap().translation;
        assert!((t.x - expected.x).abs() < 1e-3, "첫 방 중앙 x: {:?} vs {:?}", t, expected);
        assert!((t.y - expected.y).abs() < 1e-3, "첫 방 중앙 y: {:?} vs {:?}", t, expected);
    }

    #[test]
    fn 스폰은_방이_없으면_맵_중앙에_플레이어를_만든다() {
        use crate::modules::map::{MAP_WIDTH, MAP_HEIGHT};
        let (mut app, count) = run_spawn_player(false);
        assert_eq!(count, 1);
        let expected = tile_to_world_coords(MAP_WIDTH / 2, MAP_HEIGHT / 2);
        let t = app.world.query_filtered::<&Transform, With<Player>>()
            .iter(&app.world).next().unwrap().translation;
        assert!((t.x - expected.x).abs() < 1e-3, "방 없을 때 맵 중앙 x");
        assert!((t.y - expected.y).abs() < 1e-3, "방 없을 때 맵 중앙 y");
    }

    #[test]
    fn 스폰된_플레이어는_hp_mp_상태바_자식을_가진다() {
        let (mut app, _) = run_spawn_player(true);
        let hp = app.world.query_filtered::<Entity, With<HpBarFill>>().iter(&app.world).count();
        let mp = app.world.query_filtered::<Entity, With<MpBarFill>>().iter(&app.world).count();
        assert_eq!(hp, 1, "HP 바 전경 1개");
        assert_eq!(mp, 1, "MP 바 전경 1개");
    }

    #[test]
    fn 스폰된_플레이어는_기본_전투스탯을_가진다() {
        let (mut app, _) = run_spawn_player(true);
        let stats = app.world.query_filtered::<&CombatStats, With<Player>>()
            .iter(&app.world).next().unwrap();
        assert_eq!(stats.hp, PLAYER_HP);
        assert_eq!(stats.max_mp, PLAYER_MP);
        assert_eq!(stats.attack, PLAYER_ATK);
        assert_eq!(stats.defense, PLAYER_DEF);
    }

    // --- PlayerPlugin::build ---

    #[test]
    fn 플레이어_플러그인은_빌드시_리소스와_시스템을_등록한다() {
        let mut app = App::new();
        app.add_plugins(PlayerPlugin);
        assert!(app.world.contains_resource::<MoveHoldState>(), "MoveHoldState 등록");
        assert!(app.world.contains_resource::<PlayerPath>(), "PlayerPath 등록");
        assert!(app.world.contains_resource::<PlayerProgress>(), "PlayerProgress 등록");
    }
}

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MoveHoldState>()
            .init_resource::<PlayerPath>()
            .init_resource::<PlayerProgress>()
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
