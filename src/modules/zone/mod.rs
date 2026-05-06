use bevy::prelude::*;
use std::collections::HashMap;
use crate::modules::{
    map::{
        Map, MapResource, MapGeneratorRegistry, ApplyMapEvent,
        MAP_WIDTH, MAP_HEIGHT, TileKind, tile_to_world_coords, TILE_SIZE,
        world_to_tile_coords, GlobalTurn, GlobalSeed, zone_seed_from_idx,
    },
    player::{MovingTo, PlayerSystemSet},
    map::{UsedSpawnTiles, random_floor_tile_in_room},
    combat_feedback::{BloodStain, Z_BLOOD},
};

// ── ZoneId ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ZoneId {
    Town,
    Forest,
    Dungeon(u32),
    /// 퀘스트가 동적으로 생성하는 명명된 존 (ex: Named("desert"))
    Named(String),
}

impl ZoneId {
    pub fn display_name(&self) -> String {
        match self {
            ZoneId::Town       => "마을".into(),
            ZoneId::Forest     => "숲".into(),
            ZoneId::Dungeon(n) => format!("던전 {}층", n),
            ZoneId::Named(n)   => n.clone(),
        }
    }

    pub fn algorithm(&self) -> &'static str {
        match self {
            ZoneId::Town       => "organic_village",
            ZoneId::Forest     => "forest",
            ZoneId::Dungeon(_) => "bsp",
            ZoneId::Named(_)   => "bsp", // 실제 생성기는 NamedZoneConfig에서 조회
        }
    }
}

/// global_seed 와 ZoneId 로부터 해당 존의 맵 시드를 파생한다.
pub fn zone_seed(global_seed: u64, zone_id: &ZoneId) -> u64 {
    let idx: u64 = match zone_id {
        ZoneId::Town       => 0,
        ZoneId::Forest     => 1,
        ZoneId::Dungeon(n) => 100 + *n as u64,
        ZoneId::Named(s)   => {
            // FNV-1a — 안정적이고 표준 해시
            let mut h: u64 = 0xcbf29ce484222325;
            for b in s.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            h
        }
    };
    zone_seed_from_idx(global_seed, idx)
}

// ── NamedZoneConfig ───────────────────────────────────────────────────────────

/// 퀘스트 포탈이 동적으로 등록하는 Named 존 설정
#[derive(Clone)]
pub struct NamedZoneEntry {
    pub generator: String,
    pub origin: ZoneId,
}

#[derive(Resource, Default)]
pub struct NamedZoneConfig {
    pub zones: HashMap<String, NamedZoneEntry>,
}

/// 퀘스트 액션이 발행 → handle_spawn_quest_portal 시스템이 처리
#[derive(Event)]
pub struct SpawnQuestPortalEvent {
    pub zone: String,
    pub generator: String,
}

