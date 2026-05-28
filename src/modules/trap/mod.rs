use bevy::prelude::*;
use rand::seq::SliceRandom;
use crate::modules::{
    map::{
        Map, MapResource, UsedSpawnTiles,
        PlayerActedEvent, TILE_SIZE,
        tile_to_world_coords, world_to_tile_coords, random_floor_tile_anywhere,
    },
    combat::{CombatStats, Defeated},
    player::Player,
    monster::{Monster, PlayerDetectedEvent},
    elemental::{Element, ElementalApplyEvent},
    item::{PlayerEquipment, AccessoryKind},
    ui::LogMessage,
};

/// 액세서리 슬롯 ID — 착용 시 시야 내 숨김 함정을 자동 노출.
pub const TRAP_SCOPE_ID: &str = "trap_scope";

/// trap_scope 효과 반경 (체비쇼프). 일반 reveal_hidden_traps(인접 1) 보다 훨씬 넓다.
pub const TRAP_SCOPE_RADIUS: i32 = 8;

/// `(equipment)` 가 함정 등불(`trap_scope`)을 착용 중인지 (순수 함수).
pub fn player_has_trap_scope(equipment: &PlayerEquipment) -> bool {
    equipment.accessory == Some(AccessoryKind(TRAP_SCOPE_ID))
}

// ── 데이터 ───────────────────────────────────────────────────────────────────

/// 함정의 종류. 각 종류는 발동 시 서로 다른 효과를 낸다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize)]
pub enum TrapKind {
    /// 가시 함정 — 밟은 대상에게 즉시 피해.
    Spike,
    /// 독 함정 — 밟은 대상에게 독 상태(ElementalApplyEvent(Poison))를 부여.
    Poison,
    /// 경보 함정 — `stealth_blown` 플래그를 세우고 주변 가드를 경계시킨다
    /// (PlayerDetectedEvent 재사용). 잠입 구역 시너지.
    Alarm,
    /// 전이 함정 — 밟은 대상을 맵의 무작위 통과 타일로 순간이동시킨다.
    Teleport,
}

impl TrapKind {
    /// 표시용 한글 이름.
    pub fn name_ko(self) -> &'static str {
        match self {
            TrapKind::Spike    => "가시 함정",
            TrapKind::Poison   => "독 함정",
            TrapKind::Alarm    => "경보 함정",
            TrapKind::Teleport => "전이 함정",
        }
    }

    /// 노출된 함정의 글리프(숨김 함정은 글리프를 표시하지 않는다).
    pub fn glyph(self) -> &'static str {
        match self {
            TrapKind::Spike    => "^",
            TrapKind::Poison   => "*",
            TrapKind::Alarm    => "!",
            TrapKind::Teleport => "&",
        }
    }

    /// 노출된 함정의 렌더 색.
    pub fn color(self) -> Color {
        match self {
            TrapKind::Spike    => Color::rgb(0.8, 0.8, 0.85),
            TrapKind::Poison   => Color::rgb(0.4, 0.85, 0.3),
            TrapKind::Alarm    => Color::rgb(1.0, 0.85, 0.1),
            TrapKind::Teleport => Color::rgb(0.7, 0.4, 1.0),
        }
    }

    /// 가시 함정이 한 번에 주는 피해량.
    pub fn spike_damage(self) -> i32 { 8 }

    /// 발동 후 함정이 사라지는지(1회성) 여부.
    /// 가시/전이는 1회성, 독/경보는 지속(여러 번 발동)으로 둔다.
    pub fn is_one_shot(self) -> bool {
        matches!(self, TrapKind::Spike | TrapKind::Teleport)
    }
}

/// 타일 위에 놓인 함정 엔티티. 좌표(`tile_x`,`tile_y`)와 종류, 숨김 여부를 가진다.
#[derive(Component, Debug)]
pub struct Trap {
    pub kind: TrapKind,
    pub tile_x: usize,
    pub tile_y: usize,
    pub hidden: bool,
}

