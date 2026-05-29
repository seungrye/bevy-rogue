use crate::modules::{
    map::{
        draw_map, Map, MapResource, OccupiedTiles, MonsterTiles,
        tile_to_world_coords, world_to_tile_coords, is_in_view, is_interactable_tile, FOV_FRONT, FOV_BACK,
        MAP_HEIGHT, MAP_WIDTH, TILE_SIZE,
        MapSystemSet, PlayerRespawnEvent, PlayerActedEvent, BumpTileEvent, AttackMonsterEvent,
        GlobalTurn,
    },
    combat::{CombatStats, Defeated, Speed},
    item::EquipmentPanelOpen,
    ui::{help::HelpPanelOpen, shop::ShopPanelOpen, guide_panel::GuidePanelOpen},
    elemental::ElementalStatus,
    villager::{Villager, VillagerSystemSet},
    lighting::{LightMap, LightLevel, distance_falloff_alpha, memory_fade_factor, DARK_DIM_FACTOR},
};
use bevy::input::touch::Touches;
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

/// 마우스 좌클릭으로 시작된 자동 상호작용 대상.
///
/// - `Npc(entity)`: 클릭 타일에 villager 가 있을 때. 매 턴 그 villager 의 현재
///   위치로 경로를 재계산하다가 체비쇼프 거리 1 이내가 되면 자동으로 범프해
///   기존 `handle_bump` 흐름(말 걸기/상점/대화)으로 연결한다. villager 가
///   despawn 되면 follow 가 안전하게 종료된다.
/// - `Tile(x, y)`: 클릭 타일이 카운터 같은 interactable 비-walkable 타일일 때.
///   인접한 walkable 타일 중 가장 가까운 곳으로 경로를 깐 뒤, 도착(또는 인접
///   체비쇼프 1) 시 자동으로 `BumpTileEvent(클릭타일)` 를 발행한다. 카운터는
///   움직이지 않으므로 단발 — 재계산 불필요.
/// - `None`: 평범한 walkable 빈 타일 클릭(=기존 단순 이동) 또는 follow 비활성.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractTarget {
    Npc(Entity),
    Tile(usize, usize),
}

#[derive(Resource, Default)]
pub struct MouseInteractTarget(pub Option<InteractTarget>);

/// 두 타일 사이 체비쇼프(8방향) 거리.
///
/// 인접 판정에 사용 — 거리 1 이면 카디널·대각 8방향 중 하나에 닿아 있어
/// 범프(말 걸기/카운터 상호작용)가 가능한 거리다.
pub fn chebyshev_distance(a: (usize, usize), b: (usize, usize)) -> i32 {
    let (ax, ay) = (a.0 as i32, a.1 as i32);
    let (bx, by) = (b.0 as i32, b.1 as i32);
    (ax - bx).abs().max((ay - by).abs())
}

/// 비-walkable 타일(`target_tile`, 예: 카운터)에 인접한 walkable 8-이웃 중
/// `from_tile` 에서 BFS 가장 짧은 경로로 도달 가능한 타일 하나를 돌려준다.
///
/// 후보가 없거나 모두 도달 불가면 `None`. 두 후보의 BFS 거리가 같으면
/// `find_path` 가 먼저 발견하는 쪽(`deltas` 순서대로 평가)을 고른다.
pub fn nearest_adjacent_walkable(
    map: &Map,
    target_tile: (usize, usize),
    from_tile: (usize, usize),
) -> Option<(usize, usize)> {
    let (tx, ty) = target_tile;
    let deltas: [(i32, i32); 8] = [
        (-1, 0), (1, 0), (0, -1), (0, 1),
        (-1, -1), (1, -1), (-1, 1), (1, 1),
    ];
    let mut best: Option<((usize, usize), usize)> = None;
    for (dx, dy) in deltas {
        let nx = tx as i32 + dx;
        let ny = ty as i32 + dy;
        if nx < 0 || ny < 0 { continue; }
        let (nx, ny) = (nx as usize, ny as usize);
        if nx >= map.width || ny >= map.height { continue; }
        if !map.get_tile(nx, ny).is_walkable() { continue; }
        // 같은 자리면 이미 도착 — 거리 0.
        if (nx, ny) == from_tile {
            return Some((nx, ny));
        }
        let path = pathfinding::find_path(map, from_tile, (nx, ny));
        if path.is_empty() { continue; }
        let len = path.len();
        match best {
            Some((_, blen)) if blen <= len => {}
            _ => best = Some(((nx, ny), len)),
        }
    }
    best.map(|(t, _)| t)
}

/// 마우스 클릭 한 번이 어떤 결정으로 이어질지 분류한 결과.
///
/// `on_mouse_click` 시스템은 viewport 변환과 ECS 쿼리를 거친 뒤,
/// 이 결정만큼은 순수 함수로 분리해 단위 테스트로 모든 분기를 커버한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClickDecision {
    /// 클릭 타일이 viewport 변환 실패·맵 밖 등으로 처리 불가 — 아무것도 하지 않는다.
    Ignore,
    /// 클릭 타일에 villager 가 있다 → 그 엔티티를 추적. 동봉된 경로로 접근.
    FollowNpc { entity: Entity, path: VecDeque<(usize, usize)> },
    /// 클릭 타일이 interactable 비-walkable(카운터 등) → 인접 walkable 까지
    /// 이동하면서 도착 시 `(tx, ty)` 로 자동 범프.
    InteractTile { tile: (usize, usize), path: VecDeque<(usize, usize)> },
    /// 평범한 walkable 빈 타일 → 단순 이동.
    Walk { path: VecDeque<(usize, usize)> },
    /// 이미 인접한 villager 클릭 → 즉시 `BumpTileEvent(tile)` (follow 불필요).
    /// `tile` 은 villager 의 현재 좌표. PlayerActedEvent 가 안 와도 즉시 처리되어야
    /// 사용자가 "한 박자 늦게 말 거는" 체감을 받지 않게 한다.
    ImmediateBump { tile: (usize, usize) },
}

/// 클릭 한 번이 만들 동작을 결정한다 (순수 함수, 헤드리스 단위 테스트로 커버).
///
/// `villager_at` 클로저는 해당 타일에 있는 villager 엔티티(있으면)를 반환한다.
/// `target_tile` 이 `None` 이면(=viewport 변환 실패) `Ignore`. 경계 밖이면 `Ignore`.
///
/// "이미 인접" 케이스(체비쇼프 ≤ 1)는 `ImmediateBump` 로 분류해 `on_mouse_click`
/// 이 PlayerActedEvent 대기 없이 즉시 BumpTileEvent 를 보내게 한다.
pub fn classify_click(
    target_tile: Option<(usize, usize)>,
    map: &Map,
    player_tile: (usize, usize),
    villager_at: impl Fn(usize, usize) -> Option<Entity>,
) -> ClickDecision {
    let Some((tx, ty)) = target_tile else { return ClickDecision::Ignore; };
    if tx >= map.width || ty >= map.height { return ClickDecision::Ignore; }

    // 1) NPC 추적 — 타일 walkable 여부와 무관(villager 는 Floor 위에 있으니
    //    일반적으로는 walkable 이지만, 우선순위는 NPC).
    if villager_at(tx, ty).is_some() {
        // 이미 인접하면 follow 불필요 — 즉시 범프.
        if chebyshev_distance(player_tile, (tx, ty)) <= 1 {
            return ClickDecision::ImmediateBump { tile: (tx, ty) };
        }
        let entity = villager_at(tx, ty).unwrap();
        let path = pathfinding::find_path(map, player_tile, (tx, ty));
        return ClickDecision::FollowNpc { entity, path: VecDeque::from(path) };
    }

    let tile = map.get_tile(tx, ty);
    let walkable = tile.is_walkable();

    if !walkable {
        // 2) 카운터 등 interactable 비-walkable — 인접 walkable 로 가서 자동 범프.
        if is_interactable_tile(tile) {
            // 이미 카운터 인접이면 즉시 범프 — turn 소비 없이.
            if chebyshev_distance(player_tile, (tx, ty)) <= 1 {
                return ClickDecision::ImmediateBump { tile: (tx, ty) };
            }
            if let Some(adj) = nearest_adjacent_walkable(map, (tx, ty), player_tile) {
                // adj == player_tile 케이스는 위 chebyshev≤1 가드에서 ImmediateBump
                // 으로 이미 처리됨 — 여기 도달했으면 player_tile != adj.
                let path = VecDeque::from(pathfinding::find_path(map, player_tile, adj));
                if path.is_empty() {
                    return ClickDecision::Ignore;
                }
                return ClickDecision::InteractTile { tile: (tx, ty), path };
            }
        }
        return ClickDecision::Ignore;
    }

    // 3) 평범한 walkable 빈 타일.
    let path = pathfinding::find_path(map, player_tile, (tx, ty));
    if path.is_empty() {
        ClickDecision::Ignore
    } else {
        ClickDecision::Walk { path: VecDeque::from(path) }
    }
}

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