// ── ZonePersistence ──────────────────────────────────────────────────────────

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct SavedBloodStain {
    pub tile_x: usize,
    pub tile_y: usize,
    pub alpha: f32,
    pub decay_per_turn: f32,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct MonsterSlot {
    pub data_idx: usize,
    pub respawn_at_turn: Option<u64>,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ZoneSnapshot {
    pub blood_stains: Vec<SavedBloodStain>,
    pub monster_slots: Vec<MonsterSlot>,
    pub last_visited_turn: u64,
}

#[derive(Resource, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ZonePersistence(pub HashMap<ZoneId, ZoneSnapshot>);

// ── WorldState ───────────────────────────────────────────────────────────────

#[derive(Resource)]
pub struct WorldState {
    pub current: ZoneId,
    pub maps: HashMap<ZoneId, Map>,
}

impl Default for WorldState {
    fn default() -> Self {
        Self { current: ZoneId::Town, maps: HashMap::new() }
    }
}

impl WorldState {
    pub fn cache_current(&mut self, map: Map) {
        self.maps.insert(self.current.clone(), map);
    }
    #[allow(dead_code)]
    pub fn get_cached(&self, id: &ZoneId) -> Option<&Map> {
        self.maps.get(id)
    }
}

// ── ZonePortal ───────────────────────────────────────────────────────────────

#[derive(Component)]
pub struct ZonePortal {
    pub target: ZoneId,
    /// 이 포털을 통해 도착했을 때 플레이어 스폰 위치
    pub arrive_from: PortalDirection,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PortalDirection {
    North,   // 맵 북쪽 경계 → 도착 시 남쪽에서 스폰
    South,   // 맵 남쪽 경계 → 도착 시 북쪽에서 스폰
    StairDown,
    StairUp,
}

impl PortalDirection {
    fn glyph(&self) -> &'static str {
        match self {
            PortalDirection::North | PortalDirection::South => "⬡",
            PortalDirection::StairDown => ">",
            PortalDirection::StairUp   => "<",
        }
    }
}

// ── Events ───────────────────────────────────────────────────────────────────

#[derive(Event)]
pub struct ZoneTransitionEvent {
    pub target: ZoneId,
    pub arrive_from: PortalDirection,
}

// ── Plugin ───────────────────────────────────────────────────────────────────

pub struct ZonePlugin;

impl Plugin for ZonePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldState>()
            .init_resource::<ZonePersistence>()
            .init_resource::<NamedZoneConfig>()
            .add_event::<ZoneTransitionEvent>()
            .add_event::<SpawnQuestPortalEvent>()
            .add_systems(Startup, cache_initial_map.after(crate::modules::map::draw_map))
            .add_systems(Update, (
                handle_spawn_quest_portal,
                check_portal_collision,
                handle_zone_transition,
                spawn_portals_after_apply,
                restore_zone_state,
                discover_portals_in_fov,
            ).chain().after(PlayerSystemSet::MovementComplete));
    }
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// 시작 맵을 Town 으로 WorldState 에 등록한다
fn cache_initial_map(
    map_res: Res<MapResource>,
    mut world: ResMut<WorldState>,
    mut registry: ResMut<MapGeneratorRegistry>,
) {
    registry.select_by_name(ZoneId::Town.algorithm());
    world.cache_current(map_res.0.clone());
}

/// 플레이어가 ZonePortal 위치에 있으면 ZoneTransitionEvent 발행
fn check_portal_collision(
    player_q: Query<(&Transform, Option<&MovingTo>), With<crate::modules::player::Player>>,
    portal_q: Query<(&Transform, &ZonePortal)>,
    mut ev: EventWriter<ZoneTransitionEvent>,
    mut triggered: Local<bool>,
    acted: EventReader<crate::modules::map::PlayerActedEvent>,
) {
    if acted.is_empty() {
        *triggered = false;
        return;
    }
    if *triggered { return; }

    let Ok((pt, moving_to)) = player_q.get_single() else { return };
    // MovingTo 목적지 우선 사용 — Transform 은 lerp 중간값이라 실제 타일과 불일치
    let (px, py) = moving_to
        .map(|m| world_to_tile_coords(m.target))
        .unwrap_or_else(|| world_to_tile_coords(pt.translation));

    for (portal_t, portal) in portal_q.iter() {
        let (tx, ty) = world_to_tile_coords(portal_t.translation);
        if px == tx && py == ty {
            ev.send(ZoneTransitionEvent {
                target: portal.target.clone(),
                arrive_from: portal.arrive_from.clone(),
            });
            *triggered = true;
            return;
        }
    }
}

/// ZoneTransitionEvent 를 받아 맵을 교체하고 ApplyMapEvent 를 발행한다
fn handle_zone_transition(
    mut ev: EventReader<ZoneTransitionEvent>,
    mut world: ResMut<WorldState>,
    map_res: Res<MapResource>,
    mut apply_ev: EventWriter<ApplyMapEvent>,
    mut registry: ResMut<MapGeneratorRegistry>,
    portals: Query<Entity, With<ZonePortal>>,
    mut commands: Commands,
    mut log: EventWriter<crate::modules::ui::LogMessage>,
    blood_query: Query<(Entity, &BloodStain, &Transform)>,
    global_turn: Res<GlobalTurn>,
    mut persistence: ResMut<ZonePersistence>,
    named_config: Res<NamedZoneConfig>,
    global_seed: Res<GlobalSeed>,
) {
    for transition in ev.read() {
        // 떠나는 존의 혈흔을 스냅샷에 저장하고 엔티티 제거
        let from_zone = world.current.clone();
        let snapshot = persistence.0.entry(from_zone).or_default();
        snapshot.blood_stains = blood_query.iter().map(|(_, stain, transform)| {
            let (tx, ty) = world_to_tile_coords(transform.translation);
            SavedBloodStain { tile_x: tx, tile_y: ty, alpha: stain.alpha, decay_per_turn: stain.decay_per_turn }
        }).collect();
        snapshot.last_visited_turn = global_turn.0;
        for (entity, _, _) in blood_query.iter() {
            commands.entity(entity).despawn();
        }

        // 현재 맵 캐시에 저장
        world.cache_current(map_res.0.clone());

        let target = transition.target.clone();

        // 캐시된 맵 사용 or 새로 생성
        let map = if let Some(cached) = world.maps.get(&target) {
            cached.clone()
        } else {
            // Named 존은 NamedZoneConfig 에서 생성기를 조회한다
            let algo = if let ZoneId::Named(ref name) = target {
                named_config.zones.get(name)
                    .map(|e| e.generator.clone())
                    .unwrap_or_else(|| "bsp".to_string())
            } else {
                target.algorithm().to_string()
            };
            registry.select_by_name(&algo);
            let seed = zone_seed(global_seed.0, &target);
            let mut new_map = registry.current()
                .map(|g| g.generate(MAP_WIDTH, MAP_HEIGHT, seed))
                .unwrap_or_else(|| Map::new(MAP_WIDTH, MAP_HEIGHT));
            new_map.algorithm = algo;
            new_map
        };

        // 도착 위치 계산
        let spawn_pos = arrival_pos(&map, &transition.arrive_from);

        log.send(crate::modules::ui::LogMessage(
            format!("{} 진입.", target.display_name())
        ));

        // 기존 포털 제거 (새 맵에 맞게 재스폰됨)
        for e in portals.iter() { commands.entity(e).despawn(); }

        world.maps.insert(target.clone(), map.clone());
        world.current = target;

        apply_ev.send(ApplyMapEvent { map, spawn_pos: Some(spawn_pos) });
    }
}

/// ApplyMapEvent 이후 (맵 타일 재스폰 완료 다음 프레임) 포털을 스폰한다
fn spawn_portals_after_apply(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    world: Res<WorldState>,
    portal_q: Query<(), With<ZonePortal>>,
    map_res: Res<MapResource>,
    named_config: Res<NamedZoneConfig>,
    mut used_spawn: ResMut<UsedSpawnTiles>,
) {
    if !map_res.is_changed() { return; }
    if !portal_q.is_empty() { return; }  // 이미 스폰됨

    let mut rng = rand::thread_rng();
    let map = &map_res.0;
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");

    let mut portals = zone_portals(&world.current);

    // Named 존 안에 있으면 원점으로 돌아가는 포탈을 추가한다
    if let ZoneId::Named(ref name) = world.current {
        if let Some(entry) = named_config.zones.get(name) {
            portals.push((PortalDirection::StairUp, entry.origin.clone()));
        }
    }

    // 현재 존이 origin 인 Named 존들의 진입 포탈을 재스폰한다 (존 재방문 시)
    for (zone_name, entry) in &named_config.zones {
        if entry.origin == world.current {
            portals.push((PortalDirection::StairDown, ZoneId::Named(zone_name.clone())));
        }
    }

    for (dir, target) in portals {
        let is_quest_portal = matches!(target, ZoneId::Named(_));
        if let Some((px, py)) = portal_tile(map, &dir, &mut used_spawn.0, &mut rng) {
            let coord = tile_to_world_coords(px, py);
            let glyph = dir.glyph();
            let color = if is_quest_portal {
                Color::rgb(0.8, 0.2, 0.8)  // 퀘스트 포탈 — 보라색
            } else {
                match dir {
                    PortalDirection::StairDown => Color::YELLOW,
                    PortalDirection::StairUp   => Color::CYAN,
                    _                          => Color::rgba(0.5, 1.0, 0.5, 0.7),
                }
            };
            commands.spawn((
                Text2dBundle {
                    text: Text::from_section(glyph, TextStyle {
                        font: font.clone(),
                        font_size: TILE_SIZE,
                        color,
                    }),
                    transform: Transform::from_xyz(coord.x, coord.y, 1.5),
                    ..default()
                },
                ZonePortal { target, arrive_from: dir },
            ));
        }
    }
}

/// SpawnQuestPortalEvent 를 받아 NamedZoneConfig 에 등록하고 현재 맵에 포탈을 스폰한다
fn handle_spawn_quest_portal(
    mut ev: EventReader<SpawnQuestPortalEvent>,
    mut named_config: ResMut<NamedZoneConfig>,
    world: Res<WorldState>,
    map_res: Res<MapResource>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut used_spawn: ResMut<UsedSpawnTiles>,
) {
    for event in ev.read() {
        // 이미 등록된 Named 존은 중복 생성하지 않는다
        if named_config.zones.contains_key(&event.zone) { continue; }

        named_config.zones.insert(event.zone.clone(), NamedZoneEntry {
            generator: event.generator.clone(),
            origin: world.current.clone(),
        });

        let mut rng = rand::thread_rng();
        let map = &map_res.0;
        let dir = PortalDirection::StairDown;
        if let Some((px, py)) = portal_tile(map, &dir, &mut used_spawn.0, &mut rng) {
            let coord = tile_to_world_coords(px, py);
            let font = asset_server.load("fonts/FiraMono-Medium.ttf");
            commands.spawn((
                Text2dBundle {
                    text: Text::from_section(dir.glyph(), TextStyle {
                        font,
                        font_size: TILE_SIZE,
                        color: Color::rgb(0.8, 0.2, 0.8),
                    }),
                    transform: Transform::from_xyz(coord.x, coord.y, 1.5),
                    ..default()
                },
                ZonePortal {
                    target: ZoneId::Named(event.zone.clone()),
                    arrive_from: dir,
                },
            ));
        }
    }
}

/// 존 복귀 시 스냅샷에서 혈흔을 경과 턴만큼 감소하여 복원한다.
/// 몬스터 리스폰 타이머 처리는 monster::respawn_on_regen 에서 담당한다.
fn restore_zone_state(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    map_res: Res<MapResource>,
    world: Res<WorldState>,
    global_turn: Res<GlobalTurn>,
    mut persistence: ResMut<ZonePersistence>,
) {
    if !map_res.is_changed() { return; }
    let zone = world.current.clone();
    let Some(snapshot) = persistence.0.get_mut(&zone) else { return };

    let turns_passed = global_turn.0.saturating_sub(snapshot.last_visited_turn);
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");

    for stain in snapshot.blood_stains.drain(..) {
        let adjusted = (stain.alpha - turns_passed as f32 * stain.decay_per_turn).max(0.0);
        if adjusted <= 0.0 { continue; }
        let pos = tile_to_world_coords(stain.tile_x, stain.tile_y);
        commands.spawn((
            Text2dBundle {
                text: Text::from_section("%", TextStyle {
                    font: font.clone(),
                    font_size: TILE_SIZE,
                    color: Color::rgba(0.8, 0.0, 0.0, adjusted),
                }),
                transform: Transform::from_xyz(pos.x, pos.y, Z_BLOOD),
                ..default()
            },
            BloodStain { alpha: adjusted, decay_per_turn: stain.decay_per_turn },
        ));
    }
}

/// 포털 엔티티가 FOV 안에 들어오면 DiscoveredMarkers 에 추가한다
fn discover_portals_in_fov(
    map_res: Res<MapResource>,
    world: Res<WorldState>,
    portal_q: Query<(&Transform, &ZonePortal)>,
    mut markers: ResMut<crate::modules::ui::minimap::DiscoveredMarkers>,
) {
    let map = map_res.map();
    for (transform, portal) in portal_q.iter() {
        let (tx, ty) = world_to_tile_coords(transform.translation);
        if tx >= map.width || ty >= map.height { continue; }
        let idx = map.index(tx, ty);
        if !map.tiles[idx].visible { continue; }
        let kind = match portal.arrive_from {
            PortalDirection::StairDown => crate::modules::ui::minimap::MarkerKind::StairDown,
            PortalDirection::StairUp   => crate::modules::ui::minimap::MarkerKind::StairUp,
            _                          => crate::modules::ui::minimap::MarkerKind::Portal,
        };
        markers.add(tx, ty, kind, world.current.clone());
    }
}

// ── Helper: 존별 포털 정의 ───────────────────────────────────────────────────

fn zone_portals(zone: &ZoneId) -> Vec<(PortalDirection, ZoneId)> {
    match zone {
        ZoneId::Town => vec![
            (PortalDirection::South, ZoneId::Forest),
        ],
        ZoneId::Forest => vec![
            (PortalDirection::North, ZoneId::Town),
            (PortalDirection::South, ZoneId::Dungeon(1)),
        ],
        ZoneId::Dungeon(1) => vec![
            (PortalDirection::North, ZoneId::Forest),
            (PortalDirection::StairDown, ZoneId::Dungeon(2)),
        ],
        ZoneId::Dungeon(2) => vec![
            (PortalDirection::StairUp, ZoneId::Dungeon(1)),
        ],
        ZoneId::Dungeon(n) => {
            let n = *n;
            vec![
                (PortalDirection::StairUp, ZoneId::Dungeon(n - 1)),
                (PortalDirection::StairDown, ZoneId::Dungeon(n + 1)),
            ]
        }
        ZoneId::Named(_) => vec![],
    }
}

/// PortalDirection 에 따른 포털 타일 위치를 찾는다.
/// StairDown/StairUp 은 해당 방의 랜덤 Floor 타일에 배치하며, used 에 기록해 중복을 방지한다.
fn portal_tile(
    map: &Map,
    dir: &PortalDirection,
    used: &mut std::collections::HashSet<(usize, usize)>,
    rng: &mut impl rand::Rng,
) -> Option<(usize, usize)> {
    match dir {
        PortalDirection::North => {
            let cx = map.width / 2;
            for y in 0..map.height {
                if map.get_tile(cx, y) == TileKind::Floor {
                    return Some((cx, y));
                }
            }
            None
        }
        PortalDirection::South => {
            let cx = map.width / 2;
            for y in (0..map.height).rev() {
                if map.get_tile(cx, y) == TileKind::Floor {
                    return Some((cx, y));
                }
            }
            None
        }
        PortalDirection::StairDown => {
            // 마지막 방의 랜덤 Floor 타일
            map.rooms.last().and_then(|r| random_floor_tile_in_room(r, map, used, rng))
                .or_else(|| map.rooms.last().map(|r| r.center()))
        }
        PortalDirection::StairUp => {
            // 첫 번째 방의 랜덤 Floor 타일 (플레이어 스폰 위치는 used 에 이미 예약됨)
            map.rooms.first().and_then(|r| random_floor_tile_in_room(r, map, used, rng))
                .or_else(|| map.rooms.first().map(|r| r.center()))
        }
    }
}

/// 도착 방향에 따라 플레이어 스폰 위치를 계산한다
fn arrival_pos(map: &Map, arrive_from: &PortalDirection) -> (usize, usize) {
    match arrive_from {
        PortalDirection::North => {
            // 남쪽에서 올라옴 → 맵 남쪽 첫 Floor
            let cx = map.width / 2;
            for y in (0..map.height).rev() {
                if map.get_tile(cx, y) == TileKind::Floor {
                    let y2 = (y + 1).min(map.height - 1);
                    if map.get_tile(cx, y2) == TileKind::Floor { return (cx, y2); }
                    return (cx, y);
                }
            }
            (cx, map.height / 2)
        }
        PortalDirection::South => {
            // 북쪽에서 내려옴 → 맵 북쪽 첫 Floor
            let cx = map.width / 2;
            for y in 0..map.height {
                if map.get_tile(cx, y) == TileKind::Floor {
                    let y2 = y.saturating_sub(1);
                    if map.get_tile(cx, y2) == TileKind::Floor { return (cx, y2); }
                    return (cx, y);
                }
            }
            (cx, map.height / 2)
        }
        PortalDirection::StairDown => {
            // 계단을 올라옴 → 첫 방 중앙
            map.rooms.first().map(|r| r.center()).unwrap_or((map.width / 2, map.height / 2))
        }
        PortalDirection::StairUp => {
            // 계단을 내려옴 → 마지막 방 중앙
            map.rooms.last().map(|r| r.center()).unwrap_or((map.width / 2, map.height / 2))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_id_display_name() {
        assert_eq!(ZoneId::Town.display_name(), "마을");
        assert_eq!(ZoneId::Forest.display_name(), "숲");
        assert_eq!(ZoneId::Dungeon(2).display_name(), "던전 2층");
    }

    #[test]
    fn zone_id_algorithm() {
        assert_eq!(ZoneId::Town.algorithm(), "organic_village");
        assert_eq!(ZoneId::Forest.algorithm(), "forest");
        assert_eq!(ZoneId::Dungeon(1).algorithm(), "bsp");
    }

    #[test]
    fn world_state_cache_and_retrieve() {
        let mut ws = WorldState::default();
        let map = Map::new(10, 10);
        ws.cache_current(map);
        assert!(ws.get_cached(&ZoneId::Town).is_some());
        assert!(ws.get_cached(&ZoneId::Forest).is_none());
    }

    #[test]
    fn dungeon_2_has_only_stair_up() {
        let portals = zone_portals(&ZoneId::Dungeon(2));
        assert_eq!(portals.len(), 1);
        assert!(matches!(portals[0].0, PortalDirection::StairUp));
        assert_eq!(portals[0].1, ZoneId::Dungeon(1));
    }

    #[test]
    fn arrival_pos_north_returns_south_area() {
        let mut map = Map::new(20, 20);
        // 남쪽 행에 Floor 생성
        for x in 0..20 { map.set_tile(x, 18, crate::modules::map::TileKind::Floor); }
        let (_, y) = arrival_pos(&map, &PortalDirection::North);
        assert!(y >= 10, "남쪽 스폰이어야 함: y={}", y);
    }

    #[test]
    fn named_zone_portals_empty() {
        let portals = zone_portals(&ZoneId::Named("desert".to_string()));
        assert!(portals.is_empty(), "Named 존의 정적 포탈은 없어야 한다");
    }

    #[test]
    fn named_zone_config_registers_entry() {
        let mut config = NamedZoneConfig::default();
        config.zones.insert("desert".to_string(), NamedZoneEntry {
            generator: "desert_gen".to_string(),
            origin: ZoneId::Town,
        });
        let entry = config.zones.get("desert").expect("등록된 Named 존이 있어야 한다");
        assert_eq!(entry.generator, "desert_gen");
        assert_eq!(entry.origin, ZoneId::Town);
    }

    #[test]
    fn named_zone_display_name_uses_zone_name() {
        assert_eq!(ZoneId::Named("사막".to_string()).display_name(), "사막");
        assert_eq!(ZoneId::Named("콰스".to_string()).display_name(), "콰스");
    }
}