/// 플레이어가 설치한 "아군" 함정 마커(§B-2). 이 마커가 붙은 함정은
/// 몬스터 진입에만 발동하고 플레이어 자신은 밟아도 발동하지 않는다.
/// 함정 컴포넌트/발동 로직은 기존 `Trap` 을 그대로 재사용하고, 발동 주체
/// 구분만 이 마커로 한다(특수 케이스 예외처리 대신 마커로 일관 처리).
#[derive(Component, Debug)]
pub struct PlayerTrap;

// ── 이벤트 ───────────────────────────────────────────────────────────────────

/// 함정을 스폰하라는 요청. 생성기/퀘스트(`PlaceTraps`) 등이 발행한다.
/// `count` 마리를 현재 맵의 무작위 통과 타일에 배치한다.
#[derive(Event)]
pub struct SpawnTrapEvent {
    pub kind: TrapKind,
    pub count: u32,
    /// 스폰 시 숨김(true) 여부.
    pub hidden: bool,
}

/// 함정이 발동했음을 알리는 이벤트(로그/연출/통계용).
#[derive(Event, Debug, PartialEq, Eq)]
pub struct TrapTriggeredEvent {
    pub kind: TrapKind,
    pub tile_x: usize,
    pub tile_y: usize,
    /// 발동시킨 대상이 플레이어면 true.
    pub by_player: bool,
}

// ── 순수 판정 함수 ───────────────────────────────────────────────────────────

/// 대상 좌표 `(ex, ey)` 가 함정 좌표 `(tx, ty)` 와 같으면 발동(true).
/// 진입 판정을 한 곳에 모아 단독 테스트할 수 있게 한 순수 함수다.
pub fn trap_triggers_at(tx: usize, ty: usize, ex: usize, ey: usize) -> bool {
    tx == ex && ty == ey
}

/// 숨김 함정이 노출돼야 하는지 판정한다(순수 함수).
///
/// 이미 노출(`!hidden`)이면 그대로 노출 유지. 숨김이면 플레이어와의
/// 체비쇼프 거리가 `reveal_dist` 이하일 때(인접/근접) 노출한다.
pub fn should_reveal(hidden: bool, tx: usize, ty: usize, px: usize, py: usize, reveal_dist: i32) -> bool {
    if !hidden {
        return true;
    }
    let dx = (tx as i32 - px as i32).abs();
    let dy = (ty as i32 - py as i32).abs();
    dx.max(dy) <= reveal_dist
}

/// 맵에서 무작위 통과 타일을 하나 골라 전이 함정의 목적지로 반환한다(순수 함수).
///
/// `exclude` 에 든 좌표(현재 위치 등)는 제외한다. 통과 가능한 타일이 하나도
/// 없으면(또는 전부 제외되면) None.
pub fn random_teleport_destination(
    map: &Map,
    exclude: (usize, usize),
    rng: &mut impl rand::Rng,
) -> Option<(usize, usize)> {
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    for y in 0..map.height {
        for x in 0..map.width {
            if (x, y) != exclude && map.get_tile(x, y).is_walkable() {
                candidates.push((x, y));
            }
        }
    }
    candidates.choose(rng).copied()
}

// ── 순수 판정 함수: 플레이어 함정 설치/해제 (§B-2) ────────────────────────────

/// 플레이어 함정을 `(tx, ty)` 에 설치할 수 있는지 판정한다(순수 함수).
///
/// - 맵 범위 안이며 통과 가능한(`is_walkable`) 타일이어야 한다(벽/물 불가).
/// - 몬스터·주민 등 다른 엔티티가 점유하지 않아야 한다(`occupied`).
/// 점유/지형 판정만 떼어 단독 테스트할 수 있게 한 순수 함수다.
pub fn can_place_trap(
    map: &Map,
    occupied: &std::collections::HashSet<(usize, usize)>,
    tx: usize, ty: usize,
) -> bool {
    if tx >= map.width || ty >= map.height {
        return false;
    }
    if !map.get_tile(tx, ty).is_walkable() {
        return false;
    }
    !occupied.contains(&(tx, ty))
}