/// 이동/클릭 입력을 막아야 하는 모달 패널 상태 묶음. (시스템 파라미터 16개 제한 회피용)
#[derive(bevy::ecs::system::SystemParam)]
struct PanelGuards<'w> {
    equipment_open: Res<'w, EquipmentPanelOpen>,
    shop_open: Res<'w, ShopPanelOpen>,
    help_open: Res<'w, HelpPanelOpen>,
    guide_open: Res<'w, GuidePanelOpen>,
}

impl PanelGuards<'_> {
    fn any_open(&self) -> bool {
        self.equipment_open.0 || self.shop_open.0 || self.help_open.0 || self.guide_open.0
    }
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
    mut interact_target: ResMut<MouseInteractTarget>,
    player_query: Query<(Entity, &Transform), (With<Player>, Without<MovingTo>, Without<Defeated>)>,
    map_res: Res<MapResource>,
    occupied: Res<OccupiedTiles>,
    monster_tiles: Res<MonsterTiles>,
    mut acted: EventWriter<PlayerActedEvent>,
    mut bump: EventWriter<BumpTileEvent>,
    mut attack: EventWriter<AttackMonsterEvent>,
    panels: PanelGuards,
    ranged: Res<crate::modules::ranged::RangedTargeting>,
) {
    if panels.any_open() { return; }
    if ranged.active { return; }
    let Ok((entity, transform)) = player_query.get_single() else { return };

    // 스페이스바: 제자리 대기 — hold state 초기화 후 턴 소비
    if keyboard_input.just_pressed(KeyCode::Space) {
        hold_state.dir = IVec2::ZERO;
        hold_state.elapsed = 0.0;
        player_path.0.clear();
        interact_target.0 = None;
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

    // 키 입력이 있으면 자동 이동 경로 + follow 취소
    if dir != IVec2::ZERO || just_pressed {
        player_path.0.clear();
        interact_target.0 = None;
    }

    // 자동 이동 경로 소비 (키 입력 없을 때)
    if dir == IVec2::ZERO && !player_path.0.is_empty() {
        if !tick_hold(&mut hold_state, IVec2::ONE, false, time.delta_seconds()) { return; }

        let (tx, ty) = player_path.0.front().copied().unwrap();

        if monster_tiles.0.contains(&(tx, ty)) {
            player_path.0.clear();
            interact_target.0 = None;
            attack.send(AttackMonsterEvent(tx, ty));
            acted.send(PlayerActedEvent);
        } else if occupied.0.contains(&(tx, ty)) {
            player_path.0.clear();
            interact_target.0 = None;
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

    if !map.get_tile(tx, ty).is_walkable() {
        // 통행 불가 타일이라도 상호작용 가능(가판대 등)이면 범프로 상호작용한다.
        // (handle_bump 가 카운터 너머 vendor 상점을 연다.)
        if is_interactable_tile(map.get_tile(tx, ty)) {
            bump.send(BumpTileEvent(tx, ty));
            acted.send(PlayerActedEvent);
        }
        return;
    }

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

/// 플레이어 위치에서 클릭 위치(타일 좌표)까지 새 경로/상호작용 대상을 적용하는 순수 결정 로직.
///
/// 윈도우/카메라 viewport 변환은 헤드리스 테스트로 재현하기 어려워 시스템 쪽에 남기고,
/// 변환 결과(`target_world: Option<Vec2>`)만 받아 모든 분기를 `classify_click` 으로 위임한다.
fn plan_click_path(
    target_world: Option<Vec2>,
    map: &Map,
    player_world: Vec3,
    villager_at: impl Fn(usize, usize) -> Option<Entity>,
) -> ClickDecision {
    let Some(world_pos) = target_world else { return ClickDecision::Ignore; };
    let world_vec3 = Vec3::new(world_pos.x, world_pos.y, 0.0);
    let target_tile = world_to_tile_coords(world_vec3);
    let player_tile = world_to_tile_coords(player_world);
    classify_click(Some(target_tile), map, player_tile, villager_at)
}

/// 마우스/터치 클릭 한 번이 만든 `ClickDecision` 을 실제 상태(`PlayerPath`,
/// `MouseInteractTarget`)와 즉시 범프 이벤트로 변환한다.
///
/// 마우스 좌클릭과 터치 탭은 입력 채널만 다르고 결정 적용 흐름은 동일해야 하므로
/// 이 분기 처리를 한 곳에 모아둔다. 두 시스템(`on_mouse_click`/`on_touch_tap`)이
/// viewport 변환·플레이어/맵 조회 끝에 이 헬퍼를 호출한다.
fn apply_click_decision(
    decision: ClickDecision,
    player_path: &mut PlayerPath,
    target: &mut MouseInteractTarget,
    bump: &mut EventWriter<BumpTileEvent>,
) {
    match decision {
        ClickDecision::Ignore => {
            // 새 클릭이 무시되어도 기존 follow/path 는 정리한다(전형적인 cancel).
            target.0 = None;
            player_path.0.clear();
        }
        ClickDecision::FollowNpc { entity, path } => {
            target.0 = Some(InteractTarget::Npc(entity));
            player_path.0 = path;
        }
        ClickDecision::InteractTile { tile, path } => {
            target.0 = Some(InteractTarget::Tile(tile.0, tile.1));
            player_path.0 = path;
        }
        ClickDecision::Walk { path } => {
            target.0 = None;
            player_path.0 = path;
        }
        ClickDecision::ImmediateBump { tile } => {
            // 이미 인접 — turn 소비 없이 즉시 범프.
            bump.send(BumpTileEvent(tile.0, tile.1));
            target.0 = None;
            player_path.0.clear();
        }
    }
}

/// 플레이어 위치에서 클릭한 타일까지 자동 이동 경로 + (필요 시) 추적 대상을 정한다.
///
/// 모달 패널이 열려 있을 때는 마우스 경로 이동도 무시한다. 키보드 이동과 같은
/// 기준을 적용해 오버레이가 단순한 투명 UI가 아니라 상호작용 경계로 동작하게 한다.
///
/// 모바일 브라우저는 터치 후 합성(synthetic) `mousedown` 을 보낼 수 있다.
/// 같은 프레임에 `Touches::any_just_pressed()` 가 참이면 `on_touch_tap` 이 이미
/// 같은 위치를 처리했으므로 마우스 분기를 SKIP 한다 — 동일 입력의 중복 적용 방지.
fn on_mouse_click(
    mouse_input: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<Camera>>,
    player_query: Query<&Transform, (With<Player>, Without<Defeated>)>,
    map_res: Res<MapResource>,
    mut player_path: ResMut<PlayerPath>,
    mut target: ResMut<MouseInteractTarget>,
    villager_query: Query<(Entity, &Villager)>,
    mut bump: EventWriter<BumpTileEvent>,
    panels: PanelGuards,
    ranged: Res<crate::modules::ranged::RangedTargeting>,
) {
    if !mouse_input.just_pressed(MouseButton::Left) { return; }
    // 같은 프레임 터치가 있었다면 on_touch_tap 이 이미 같은 위치를 처리 — 중복 방지.
    if touches.any_just_pressed() { return; }
    if panels.any_open() { return; }
    if ranged.active { return; }  // 원격 모드 중에는 ranged 시스템이 마우스 처리

    let Ok(window) = windows.get_single() else { return };
    let Ok((camera, cam_transform)) = camera_q.get_single() else { return };
    let Ok(player_transform) = player_query.get_single() else { return };

    let Some(cursor_pos) = window.cursor_position() else { return };
    let world_pos = camera.viewport_to_world_2d(cam_transform, cursor_pos);

    // 새 클릭 입력은 무조건 이전 follow/path 를 정리하고 새 결정을 적용한다.
    let villagers: Vec<(Entity, usize, usize)> = villager_query.iter()
        .map(|(e, v)| (e, v.tile_x, v.tile_y)).collect();
    let villager_at = |x: usize, y: usize| -> Option<Entity> {
        villagers.iter().find(|(_, vx, vy)| *vx == x && *vy == y).map(|(e, _, _)| *e)
    };

    // viewport 변환 성공 시에만 Some — 헤드리스 테스트는 viewport_to_world_2d 가
    // 항상 None 이라 이 분기의 Some 쪽은 직접 검증 불가(classify_click 을 순수
    // 함수로 분리해 결정 분기는 단위 테스트로 모두 커버). // 도달 불가 방어코드
    let decision = plan_click_path(
        world_pos, map_res.map(), player_transform.translation, villager_at,
    );
    apply_click_decision(decision, &mut player_path, &mut target, &mut bump);
}

/// 모바일 터치 탭을 마우스 좌클릭과 동일한 흐름으로 처리한다.
///
/// `Touches::iter_just_pressed()` 의 첫 항목 위치를 cursor_pos 대신 사용해
/// `viewport_to_world_2d` → `plan_click_path` → `apply_click_decision` 의 같은
/// 결정 파이프라인을 탄다. 마우스 분기와의 유일한 차이는 입력 채널(터치 좌표)뿐.
///
/// 이 시스템은 `on_mouse_click` 보다 먼저 돌고, 같은 프레임에 합성 마우스 클릭이
/// 함께 와도 `on_mouse_click` 이 `Touches::any_just_pressed` 가드로 SKIP 한다.
fn on_touch_tap(
    touches: Res<Touches>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<Camera>>,
    player_query: Query<&Transform, (With<Player>, Without<Defeated>)>,
    map_res: Res<MapResource>,
    mut player_path: ResMut<PlayerPath>,
    mut target: ResMut<MouseInteractTarget>,
    villager_query: Query<(Entity, &Villager)>,
    mut bump: EventWriter<BumpTileEvent>,
    panels: PanelGuards,
    ranged: Res<crate::modules::ranged::RangedTargeting>,
) {
    // 이번 프레임에 막 시작된 터치 하나만 처리(멀티터치는 무시 — 게임 자체가 단일 포인터 모델).
    let Some(touch) = touches.iter_just_pressed().next() else { return };
    if panels.any_open() { return; }
    if ranged.active { return; }

    let Ok(window) = windows.get_single() else { return };
    let Ok((camera, cam_transform)) = camera_q.get_single() else { return };
    let Ok(player_transform) = player_query.get_single() else { return };

    let touch_pos = touch.position();
    // window 가 살아 있는지 확인 — get_single 이 Ok 인 시점에서 보장되지만,
    // viewport_to_world_2d 는 cursor_position 와 같은 좌표계(픽셀)를 요구하므로
    // 그대로 넘긴다. 헤드리스에서는 변환이 None → Ignore 로 흘러간다.
    let _ = window;
    let world_pos = camera.viewport_to_world_2d(cam_transform, touch_pos);

    let villagers: Vec<(Entity, usize, usize)> = villager_query.iter()
        .map(|(e, v)| (e, v.tile_x, v.tile_y)).collect();
    let villager_at = |x: usize, y: usize| -> Option<Entity> {
        villagers.iter().find(|(_, vx, vy)| *vx == x && *vy == y).map(|(e, _, _)| *e)
    };

    let decision = plan_click_path(
        world_pos, map_res.map(), player_transform.translation, villager_at,
    );
    apply_click_decision(decision, &mut player_path, &mut target, &mut bump);
}

/// 매 턴(`PlayerActedEvent`) villager 의 새 위치로 follow 경로를 재계산한다.
///
/// - `MouseInteractTarget::Npc(e)`:
///   - villager 엔티티가 despawn 됐으면 follow 안전 종료.
///   - 플레이어와의 체비쇼프 거리 ≤ 1 이면 `BumpTileEvent(villager.tile)` 발행 +
///     follow/path 정리(말 걸기/상점/대화는 다음 프레임 `handle_bump` 가 처리).
///   - 아니면 `find_path` 로 경로 갱신.
/// - `MouseInteractTarget::Tile(x, y)`(카운터 등 고정 좌표):
///   - 체비쇼프 ≤ 1 이면 `BumpTileEvent(x, y)` 발행 + follow/path 정리.
///   - 아니면 그대로 — 카운터는 움직이지 않으므로 path 는 그대로 두면 된다.
///
/// PlayerActedEvent 는 발행하지 않는다 — 플레이어가 직전 이동으로 이미 턴을
/// 소비했기 때문이다(중복 발행 시 villager_turn 이 한 턴 더 돌아 NPC 가 두 번
/// 움직이는 회귀). bump 는 다음 프레임 handle_bump 가 단순히 추가 처리만 한다.
fn refresh_follow_path(
    mut events: EventReader<PlayerActedEvent>,
    mut target: ResMut<MouseInteractTarget>,
    mut player_path: ResMut<PlayerPath>,
    player_query: Query<&Transform, (With<Player>, Without<Defeated>)>,
    map_res: Res<MapResource>,
    villager_query: Query<&Villager>,
    mut bump: EventWriter<BumpTileEvent>,
) {
    if events.read().next().is_none() { return; }
    let Some(t) = target.0 else { return; };
    let Ok(transform) = player_query.get_single() else {
        // 플레이어가 사라졌으면 follow 도 함께 정리.
        target.0 = None;
        player_path.0.clear();
        return;
    };
    let player_tile = world_to_tile_coords(transform.translation);

    match t {
        InteractTarget::Npc(entity) => {
            let Ok(villager) = villager_query.get(entity) else {
                // villager 가 despawn 됐으면 follow 안전 종료.
                target.0 = None;
                player_path.0.clear();
                return;
            };
            let v_tile = (villager.tile_x, villager.tile_y);
            if chebyshev_distance(player_tile, v_tile) <= 1 {
                bump.send(BumpTileEvent(v_tile.0, v_tile.1));
                target.0 = None;
                player_path.0.clear();
            } else {
                let path = pathfinding::find_path(map_res.map(), player_tile, v_tile);
                player_path.0 = VecDeque::from(path);
                // 경로가 비면 (도달 불가) 다음 턴에도 못 가니 path 는 비어도 follow 는 유지 —
                // 다음 턴 villager 가 다시 walkable 위치로 오면 재경로가 잡힌다.
            }
        }
        InteractTarget::Tile(tx, ty) => {
            if chebyshev_distance(player_tile, (tx, ty)) <= 1 {
                bump.send(BumpTileEvent(tx, ty));
                target.0 = None;
                player_path.0.clear();
            }
            // 그 외엔 path 그대로 — 카운터는 움직이지 않으므로 처음 깐 경로가 유효.
        }
    }
}

fn respawn_player_on_regen(
    mut commands: Commands,
    mut events: EventReader<PlayerRespawnEvent>,
    mut player_query: Query<(Entity, &mut Transform), With<Player>>,
    mut player_path: ResMut<PlayerPath>,
    mut interact_target: ResMut<MouseInteractTarget>,
) {
    for PlayerRespawnEvent(x, y) in events.read() {
        if let Ok((entity, mut transform)) = player_query.get_single_mut() {
            let wp = tile_to_world_coords(*x, *y);
            transform.translation = Vec3::new(wp.x, wp.y, 1.0);
            commands.entity(entity).remove::<MovingTo>();
            player_path.0.clear();
            interact_target.0 = None;
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

/// 테스트 전용 별칭 — `update_fov` 시스템을 lighting 통합 테스트에서도 호출할 수 있게
/// 같은 시그니처로 노출한다.
#[cfg(test)]
pub(crate) fn update_fov_for_test(
    player_query: Query<(&Transform, &Facing), With<Player>>,
    map_res: ResMut<MapResource>,
    global_turn: Option<Res<GlobalTurn>>,
    light_map: Option<Res<LightMap>>,
    last_pos: Local<Option<(IVec2, IVec2)>>,
) {
    update_fov(player_query, map_res, global_turn, light_map, last_pos);
}

fn update_fov(
    player_query: Query<(&Transform, &Facing), With<Player>>,
    mut map_res: ResMut<MapResource>,
    global_turn: Option<Res<GlobalTurn>>,
    light_map: Option<Res<LightMap>>,
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

    // 성능 로그용 타이머 — web-time::Instant 는 native/wasm 양쪽에서 동작한다.
    // (std::time::Instant 는 wasm32 에서 패닉.)
    let start = web_time::Instant::now();
    let now_turn: u32 = global_turn.as_ref().map(|t| t.0 as u32).unwrap_or(0);
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

                // brightness state 갱신 — 누적 가시화 상태.
                // 이전 상태에 (마지막 본 이후) 망각 감쇠를 먼저 적용한 뒤, 현재 시야 강도
                // (거리 감쇠 × 광량 분기) 와 채널별 max — 사용자 의도:
                // '망각 30 + 시야 10 → 30' (state stays).
                let elapsed = match map.tiles[idx].last_seen_turn {
                    Some(t) => now_turn.saturating_sub(t),
                    None => 0,
                };
                let decayed = map.tiles[idx].brightness * memory_fade_factor(elapsed);
                let dx = (x - cur.x).abs();
                let dy = (y - cur.y).abs();
                let d = dx.max(dy);
                let falloff = distance_falloff_alpha(d);
                // LightMap 없으면(테스트 등) Bright 로 간주 — 광량 분기 없는 환경 호환.
                let light_factor = match light_map.as_ref().map(|lm| lm.at(x as usize, y as usize)) {
                    Some(LightLevel::Dark) => DARK_DIM_FACTOR * falloff,
                    _ => falloff,
                };
                map.tiles[idx].brightness = decayed.max(light_factor);
                map.tiles[idx].last_seen_turn = Some(now_turn);
            }
        }
    }
    // FOV 계산이 5ms 이상 걸릴 때만 찍는 성능 로그. 테스트 맵은 작아 항상 즉시
    // 끝나므로 이 분기의 True 쪽은 결정론적으로 도달 불가. // 도달 불가 방어코드
    let elapsed = start.elapsed();
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
        app.init_resource::<MouseInteractTarget>();
        app.init_resource::<OccupiedTiles>();
        app.init_resource::<MonsterTiles>();
        app.insert_resource(EquipmentPanelOpen(false));
        app.insert_resource(ShopPanelOpen(false));
        app.insert_resource(HelpPanelOpen(false));
        app.insert_resource(GuidePanelOpen(false));
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
            app.init_resource::<MouseInteractTarget>();
            app.init_resource::<OccupiedTiles>();
            app.init_resource::<MonsterTiles>();
            app.insert_resource(EquipmentPanelOpen(false));
            app.insert_resource(ShopPanelOpen(false));
            app.insert_resource(HelpPanelOpen(false));
            app.insert_resource(GuidePanelOpen(false));
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
    fn 카운터로_이동하면_이동대신_범프로_상호작용한다() {
        // 가판대(Counter)는 통행 불가지만 향해 이동하면 BumpTileEvent 로 상점을 연다.
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(6, 5, TileKind::Counter);
        h.press(KeyCode::ArrowRight);
        h.update();
        assert!(!h.moving(), "카운터로는 이동하지 않는다");
        assert_eq!(h.bump_targets(), vec![(6, 5)], "카운터를 향한 이동은 범프 이벤트");
        assert_eq!(h.acted_count(), 1, "카운터 상호작용도 턴을 소비한다");
    }

    #[test]
    fn 일반_벽으로_이동하면_범프도_이동도_없다() {
        // 상호작용 불가 통행불가 타일(Wall)은 범프도 이동도 만들지 않는다(기존 동작 회귀).
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(6, 5, TileKind::Wall);
        h.press(KeyCode::ArrowRight);
        h.update();
        assert!(!h.moving());
        assert!(h.bump_targets().is_empty(), "벽은 범프 이벤트를 만들지 않는다");
        assert_eq!(h.acted_count(), 0, "벽으로의 이동은 턴을 소비하지 않는다");
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
        app.init_resource::<MouseInteractTarget>();
        app.add_systems(Update, respawn_player_on_regen);
        let e = app.world.spawn((
            Player,
            Transform::from_xyz(0.0, 0.0, 1.0),
            MovingTo { target: Vec3::ZERO },
        )).id();
        app.world.resource_mut::<PlayerPath>().0.push_back((1, 1));
        app.world.resource_mut::<MouseInteractTarget>().0 = Some(InteractTarget::Tile(3, 3));
        app.world.send_event(PlayerRespawnEvent(7, 8));
        app.update();
        let expected = tile_to_world_coords(7, 8);
        let t = app.world.get::<Transform>(e).unwrap().translation;
        assert_eq!(t, Vec3::new(expected.x, expected.y, 1.0), "리스폰 위치로 이동");
        assert!(!app.world.entity(e).contains::<MovingTo>(), "MovingTo 제거");
        assert_eq!(app.world.resource::<PlayerPath>().0.len(), 0, "자동 경로 정리");
        assert!(app.world.resource::<MouseInteractTarget>().0.is_none(),
            "리스폰 시 follow 도 함께 정리되어야 한다");
    }

    #[test]
    fn 리스폰이벤트가_없으면_플레이어를_옮기지_않는다() {
        let mut app = App::new();
        app.add_event::<PlayerRespawnEvent>();
        app.init_resource::<PlayerPath>();
        app.init_resource::<MouseInteractTarget>();
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
        app.init_resource::<MouseInteractTarget>();
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

    // --- plan_click_path / classify_click (마우스 결정 로직 순수 함수) ---

    fn click_map() -> Map {
        let mut map = Map::new(20, 20);
        for y in 4..8 { for x in 4..8 { map.set_tile(x, y, TileKind::Floor); } }
        map
    }

    /// villager 없는 빈 클로저 — 헬퍼로 분리해 가독성↑.
    fn no_villager(_x: usize, _y: usize) -> Option<Entity> { None }

    #[test]
    fn 클릭경로계산은_viewport_변환_실패시_무시한다() {
        let map = click_map();
        let player_world = tile_to_world_coords(5, 5).extend(1.0);
        assert!(matches!(
            plan_click_path(None, &map, player_world, no_villager),
            ClickDecision::Ignore
        ));
    }

    #[test]
    fn 클릭경로계산은_목적지가_바닥이면_단순이동_경로를_반환한다() {
        let map = click_map();
        let player_world = tile_to_world_coords(5, 5).extend(1.0);
        let target = tile_to_world_coords(7, 5); // Floor
        let d = plan_click_path(Some(target), &map, player_world, no_villager);
        match d {
            ClickDecision::Walk { path } => {
                assert!(!path.is_empty(), "바닥 클릭이면 경로가 비어있지 않아야 한다");
                assert_eq!(*path.back().unwrap(), (7, 5), "경로 끝은 클릭한 타일");
            }
            _ => panic!("바닥 클릭은 Walk 분기여야 한다: {:?}", d),
        }
    }

    #[test]
    fn 클릭경로계산은_목적지가_벽이면_무시한다() {
        let map = click_map(); // (0,0) 등은 기본 Wall
        let player_world = tile_to_world_coords(5, 5).extend(1.0);
        let target = tile_to_world_coords(0, 0); // Wall
        assert!(matches!(
            plan_click_path(Some(target), &map, player_world, no_villager),
            ClickDecision::Ignore
        ), "벽 클릭은 무시되어야 한다");
    }

    #[test]
    fn 클릭경로계산은_목적지가_물이면_무시한다() {
        let mut map = click_map();
        map.set_tile(7, 5, TileKind::Water);
        let player_world = tile_to_world_coords(5, 5).extend(1.0);
        let target = tile_to_world_coords(7, 5); // Water
        assert!(matches!(
            plan_click_path(Some(target), &map, player_world, no_villager),
            ClickDecision::Ignore
        ), "물 클릭은 이동 불가라 무시되어야 한다");
    }

    #[test]
    fn 클릭경로계산은_목적지가_모래면_단순이동_경로를_반환한다() {
        let mut map = click_map();
        map.set_tile(7, 5, TileKind::Sand);
        let player_world = tile_to_world_coords(5, 5).extend(1.0);
        let target = tile_to_world_coords(7, 5); // Sand
        match plan_click_path(Some(target), &map, player_world, no_villager) {
            ClickDecision::Walk { path } => assert_eq!(*path.back().unwrap(), (7, 5)),
            other => panic!("모래 클릭은 Walk 분기여야 한다: {:?}", other),
        }
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
            app.init_resource::<Touches>();
            app.insert_resource(MapResource(click_map()));
            app.init_resource::<PlayerPath>();
            app.init_resource::<MouseInteractTarget>();
            app.insert_resource(EquipmentPanelOpen(false));
            app.insert_resource(ShopPanelOpen(false));
            app.insert_resource(HelpPanelOpen(false));
            app.insert_resource(GuidePanelOpen(false));
            app.init_resource::<RangedTargeting>();
            app.add_event::<BumpTileEvent>();
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
            "헤드리스에서는 viewport 변환이_실패해 경로가 비어 있다");
    }

    // --- on_touch_tap 시스템: 모바일 터치 입력은 마우스 좌클릭과 동일 흐름 ---

    /// 터치 탭 하네스. 마우스와 같이 윈도우/카메라/플레이어를 두고,
    /// `Touches` 리소스에 just_pressed 터치를 시드한 뒤 `on_touch_tap` 을 돌린다.
    ///
    /// 헤드리스라 viewport_to_world_2d 가 항상 None — 결정 분기 자체는 순수
    /// `classify_click` 테스트가 커버하고, 여기서는 시스템의 가드/경로(터치 시드의
    /// 유무, 패널/원격/엔티티 누락 등)와 마우스 가드와의 상호 작용을 검증한다.
    struct TouchHarness { app: App }
    impl TouchHarness {
        fn new() -> Self {
            let mut app = App::new();
            app.insert_resource(ButtonInput::<MouseButton>::default());
            app.init_resource::<Touches>();
            app.add_event::<bevy::input::touch::TouchInput>();
            app.insert_resource(MapResource(click_map()));
            app.init_resource::<PlayerPath>();
            app.init_resource::<MouseInteractTarget>();
            app.insert_resource(EquipmentPanelOpen(false));
            app.insert_resource(ShopPanelOpen(false));
            app.insert_resource(HelpPanelOpen(false));
            app.insert_resource(GuidePanelOpen(false));
            app.init_resource::<RangedTargeting>();
            app.add_event::<BumpTileEvent>();
            // touch_screen_input_system 이 먼저 돌아 Touches 리소스에 just_pressed 가
            // 채워진 뒤 on_touch_tap → on_mouse_click 순으로 실행된다 — 실제 PlayerPlugin
            // 의 순서(on_touch_tap.before(on_mouse_click))와 동일하게 배치한다.
            app.add_systems(Update, (
                bevy::input::touch::touch_screen_input_system,
                on_touch_tap,
                on_mouse_click,
            ).chain());
            Self { app }
        }
        fn spawn_window(&mut self) -> Entity {
            let mut window = Window::default();
            window.resolution.set(800.0, 600.0);
            self.app.world.spawn(window).id()
        }
        fn spawn_camera(&mut self) {
            self.app.world.spawn((Camera::default(), GlobalTransform::default()));
        }
        fn spawn_player(&mut self) {
            let pos = tile_to_world_coords(5, 5);
            self.app.world.spawn((Player, Transform::from_xyz(pos.x, pos.y, 1.0)));
        }
        /// 시작(`Started`) 상태의 터치 이벤트를 큐에 넣는다. 다음 `update()` 에서
        /// `touch_screen_input_system` 이 이를 소비해 `Touches` 의 just_pressed 에 등록한다.
        fn tap(&mut self, window: Entity, pos: Vec2) {
            self.app.world.send_event(bevy::input::touch::TouchInput {
                phase: bevy::input::touch::TouchPhase::Started,
                position: pos,
                window,
                force: None,
                id: 1,
            });
        }
        fn update(&mut self) { self.app.update(); }
        fn path_len(&self) -> usize { self.app.world.resource::<PlayerPath>().0.len() }
    }

    #[test]
    fn 터치_이벤트가_없으면_터치탭은_경로처리를_하지_않는다() {
        let mut h = TouchHarness::new();
        h.spawn_window();
        h.spawn_camera();
        h.spawn_player();
        h.update(); // 터치 시드 안 함
        assert_eq!(h.path_len(), 0);
    }

    #[test]
    fn 터치탭은_장비패널이_열려있으면_무시된다() {
        let mut h = TouchHarness::new();
        let w = h.spawn_window();
        h.spawn_camera();
        h.spawn_player();
        h.app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        h.tap(w, Vec2::new(400.0, 300.0));
        h.update();
        assert_eq!(h.path_len(), 0);
    }

    #[test]
    fn 터치탭은_상점패널이_열려있으면_무시된다() {
        let mut h = TouchHarness::new();
        let w = h.spawn_window();
        h.spawn_camera();
        h.spawn_player();
        h.app.world.resource_mut::<ShopPanelOpen>().0 = true;
        h.tap(w, Vec2::new(400.0, 300.0));
        h.update();
        assert_eq!(h.path_len(), 0);
    }

    #[test]
    fn 터치탭은_도움말패널이_열려있으면_무시된다() {
        let mut h = TouchHarness::new();
        let w = h.spawn_window();
        h.spawn_camera();
        h.spawn_player();
        h.app.world.resource_mut::<HelpPanelOpen>().0 = true;
        h.tap(w, Vec2::new(400.0, 300.0));
        h.update();
        assert_eq!(h.path_len(), 0);
    }

    #[test]
    fn 터치탭은_원격모드_활성중이면_무시된다() {
        // ranged 모드는 ranged 시스템이 터치/마우스를 처리하므로 여기서는 무시.
        let mut h = TouchHarness::new();
        let w = h.spawn_window();
        h.spawn_camera();
        h.spawn_player();
        h.app.world.resource_mut::<RangedTargeting>().active = true;
        h.tap(w, Vec2::new(400.0, 300.0));
        h.update();
        assert_eq!(h.path_len(), 0);
    }

    #[test]
    fn 터치탭은_윈도우가_없으면_무시된다() {
        let mut h = TouchHarness::new();
        h.spawn_camera();
        h.spawn_player();
        // window 이벤트의 `window` 필드는 Entity 값만 있으면 되므로 더미를 만든다.
        let dummy_window = h.app.world.spawn_empty().id();
        h.tap(dummy_window, Vec2::new(400.0, 300.0));
        h.update();
        assert_eq!(h.path_len(), 0);
    }

    #[test]
    fn 터치탭은_카메라가_없으면_무시된다() {
        let mut h = TouchHarness::new();
        let w = h.spawn_window();
        h.spawn_player();
        h.tap(w, Vec2::new(400.0, 300.0));
        h.update();
        assert_eq!(h.path_len(), 0);
    }

    #[test]
    fn 터치탭은_플레이어가_없으면_무시된다() {
        let mut h = TouchHarness::new();
        let w = h.spawn_window();
        h.spawn_camera();
        h.tap(w, Vec2::new(400.0, 300.0));
        h.update();
        assert_eq!(h.path_len(), 0);
    }

    #[test]
    fn 터치탭은_모든_가드를_통과해도_헤드리스에서는_viewport변환이_실패한다() {
        // 윈도우/카메라/플레이어/터치 좌표가 모두 있어 가드를 통과하지만,
        // viewport_to_world_2d 는 헤드리스에서 None 이라 경로가 채워지지 않는다.
        // 헤드리스에서는 마우스 클릭과 동일하게 viewport 변환만 실패할 뿐, 같은
        // 결정 파이프라인(plan_click_path → apply_click_decision)을 탄다.
        let mut h = TouchHarness::new();
        let w = h.spawn_window();
        h.spawn_camera();
        h.spawn_player();
        h.tap(w, Vec2::new(400.0, 300.0));
        h.update();
        assert_eq!(h.path_len(), 0);
    }

    #[test]
    fn 같은_프레임에_터치가_있으면_synthetic_마우스_클릭은_무시된다() {
        // 모바일 브라우저의 합성(synthetic) mousedown 중복 방지 가드 검증:
        // 같은 프레임에 Touches::any_just_pressed() 가 true 이면 on_mouse_click 은
        // 어떤 부수효과도 만들지 않아야 한다 (viewport_to_world_2d 도 호출 안 함).
        //
        // 마우스 클릭의 부수효과 중 가장 관측이 쉬운 것: 미리 채워둔 follow 가
        // Ignore 분기에서 비워지는 동작. 가드가 있으면 그 비우기조차 일어나지 않는다.
        //
        // 따라서 on_touch_tap 은 빼고 on_mouse_click 만 등록한 별도 App 로
        // 가드의 효과를 단독으로 검증한다.
        let mut app = App::new();
        app.insert_resource(ButtonInput::<MouseButton>::default());
        app.init_resource::<Touches>();
        app.add_event::<bevy::input::touch::TouchInput>();
        app.insert_resource(MapResource(click_map()));
        app.init_resource::<PlayerPath>();
        app.init_resource::<MouseInteractTarget>();
        app.insert_resource(EquipmentPanelOpen(false));
        app.insert_resource(ShopPanelOpen(false));
        app.insert_resource(HelpPanelOpen(false));
        app.insert_resource(GuidePanelOpen(false));
        app.init_resource::<RangedTargeting>();
        app.add_event::<BumpTileEvent>();
        // 합성 마우스만 도는 시나리오 — touch_screen_input_system 이 먼저 도는 건
        // 실제 PlayerPlugin 스케줄과 동일하게 두지만, on_touch_tap 은 빼서
        // 가드가 막은 효과가 마우스 시스템 단독으로 관측되게 한다.
        app.add_systems(Update, (
            bevy::input::touch::touch_screen_input_system,
            on_mouse_click,
        ).chain());

        let mut window = Window::default();
        window.resolution.set(800.0, 600.0);
        window.set_physical_cursor_position(Some(bevy::math::DVec2::new(400.0, 300.0)));
        let w = app.world.spawn(window).id();
        app.world.spawn((Camera::default(), GlobalTransform::default()));
        let pos = tile_to_world_coords(5, 5);
        app.world.spawn((Player, Transform::from_xyz(pos.x, pos.y, 1.0)));

        // 미리 채워둔 follow — 마우스 가드가 작동하지 않으면 Ignore 분기로 비워질 것이다.
        app.world.resource_mut::<PlayerPath>().0.push_back((6, 5));
        app.world.resource_mut::<MouseInteractTarget>().0 = Some(InteractTarget::Tile(7, 5));

        // 같은 프레임에 터치와 마우스가 모두 들어온다 — touch_screen_input_system
        // 이 just_pressed 를 채우고, 그다음 on_mouse_click 이 그 플래그를 보고 SKIP.
        app.world.send_event(bevy::input::touch::TouchInput {
            phase: bevy::input::touch::TouchPhase::Started,
            position: Vec2::new(400.0, 300.0),
            window: w,
            force: None,
            id: 7,
        });
        app.world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);

        app.update();

        // 가드가 동작했다면 마우스 분기 자체가 SKIP 되어 follow 가 그대로 유지된다.
        assert_eq!(app.world.resource::<PlayerPath>().0.len(), 1,
            "터치가 같은 프레임에 있으면 마우스 분기가 SKIP 되어 path 가 비워지지 않는다");
        assert_eq!(app.world.resource::<MouseInteractTarget>().0, Some(InteractTarget::Tile(7, 5)),
            "터치가 같은 프레임에 있으면 마우스 분기가 SKIP 되어 follow 도 유지된다");
    }

    #[test]
    fn 터치가_없는_마우스_클릭은_가드를_지나_Ignore_분기로_follow를_비운다() {
        // 위 테스트의 음성 회귀(negative control):
        // 똑같은 조건에서 터치만 빼면 가드가 작동하지 않아 follow 가 비워지는 게 정상.
        // 헤드리스 viewport 변환이 None → Ignore → follow/path 비움.
        let mut h = ClickHarness::new();
        h.spawn_window_with_cursor(Some(Vec2::new(400.0, 300.0)));
        h.spawn_camera();
        h.spawn_player();
        h.app.world.resource_mut::<PlayerPath>().0.push_back((6, 5));
        h.app.world.resource_mut::<MouseInteractTarget>().0 = Some(InteractTarget::Tile(7, 5));
        h.click();
        h.update();
        assert_eq!(h.app.world.resource::<PlayerPath>().0.len(), 0,
            "터치가 없는 일반 마우스 클릭은 Ignore 분기에서 path 를 비운다");
        assert!(h.app.world.resource::<MouseInteractTarget>().0.is_none(),
            "터치가 없는 일반 마우스 클릭은 Ignore 분기에서 follow 도 비운다");
    }

    // --- apply_click_decision (마우스/터치 공통 적용 헬퍼) ---

    /// apply_click_decision 헬퍼 테스트용 — EventWriter 만 따로 떼어내 채워 본다.
    fn run_apply(decision: ClickDecision) -> (PlayerPath, MouseInteractTarget, Vec<(usize, usize)>) {
        let mut app = App::new();
        app.add_event::<BumpTileEvent>();
        app.init_resource::<PlayerPath>();
        app.init_resource::<MouseInteractTarget>();
        // 이미 follow/path 가 차 있는 상태에서 결정이 어떻게 덮어쓰는지 검증.
        app.world.resource_mut::<PlayerPath>().0.push_back((9, 9));
        app.world.resource_mut::<MouseInteractTarget>().0 = Some(InteractTarget::Tile(9, 9));
        app.add_systems(Update, move |
            mut path: ResMut<PlayerPath>,
            mut target: ResMut<MouseInteractTarget>,
            mut bump: EventWriter<BumpTileEvent>,
        | {
            apply_click_decision(decision.clone(), &mut path, &mut target, &mut bump);
        });
        app.update();
        let path = std::mem::take(&mut *app.world.resource_mut::<PlayerPath>());
        let target = std::mem::take(&mut *app.world.resource_mut::<MouseInteractTarget>());
        let events = app.world.resource::<Events<BumpTileEvent>>();
        let bumps: Vec<(usize, usize)> = events.get_reader().read(events).map(|e| (e.0, e.1)).collect();
        (path, target, bumps)
    }

    #[test]
    fn 결정적용은_Ignore면_follow와_path를_정리한다() {
        let (path, target, bumps) = run_apply(ClickDecision::Ignore);
        assert_eq!(path.0.len(), 0);
        assert!(target.0.is_none());
        assert!(bumps.is_empty());
    }

    #[test]
    fn 결정적용은_Walk면_follow를_비우고_path를_채운다() {
        let (path, target, bumps) = run_apply(ClickDecision::Walk {
            path: VecDeque::from(vec![(6, 5), (7, 5)]),
        });
        assert_eq!(path.0.len(), 2);
        assert!(target.0.is_none(), "Walk 는 follow 가 없다");
        assert!(bumps.is_empty());
    }

    #[test]
    fn 결정적용은_InteractTile면_path와_Tile타겟을_세팅한다() {
        let (path, target, bumps) = run_apply(ClickDecision::InteractTile {
            tile: (7, 7),
            path: VecDeque::from(vec![(6, 6)]),
        });
        assert_eq!(path.0.len(), 1);
        assert_eq!(target.0, Some(InteractTarget::Tile(7, 7)));
        assert!(bumps.is_empty());
    }

    #[test]
    fn 결정적용은_FollowNpc면_path와_NPC타겟을_세팅한다() {
        let mut world = World::new();
        let npc = dummy_entity(&mut world);
        let (path, target, bumps) = run_apply(ClickDecision::FollowNpc {
            entity: npc,
            path: VecDeque::from(vec![(6, 6), (7, 7)]),
        });
        assert_eq!(path.0.len(), 2);
        assert_eq!(target.0, Some(InteractTarget::Npc(npc)));
        assert!(bumps.is_empty());
    }

    #[test]
    fn 결정적용은_ImmediateBump면_즉시_BumpTileEvent를_보내고_follow를_정리한다() {
        let (path, target, bumps) = run_apply(ClickDecision::ImmediateBump { tile: (6, 5) });
        assert_eq!(path.0.len(), 0, "즉시 범프는 path 를 비운다");
        assert!(target.0.is_none(), "즉시 범프는 follow 를 비운다");
        assert_eq!(bumps, vec![(6, 5)], "즉시 BumpTileEvent 발행");
    }

    // --- classify_click (NPC 추적 / 카운터 인접 / 평범 walkable 분기) ---

    /// villager 검색 클로저용 — 더미 entity 를 발급하기 위해 spawn 으로 만든다.
    fn dummy_entity(world: &mut World) -> Entity {
        world.spawn_empty().id()
    }

    #[test]
    fn 클릭타일에_villager가_있으면_그_NPC를_추적_결정한다() {
        // 빈 World 에서 Entity 만 만든다 — 비교용.
        let mut world = World::new();
        let npc = dummy_entity(&mut world);
        let map = click_map();
        let d = classify_click(Some((7, 5)), &map, (5, 5),
            |x, y| if (x, y) == (7, 5) { Some(npc) } else { None });
        match d {
            ClickDecision::FollowNpc { entity, path } => {
                assert_eq!(entity, npc, "클릭 타일의 villager 엔티티");
                assert!(!path.is_empty(), "추적용 초기 경로가 존재");
                assert_eq!(*path.back().unwrap(), (7, 5), "초기 경로 끝은 villager 타일");
            }
            other => panic!("villager 클릭은 FollowNpc 분기여야 한다: {:?}", other),
        }
    }

    #[test]
    fn 클릭타일이_카운터면_인접_walkable로_가는_경로와_타일타겟을_낸다() {
        // 멀리 떨어진 카운터(7,7) 를 (5,5) 에서 클릭 — 인접 walkable 까지 path + InteractTile.
        let mut map = click_map();
        map.set_tile(7, 7, TileKind::Counter);
        // (7,7) 인접 walkable: (6,6),(6,7),(7,6) 등. (5,5)~(7,7) chebyshev=2.
        let d = classify_click(Some((7, 7)), &map, (5, 5), no_villager);
        match d {
            ClickDecision::InteractTile { tile, path } => {
                assert_eq!(tile, (7, 7), "InteractTile 의 좌표는 클릭한 카운터");
                assert!(!path.is_empty(), "멀리 떨어진 카운터는 경로가 비어있지 않다");
            }
            other => panic!("카운터 클릭은 InteractTile 분기여야 한다: {:?}", other),
        }
    }

    #[test]
    fn 이미_인접한_카운터를_클릭하면_즉시_범프_결정이다() {
        // (6,6) 카운터, (5,5) 플레이어 → chebyshev=1 → ImmediateBump.
        let mut map = click_map();
        map.set_tile(6, 6, TileKind::Counter);
        match classify_click(Some((6, 6)), &map, (5, 5), no_villager) {
            ClickDecision::ImmediateBump { tile } => assert_eq!(tile, (6, 6)),
            other => panic!("인접 카운터는 ImmediateBump 분기여야 한다: {:?}", other),
        }
    }

    #[test]
    fn 이미_인접한_NPC를_클릭하면_즉시_범프_결정이다() {
        // (6,5) villager, (5,5) 플레이어 → chebyshev=1 → ImmediateBump.
        let mut world = World::new();
        let npc = dummy_entity(&mut world);
        let map = click_map();
        match classify_click(Some((6, 5)), &map, (5, 5),
            |x, y| if (x, y) == (6, 5) { Some(npc) } else { None })
        {
            ClickDecision::ImmediateBump { tile } => assert_eq!(tile, (6, 5)),
            other => panic!("인접 NPC 는 ImmediateBump 분기여야 한다: {:?}", other),
        }
    }

    #[test]
    fn 클릭타일이_카운터이고_인접_walkable이_없으면_무시한다() {
        // (0,0) 카운터 — 주변이 모두 Wall(맵 기본) 이고 경계 밖.
        let mut map = Map::new(20, 20);
        map.set_tile(0, 0, TileKind::Counter);
        // 플레이어 위치도 도달 불가한 위치.
        let d = classify_click(Some((0, 0)), &map, (10, 10), no_villager);
        assert!(matches!(d, ClickDecision::Ignore), "인접 walkable 이 없으면 Ignore");
    }

    #[test]
    fn 클릭타일이_경계_밖이면_무시한다() {
        let map = click_map();
        let d = classify_click(Some((9999, 9999)), &map, (5, 5), no_villager);
        assert!(matches!(d, ClickDecision::Ignore));
    }

    #[test]
    fn 클릭타일이_도달_불가한_walkable이면_무시한다() {
        // (5,5) 와 (10,10) 둘 다 Floor 지만 연결되지 않은 섬 → path 가 비어 Ignore.
        let mut map = Map::new(20, 20);
        map.set_tile(5, 5, TileKind::Floor);
        map.set_tile(10, 10, TileKind::Floor);
        let d = classify_click(Some((10, 10)), &map, (5, 5), no_villager);
        assert!(matches!(d, ClickDecision::Ignore),
            "도달 불가 walkable 은 Ignore");
    }

    // --- chebyshev_distance / nearest_adjacent_walkable ---

    #[test]
    fn 체비쇼프거리는_8방향_최댓값을_쓴다() {
        assert_eq!(chebyshev_distance((5, 5), (5, 5)), 0);
        assert_eq!(chebyshev_distance((5, 5), (6, 5)), 1, "카디널 인접");
        assert_eq!(chebyshev_distance((5, 5), (6, 6)), 1, "대각 인접");
        assert_eq!(chebyshev_distance((5, 5), (7, 5)), 2);
        assert_eq!(chebyshev_distance((0, 0), (3, 5)), 5);
    }

    #[test]
    fn 인접_walkable탐색은_플레이어와_가장_가까운_타일을_고른다() {
        // (5,5)~(7,7) Floor + (6,6) Counter. 플레이어 (5,5) 의 인접 walkable
        // 후보(카운터 주변) 중 (5,5) 자신이 거리 0 으로 최적.
        let mut map = click_map();
        map.set_tile(6, 6, TileKind::Counter);
        let adj = nearest_adjacent_walkable(&map, (6, 6), (5, 5));
        assert_eq!(adj, Some((5, 5)), "플레이어 자신이 카운터 인접 walkable 중 거리 0");
    }

    #[test]
    fn 인접_walkable이_없으면_None을_반환한다() {
        // (0,0) Counter, 주변 모두 맵 밖이거나 Wall.
        let mut map = Map::new(20, 20);
        map.set_tile(0, 0, TileKind::Counter);
        assert!(nearest_adjacent_walkable(&map, (0, 0), (10, 10)).is_none());
    }

    // --- refresh_follow_path 시스템 (NPC 추적 / 카운터 / despawn / 취소) ---

    /// refresh_follow_path 하네스 — Player, Villager, 모든 이벤트/리소스를 둔다.
    struct FollowHarness {
        app: App,
        player: Entity,
    }

    impl FollowHarness {
        fn new(player_tile: (usize, usize)) -> Self {
            let mut app = App::new();
            app.add_event::<PlayerActedEvent>();
            app.add_event::<BumpTileEvent>();
            app.init_resource::<PlayerPath>();
            app.init_resource::<MouseInteractTarget>();
            let mut map = Map::new(20, 20);
            for y in 0..20 { for x in 0..20 { map.set_tile(x, y, TileKind::Floor); } }
            app.insert_resource(MapResource(map));
            let pos = tile_to_world_coords(player_tile.0, player_tile.1);
            let player = app.world.spawn((
                Player,
                Transform::from_xyz(pos.x, pos.y, 1.0),
            )).id();
            app.add_systems(Update, refresh_follow_path);
            Self { app, player }
        }
        fn spawn_villager_at(&mut self, tile: (usize, usize)) -> Entity {
            self.app.world.spawn((
                Villager {
                    id: "v".into(), name: "촌부".into(),
                    dialogues: vec![], dialogue_idx: 0,
                    tile_x: tile.0, tile_y: tile.1,
                    just_bumped: false, quest_dialogue_idx: 0,
                    base_color: Color::WHITE, home_room: None,
                    stationary: false, vendor: false,
                },
            )).id()
        }
        fn set_target(&mut self, t: InteractTarget) {
            self.app.world.resource_mut::<MouseInteractTarget>().0 = Some(t);
        }
        fn fire_acted(&mut self) {
            self.app.world.send_event(PlayerActedEvent);
        }
        fn update(&mut self) { self.app.update(); }
        fn bump_targets(&mut self) -> Vec<(usize, usize)> {
            let events = self.app.world.resource::<Events<BumpTileEvent>>();
            let mut r = events.get_reader();
            r.read(events).map(|e| (e.0, e.1)).collect()
        }
        fn target(&self) -> Option<InteractTarget> {
            self.app.world.resource::<MouseInteractTarget>().0
        }
        fn path_len(&self) -> usize { self.app.world.resource::<PlayerPath>().0.len() }
    }

    #[test]
    fn 마우스로_NPC를_클릭하면_그_NPC를_추적해_인접에_도달하면_자동으로_말을_건다() {
        // 플레이어 (5,5), villager 인접 (6,5) → chebyshev=1 → 즉시 BumpTileEvent.
        let mut h = FollowHarness::new((5, 5));
        let npc = h.spawn_villager_at((6, 5));
        h.set_target(InteractTarget::Npc(npc));
        h.fire_acted();
        h.update();
        assert_eq!(h.bump_targets(), vec![(6, 5)], "인접 villager 자동 범프");
        assert!(h.target().is_none(), "범프 후 follow 종료");
        assert_eq!(h.path_len(), 0, "범프 후 path 정리");
    }

    #[test]
    fn NPC가_움직이면_경로가_갱신된다() {
        // 플레이어 (5,5), villager 멀리 (12,12) → 경로 재계산. 범프 없음.
        let mut h = FollowHarness::new((5, 5));
        let npc = h.spawn_villager_at((12, 12));
        h.set_target(InteractTarget::Npc(npc));
        h.fire_acted();
        h.update();
        assert!(h.bump_targets().is_empty(), "거리 멀면 범프 안 함");
        assert!(matches!(h.target(), Some(InteractTarget::Npc(_))), "follow 유지");
        assert!(h.path_len() > 0, "재계산된 경로가 채워져야 한다");
        // villager 위치를 옮긴 뒤 다시 acted → 경로 다시 계산
        h.app.world.get_mut::<Villager>(npc).unwrap().tile_x = 10;
        h.app.world.get_mut::<Villager>(npc).unwrap().tile_y = 10;
        h.fire_acted();
        h.update();
        let path = &h.app.world.resource::<PlayerPath>().0;
        assert_eq!(*path.back().unwrap(), (10, 10), "갱신된 경로의 끝이 새 villager 타일");
    }

    #[test]
    fn 추적하던_villager가_despawn되면_follow가_안전하게_종료된다() {
        let mut h = FollowHarness::new((5, 5));
        let npc = h.spawn_villager_at((12, 12));
        h.set_target(InteractTarget::Npc(npc));
        // villager 를 미리 despawn
        h.app.world.despawn(npc);
        h.fire_acted();
        h.update(); // 패닉하지 않으면 통과
        assert!(h.target().is_none(), "despawn 시 follow 종료");
        assert_eq!(h.path_len(), 0, "despawn 시 path 정리");
    }

    #[test]
    fn 카운터타일을_타겟으로_두고_인접하면_자동으로_범프한다() {
        // 플레이어 (5,5), 카운터 (6,5) → chebyshev=1 → 즉시 BumpTileEvent((6,5)).
        let mut h = FollowHarness::new((5, 5));
        h.set_target(InteractTarget::Tile(6, 5));
        h.fire_acted();
        h.update();
        assert_eq!(h.bump_targets(), vec![(6, 5)], "인접 시 카운터 자동 범프");
        assert!(h.target().is_none(), "범프 후 target 정리");
    }

    #[test]
    fn 카운터타일_타겟은_멀면_그대로_둔다() {
        let mut h = FollowHarness::new((5, 5));
        h.set_target(InteractTarget::Tile(15, 15));
        h.fire_acted();
        h.update();
        assert!(h.bump_targets().is_empty(), "거리 멀면 범프 안 함");
        assert!(matches!(h.target(), Some(InteractTarget::Tile(15, 15))), "follow 유지");
    }

    #[test]
    fn refresh는_PlayerActedEvent가_없으면_아무것도_안한다() {
        let mut h = FollowHarness::new((5, 5));
        let npc = h.spawn_villager_at((6, 5));
        h.set_target(InteractTarget::Npc(npc));
        h.update(); // acted 안 보냄
        assert!(h.bump_targets().is_empty(), "acted 없으면 범프 없음");
        assert!(matches!(h.target(), Some(InteractTarget::Npc(_))), "target 유지");
    }

    #[test]
    fn refresh는_target이_없으면_아무것도_안한다() {
        let mut h = FollowHarness::new((5, 5));
        h.fire_acted();
        h.update();
        assert!(h.bump_targets().is_empty());
        assert!(h.target().is_none());
    }

    #[test]
    fn refresh는_플레이어가_없으면_target과_path를_정리한다() {
        let mut h = FollowHarness::new((5, 5));
        let npc = h.spawn_villager_at((6, 5));
        h.set_target(InteractTarget::Npc(npc));
        // Player 컴포넌트 제거(=쿼리 실패)
        h.app.world.entity_mut(h.player).remove::<Player>();
        h.fire_acted();
        h.update();
        assert!(h.target().is_none(), "플레이어 없음 → follow 정리");
        assert_eq!(h.path_len(), 0);
    }

    // --- follow 취소: 키보드 이동 / 스페이스 / 새 클릭 / 리스폰 ---

    #[test]
    fn 키보드_이동이_들어오면_follow가_취소된다() {
        let mut h = MoveHarness::new(20, 20);
        h.set_tile(6, 5, TileKind::Floor);
        // follow 와 path 를 미리 채워둔다
        h.app.world.resource_mut::<MouseInteractTarget>().0
            = Some(InteractTarget::Tile(10, 10));
        h.app.world.resource_mut::<PlayerPath>().0.push_back((6, 5));
        h.press(KeyCode::ArrowRight);
        h.update();
        assert!(h.app.world.resource::<MouseInteractTarget>().0.is_none(),
            "키 입력 시 follow 가 취소되어야 한다");
        assert_eq!(h.path_len(), 0, "키 입력 시 path 도 취소");
    }

    #[test]
    fn 스페이스로_대기하면_follow가_취소된다() {
        let mut h = MoveHarness::new(20, 20);
        h.app.world.resource_mut::<MouseInteractTarget>().0
            = Some(InteractTarget::Tile(10, 10));
        h.app.world.resource_mut::<PlayerPath>().0.push_back((10, 10));
        h.press(KeyCode::Space);
        h.update();
        assert!(h.app.world.resource::<MouseInteractTarget>().0.is_none());
        assert_eq!(h.path_len(), 0);
    }

    #[test]
    fn 자동경로상_장애물_범프시_follow도_함께_정리된다() {
        let mut h = run_path_step(|h| {
            h.set_tile(6, 5, TileKind::Floor);
            h.app.world.resource_mut::<OccupiedTiles>().0.insert((6, 5));
            h.app.world.resource_mut::<PlayerPath>().0.push_back((6, 5));
            h.app.world.resource_mut::<MouseInteractTarget>().0
                = Some(InteractTarget::Npc(Entity::from_raw(123)));
        });
        assert!(h.app.world.resource::<MouseInteractTarget>().0.is_none(),
            "장애물 범프 시 follow 도 정리");
        assert_eq!(h.bump_targets(), vec![(6, 5)]);
    }

    #[test]
    fn 자동경로상_몬스터_공격시_follow도_함께_정리된다() {
        let mut h = run_path_step(|h| {
            h.set_tile(6, 5, TileKind::Floor);
            h.app.world.resource_mut::<MonsterTiles>().0.insert((6, 5));
            h.app.world.resource_mut::<PlayerPath>().0.push_back((6, 5));
            h.app.world.resource_mut::<MouseInteractTarget>().0
                = Some(InteractTarget::Npc(Entity::from_raw(123)));
        });
        assert!(h.app.world.resource::<MouseInteractTarget>().0.is_none(),
            "몬스터 공격 시 follow 도 정리");
        assert_eq!(h.attack_targets(), vec![(6, 5)]);
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
            .init_resource::<MouseInteractTarget>()
            .init_resource::<PlayerProgress>()
            .configure_sets(Update, PlayerSystemSet::MovementComplete.after(PlayerSystemSet::Movement))
            .add_systems(Startup, spawn_player.after(draw_map))
            .add_systems(Update, (
                // 터치 탭은 마우스 좌클릭과 동일 흐름이지만 입력 채널만 다르다.
                // on_mouse_click 보다 먼저 돌려, 같은 프레임 합성 마우스 클릭이 와도
                // on_mouse_click 의 Touches 가드가 SKIP 하게 한다 — 중복 적용 방지.
                on_touch_tap.before(on_mouse_click),
                on_mouse_click.before(PlayerSystemSet::Movement),
                player_movement.in_set(PlayerSystemSet::Movement),
                smooth_player_lerp.in_set(PlayerSystemSet::MovementComplete),
                // 추적 갱신은 villager_turn 직후(VillagerSystemSet::Turn 뒤) 돌려, NPC 의
                // 이번 턴 이동 결과(tile_x/tile_y 갱신본) 위에서 경로/인접 판정을 한다.
                // 이렇게 해야 "플레이어가 NPC 쫓아가지만 NPC 가 옆 칸으로 떠나 빈자리에 도착"
                // 하는 회귀가 발생하지 않고, 인접 시 즉시 BumpTileEvent 가 다음 프레임
                // handle_bump 로 연결된다.
                refresh_follow_path
                    .after(PlayerSystemSet::MovementComplete)
                    .after(VillagerSystemSet::Turn),
                update_fov.after(PlayerSystemSet::MovementComplete),
                camera_follow_player.after(update_fov),
                update_player_bars,
                respawn_player_on_regen.after(MapSystemSet::ExecuteRegen),
            ));
    }
}
