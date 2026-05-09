use bevy::prelude::*;
use std::collections::{HashMap, HashSet};
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
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NamedZoneEntry {
    pub generator: String,
    pub origin: ZoneId,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NamedZoneConfig {
    pub zones: HashMap<String, NamedZoneEntry>,
}

/// 퀘스트 액션이 발행 → handle_spawn_quest_portal 시스템이 처리.
/// `placement` 가 NearGiver 인 경우 `quest_id` 로 giver_npc 를 조회한다.
#[derive(Event)]
pub struct SpawnQuestPortalEvent {
    pub zone: String,
    pub generator: String,
    pub placement: crate::modules::quest::PortalPlacement,
    pub quest_id: String,
}

/// 특정 Named zone 의 포탈 / 등록 / 영속화 / 미니맵 마커 를 모두 정리한다.
/// 퀘스트 종료 시 ClosePortal 액션을 통해 발행된다.
#[derive(Event)]
pub struct CloseQuestPortalEvent {
    pub zone: String,
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

/// 영속화되는 포털 위치 — 같은 존 재방문 시 같은 자리에 복원하기 위함.
/// 현재 게임 코드는 portal_tile() 로 매번 랜덤 위치를 정해 위치가 바뀌는 버그가 있었다.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct SavedPortal {
    pub tile_x: usize,
    pub tile_y: usize,
    pub target: ZoneId,
    pub arrive_from: PortalDirection,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ZoneSnapshot {
    pub blood_stains: Vec<SavedBloodStain>,
    pub monster_slots: Vec<MonsterSlot>,
    pub last_visited_turn: u64,
    #[serde(default)]
    pub portals: Vec<SavedPortal>,
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

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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
            .add_event::<CloseQuestPortalEvent>()
            .add_systems(Startup, cache_initial_map.after(crate::modules::map::draw_map))
            .add_systems(Update, (
                handle_spawn_quest_portal,
                handle_close_quest_portal,
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
    registry: Res<MapGeneratorRegistry>,
    portals: Query<(Entity, &Transform, &ZonePortal)>,
    mut commands: Commands,
    mut log: EventWriter<crate::modules::ui::LogMessage>,
    blood_query: Query<(Entity, &BloodStain, &Transform)>,
    global_turn: Res<GlobalTurn>,
    mut persistence: ResMut<ZonePersistence>,
    named_config: Res<NamedZoneConfig>,
    global_seed: Res<GlobalSeed>,
    mut used_spawn: ResMut<UsedSpawnTiles>,
) {
    for transition in ev.read() {
        // 떠나는 존의 혈흔과 포털 위치를 스냅샷에 저장하고 엔티티 제거
        let from_zone = world.current.clone();
        let snapshot = persistence.0.entry(from_zone.clone()).or_default();
        snapshot.blood_stains = blood_query.iter().map(|(_, stain, transform)| {
            let (tx, ty) = world_to_tile_coords(transform.translation);
            SavedBloodStain { tile_x: tx, tile_y: ty, alpha: stain.alpha, decay_per_turn: stain.decay_per_turn }
        }).collect();
        snapshot.portals = portals.iter().map(|(_, transform, portal)| {
            let (tx, ty) = world_to_tile_coords(transform.translation);
            SavedPortal {
                tile_x: tx, tile_y: ty,
                target: portal.target.clone(),
                arrive_from: portal.arrive_from.clone(),
            }
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
            let seed = zone_seed(global_seed.0, &target);
            let mut new_map = registry.generate_with(&algo, MAP_WIDTH, MAP_HEIGHT, seed)
                .unwrap_or_else(|| {
                    warn!("알 수 없는 맵 생성기 {} - 빈 맵을 생성합니다", algo);
                    Map::new(MAP_WIDTH, MAP_HEIGHT)
                });
            new_map.seed = seed;
            new_map.algorithm = algo;
            new_map
        };

        // 도착 zone 의 포털 위치를 미리 생성·저장하여 spawn_portals_after_apply 와 일치시킨다.
        // 첫 방문이라도 player 가 정확히 return portal 위치에서 spawn 된다.
        ensure_zone_portals_persisted(&target, &map, &named_config, &mut persistence, &mut used_spawn.0);

        // 도착 위치: 도착 zone 의 saved portal 중 target 이 from_zone 인 것 (return portal)
        let spawn_pos = persistence.0.get(&target)
            .and_then(|snap| snap.portals.iter().find(|p| p.target == from_zone))
            .map(|p| (p.tile_x, p.tile_y))
            .unwrap_or_else(|| arrival_pos(&map, &transition.arrive_from));

        log.send(crate::modules::ui::LogMessage(
            format!("{} 진입.", target.display_name())
        ));

        // 기존 포털 제거 (새 맵에 맞게 재스폰됨; 위치는 위에서 persistence 에 저장됨)
        for (entity, _, _) in portals.iter() { commands.entity(entity).despawn(); }

        world.maps.insert(target.clone(), map.clone());
        world.current = target;

        apply_ev.send(ApplyMapEvent { map, spawn_pos: Some(spawn_pos) });
    }
}

/// ApplyMapEvent 이후 (맵 타일 재스폰 완료 다음 프레임) 포털을 스폰한다.
///
/// 영속화된 포털 위치가 있으면 그대로 복원해 같은 존 재방문 시 위치 일관성을 보장한다.
/// 첫 방문 시 persistence 가 비어있으면 새로 생성하여 저장한다.
fn spawn_portals_after_apply(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    world: Res<WorldState>,
    portal_q: Query<(), With<ZonePortal>>,
    map_res: Res<MapResource>,
    named_config: Res<NamedZoneConfig>,
    mut persistence: ResMut<ZonePersistence>,
    mut used_spawn: ResMut<UsedSpawnTiles>,
) {
    if !map_res.is_changed() { return; }
    if !portal_q.is_empty() { return; }  // 이미 스폰됨

    // persistence 에 포털이 없으면 새로 생성·저장 (첫 방문 시 startup 경로 포함)
    ensure_zone_portals_persisted(&world.current, &map_res.0, &named_config, &mut persistence, &mut used_spawn.0);

    let font = asset_server.load("fonts/FiraMono-Medium.ttf");
    if let Some(snapshot) = persistence.0.get(&world.current) {
        for saved in &snapshot.portals {
            spawn_portal_entity(
                &mut commands, &font,
                saved.tile_x, saved.tile_y,
                saved.arrive_from.clone(), saved.target.clone(),
            );
        }
    }
}

/// persistence[zone].portals 가 비어있으면 zone_portals + Named 진입 포털을
/// portal_tile() 로 배치하여 저장한다. 이미 저장된 경우 no-op.
///
/// 호출자: spawn_portals_after_apply, handle_zone_transition (도착 위치 결정 전).
fn ensure_zone_portals_persisted(
    zone: &ZoneId,
    map: &Map,
    named_config: &NamedZoneConfig,
    persistence: &mut ZonePersistence,
    used_spawn: &mut HashSet<(usize, usize)>,
) {
    if persistence.0.get(zone).map(|s| !s.portals.is_empty()).unwrap_or(false) {
        return;
    }

    let mut portals = zone_portals(zone);
    // Named 존 안에 있으면 원점으로 돌아가는 포탈을 추가한다
    if let ZoneId::Named(ref name) = zone {
        if let Some(entry) = named_config.zones.get(name) {
            portals.push((PortalDirection::StairUp, entry.origin.clone()));
        }
    }
    // 현재 존이 origin 인 Named 존들의 진입 포탈
    for (zone_name, entry) in &named_config.zones {
        if entry.origin == *zone {
            portals.push((PortalDirection::StairDown, ZoneId::Named(zone_name.clone())));
        }
    }

    let mut rng = rand::thread_rng();
    let mut placed: Vec<SavedPortal> = Vec::new();
    for (dir, target) in portals {
        if let Some((px, py)) = portal_tile(map, &dir, used_spawn, &mut rng) {
            placed.push(SavedPortal { tile_x: px, tile_y: py, target, arrive_from: dir });
        }
    }
    persistence.0.entry(zone.clone()).or_default().portals = placed;
}

/// ZonePortal 엔티티 한 개를 지정 좌표에 스폰한다 — saved/random 양쪽 경로에서 공유
fn spawn_portal_entity(
    commands: &mut Commands,
    font: &Handle<Font>,
    tile_x: usize,
    tile_y: usize,
    dir: PortalDirection,
    target: ZoneId,
) {
    let coord = tile_to_world_coords(tile_x, tile_y);
    let glyph = dir.glyph();
    let is_quest_portal = matches!(target, ZoneId::Named(_));
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

/// SpawnQuestPortalEvent 를 받아 NamedZoneConfig 에 등록하고 현재 맵에 포탈을 스폰한다
fn handle_spawn_quest_portal(
    mut ev: EventReader<SpawnQuestPortalEvent>,
    mut named_config: ResMut<NamedZoneConfig>,
    world: Res<WorldState>,
    map_res: Res<MapResource>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut used_spawn: ResMut<UsedSpawnTiles>,
    mut markers: ResMut<crate::modules::ui::minimap::DiscoveredMarkers>,
    quest_registry: Res<crate::modules::quest::QuestRegistry>,
    villager_q: Query<(&Transform, &crate::modules::villager::Villager)>,
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

        // giver NPC 위치 조회 — NearGiver 에서만 사용. 같은 zone 의 첫 매치.
        let giver_pos = quest_registry.get(&event.quest_id)
            .and_then(|def| {
                let giver_name = &def.giver_npc;
                villager_q.iter()
                    .find(|(_, v)| &v.name == giver_name)
                    .map(|(t, _)| world_to_tile_coords(t.translation))
            });

        let pos = compute_portal_pos(map, &event.placement, giver_pos, &mut used_spawn.0, &mut rng)
            .or_else(|| portal_tile(map, &dir, &mut used_spawn.0, &mut rng));

        if let Some((px, py)) = pos {
            let coord = tile_to_world_coords(px, py);
            let font = asset_server.load("fonts/FiraMono-Medium.ttf");
            // 퀘스트로 새로 생성된 포털은 즉시 미니맵 마커 등록 — quest 받은 직후
            // 멀리 있어도 위치를 알 수 있게 한다 (FOV 검사 우회).
            markers.add(px, py, crate::modules::ui::minimap::MarkerKind::Portal, world.current.clone());
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

/// CloseQuestPortalEvent 를 받아 해당 Named zone 의 portal entity, 영속화 데이터,
/// 등록, 미니맵 마커를 모두 정리한다. 퀘스트 종료 시 ClosePortal 액션을 통해 호출됨.
fn handle_close_quest_portal(
    mut ev: EventReader<CloseQuestPortalEvent>,
    mut named_config: ResMut<NamedZoneConfig>,
    mut persistence: ResMut<ZonePersistence>,
    mut commands: Commands,
    portal_q: Query<(Entity, &Transform, &ZonePortal)>,
    mut markers: ResMut<crate::modules::ui::minimap::DiscoveredMarkers>,
    world: Res<WorldState>,
) {
    for event in ev.read() {
        let target = ZoneId::Named(event.zone.clone());

        // 1) 활성 portal entity 제거 + 미니맵 마커 제거
        for (entity, transform, portal) in portal_q.iter() {
            if portal.target != target { continue; }
            let (tx, ty) = world_to_tile_coords(transform.translation);
            commands.entity(entity).despawn();
            // 현재 zone 의 그 위치 portal 마커 제거 (모든 MarkerKind 시도)
            markers.remove_at(tx, ty, crate::modules::ui::minimap::MarkerKind::Portal, &world.current);
            markers.remove_at(tx, ty, crate::modules::ui::minimap::MarkerKind::StairDown, &world.current);
            markers.remove_at(tx, ty, crate::modules::ui::minimap::MarkerKind::StairUp, &world.current);
        }

        // 2) 모든 saved persistence 에서 해당 target portal 제거
        for snap in persistence.0.values_mut() {
            snap.portals.retain(|p| p.target != target);
        }

        // 3) NamedZoneConfig 에서 zone 등록 제거
        named_config.zones.remove(&event.zone);

        info!("퀘스트 포탈 정리: Named({}) — 모든 진입 경로 닫힘", event.zone);
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

/// 퀘스트 포털 배치 — `PortalPlacement` 에 따라 위치를 결정한다.
/// 실패 시 `None` — 호출자가 fallback (StairDown) 으로 넘어간다.
fn compute_portal_pos(
    map: &Map,
    placement: &crate::modules::quest::PortalPlacement,
    giver_pos: Option<(usize, usize)>,
    used: &mut std::collections::HashSet<(usize, usize)>,
    rng: &mut impl rand::Rng,
) -> Option<(usize, usize)> {
    use crate::modules::quest::PortalPlacement;
    use crate::modules::map::random_floor_tile_anywhere;
    match placement {
        PortalPlacement::InsideRoom => {
            random_floor_tile_anywhere(&map.rooms, map, used, rng)
        }
        PortalPlacement::Random => {
            random_floor_tile_anywhere(&map.rooms, map, used, rng)
        }
        PortalPlacement::Border => border_floor_tile(map, used),
        PortalPlacement::NearGiver { radius } => {
            giver_pos
                .and_then(|(gx, gy)| floor_tile_near(map, gx, gy, *radius, used, rng))
                .or_else(|| random_floor_tile_anywhere(&map.rooms, map, used, rng))
        }
    }
}

/// 맵 외곽선에서 가장 가까운 Floor — 외곽 ring 부터 안쪽으로 한 ring 씩 스캔.
/// 마을·야외 맵 입구로 자연스럽도록 한 번 발견하면 즉시 반환한다.
fn border_floor_tile(
    map: &Map,
    used: &mut std::collections::HashSet<(usize, usize)>,
) -> Option<(usize, usize)> {
    let max_ring = (map.width.min(map.height)) / 2;
    for ring in 0..max_ring {
        // ring=0 은 가장 바깥 한 줄. ring=k 면 (k, k) ~ (w-1-k, h-1-k) 의 외곽선.
        let x0 = ring;
        let y0 = ring;
        let x1 = map.width.saturating_sub(ring + 1);
        let y1 = map.height.saturating_sub(ring + 1);
        if x0 > x1 || y0 > y1 { break; }
        let mut candidates: Vec<(usize, usize)> = Vec::new();
        for x in x0..=x1 {
            for &y in &[y0, y1] {
                if map.get_tile(x, y) == TileKind::Floor && !used.contains(&(x, y)) {
                    candidates.push((x, y));
                }
            }
        }
        for y in (y0 + 1)..y1 {
            for &x in &[x0, x1] {
                if map.get_tile(x, y) == TileKind::Floor && !used.contains(&(x, y)) {
                    candidates.push((x, y));
                }
            }
        }
        if let Some(&(x, y)) = candidates.first() {
            used.insert((x, y));
            return Some((x, y));
        }
    }
    None
}

/// `(cx, cy)` 반경 `radius` 안의 Floor 한 칸 — used 미점유. 후보 중 랜덤 하나.
fn floor_tile_near(
    map: &Map,
    cx: usize,
    cy: usize,
    radius: usize,
    used: &mut std::collections::HashSet<(usize, usize)>,
    rng: &mut impl rand::Rng,
) -> Option<(usize, usize)> {
    use rand::seq::SliceRandom;
    let r = radius as i32;
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    for dy in -r..=r {
        for dx in -r..=r {
            if dx == 0 && dy == 0 { continue; }
            let nx = cx as i32 + dx;
            let ny = cy as i32 + dy;
            if nx < 0 || ny < 0 || nx >= map.width as i32 || ny >= map.height as i32 { continue; }
            let (ux, uy) = (nx as usize, ny as usize);
            if map.get_tile(ux, uy) == TileKind::Floor && !used.contains(&(ux, uy)) {
                candidates.push((ux, uy));
            }
        }
    }
    candidates.shuffle(rng);
    let &(x, y) = candidates.first()?;
    used.insert((x, y));
    Some((x, y))
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
    use crate::modules::map::Rect;
    use rand::SeedableRng;

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

    #[test]
    fn border_floor_tile_picks_outermost_floor() {
        // 외곽 한 칸은 모두 Wall, 두 번째 ring 부터 Floor — 두 번째 ring 에서 발견되어야.
        let mut map = Map::new(10, 10);
        for y in 1..9 { for x in 1..9 { map.set_tile(x, y, TileKind::Floor); } }
        let mut used = std::collections::HashSet::new();
        let (x, y) = border_floor_tile(&map, &mut used).expect("Floor 발견 실패");
        // ring=1 외곽선이어야 한다 — 한 변에 닿아 있음.
        assert!(x == 1 || x == 8 || y == 1 || y == 8, "외곽 ring 이어야: ({}, {})", x, y);
    }

    #[test]
    fn border_floor_tile_returns_none_when_no_floor() {
        let map = Map::new(10, 10);
        let mut used = std::collections::HashSet::new();
        assert!(border_floor_tile(&map, &mut used).is_none());
    }

    #[test]
    fn compute_portal_pos_inside_room_returns_floor() {
        use crate::modules::quest::PortalPlacement;
        let mut map = Map::new(20, 20);
        for y in 5..10 { for x in 5..10 { map.set_tile(x, y, TileKind::Floor); } }
        map.rooms.push(Rect::new(5, 5, 5, 5));
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        let (x, y) = compute_portal_pos(&map, &PortalPlacement::InsideRoom, None,
            &mut used, &mut rng).expect("InsideRoom 실패");
        assert_eq!(map.get_tile(x, y), TileKind::Floor);
    }

    #[test]
    fn compute_portal_pos_near_giver_within_radius() {
        use crate::modules::quest::PortalPlacement;
        let mut map = Map::new(20, 20);
        for y in 0..20 { for x in 0..20 { map.set_tile(x, y, TileKind::Floor); } }
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        let (x, y) = compute_portal_pos(&map, &PortalPlacement::NearGiver { radius: 2 },
            Some((10, 10)), &mut used, &mut rng).expect("NearGiver 실패");
        assert!((x as i32 - 10).abs() <= 2 && (y as i32 - 10).abs() <= 2,
            "반경 2 안이어야: ({}, {})", x, y);
    }

    #[test]
    fn compute_portal_pos_near_giver_falls_back_when_no_giver() {
        use crate::modules::quest::PortalPlacement;
        let mut map = Map::new(20, 20);
        for y in 5..10 { for x in 5..10 { map.set_tile(x, y, TileKind::Floor); } }
        map.rooms.push(Rect::new(5, 5, 5, 5));
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        let (x, y) = compute_portal_pos(&map, &PortalPlacement::NearGiver { radius: 2 },
            None, &mut used, &mut rng).expect("fallback 실패");
        assert_eq!(map.get_tile(x, y), TileKind::Floor);
    }

    #[test]
    fn ensure_zone_portals_persisted_populates_empty_zone() {
        // 첫 방문 시 persistence 가 비어있으면 portal 을 생성·저장해야 한다
        let mut map = Map::new(40, 30);
        for y in 1..29 { for x in 1..39 { map.set_tile(x, y, TileKind::Floor); } }
        map.rooms = vec![crate::modules::map::Rect::new(2, 2, 36, 26)];

        let mut persistence = ZonePersistence::default();
        let named = NamedZoneConfig::default();
        let mut used = std::collections::HashSet::new();
        ensure_zone_portals_persisted(&ZoneId::Forest, &map, &named, &mut persistence, &mut used);

        let snap = persistence.0.get(&ZoneId::Forest).expect("snapshot 생성됨");
        assert!(!snap.portals.is_empty(), "Forest 존은 기본 포털이 있어야 한다");
    }

    #[test]
    fn ensure_zone_portals_persisted_is_noop_when_already_populated() {
        // 이미 저장된 portal 이 있으면 건드리지 않는다 (재방문 시 위치 일관성)
        let mut map = Map::new(40, 30);
        for y in 1..29 { for x in 1..39 { map.set_tile(x, y, TileKind::Floor); } }
        map.rooms = vec![crate::modules::map::Rect::new(2, 2, 36, 26)];

        let mut persistence = ZonePersistence::default();
        let original = vec![SavedPortal {
            tile_x: 5, tile_y: 7,
            target: ZoneId::Town,
            arrive_from: PortalDirection::StairUp,
        }];
        persistence.0.entry(ZoneId::Forest).or_default().portals = original.clone();

        let named = NamedZoneConfig::default();
        let mut used = std::collections::HashSet::new();
        ensure_zone_portals_persisted(&ZoneId::Forest, &map, &named, &mut persistence, &mut used);

        let snap = persistence.0.get(&ZoneId::Forest).unwrap();
        assert_eq!(snap.portals.len(), original.len(), "기존 portal 보존");
        assert_eq!(snap.portals[0].tile_x, 5);
        assert_eq!(snap.portals[0].tile_y, 7);
    }

    #[test]
    fn zone_snapshot_default_has_empty_portals() {
        let s = ZoneSnapshot::default();
        assert!(s.portals.is_empty());
    }

    #[test]
    fn saved_portal_serde_roundtrip() {
        let p = SavedPortal {
            tile_x: 12, tile_y: 7,
            target: ZoneId::Named("herb_glade".into()),
            arrive_from: PortalDirection::StairDown,
        };
        let s = ron::ser::to_string(&p).unwrap();
        let parsed: SavedPortal = ron::de::from_str(&s).unwrap();
        assert_eq!(parsed.tile_x, 12);
        assert_eq!(parsed.tile_y, 7);
        assert_eq!(parsed.target, ZoneId::Named("herb_glade".into()));
        assert_eq!(parsed.arrive_from, PortalDirection::StairDown);
    }

    #[test]
    fn zone_snapshot_serialization_preserves_portals() {
        let mut snap = ZoneSnapshot::default();
        snap.portals.push(SavedPortal {
            tile_x: 5, tile_y: 9,
            target: ZoneId::Named("herb_glade".into()),
            arrive_from: PortalDirection::StairDown,
        });
        snap.portals.push(SavedPortal {
            tile_x: 30, tile_y: 18,
            target: ZoneId::Forest,
            arrive_from: PortalDirection::North,
        });
        let s = ron::ser::to_string(&snap).unwrap();
        let parsed: ZoneSnapshot = ron::de::from_str(&s).unwrap();
        assert_eq!(parsed.portals.len(), 2);
        assert_eq!(parsed.portals[0].tile_x, 5);
        assert_eq!(parsed.portals[1].target, ZoneId::Forest);
    }

    #[test]
    fn zone_snapshot_deserializes_legacy_format_without_portals() {
        // 기존 저장 데이터(portals 필드 없음) 호환성 — #[serde(default)] 검증
        let legacy = r#"(blood_stains: [], monster_slots: [], last_visited_turn: 5)"#;
        let parsed: ZoneSnapshot = ron::de::from_str(legacy).unwrap();
        assert_eq!(parsed.last_visited_turn, 5);
        assert!(parsed.portals.is_empty());
    }
}