/// 해제 성공 여부를 판정한다(순수 함수).
///
/// 해제 도구(`has_tool`)를 들고 있으면 항상 확정 성공. 도구가 없으면
/// `roll`(0.0..1.0) 이 `DISARM_CHANCE_NO_TOOL` 미만일 때만 성공한다.
pub fn disarm_succeeds(has_tool: bool, roll: f32) -> bool {
    has_tool || roll < DISARM_CHANCE_NO_TOOL
}

/// 해제 도구 없이 맨손으로 해제를 시도할 때의 성공 확률.
pub const DISARM_CHANCE_NO_TOOL: f32 = 0.5;

/// 함정 노출 거리(체비쇼프). 이 거리 이하로 플레이어가 접근하면 숨김 함정이 드러난다.
pub const REVEAL_DIST: i32 = 1;

/// 함정 글리프의 Z. 타일(0.0) 위, 아이템(0.3) 아래에 깔아 바닥 표식처럼 보이게 한다.
const Z_TRAP: f32 = 0.25;

mod player_trap;
pub use player_trap::PlayerTrapPlugin;

// ── 플러그인 ─────────────────────────────────────────────────────────────────

pub struct TrapPlugin;

impl Plugin for TrapPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<SpawnTrapEvent>()
            .add_event::<TrapTriggeredEvent>()
            // monster/quest 플러그인이 이미 등록하지만 단독 테스트에서도
            // 동작하도록 여기서도 보장한다.
            .add_event::<PlayerDetectedEvent>()
            .add_event::<ElementalApplyEvent>()
            // reveal_traps_by_lens 가 읽는 PlayerEquipment — ItemPlugin 이 보통
            // 등록하지만 단독 테스트에서도 동작하도록 여기서도 init 한다.
            .init_resource::<PlayerEquipment>()
            .add_systems(Update, (
                handle_spawn_trap,
                trigger_traps,
                reveal_hidden_traps,
                reveal_traps_by_lens,
            ));
    }
}

// ── 시스템 ───────────────────────────────────────────────────────────────────

/// `SpawnTrapEvent` 를 받아 현재 맵의 무작위 통과 타일에 함정 엔티티를 배치한다.
fn handle_spawn_trap(
    mut commands: Commands,
    mut events: EventReader<SpawnTrapEvent>,
    map_res: Res<MapResource>,
    asset_server: Res<AssetServer>,
    mut used_spawn: ResMut<UsedSpawnTiles>,
) {
    let map = map_res.map();
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");
    let mut rng = rand::thread_rng();
    for ev in events.read() {
        for _ in 0..ev.count {
            let Some((tx, ty)) = random_floor_tile_anywhere(&map.rooms, map, &mut used_spawn.0, &mut rng) else {
                info!("함정 스폰 실패 — 통과타일 없음: {}", ev.kind.name_ko());
                continue;
            };
            spawn_trap_entity(&mut commands, &font, ev.kind, tx, ty, ev.hidden);
        }
    }
}

/// 함정 한 개를 (tx, ty) 에 엔티티로 스폰한다. 숨김 함정은 글리프를 감춘다
/// (Visibility::Hidden). 자연 스폰·퀘스트가 공유하는 단일 생성 경로.
/// 스폰한 엔티티 id 를 돌려줘 호출부가 마커(예: PlayerTrap)를 덧붙일 수 있게 한다.
fn spawn_trap_entity(
    commands: &mut Commands,
    font: &Handle<Font>,
    kind: TrapKind,
    tx: usize, ty: usize,
    hidden: bool,
) -> Entity {
    let coord = tile_to_world_coords(tx, ty);
    commands.spawn((
        Text2dBundle {
            text: Text::from_section(kind.glyph(), TextStyle {
                font: font.clone(),
                font_size: TILE_SIZE,
                color: kind.color(),
            }),
            transform: Transform::from_xyz(coord.x, coord.y, Z_TRAP),
            visibility: if hidden { Visibility::Hidden } else { Visibility::Visible },
            ..default()
        },
        Trap { kind, tile_x: tx, tile_y: ty, hidden },
    )).id()
}

/// 매 턴(`PlayerActedEvent`) 플레이어·몬스터 위치와 함정 타일을 비교해 발동시킨다.
///
/// 효과는 종류별로 분리한 `apply_trap_effect` 가 처리하고, 발동 후 1회성 함정은
/// despawn 한다. 한 타일에 여러 함정이 있어도 각각 독립적으로 판정한다.
#[allow(clippy::too_many_arguments)]
fn trigger_traps(
    mut commands: Commands,
    mut events: EventReader<PlayerActedEvent>,
    mut trap_query: Query<(Entity, &mut Trap, &mut Visibility, Has<PlayerTrap>)>,
    mut player_query: Query<(Entity, &mut Transform, &mut CombatStats), With<Player>>,
    monster_query: Query<&Monster>,
    mut elemental: EventWriter<ElementalApplyEvent>,
    mut detected: EventWriter<PlayerDetectedEvent>,
    mut triggered: EventWriter<TrapTriggeredEvent>,
    mut log: EventWriter<LogMessage>,
    map_res: Res<MapResource>,
) {
    if events.read().next().is_none() { return; }

    // 플레이어 엔티티/현재 타일 (이동 완료 후 좌표). 없으면(테스트 등) None.
    let player_info: Option<(Entity, (usize, usize))> = player_query.get_single()
        .ok()
        .map(|(e, t, _)| (e, world_to_tile_coords(t.translation)));

    let mut rng = rand::thread_rng();

    for (trap_entity, mut trap, mut vis, is_player_trap) in trap_query.iter_mut() {
        // 플레이어 발동 우선 판정. 단, 플레이어가 설치한 아군 함정(PlayerTrap)은
        // 플레이어 자신을 발동시키지 않는다(§B-2 — 몬스터 진입 시에만 발동).
        let player_on = !is_player_trap && player_info
            .map(|(_, (px, py))| trap_triggers_at(trap.tile_x, trap.tile_y, px, py))
            .unwrap_or(false);
        // 몬스터 발동 — 같은 타일의 살아있는 몬스터가 있으면 발동.
        let monster_on = monster_query.iter()
            .any(|m| trap_triggers_at(trap.tile_x, trap.tile_y, m.tile_x, m.tile_y));

        if !player_on && !monster_on { continue; }

        // 발동되면 항상 노출된다.
        trap.hidden = false;
        *vis = Visibility::Visible;

        // 효과 적용 — 종류별. 플레이어가 밟았으면(`player_on`) player_info 가
        // 반드시 Some 이므로 unwrap 안전.
        apply_trap_effect(
            trap.kind, player_on, player_info,
            &mut player_query,
            &mut elemental, &mut detected, &mut log,
            map_res.map(), &mut rng, &mut commands,
        );

        triggered.send(TrapTriggeredEvent {
            kind: trap.kind,
            tile_x: trap.tile_x,
            tile_y: trap.tile_y,
            by_player: player_on,
        });
        log.send(LogMessage(format!(
            "{} 발동! ({}, {})", trap.kind.name_ko(), trap.tile_x, trap.tile_y
        )));

        if trap.kind.is_one_shot() {
            commands.entity(trap_entity).despawn();
        }
    }
}

/// 함정 종류별 효과를 적용한다. 플레이어가 밟았으면(`by_player`) 플레이어에게
/// 효과를 주고, 그렇지 않으면(몬스터만 밟음) 대상이 있는 효과는 건너뛴다.
/// `by_player` 가 참이면 `player_info` 는 항상 Some 이다(호출부가 보장).
///
/// - Spike: 플레이어 HP 차감(0 이하면 Defeated).
/// - Poison: 플레이어에게 Poison 원소 부여.
/// - Alarm: PlayerDetectedEvent 발행(quest 모듈이 stealth_blown set + 가드 경계).
/// - Teleport: 플레이어를 무작위 통과 타일로 이동.
#[allow(clippy::too_many_arguments)]
fn apply_trap_effect(
    kind: TrapKind,
    by_player: bool,
    player_info: Option<(Entity, (usize, usize))>,
    player_query: &mut Query<(Entity, &mut Transform, &mut CombatStats), With<Player>>,
    elemental: &mut EventWriter<ElementalApplyEvent>,
    detected: &mut EventWriter<PlayerDetectedEvent>,
    log: &mut EventWriter<LogMessage>,
    map: &Map,
    rng: &mut impl rand::Rng,
    commands: &mut Commands,
) {
    // Alarm 은 대상이 필요 없으므로 player 유무와 무관하게 처리한다.
    // 경보는 누가 밟든(플레이어/몬스터) 경계를 울린다 — stealth_blown 재사용.
    if kind == TrapKind::Alarm {
        detected.send(PlayerDetectedEvent);
        return;
    }

    // 그 외 종류(Spike/Poison/Teleport)는 플레이어가 밟았을 때만 효과가 있다.
    if !by_player { return; }
    let (player_entity, (px, py)) = player_info
        .expect("player_on 이 참이면 player_info 는 Some 이다");

    match kind {
        TrapKind::Spike => {
            // player_on 참이면 query 에 플레이어가 있다 — mut 접근.
            let mut stats = player_query.get_mut(player_entity).expect("플레이어 CombatStats").2;
            stats.hp -= kind.spike_damage();
            if stats.hp <= 0 {
                commands.entity(player_entity).insert(Defeated);
                log.send(LogMessage("가시 함정에 찔려 사망했습니다...".into()));
            }
        }
        TrapKind::Poison => {
            elemental.send(ElementalApplyEvent { target: player_entity, element: Element::Poison });
        }
        TrapKind::Teleport => {
            if let Some((nx, ny)) = random_teleport_destination(map, (px, py), rng) {
                let wp = tile_to_world_coords(nx, ny);
                commands.entity(player_entity).insert(Transform::from_xyz(wp.x, wp.y, 1.0));
            }
        }
        // Alarm 은 위 early-return 에서 처리됨. 정상 입력으론 도달 불가. // 도달 불가 방어코드
        TrapKind::Alarm => unreachable!("Alarm 은 위에서 처리"),
    }
}

/// 숨김 함정 노출 — 플레이어가 `REVEAL_DIST` 이하로 접근하면 글리프를 드러낸다.
/// 매 프레임 플레이어 위치 기준으로 평가한다.
fn reveal_hidden_traps(
    mut trap_query: Query<(&mut Trap, &mut Visibility)>,
    player_query: Query<&Transform, With<Player>>,
) {
    let Ok(transform) = player_query.get_single() else { return };
    let (px, py) = world_to_tile_coords(transform.translation);
    for (mut trap, mut vis) in trap_query.iter_mut() {
        if !trap.hidden { continue; }
        if should_reveal(trap.hidden, trap.tile_x, trap.tile_y, px, py, REVEAL_DIST) {
            trap.hidden = false;
            *vis = Visibility::Visible;
        }
    }
}

/// `trap_scope`(광부의 등불) 액세서리 착용 시, 인접 제약 없이 반경 `TRAP_SCOPE_RADIUS`
/// 이내의 모든 숨김 함정을 즉시 드러낸다. 미착용이면 no-op.
///
/// `reveal_hidden_traps` 의 인접 1칸 노출과 독립적으로 동작해, 두 시스템이
/// 같은 함정을 양쪽에서 노출시켜도 idempotent (이미 노출이면 그대로 둠).
fn reveal_traps_by_lens(
    mut trap_query: Query<(&mut Trap, &mut Visibility)>,
    player_query: Query<&Transform, With<Player>>,
    equipment: Res<PlayerEquipment>,
) {
    if !player_has_trap_scope(&equipment) { return; }
    let Ok(transform) = player_query.get_single() else { return };
    let (px, py) = world_to_tile_coords(transform.translation);
    for (mut trap, mut vis) in trap_query.iter_mut() {
        if !trap.hidden { continue; }
        if should_reveal(trap.hidden, trap.tile_x, trap.tile_y, px, py, TRAP_SCOPE_RADIUS) {
            trap.hidden = false;
            *vis = Visibility::Visible;
        }
    }
}

#[cfg(test)]
mod tests;
