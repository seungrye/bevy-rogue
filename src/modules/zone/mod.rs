use bevy::prelude::*;
use std::collections::{HashMap, HashSet};
use crate::modules::{
    map::{
        Map, MapResource, MapGeneratorRegistry, ApplyMapEvent,
        MAP_WIDTH, MAP_HEIGHT, tile_to_world_coords, TILE_SIZE,
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
    // 도달 불가 방어코드: 바로 위 ensure_zone_portals_persisted 가 entry().or_default()
    // 로 현재 존 항목을 항상 삽입하므로 get() 은 언제나 Some 이다.
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
        // 마을(Town)은 신규 게임 시작 시 어떤 기본 포털도 두지 않는다.
        //   - 사용자 혼란 방지: 시작하자마자 정체불명 포털이 보이지 않게 한다.
        //   - 퀘스트가 발급하는 OpenPortal 만 마을에 포털을 만든다.
        //   - 다른 zone(Forest 등) 에서 마을로 되돌아오는 return portal 은
        //     그 zone 의 `zone_portals` 에서 정의하므로 유지된다.
        ZoneId::Town => vec![],
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
        // 도달 불가 방어코드: ring < min(w,h)/2 이므로 x0>x1·y0>y1 이 성립하지 않는다.
        if x0 > x1 || y0 > y1 { break; }
        let mut candidates: Vec<(usize, usize)> = Vec::new();
        for x in x0..=x1 {
            for &y in &[y0, y1] {
                if map.get_tile(x, y).is_walkable() && !used.contains(&(x, y)) {
                    candidates.push((x, y));
                }
            }
        }
        for y in (y0 + 1)..y1 {
            for &x in &[x0, x1] {
                if map.get_tile(x, y).is_walkable() && !used.contains(&(x, y)) {
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
            if map.get_tile(ux, uy).is_walkable() && !used.contains(&(ux, uy)) {
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
                if map.get_tile(cx, y).is_walkable() {
                    return Some((cx, y));
                }
            }
            None
        }
        PortalDirection::South => {
            let cx = map.width / 2;
            for y in (0..map.height).rev() {
                if map.get_tile(cx, y).is_walkable() {
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
                if map.get_tile(cx, y).is_walkable() {
                    let y2 = (y + 1).min(map.height - 1);
                    if map.get_tile(cx, y2).is_walkable() { return (cx, y2); }
                    return (cx, y);
                }
            }
            (cx, map.height / 2)
        }
        PortalDirection::South => {
            // 북쪽에서 내려옴 → 맵 북쪽 첫 Floor
            let cx = map.width / 2;
            for y in 0..map.height {
                if map.get_tile(cx, y).is_walkable() {
                    let y2 = y.saturating_sub(1);
                    if map.get_tile(cx, y2).is_walkable() { return (cx, y2); }
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
    #![allow(non_snake_case)]
    use super::*;
    use crate::modules::map::{Rect, TileKind};
    use rand::SeedableRng;

    #[test]
    fn 존ID는_종류별로_한글_표시이름을_반환한다() {
        assert_eq!(ZoneId::Town.display_name(), "마을");
        assert_eq!(ZoneId::Forest.display_name(), "숲");
        assert_eq!(ZoneId::Dungeon(2).display_name(), "던전 2층");
    }

    #[test]
    fn 존ID는_종류별로_맵생성기_알고리즘_이름을_반환한다() {
        assert_eq!(ZoneId::Town.algorithm(), "organic_village");
        assert_eq!(ZoneId::Forest.algorithm(), "forest");
        assert_eq!(ZoneId::Dungeon(1).algorithm(), "bsp");
    }

    #[test]
    fn 월드상태는_현재존_맵을_캐시하고_존ID로_다시_꺼낼_수_있다() {
        let mut ws = WorldState::default();
        let map = Map::new(10, 10);
        ws.cache_current(map);
        assert!(ws.get_cached(&ZoneId::Town).is_some());
        assert!(ws.get_cached(&ZoneId::Forest).is_none());
    }

    #[test]
    fn 던전2층은_위로_올라가는_계단_포탈_하나만_가진다() {
        let portals = zone_portals(&ZoneId::Dungeon(2));
        assert_eq!(portals.len(), 1);
        assert!(matches!(portals[0].0, PortalDirection::StairUp));
        assert_eq!(portals[0].1, ZoneId::Dungeon(1));
    }

    #[test]
    fn 북쪽포탈로_도착하면_플레이어는_맵_남쪽_영역에_스폰된다() {
        let mut map = Map::new(20, 20);
        // 남쪽 행에 Floor 생성
        for x in 0..20 { map.set_tile(x, 18, crate::modules::map::TileKind::Floor); }
        let (_, y) = arrival_pos(&map, &PortalDirection::North);
        assert!(y >= 10, "남쪽 스폰이어야 함: y={}", y);
    }

    #[test]
    fn Named존은_정적_포탈_정의가_비어있다() {
        let portals = zone_portals(&ZoneId::Named("desert".to_string()));
        assert!(portals.is_empty(), "Named 존의 정적 포탈은 없어야 한다");
    }

    #[test]
    fn Named존설정에_항목을_등록하면_이름으로_조회된다() {
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
    fn Named존의_표시이름은_존_이름을_그대로_쓴다() {
        assert_eq!(ZoneId::Named("사막".to_string()).display_name(), "사막");
        assert_eq!(ZoneId::Named("콰스".to_string()).display_name(), "콰스");
    }

    #[test]
    fn 외곽_경계_타일_탐색은_가장_바깥쪽_이동가능_타일을_고른다() {
        // 외곽 한 칸은 모두 Wall, 두 번째 ring 부터 Floor — 두 번째 ring 에서 발견되어야.
        let mut map = Map::new(10, 10);
        for y in 1..9 { for x in 1..9 { map.set_tile(x, y, TileKind::Floor); } }
        let mut used = std::collections::HashSet::new();
        let (x, y) = border_floor_tile(&map, &mut used).expect("Floor 발견 실패");
        // ring=1 외곽선이어야 한다 — 한 변에 닿아 있음.
        assert!(x == 1 || x == 8 || y == 1 || y == 8, "외곽 ring 이어야: ({}, {})", x, y);
    }

    #[test]
    fn 이동가능_타일이_하나도_없으면_외곽_경계_탐색은_None을_반환한다() {
        let map = Map::new(10, 10);
        let mut used = std::collections::HashSet::new();
        assert!(border_floor_tile(&map, &mut used).is_none());
    }

    #[test]
    fn 외곽_경계_탐색은_이미_사용중인_경계_타일은_건너뛴다() {
        // used 에 들어있는 외곽 Floor 는 !used.contains 가 False (690/697:56) 라 제외된다.
        let mut map = Map::new(10, 10);
        for y in 1..9 { for x in 1..9 { map.set_tile(x, y, TileKind::Floor); } }
        let mut used = std::collections::HashSet::new();
        // ring=1 의 가로 변(윗줄/아랫줄)과 세로 변 일부를 점유시킨다.
        for x in 1..9 { used.insert((x, 1)); used.insert((x, 8)); }
        used.insert((1, 2)); used.insert((8, 2));
        used.insert((1, 3)); used.insert((8, 3));
        let (x, y) = border_floor_tile(&map, &mut used).expect("미점유 경계 Floor 실패");
        // 점유되지 않은 세로 변(왼/오른쪽) 의 한 칸이어야 한다.
        assert!((x == 1 || x == 8) && y >= 4, "미점유 경계 타일: ({}, {})", x, y);
    }

    #[test]
    fn 방내부_배치는_방의_이동가능_타일_위치를_반환한다() {
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
    fn 의뢰인근처_배치는_지정_반경_안의_타일을_반환한다() {
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
    fn 의뢰인근처_배치는_의뢰인이_없으면_방내부_배치로_대체된다() {
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
    fn 포탈영속화_보장은_비어있는_존에_기본_포탈을_생성하여_저장한다() {
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
    fn 포탈영속화_보장은_이미_저장된_포탈이_있으면_아무것도_바꾸지_않는다() {
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
    fn 존스냅샷_기본값은_포탈_목록이_비어있다() {
        let s = ZoneSnapshot::default();
        assert!(s.portals.is_empty());
    }

    #[test]
    fn 저장포탈은_직렬화_역직렬화_왕복후에도_값이_보존된다() {
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
    fn 존스냅샷_직렬화는_여러_포탈을_모두_보존한다() {
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
    fn 존스냅샷은_포탈_필드가_없는_구버전_저장본도_역직렬화한다() {
        // 기존 저장 데이터(portals 필드 없음) 호환성 — #[serde(default)] 검증
        let legacy = r#"(blood_stains: [], monster_slots: [], last_visited_turn: 5)"#;
        let parsed: ZoneSnapshot = ron::de::from_str(legacy).unwrap();
        assert_eq!(parsed.last_visited_turn, 5);
        assert!(parsed.portals.is_empty());
    }

    // ── 순수 함수 추가 커버리지 ──────────────────────────────────────────────

    #[test]
    fn 존시드는_종류별로_서로_다른_안정적_시드를_파생한다() {
        // Town(0)/Forest(1)/Dungeon(n)/Named(FNV) 각 분기를 모두 실행한다.
        let g = 12345u64;
        let town = zone_seed(g, &ZoneId::Town);
        let forest = zone_seed(g, &ZoneId::Forest);
        let d1 = zone_seed(g, &ZoneId::Dungeon(1));
        let d2 = zone_seed(g, &ZoneId::Dungeon(2));
        let named = zone_seed(g, &ZoneId::Named("desert".to_string()));
        // 같은 입력이면 시드는 안정적이다
        assert_eq!(named, zone_seed(g, &ZoneId::Named("desert".to_string())));
        // 서로 다른 존은 다른 시드를 가진다
        let set: std::collections::HashSet<u64> = [town, forest, d1, d2, named].into_iter().collect();
        assert_eq!(set.len(), 5, "존마다 시드가 달라야 한다");
        // FNV 해시는 이름이 다르면 다른 시드를 만든다
        assert_ne!(named, zone_seed(g, &ZoneId::Named("forest_glade".to_string())));
    }

    #[test]
    fn Named존의_알고리즘은_기본_bsp를_반환한다() {
        assert_eq!(ZoneId::Named("desert".to_string()).algorithm(), "bsp");
    }

    #[test]
    fn 일반_던전층은_위아래_두_계단_포탈을_가진다() {
        // Dungeon(3) 은 와일드카드 arm(n) 으로 StairUp(2)/StairDown(4) 두 개를 만든다.
        let portals = zone_portals(&ZoneId::Dungeon(3));
        assert_eq!(portals.len(), 2);
        assert!(matches!(portals[0].0, PortalDirection::StairUp));
        assert_eq!(portals[0].1, ZoneId::Dungeon(2));
        assert!(matches!(portals[1].0, PortalDirection::StairDown));
        assert_eq!(portals[1].1, ZoneId::Dungeon(4));
    }

    #[test]
    fn 마을은_기본_포털을_갖지_않고_던전1층은_위아래_두_포털을_가진다() {
        // Town arm: 사용자 혼란 줄이려고 신규 게임 시작 시 기본 포털을 두지 않는다.
        //   - 퀘스트 OpenPortal 로만 생기고, 다른 zone 의 return 포털은 그쪽 정의에 있다.
        let town = zone_portals(&ZoneId::Town);
        assert!(town.is_empty(), "마을은 기본 자동 포털이 없어야 한다");
        let d1 = zone_portals(&ZoneId::Dungeon(1));
        assert_eq!(d1.len(), 2);
        assert_eq!(d1[0].1, ZoneId::Forest);
        assert_eq!(d1[1].1, ZoneId::Dungeon(2));
    }

    #[test]
    fn 숲은_마을로_돌아가는_복귀_포털을_여전히_가진다() {
        // 다른 zone(여기서는 Forest) 에서 마을로 돌아오는 return 포털은 유지된다.
        let forest = zone_portals(&ZoneId::Forest);
        assert!(
            forest.iter().any(|(_, target)| *target == ZoneId::Town),
            "숲에서 마을로 돌아가는 포털은 살아 있어야 한다 (마을에 갇히지 않게)",
        );
    }

    #[test]
    fn 포탈방향별_글리프는_방향에_맞는_기호를_반환한다() {
        assert_eq!(PortalDirection::North.glyph(), "⬡");
        assert_eq!(PortalDirection::South.glyph(), "⬡");
        assert_eq!(PortalDirection::StairDown.glyph(), ">");
        assert_eq!(PortalDirection::StairUp.glyph(), "<");
    }

    #[test]
    fn 무작위_배치는_방의_이동가능_타일을_반환한다() {
        use crate::modules::quest::PortalPlacement;
        let mut map = Map::new(20, 20);
        for y in 5..10 { for x in 5..10 { map.set_tile(x, y, TileKind::Floor); } }
        map.rooms.push(Rect::new(5, 5, 5, 5));
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let (x, y) = compute_portal_pos(&map, &PortalPlacement::Random, None,
            &mut used, &mut rng).expect("Random 실패");
        assert!(map.get_tile(x, y).is_walkable());
    }

    #[test]
    fn 외곽_배치는_맵_경계의_이동가능_타일을_반환한다() {
        use crate::modules::quest::PortalPlacement;
        let mut map = Map::new(10, 10);
        for y in 1..9 { for x in 1..9 { map.set_tile(x, y, TileKind::Floor); } }
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        let (x, y) = compute_portal_pos(&map, &PortalPlacement::Border, None,
            &mut used, &mut rng).expect("Border 실패");
        assert!(x == 1 || x == 8 || y == 1 || y == 8, "외곽 ring 이어야: ({}, {})", x, y);
    }

    #[test]
    fn 의뢰인근처_탐색은_맵_경계를_벗어나는_후보는_제외한다() {
        // 의뢰인이 (0,0) 모서리면 음수·범위초과 좌표 분기(726행)를 탄다.
        let mut map = Map::new(20, 20);
        for y in 0..3 { for x in 0..3 { map.set_tile(x, y, TileKind::Floor); } }
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        let (x, y) = floor_tile_near(&map, 0, 0, 2, &mut used, &mut rng)
            .expect("모서리 근처 Floor 발견 실패");
        // 음수로 나간 좌표는 후보에서 빠지므로 결과는 항상 맵 안의 Floor.
        assert!(x < 20 && y < 20 && map.get_tile(x, y).is_walkable());
        assert!((x as i32 - 0).abs() <= 2 && (y as i32 - 0).abs() <= 2);
    }

    #[test]
    fn 의뢰인근처에_이동가능_타일이_없으면_None을_반환한다() {
        let map = Map::new(20, 20); // 전부 Wall
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        assert!(floor_tile_near(&map, 10, 10, 2, &mut used, &mut rng).is_none());
    }

    #[test]
    fn 의뢰인근처_탐색은_맵_우하단_경계초과_좌표도_제외한다() {
        // 의뢰인이 (19,19) 모서리면 nx>=width, ny>=height 분기(727:36/62)를 탄다.
        let w = 20; let h = 20;
        let mut map = Map::new(w, h);
        for y in h - 3..h { for x in w - 3..w { map.set_tile(x, y, TileKind::Floor); } }
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(11);
        let (x, y) = floor_tile_near(&map, w - 1, h - 1, 2, &mut used, &mut rng)
            .expect("우하단 근처 Floor 발견 실패");
        assert!(x < w && y < h && map.get_tile(x, y).is_walkable());
    }

    #[test]
    fn 의뢰인근처_탐색은_이미_사용중인_타일은_후보에서_제외한다() {
        // used 에 들어있는 인접 타일은 !used.contains 가 False (729:54) 라 제외된다.
        let mut map = Map::new(20, 20);
        // (10,10) 주변에 floor 두 칸만
        map.set_tile(11, 10, TileKind::Floor);
        map.set_tile(9, 10, TileKind::Floor);
        let mut used = std::collections::HashSet::new();
        used.insert((11, 10)); // 한 칸은 이미 점유
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        let pos = floor_tile_near(&map, 10, 10, 2, &mut used, &mut rng)
            .expect("미점유 Floor 발견 실패");
        assert_eq!(pos, (9, 10), "점유 안 된 칸만 후보가 된다");
    }

    #[test]
    fn 북쪽포탈_타일은_중앙열의_첫_이동가능_타일을_찾는다() {
        let mut map = Map::new(20, 20);
        let cx = 20 / 2;
        map.set_tile(cx, 4, TileKind::Floor);
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        let (px, py) = portal_tile(&map, &PortalDirection::North, &mut used, &mut rng)
            .expect("북쪽 포탈 타일 실패");
        assert_eq!((px, py), (cx, 4));
    }

    #[test]
    fn 북쪽포탈_타일은_중앙열에_이동가능_타일이_없으면_None이다() {
        let map = Map::new(20, 20); // 전부 Wall
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        assert!(portal_tile(&map, &PortalDirection::North, &mut used, &mut rng).is_none());
    }

    #[test]
    fn 남쪽포탈_타일은_중앙열의_가장_아래쪽_이동가능_타일을_찾는다() {
        let mut map = Map::new(20, 20);
        let cx = 20 / 2;
        map.set_tile(cx, 3, TileKind::Floor);
        map.set_tile(cx, 17, TileKind::Floor);
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        let (px, py) = portal_tile(&map, &PortalDirection::South, &mut used, &mut rng)
            .expect("남쪽 포탈 타일 실패");
        assert_eq!((px, py), (cx, 17), "남쪽은 가장 아래쪽 Floor");
    }

    #[test]
    fn 남쪽포탈_타일은_중앙열에_이동가능_타일이_없으면_None이다() {
        let map = Map::new(20, 20);
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        assert!(portal_tile(&map, &PortalDirection::South, &mut used, &mut rng).is_none());
    }

    #[test]
    fn 아래계단_포탈은_마지막_방의_이동가능_타일을_쓰고_없으면_방중앙으로_대체한다() {
        // 방이 있고 Floor 있음 → 방 안 Floor
        let mut map = Map::new(20, 20);
        for y in 12..18 { for x in 12..18 { map.set_tile(x, y, TileKind::Floor); } }
        map.rooms.push(Rect::new(12, 12, 5, 5));
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        let (px, py) = portal_tile(&map, &PortalDirection::StairDown, &mut used, &mut rng)
            .expect("아래계단 실패");
        assert!(map.get_tile(px, py).is_walkable());

        // 방은 있으나 Floor 가 없음 → center() 대체 경로
        let mut empty = Map::new(20, 20);
        empty.rooms.push(Rect::new(2, 2, 4, 4));
        let mut used2 = std::collections::HashSet::new();
        let center = empty.rooms.last().unwrap().center();
        let pos = portal_tile(&empty, &PortalDirection::StairDown, &mut used2, &mut rng)
            .expect("center 대체 실패");
        assert_eq!(pos, center);
    }

    #[test]
    fn 위계단_포탈은_첫_방의_이동가능_타일을_쓰고_없으면_방중앙으로_대체한다() {
        let mut map = Map::new(20, 20);
        for y in 2..8 { for x in 2..8 { map.set_tile(x, y, TileKind::Floor); } }
        map.rooms.push(Rect::new(2, 2, 5, 5));
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        let (px, py) = portal_tile(&map, &PortalDirection::StairUp, &mut used, &mut rng)
            .expect("위계단 실패");
        assert!(map.get_tile(px, py).is_walkable());

        let mut empty = Map::new(20, 20);
        empty.rooms.push(Rect::new(3, 3, 4, 4));
        let mut used2 = std::collections::HashSet::new();
        let center = empty.rooms.first().unwrap().center();
        let pos = portal_tile(&empty, &PortalDirection::StairUp, &mut used2, &mut rng)
            .expect("center 대체 실패");
        assert_eq!(pos, center);
    }

    #[test]
    fn 북쪽도착은_남쪽_Floor_위_한칸이_이동가능하면_그_위로_스폰한다() {
        // y2(한 칸 위)가 walkable 인 경로 (788행 True).
        let mut map = Map::new(20, 20);
        let cx = 20 / 2;
        map.set_tile(cx, 17, TileKind::Floor);
        map.set_tile(cx, 18, TileKind::Floor);
        let (x, y) = arrival_pos(&map, &PortalDirection::North);
        assert_eq!(x, cx);
        assert_eq!(y, 18, "아래 Floor 의 한 칸 위가 walkable 이면 거기로");
    }

    #[test]
    fn 북쪽도착은_Floor_위가_벽이면_그_Floor_자리에_스폰한다() {
        // y2 가 walkable 이 아닌 경로 (788행 False) — 맨 아래 행 하나만 Floor.
        let mut map = Map::new(20, 20);
        let cx = 20 / 2;
        map.set_tile(cx, 19, TileKind::Floor);
        let (x, y) = arrival_pos(&map, &PortalDirection::North);
        assert_eq!((x, y), (cx, 19));
    }

    #[test]
    fn 북쪽도착은_Floor가_전혀_없으면_맵_중앙으로_대체한다() {
        let map = Map::new(20, 20);
        let (x, y) = arrival_pos(&map, &PortalDirection::North);
        assert_eq!((x, y), (10, 10));
    }

    #[test]
    fn 남쪽도착은_북쪽_Floor_아래_한칸이_이동가능하면_그_아래로_스폰한다() {
        // South: y2(한 칸 아래) walkable 경로 (798/800행).
        let mut map = Map::new(20, 20);
        let cx = 20 / 2;
        map.set_tile(cx, 1, TileKind::Floor);
        map.set_tile(cx, 2, TileKind::Floor);
        let (x, y) = arrival_pos(&map, &PortalDirection::South);
        assert_eq!(x, cx);
        // 위에서부터 첫 Floor 는 y=1, 그 한 칸 아래(0)가 walkable? y2=saturating_sub(1)=0 은 Wall.
        // 실제로는 첫 Floor=1, y2=0 은 벽이므로 (cx,1) 반환.
        assert_eq!(y, 1);
    }

    #[test]
    fn 남쪽도착은_위쪽_Floor_바로_위가_이동가능하면_그_위칸으로_스폰한다() {
        // 맨 윗행이 Floor 가 아니고 두번째행부터 Floor 두 줄 → 첫 Floor 의 한 칸 위(walkable)로.
        let mut map = Map::new(20, 20);
        let cx = 20 / 2;
        map.set_tile(cx, 5, TileKind::Floor);
        map.set_tile(cx, 6, TileKind::Floor);
        let (x, y) = arrival_pos(&map, &PortalDirection::South);
        assert_eq!(x, cx);
        // 첫 Floor=5, y2=4 는 벽 → (cx,5). 한 칸 위 walkable 경로는 연속 두 행 Floor 가 위에서 시작해야.
        assert_eq!(y, 5);
    }

    #[test]
    fn 남쪽도착은_상단부터_연속_Floor면_위쪽_walkable_칸으로_스폰한다() {
        let mut map = Map::new(20, 20);
        let cx = 20 / 2;
        // y=0,1 둘 다 Floor → 첫 Floor=0? 아니, y=0 도 Floor. 첫 Floor=0, y2=sub(1)=0 (sat) walkable → (cx,0)
        map.set_tile(cx, 0, TileKind::Floor);
        map.set_tile(cx, 1, TileKind::Floor);
        let (x, y) = arrival_pos(&map, &PortalDirection::South);
        assert_eq!(x, cx);
        assert_eq!(y, 0);
    }

    #[test]
    fn 남쪽도착은_Floor가_전혀_없으면_맵_중앙으로_대체한다() {
        let map = Map::new(20, 20);
        let (x, y) = arrival_pos(&map, &PortalDirection::South);
        assert_eq!((x, y), (10, 10));
    }

    #[test]
    fn 아래계단_도착은_첫_방_중앙이고_방이_없으면_맵_중앙이다() {
        let mut map = Map::new(20, 20);
        map.rooms.push(Rect::new(4, 4, 4, 4));
        let c = map.rooms.first().unwrap().center();
        assert_eq!(arrival_pos(&map, &PortalDirection::StairDown), c);
        let empty = Map::new(20, 20);
        assert_eq!(arrival_pos(&empty, &PortalDirection::StairDown), (10, 10));
    }

    #[test]
    fn 위계단_도착은_마지막_방_중앙이고_방이_없으면_맵_중앙이다() {
        let mut map = Map::new(20, 20);
        map.rooms.push(Rect::new(2, 2, 3, 3));
        map.rooms.push(Rect::new(12, 12, 4, 4));
        let c = map.rooms.last().unwrap().center();
        assert_eq!(arrival_pos(&map, &PortalDirection::StairUp), c);
        let empty = Map::new(20, 20);
        assert_eq!(arrival_pos(&empty, &PortalDirection::StairUp), (10, 10));
    }

    #[test]
    fn 포탈영속화_보장은_방에_바닥이_없으면_계단포탈을_방중앙에_배치한다() {
        // 프로덕션 thread_rng 경로의 portal_tile StairUp/StairDown center() 대체 클로저를 탄다.
        // Dungeon(3) 은 StairUp/StairDown 두 포탈을 쓰므로 두 분기를 함께 커버한다.
        let mut map = Map::new(20, 20);
        // 방은 있으나 Floor 가 하나도 없음 → random_floor_tile_in_room None → center() 대체
        map.rooms.push(Rect::new(2, 2, 4, 4));
        map.rooms.push(Rect::new(12, 12, 4, 4));
        let named = NamedZoneConfig::default();
        let mut p = ZonePersistence::default();
        let mut used = std::collections::HashSet::new();
        ensure_zone_portals_persisted(&ZoneId::Dungeon(3), &map, &named, &mut p, &mut used);
        let snap = p.0.get(&ZoneId::Dungeon(3)).unwrap();
        // 두 계단 포탈이 각 방 중앙에 배치된다.
        assert_eq!(snap.portals.len(), 2);
        let up = map.rooms.first().unwrap().center();
        let down = map.rooms.last().unwrap().center();
        assert!(snap.portals.iter().any(|p| (p.tile_x, p.tile_y) == up));
        assert!(snap.portals.iter().any(|p| (p.tile_x, p.tile_y) == down));
    }

    #[test]
    fn 포탈영속화_보장은_미등록_Named존이면_원점복귀_포탈을_추가하지_않는다() {
        // zone 이 Named 이지만 named_config 에 없으면 entry None (402:16 False) → 추가 없음.
        let mut map = Map::new(40, 30);
        for y in 1..29 { for x in 1..39 { map.set_tile(x, y, TileKind::Floor); } }
        map.rooms = vec![Rect::new(2, 2, 36, 26)];
        let named = NamedZoneConfig::default(); // 비어있음
        let mut p = ZonePersistence::default();
        let mut used = std::collections::HashSet::new();
        ensure_zone_portals_persisted(
            &ZoneId::Named("ghost".to_string()), &map, &named, &mut p, &mut used);
        let snap = p.0.get(&ZoneId::Named("ghost".to_string())).unwrap();
        // Named 의 정적 포탈은 없고, 미등록이라 원점복귀 포탈도 없다 → 빈 목록.
        assert!(snap.portals.is_empty(), "미등록 Named 존은 포탈이 없어야 한다");
    }

    #[test]
    fn 포탈영속화_보장은_Named존에서_원점복귀_포탈과_진입_포탈을_함께_배치한다() {
        // 401/402/408 분기: Named 존 안의 StairUp(origin), origin 존의 StairDown(Named).
        let mut map = Map::new(40, 30);
        for y in 1..29 { for x in 1..39 { map.set_tile(x, y, TileKind::Floor); } }
        map.rooms = vec![Rect::new(2, 2, 36, 26)];

        let mut named = NamedZoneConfig::default();
        named.zones.insert("herb_glade".to_string(), NamedZoneEntry {
            generator: "bsp".to_string(),
            origin: ZoneId::Forest,
        });

        // (1) Named 존 안: origin(Forest) 로 돌아가는 StairUp 포탈이 생성된다.
        let mut p1 = ZonePersistence::default();
        let mut used1 = std::collections::HashSet::new();
        ensure_zone_portals_persisted(
            &ZoneId::Named("herb_glade".to_string()), &map, &named, &mut p1, &mut used1);
        let snap = p1.0.get(&ZoneId::Named("herb_glade".to_string())).unwrap();
        assert!(snap.portals.iter().any(|p| p.target == ZoneId::Forest
            && matches!(p.arrive_from, PortalDirection::StairUp)));

        // (2) origin(Forest) 안: Named 존으로 가는 StairDown 진입 포탈이 추가된다.
        let mut p2 = ZonePersistence::default();
        let mut used2 = std::collections::HashSet::new();
        ensure_zone_portals_persisted(&ZoneId::Forest, &map, &named, &mut p2, &mut used2);
        let fsnap = p2.0.get(&ZoneId::Forest).unwrap();
        assert!(fsnap.portals.iter().any(|p|
            p.target == ZoneId::Named("herb_glade".to_string())
            && matches!(p.arrive_from, PortalDirection::StairDown)));
    }

    // ── App 하네스: 시스템 실행 경로 ────────────────────────────────────────

    /// 테스트용 이름표 맵 생성기 — 요청한 크기의 Floor 가득찬 맵을 만든다.
    struct FloorGen(&'static str);
    impl crate::modules::map::MapGenerator for FloorGen {
        fn generate(&self, width: usize, height: usize, _seed: u64) -> Map {
            let mut m = Map::new(width, height);
            for y in 1..height - 1 { for x in 1..width - 1 { m.set_tile(x, y, TileKind::Floor); } }
            m.rooms.push(Rect::new(2, 2, width - 4, height - 4));
            m
        }
        fn name(&self) -> &str { self.0 }
    }

    fn test_registry() -> MapGeneratorRegistry {
        let mut r = MapGeneratorRegistry::new();
        for n in ["organic_village", "forest", "bsp"] {
            r.register(Box::new(FloorGen(n)));
        }
        r
    }

    fn floor_map(w: usize, h: usize) -> Map {
        let mut m = Map::new(w, h);
        for y in 1..h - 1 { for x in 1..w - 1 { m.set_tile(x, y, TileKind::Floor); } }
        m.rooms.push(Rect::new(2, 2, w - 4, h - 4));
        m
    }

    /// 시스템 테스트에 필요한 공통 리소스/에셋을 갖춘 App 을 만든다.
    fn harness(current: ZoneId, map: Map) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app.init_asset::<Image>();
        app.insert_resource(MapResource(map));
        app.insert_resource(WorldState { current, maps: HashMap::new() });
        app.init_resource::<ZonePersistence>();
        app.init_resource::<NamedZoneConfig>();
        app.insert_resource(test_registry());
        app.insert_resource(GlobalTurn(0));
        app.insert_resource(GlobalSeed(42));
        app.init_resource::<UsedSpawnTiles>();
        app.init_resource::<crate::modules::ui::minimap::DiscoveredMarkers>();
        app.init_resource::<crate::modules::quest::QuestRegistry>();
        app
    }

    #[test]
    fn 플러그인을_추가하면_존_리소스와_이벤트가_등록되고_빌드가_실행된다() {
        let mut app = App::new();
        app.add_plugins(ZonePlugin);
        assert!(app.world.get_resource::<WorldState>().is_some());
        assert!(app.world.get_resource::<ZonePersistence>().is_some());
        assert!(app.world.get_resource::<NamedZoneConfig>().is_some());
        assert!(app.world.get_resource::<Events<ZoneTransitionEvent>>().is_some());
        assert!(app.world.get_resource::<Events<SpawnQuestPortalEvent>>().is_some());
        assert!(app.world.get_resource::<Events<CloseQuestPortalEvent>>().is_some());
    }

    #[test]
    fn 시작맵_캐시_시스템은_마을_생성기를_선택하고_현재맵을_캐시한다() {
        let mut app = harness(ZoneId::Town, floor_map(20, 20));
        app.add_systems(Update, cache_initial_map);
        app.update();
        let ws = app.world.resource::<WorldState>();
        assert!(ws.get_cached(&ZoneId::Town).is_some(), "Town 맵이 캐시됨");
        let reg = app.world.resource::<MapGeneratorRegistry>();
        assert_eq!(reg.current_name(), "organic_village");
    }

    #[test]
    fn 행동이_없으면_포탈충돌_시스템은_발동상태를_초기화하고_종료한다() {
        let mut app = harness(ZoneId::Town, floor_map(20, 20));
        app.add_event::<ZoneTransitionEvent>();
        app.add_event::<crate::modules::map::PlayerActedEvent>();
        app.add_systems(Update, check_portal_collision);
        // PlayerActedEvent 없음 → acted.is_empty() True 경로
        app.update();
        let ev = app.world.resource::<Events<ZoneTransitionEvent>>();
        assert_eq!(ev.len(), 0);
    }

    #[test]
    fn 플레이어가_포탈칸에_있고_행동했으면_포탈충돌_시스템이_전환_이벤트를_발행한다() {
        let mut app = harness(ZoneId::Town, floor_map(20, 20));
        app.add_event::<ZoneTransitionEvent>();
        app.add_event::<crate::modules::map::PlayerActedEvent>();
        app.add_systems(Update, check_portal_collision);

        let coord = tile_to_world_coords(5, 5);
        // 플레이어 (MovingTo 목적지 = 포탈 칸)
        app.world.spawn((
            crate::modules::player::Player,
            Transform::from_xyz(0.0, 0.0, 0.0),
            MovingTo { target: coord.extend(0.0) },
        ));
        app.world.spawn((
            Transform::from_xyz(coord.x, coord.y, 1.5),
            ZonePortal { target: ZoneId::Forest, arrive_from: PortalDirection::South },
        ));
        app.world.send_event(crate::modules::map::PlayerActedEvent);
        app.update();

        let ev = app.world.resource::<Events<ZoneTransitionEvent>>();
        let mut reader = ev.get_reader();
        let sent: Vec<_> = reader.read(ev).collect();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].target, ZoneId::Forest);
    }

    #[test]
    fn 포탈충돌_시스템은_같은_행동턴에_중복으로_발동하지_않는다() {
        let mut app = harness(ZoneId::Town, floor_map(20, 20));
        app.add_event::<ZoneTransitionEvent>();
        app.add_event::<crate::modules::map::PlayerActedEvent>();
        app.add_systems(Update, check_portal_collision);

        let coord = tile_to_world_coords(5, 5);
        app.world.spawn((
            crate::modules::player::Player,
            Transform::from_xyz(coord.x, coord.y, 0.0),
        ));
        app.world.spawn((
            Transform::from_xyz(coord.x, coord.y, 1.5),
            ZonePortal { target: ZoneId::Forest, arrive_from: PortalDirection::South },
        ));
        // 첫 update: 발동
        app.world.send_event(crate::modules::map::PlayerActedEvent);
        app.update();
        // 두번째 update: acted 가 또 있어도 *triggered 가 true 라 조기 종료
        app.world.send_event(crate::modules::map::PlayerActedEvent);
        app.update();

        let ev = app.world.resource::<Events<ZoneTransitionEvent>>();
        assert_eq!(ev.len(), 1, "한 번만 발행되어야 한다");
    }

    #[test]
    fn 포탈충돌_시스템은_같은_열이라도_행이_다르면_전환하지_않는다() {
        // px==tx 이지만 py!=ty (251:24 False 경로).
        let mut app = harness(ZoneId::Town, floor_map(20, 20));
        app.add_event::<ZoneTransitionEvent>();
        app.add_event::<crate::modules::map::PlayerActedEvent>();
        app.add_systems(Update, check_portal_collision);

        let player_c = tile_to_world_coords(5, 5);
        let portal_c = tile_to_world_coords(5, 9); // 같은 x, 다른 y
        app.world.spawn((
            crate::modules::player::Player,
            Transform::from_xyz(player_c.x, player_c.y, 0.0),
        ));
        app.world.spawn((
            Transform::from_xyz(portal_c.x, portal_c.y, 1.5),
            ZonePortal { target: ZoneId::Forest, arrive_from: PortalDirection::South },
        ));
        app.world.send_event(crate::modules::map::PlayerActedEvent);
        app.update();
        assert_eq!(app.world.resource::<Events<ZoneTransitionEvent>>().len(), 0);
    }

    #[test]
    fn 맵적용_후_포탈스폰은_Named대상_포탈을_퀘스트색으로_복원한다() {
        // spawn_portal_entity 의 is_quest_portal True 경로 (435:20).
        let mut app = harness(ZoneId::Forest, floor_map(40, 30));
        {
            let mut per = app.world.resource_mut::<ZonePersistence>();
            per.0.entry(ZoneId::Forest).or_default().portals = vec![
                SavedPortal { tile_x: 5, tile_y: 6,
                    target: ZoneId::Named("herb_glade".to_string()),
                    arrive_from: PortalDirection::StairDown },
            ];
        }
        app.add_systems(Update, spawn_portals_after_apply);
        app.world.resource_mut::<MapResource>().set_changed();
        app.update();
        let mut q = app.world.query::<&ZonePortal>();
        let p = q.iter(&app.world).next().expect("Named 포탈 스폰됨");
        assert_eq!(p.target, ZoneId::Named("herb_glade".to_string()));
    }

    #[test]
    fn 플레이어가_포탈칸에_없으면_포탈충돌_시스템은_전환을_발행하지_않는다() {
        let mut app = harness(ZoneId::Town, floor_map(20, 20));
        app.add_event::<ZoneTransitionEvent>();
        app.add_event::<crate::modules::map::PlayerActedEvent>();
        app.add_systems(Update, check_portal_collision);

        // 플레이어 없음 → get_single() Err 경로
        app.world.send_event(crate::modules::map::PlayerActedEvent);
        app.update();
        assert_eq!(app.world.resource::<Events<ZoneTransitionEvent>>().len(), 0);

        // 플레이어는 있으나 포탈과 다른 칸 (Transform 경로, MovingTo 없음)
        app.world.spawn((
            crate::modules::player::Player,
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        let coord = tile_to_world_coords(5, 5);
        app.world.spawn((
            Transform::from_xyz(coord.x, coord.y, 1.5),
            ZonePortal { target: ZoneId::Forest, arrive_from: PortalDirection::South },
        ));
        app.world.send_event(crate::modules::map::PlayerActedEvent);
        app.update();
        assert_eq!(app.world.resource::<Events<ZoneTransitionEvent>>().len(), 0);
    }

    #[test]
    fn 존전환_시스템은_새_존을_생성하고_맵적용_이벤트와_로그를_발행한다() {
        let mut app = harness(ZoneId::Town, floor_map(40, 30));
        app.add_event::<ZoneTransitionEvent>();
        app.add_event::<ApplyMapEvent>();
        app.add_event::<crate::modules::ui::LogMessage>();
        app.add_systems(Update, handle_zone_transition);

        // 떠나는 존의 혈흔/포탈 엔티티
        app.world.spawn((
            BloodStain { alpha: 1.0, decay_per_turn: 0.1 },
            Transform::from_translation(tile_to_world_coords(3, 3).extend(Z_BLOOD)),
        ));
        let pcoord = tile_to_world_coords(4, 4);
        let portal_e = app.world.spawn((
            Transform::from_xyz(pcoord.x, pcoord.y, 1.5),
            ZonePortal { target: ZoneId::Forest, arrive_from: PortalDirection::South },
        )).id();

        app.world.send_event(ZoneTransitionEvent {
            target: ZoneId::Forest,
            arrive_from: PortalDirection::South,
        });
        app.update();

        // 맵 적용 이벤트가 발행되었다
        let ap = app.world.resource::<Events<ApplyMapEvent>>();
        assert_eq!(ap.len(), 1);
        // 로그 발행
        assert_eq!(app.world.resource::<Events<crate::modules::ui::LogMessage>>().len(), 1);
        // 현재 존이 Forest 로 바뀌고, 떠난 존 스냅샷에 혈흔/포탈 저장됨
        let ws = app.world.resource::<WorldState>();
        assert_eq!(ws.current, ZoneId::Forest);
        let per = app.world.resource::<ZonePersistence>();
        let town_snap = per.0.get(&ZoneId::Town).unwrap();
        assert_eq!(town_snap.blood_stains.len(), 1);
        assert_eq!(town_snap.portals.len(), 1);
        // 떠난 존의 포탈 엔티티는 despawn 됨
        assert!(app.world.get_entity(portal_e).is_none());
    }

    #[test]
    fn 존전환_시스템은_캐시된_맵이_있으면_재생성하지_않고_재사용한다() {
        let mut cached = floor_map(40, 30);
        cached.algorithm = "cached_marker".to_string();
        let mut app = harness(ZoneId::Town, floor_map(40, 30));
        // Forest 를 미리 캐시
        app.world.resource_mut::<WorldState>().maps.insert(ZoneId::Forest, cached);
        app.add_event::<ZoneTransitionEvent>();
        app.add_event::<ApplyMapEvent>();
        app.add_event::<crate::modules::ui::LogMessage>();
        app.add_systems(Update, handle_zone_transition);

        app.world.send_event(ZoneTransitionEvent {
            target: ZoneId::Forest,
            arrive_from: PortalDirection::South,
        });
        app.update();

        let ws = app.world.resource::<WorldState>();
        assert_eq!(ws.maps.get(&ZoneId::Forest).unwrap().algorithm, "cached_marker",
            "캐시된 맵을 그대로 재사용해야 한다");
    }

    #[test]
    fn 존전환_시스템은_알수없는_생성기면_빈맵으로_대체생성한다() {
        // registry 에 없는 알고리즘을 쓰는 Named 존 → generate_with None → 빈맵 fallback.
        let mut app = harness(ZoneId::Town, floor_map(40, 30));
        app.add_event::<ZoneTransitionEvent>();
        app.add_event::<ApplyMapEvent>();
        app.add_event::<crate::modules::ui::LogMessage>();
        app.world.resource_mut::<NamedZoneConfig>().zones.insert(
            "void".to_string(),
            NamedZoneEntry { generator: "no_such_gen".to_string(), origin: ZoneId::Town },
        );
        app.add_systems(Update, handle_zone_transition);

        app.world.send_event(ZoneTransitionEvent {
            target: ZoneId::Named("void".to_string()),
            arrive_from: PortalDirection::StairDown,
        });
        app.update();

        let ws = app.world.resource::<WorldState>();
        assert_eq!(ws.current, ZoneId::Named("void".to_string()));
        assert!(ws.maps.contains_key(&ZoneId::Named("void".to_string())));
    }

    #[test]
    fn 존전환_시스템은_미등록_Named존이면_생성기를_bsp로_기본설정한다() {
        // target 이 Named 이고 named_config 에 없으면 algo unwrap_or_else("bsp") 분기를 탄다.
        let mut app = harness(ZoneId::Town, floor_map(40, 30));
        app.add_event::<ZoneTransitionEvent>();
        app.add_event::<ApplyMapEvent>();
        app.add_event::<crate::modules::ui::LogMessage>();
        // NamedZoneConfig 에 "lost" 항목을 등록하지 않는다.
        app.add_systems(Update, handle_zone_transition);
        app.world.send_event(ZoneTransitionEvent {
            target: ZoneId::Named("lost".to_string()),
            arrive_from: PortalDirection::StairDown,
        });
        app.update();
        let ws = app.world.resource::<WorldState>();
        // bsp 생성기(test_registry 의 FloorGen "bsp")로 생성된 맵이 캐시된다.
        let m = ws.maps.get(&ZoneId::Named("lost".to_string())).unwrap();
        assert_eq!(m.algorithm, "bsp");
    }

    #[test]
    fn 존전환_시_복귀포탈이_저장돼있으면_그_위치로_스폰위치를_정한다() {
        // 도착 존(Forest) 에 target==from(Town) 인 포탈이 미리 저장돼 있으면 그 좌표로 스폰.
        let mut app = harness(ZoneId::Town, floor_map(40, 30));
        app.add_event::<ZoneTransitionEvent>();
        app.add_event::<ApplyMapEvent>();
        app.add_event::<crate::modules::ui::LogMessage>();
        {
            let mut per = app.world.resource_mut::<ZonePersistence>();
            per.0.entry(ZoneId::Forest).or_default().portals = vec![SavedPortal {
                tile_x: 7, tile_y: 9,
                target: ZoneId::Town,
                arrive_from: PortalDirection::North,
            }];
        }
        app.add_systems(Update, handle_zone_transition);
        app.world.send_event(ZoneTransitionEvent {
            target: ZoneId::Forest,
            arrive_from: PortalDirection::South,
        });
        app.update();

        let ap = app.world.resource::<Events<ApplyMapEvent>>();
        let mut reader = ap.get_reader();
        let sent: Vec<_> = reader.read(ap).collect();
        assert_eq!(sent[0].spawn_pos, Some((7, 9)), "복귀 포탈 위치로 스폰");
    }

    #[test]
    fn 맵적용_후_포탈스폰_시스템은_영속화된_포탈을_엔티티로_복원한다() {
        let mut app = harness(ZoneId::Forest, floor_map(40, 30));
        {
            let mut per = app.world.resource_mut::<ZonePersistence>();
            per.0.entry(ZoneId::Forest).or_default().portals = vec![
                SavedPortal { tile_x: 5, tile_y: 6, target: ZoneId::Town, arrive_from: PortalDirection::North },
                SavedPortal { tile_x: 9, tile_y: 9, target: ZoneId::Dungeon(1), arrive_from: PortalDirection::South },
            ];
        }
        app.add_systems(Update, spawn_portals_after_apply);
        // MapResource 를 변경 처리시켜 is_changed() True 로 만든다
        app.world.resource_mut::<MapResource>().set_changed();
        app.update();

        let n = app.world.query::<&ZonePortal>().iter(&app.world).count();
        assert_eq!(n, 2, "저장된 포탈 두 개가 스폰됨");
    }

    #[test]
    fn 포탈스폰_시스템은_맵이_바뀌지_않았으면_아무것도_하지_않는다() {
        let mut app = harness(ZoneId::Forest, floor_map(40, 30));
        {
            let mut per = app.world.resource_mut::<ZonePersistence>();
            per.0.entry(ZoneId::Forest).or_default().portals = vec![
                SavedPortal { tile_x: 5, tile_y: 6, target: ZoneId::Town, arrive_from: PortalDirection::North },
            ];
        }
        app.add_systems(Update, spawn_portals_after_apply);
        // set_changed 하지 않음 → is_changed() 가 false 인 경로 (단, 삽입 후 첫 프레임은 changed)
        app.update(); // 첫 프레임은 변경으로 간주되어 스폰됨
        let after_first = app.world.query::<&ZonePortal>().iter(&app.world).count();
        app.update(); // 두번째 프레임: 변경 없음 → 추가 스폰 없음 + portal_q 비어있지 않음
        let after_second = app.world.query::<&ZonePortal>().iter(&app.world).count();
        assert_eq!(after_first, after_second, "맵 미변경 시 추가 스폰 없음");
    }

    #[test]
    fn 포탈스폰_시스템은_이미_포탈이_있으면_재스폰하지_않는다() {
        let mut app = harness(ZoneId::Forest, floor_map(40, 30));
        {
            let mut per = app.world.resource_mut::<ZonePersistence>();
            per.0.entry(ZoneId::Forest).or_default().portals = vec![
                SavedPortal { tile_x: 5, tile_y: 6, target: ZoneId::Town, arrive_from: PortalDirection::North },
            ];
        }
        // 이미 포탈 엔티티 하나 존재
        app.world.spawn((Transform::default(),
            ZonePortal { target: ZoneId::Town, arrive_from: PortalDirection::North }));
        app.add_systems(Update, spawn_portals_after_apply);
        app.world.resource_mut::<MapResource>().set_changed();
        app.update();
        let n = app.world.query::<&ZonePortal>().iter(&app.world).count();
        assert_eq!(n, 1, "포탈이 이미 있으면 portal_q.is_empty() False 로 조기 종료");
    }

    #[test]
    fn 포탈스폰_시스템은_영속화가_비어있으면_새로_생성하여_스폰한다() {
        // persistence 가 비어있는 첫 방문 경로 — ensure_zone_portals_persisted 가 채운다.
        let mut app = harness(ZoneId::Forest, floor_map(40, 30));
        app.add_systems(Update, spawn_portals_after_apply);
        app.world.resource_mut::<MapResource>().set_changed();
        app.update();
        let per = app.world.resource::<ZonePersistence>();
        assert!(per.0.get(&ZoneId::Forest).map(|s| !s.portals.is_empty()).unwrap_or(false));
        let n = app.world.query::<&ZonePortal>().iter(&app.world).count();
        assert!(n >= 1, "Forest 기본 포탈이 스폰됨");
    }

    #[test]
    fn 퀘스트_포탈스폰_시스템은_Named존을_등록하고_보라색_포탈을_스폰한다() {
        use crate::modules::quest::PortalPlacement;
        let mut app = harness(ZoneId::Town, floor_map(40, 30));
        app.add_event::<SpawnQuestPortalEvent>();
        app.add_systems(Update, handle_spawn_quest_portal);

        app.world.send_event(SpawnQuestPortalEvent {
            zone: "herb_glade".to_string(),
            generator: "bsp".to_string(),
            placement: PortalPlacement::InsideRoom,
            quest_id: "q1".to_string(),
        });
        app.update();

        // NamedZoneConfig 에 등록됨
        let cfg = app.world.resource::<NamedZoneConfig>();
        assert_eq!(cfg.zones.get("herb_glade").unwrap().origin, ZoneId::Town);
        // 포탈 엔티티 + 미니맵 마커
        let n = app.world.query::<&ZonePortal>().iter(&app.world).count();
        assert_eq!(n, 1);
        assert!(!app.world.resource::<crate::modules::ui::minimap::DiscoveredMarkers>().0.is_empty());
    }

    #[test]
    fn 퀘스트_포탈스폰_시스템은_이미_등록된_존은_중복생성하지_않는다() {
        use crate::modules::quest::PortalPlacement;
        let mut app = harness(ZoneId::Town, floor_map(40, 30));
        app.world.resource_mut::<NamedZoneConfig>().zones.insert(
            "herb_glade".to_string(),
            NamedZoneEntry { generator: "bsp".to_string(), origin: ZoneId::Town });
        app.add_event::<SpawnQuestPortalEvent>();
        app.add_systems(Update, handle_spawn_quest_portal);

        app.world.send_event(SpawnQuestPortalEvent {
            zone: "herb_glade".to_string(),
            generator: "bsp".to_string(),
            placement: PortalPlacement::InsideRoom,
            quest_id: "q1".to_string(),
        });
        app.update();
        let n = app.world.query::<&ZonePortal>().iter(&app.world).count();
        assert_eq!(n, 0, "이미 등록된 존은 contains_key True 로 건너뛴다");
    }

    #[test]
    fn 퀘스트_포탈스폰_의뢰인근처_배치는_의뢰인_위치_기준으로_포탈을_놓는다() {
        use crate::modules::quest::{PortalPlacement, QuestRegistry, QuestDef};
        let mut app = harness(ZoneId::Town, floor_map(40, 30));
        app.add_event::<SpawnQuestPortalEvent>();

        // 의뢰인 NPC (이름 매칭) + 퀘스트 등록
        let mut reg = QuestRegistry::default();
        reg.quests.insert("q1".to_string(), QuestDef {
            id: "q1".to_string(),
            title: "약초".to_string(),
            giver_npc: "약초상".to_string(),
            initial_phase: "p".to_string(),
            phases: HashMap::new(),
            transitions: vec![],
            spawns: vec![],
            spawn_chance: 1.0,
        });
        app.insert_resource(reg);

        let gpos = tile_to_world_coords(10, 10);
        app.world.spawn((
            Transform::from_xyz(gpos.x, gpos.y, 0.0),
            crate::modules::villager::Villager {
                id: "herbalist".to_string(),
                name: "약초상".to_string(),
                dialogues: vec![],
                dialogue_idx: 0,
                tile_x: 10, tile_y: 10,
                just_bumped: false,
                quest_dialogue_idx: 0,
                base_color: Color::WHITE,
                home_room: None,
                stationary: false,
                vendor: false,
            },
        ));
        app.add_systems(Update, handle_spawn_quest_portal);

        app.world.send_event(SpawnQuestPortalEvent {
            zone: "herb_glade".to_string(),
            generator: "bsp".to_string(),
            placement: PortalPlacement::NearGiver { radius: 3 },
            quest_id: "q1".to_string(),
        });
        app.update();

        let mut q = app.world.query::<(&Transform, &ZonePortal)>();
        let (t, _) = q.iter(&app.world).next().expect("포탈 스폰됨");
        let (px, py) = world_to_tile_coords(t.translation);
        assert!((px as i32 - 10).abs() <= 3 && (py as i32 - 10).abs() <= 3,
            "의뢰인 반경 안: ({}, {})", px, py);
    }

    #[test]
    fn 퀘스트_포탈스폰_의뢰인근처는_경계와_벽과_점유칸을_모두_제외하고_배치한다() {
        // 프로덕션 thread_rng 경로의 floor_tile_near 분기(경계초과·벽·점유)를 모두 탄다.
        use crate::modules::quest::{PortalPlacement, QuestRegistry, QuestDef};
        let mut map = Map::new(40, 30);
        // 좌상단 모서리 근처에 일부만 Floor (벽 섞임)
        map.set_tile(0, 1, TileKind::Floor);
        map.set_tile(1, 0, TileKind::Floor);
        map.set_tile(2, 1, TileKind::Floor);
        map.set_tile(1, 2, TileKind::Floor);
        map.set_tile(2, 2, TileKind::Floor);
        // 방도 하나 둬서 fallback 가능성 확보
        for y in 20..26 { for x in 20..26 { map.set_tile(x, y, TileKind::Floor); } }
        map.rooms.push(Rect::new(20, 20, 5, 5));

        let mut app = harness(ZoneId::Town, map);
        // 일부 칸을 미리 점유 (731:54 False 경로)
        app.world.resource_mut::<UsedSpawnTiles>().0.insert((2, 2));
        app.add_event::<SpawnQuestPortalEvent>();

        let mut reg = QuestRegistry::default();
        reg.quests.insert("q1".to_string(), QuestDef {
            id: "q1".to_string(), title: "t".to_string(),
            giver_npc: "현자".to_string(), initial_phase: "p".to_string(),
            phases: HashMap::new(), transitions: vec![], spawns: vec![], spawn_chance: 1.0,
        });
        app.insert_resource(reg);

        // 의뢰인을 (1,1) 모서리에 둔다 → 반경 2 면 음수 좌표가 후보로 들어와 729 분기를 탄다.
        let gpos = tile_to_world_coords(1, 1);
        app.world.spawn((
            Transform::from_xyz(gpos.x, gpos.y, 0.0),
            crate::modules::villager::Villager {
                id: "sage".to_string(), name: "현자".to_string(),
                dialogues: vec![], dialogue_idx: 0, tile_x: 1, tile_y: 1,
                just_bumped: false, quest_dialogue_idx: 0,
                base_color: Color::WHITE, home_room: None,
                stationary: false, vendor: false,
            },
        ));
        app.add_systems(Update, handle_spawn_quest_portal);
        app.world.send_event(SpawnQuestPortalEvent {
            zone: "corner_zone".to_string(),
            generator: "bsp".to_string(),
            placement: PortalPlacement::NearGiver { radius: 2 },
            quest_id: "q1".to_string(),
        });
        app.update();

        // 포탈이 모서리 근처(반경2) 미점유 Floor 에 배치되었다.
        let mut q = app.world.query::<(&Transform, &ZonePortal)>();
        let (t, _) = q.iter(&app.world).next().expect("포탈 스폰됨");
        let (px, py) = world_to_tile_coords(t.translation);
        assert!((px as i32 - 1).abs() <= 2 && (py as i32 - 1).abs() <= 2,
            "의뢰인 반경 안 배치: ({}, {})", px, py);
        assert_ne!((px, py), (2, 2), "점유 칸은 제외");
    }

    #[test]
    fn 퀘스트_포탈스폰_의뢰인근처는_근처에_자리가_없으면_방_무작위로_대체배치한다() {
        // 프로덕션 thread_rng 경로의 compute_portal_pos NearGiver 대체(or_else) 클로저를 탄다.
        // 의뢰인은 찾았지만 반경 안에 Floor 가 없어 random_floor_tile_anywhere 로 넘어간다.
        use crate::modules::quest::{PortalPlacement, QuestRegistry, QuestDef};
        let mut map = Map::new(40, 30);
        // 의뢰인 주변(10,10)은 전부 Wall, 멀리 떨어진 방에만 Floor.
        for y in 20..26 { for x in 20..26 { map.set_tile(x, y, TileKind::Floor); } }
        map.rooms.push(Rect::new(20, 20, 5, 5));

        let mut app = harness(ZoneId::Town, map);
        app.add_event::<SpawnQuestPortalEvent>();
        let mut reg = QuestRegistry::default();
        reg.quests.insert("q1".to_string(), QuestDef {
            id: "q1".to_string(), title: "t".to_string(),
            giver_npc: "촌장".to_string(), initial_phase: "p".to_string(),
            phases: HashMap::new(), transitions: vec![], spawns: vec![], spawn_chance: 1.0,
        });
        app.insert_resource(reg);
        let gpos = tile_to_world_coords(10, 10);
        app.world.spawn((
            Transform::from_xyz(gpos.x, gpos.y, 0.0),
            crate::modules::villager::Villager {
                id: "chief".to_string(), name: "촌장".to_string(),
                dialogues: vec![], dialogue_idx: 0, tile_x: 10, tile_y: 10,
                just_bumped: false, quest_dialogue_idx: 0,
                base_color: Color::WHITE, home_room: None,
                stationary: false, vendor: false,
            },
        ));
        app.add_systems(Update, handle_spawn_quest_portal);
        app.world.send_event(SpawnQuestPortalEvent {
            zone: "far_zone".to_string(),
            generator: "bsp".to_string(),
            placement: PortalPlacement::NearGiver { radius: 2 },
            quest_id: "q1".to_string(),
        });
        app.update();

        let mut q = app.world.query::<(&Transform, &ZonePortal)>();
        let (t, _) = q.iter(&app.world).next().expect("대체배치 포탈 스폰됨");
        let (px, py) = world_to_tile_coords(t.translation);
        // 의뢰인 반경 밖, 먼 방 안의 Floor 에 배치된다.
        assert!((20..26).contains(&px) && (20..26).contains(&py),
            "먼 방 무작위 배치: ({}, {})", px, py);
    }

    #[test]
    fn 퀘스트_포탈스폰_의뢰인근처는_우하단_경계초과_좌표도_제외하고_배치한다() {
        // 프로덕션 thread_rng 경로에서 nx>=width, ny>=height 분기(729:36/62)를 탄다.
        use crate::modules::quest::{PortalPlacement, QuestRegistry, QuestDef};
        let w = 40; let h = 30;
        let mut map = Map::new(w, h);
        // 우하단 모서리 근처에 Floor 몇 칸
        for y in h - 3..h { for x in w - 3..w { map.set_tile(x, y, TileKind::Floor); } }
        // fallback 용 방
        for y in 5..10 { for x in 5..10 { map.set_tile(x, y, TileKind::Floor); } }
        map.rooms.push(Rect::new(5, 5, 5, 5));

        let mut app = harness(ZoneId::Town, map);
        app.add_event::<SpawnQuestPortalEvent>();

        let mut reg = QuestRegistry::default();
        reg.quests.insert("q1".to_string(), QuestDef {
            id: "q1".to_string(), title: "t".to_string(),
            giver_npc: "수문장".to_string(), initial_phase: "p".to_string(),
            phases: HashMap::new(), transitions: vec![], spawns: vec![], spawn_chance: 1.0,
        });
        app.insert_resource(reg);

        // 의뢰인을 우하단 모서리 (w-1, h-1) 에 둔다 → 반경 2 면 경계초과 좌표가 후보로 들어온다.
        let gpos = tile_to_world_coords(w - 1, h - 1);
        app.world.spawn((
            Transform::from_xyz(gpos.x, gpos.y, 0.0),
            crate::modules::villager::Villager {
                id: "gate".to_string(), name: "수문장".to_string(),
                dialogues: vec![], dialogue_idx: 0, tile_x: w - 1, tile_y: h - 1,
                just_bumped: false, quest_dialogue_idx: 0,
                base_color: Color::WHITE, home_room: None,
                stationary: false, vendor: false,
            },
        ));
        app.add_systems(Update, handle_spawn_quest_portal);
        app.world.send_event(SpawnQuestPortalEvent {
            zone: "edge_zone".to_string(),
            generator: "bsp".to_string(),
            placement: PortalPlacement::NearGiver { radius: 2 },
            quest_id: "q1".to_string(),
        });
        app.update();

        let mut q = app.world.query::<(&Transform, &ZonePortal)>();
        let (t, _) = q.iter(&app.world).next().expect("포탈 스폰됨");
        let (px, py) = world_to_tile_coords(t.translation);
        assert!(px < w && py < h && map_floor_check(&app, px, py),
            "맵 안 Floor 에 배치: ({}, {})", px, py);
        assert!((px as i32 - (w as i32 - 1)).abs() <= 2 && (py as i32 - (h as i32 - 1)).abs() <= 2);
    }

    fn map_floor_check(app: &App, x: usize, y: usize) -> bool {
        app.world.resource::<MapResource>().0.get_tile(x, y).is_walkable()
    }

    #[test]
    fn 퀘스트_포탈스폰은_놓을_자리가_없으면_포탈을_만들지_않는다() {
        use crate::modules::quest::PortalPlacement;
        // 전부 Wall 인 맵 → compute/portal_tile 모두 None → pos None 분기.
        let mut app = harness(ZoneId::Town, Map::new(40, 30));
        app.add_event::<SpawnQuestPortalEvent>();
        app.add_systems(Update, handle_spawn_quest_portal);
        app.world.send_event(SpawnQuestPortalEvent {
            zone: "void".to_string(),
            generator: "bsp".to_string(),
            placement: PortalPlacement::InsideRoom,
            quest_id: "q1".to_string(),
        });
        app.update();
        let n = app.world.query::<&ZonePortal>().iter(&app.world).count();
        assert_eq!(n, 0, "배치 실패 시 포탈 없음");
    }

    #[test]
    fn 퀘스트_포탈정리_시스템은_엔티티_영속화_등록_마커를_모두_제거한다() {
        let mut app = harness(ZoneId::Town, floor_map(40, 30));
        app.add_event::<CloseQuestPortalEvent>();

        let target = ZoneId::Named("herb_glade".to_string());
        // 등록 + 영속화 + 마커 + 활성 포탈 엔티티
        app.world.resource_mut::<NamedZoneConfig>().zones.insert(
            "herb_glade".to_string(),
            NamedZoneEntry { generator: "bsp".to_string(), origin: ZoneId::Town });
        {
            let mut per = app.world.resource_mut::<ZonePersistence>();
            per.0.entry(ZoneId::Town).or_default().portals = vec![
                SavedPortal { tile_x: 5, tile_y: 5, target: target.clone(), arrive_from: PortalDirection::StairDown },
                SavedPortal { tile_x: 1, tile_y: 1, target: ZoneId::Forest, arrive_from: PortalDirection::South },
            ];
        }
        let coord = tile_to_world_coords(5, 5);
        app.world.resource_mut::<crate::modules::ui::minimap::DiscoveredMarkers>()
            .add(5, 5, crate::modules::ui::minimap::MarkerKind::Portal, ZoneId::Town);
        // 닫을 대상 포탈 엔티티 + 닫지 않을 다른 포탈 엔티티
        app.world.spawn((Transform::from_xyz(coord.x, coord.y, 1.5),
            ZonePortal { target: target.clone(), arrive_from: PortalDirection::StairDown }));
        app.world.spawn((Transform::default(),
            ZonePortal { target: ZoneId::Forest, arrive_from: PortalDirection::South }));

        app.add_systems(Update, handle_close_quest_portal);
        app.world.send_event(CloseQuestPortalEvent { zone: "herb_glade".to_string() });
        app.update();

        // 대상 포탈 엔티티만 제거 (다른 포탈은 유지)
        let mut remaining = app.world.query::<&ZonePortal>();
        let kinds: Vec<_> = remaining.iter(&app.world).map(|p| p.target.clone()).collect();
        assert_eq!(kinds, vec![ZoneId::Forest]);
        // 등록 제거
        assert!(!app.world.resource::<NamedZoneConfig>().zones.contains_key("herb_glade"));
        // 영속화에서 대상 포탈 제거 (Forest 포탈은 유지)
        let per = app.world.resource::<ZonePersistence>();
        let town = per.0.get(&ZoneId::Town).unwrap();
        assert_eq!(town.portals.len(), 1);
        assert_eq!(town.portals[0].target, ZoneId::Forest);
        // 마커 제거
        assert!(app.world.resource::<crate::modules::ui::minimap::DiscoveredMarkers>().0.is_empty());
    }

    #[test]
    fn 존복귀_시스템은_경과턴만큼_혈흔을_감소시켜_복원한다() {
        let mut app = harness(ZoneId::Forest, floor_map(40, 30));
        app.world.resource_mut::<GlobalTurn>().0 = 10;
        {
            let mut per = app.world.resource_mut::<ZonePersistence>();
            let snap = per.0.entry(ZoneId::Forest).or_default();
            snap.last_visited_turn = 5; // 5턴 경과
            snap.blood_stains = vec![
                // 5턴 * 0.1 = 0.5 감소 → 0.4 남음 (복원)
                SavedBloodStain { tile_x: 3, tile_y: 3, alpha: 0.9, decay_per_turn: 0.1 },
                // 5턴 * 0.5 = 2.5 → 0 이하 (스킵)
                SavedBloodStain { tile_x: 4, tile_y: 4, alpha: 1.0, decay_per_turn: 0.5 },
            ];
        }
        app.add_systems(Update, restore_zone_state);
        app.world.resource_mut::<MapResource>().set_changed();
        app.update();

        let n = app.world.query::<&BloodStain>().iter(&app.world).count();
        assert_eq!(n, 1, "살아있는 혈흔 하나만 복원");
        let alpha = app.world.query::<&BloodStain>().iter(&app.world).next().unwrap().alpha;
        assert!((alpha - 0.4).abs() < 1e-5, "alpha={}", alpha);
        // drain 으로 스냅샷 혈흔은 비워진다
        assert!(app.world.resource::<ZonePersistence>().0.get(&ZoneId::Forest).unwrap().blood_stains.is_empty());
    }

    #[test]
    fn 존복귀_시스템은_맵이_바뀌지_않았으면_아무것도_하지_않는다() {
        let mut app = harness(ZoneId::Forest, floor_map(40, 30));
        app.add_systems(Update, restore_zone_state);
        // 첫 프레임은 changed 로 간주되므로, 한 번 돌린 뒤 두번째 프레임에서 미변경 경로를 탄다.
        app.update();
        // 두번째 update 에서 변경 없음 → 조기 종료 (스냅샷 없으니 어차피 no-op이지만 분기 실행됨)
        app.update();
        assert_eq!(app.world.query::<&BloodStain>().iter(&app.world).count(), 0);
    }

    #[test]
    fn 존복귀_시스템은_현재존_스냅샷이_없으면_조기_종료한다() {
        let mut app = harness(ZoneId::Forest, floor_map(40, 30));
        // persistence 에 Forest 스냅샷 없음 → get_mut None 경로
        app.add_systems(Update, restore_zone_state);
        app.world.resource_mut::<MapResource>().set_changed();
        app.update();
        assert_eq!(app.world.query::<&BloodStain>().iter(&app.world).count(), 0);
    }

    #[test]
    fn 시야안_포탈탐색_시스템은_보이는_포탈을_방향별_마커로_등록한다() {
        let mut map = floor_map(40, 30);
        // 세 포탈 위치를 visible 로 표시
        for &(x, y) in &[(3usize, 3usize), (5, 5), (7, 7)] {
            let idx = map.index(x, y);
            map.tiles[idx].visible = true;
        }
        let mut app = harness(ZoneId::Forest, map);
        // StairDown / StairUp / 그외(North) → 각각 다른 MarkerKind
        for (dir, (x, y)) in [
            (PortalDirection::StairDown, (3, 3)),
            (PortalDirection::StairUp, (5, 5)),
            (PortalDirection::North, (7, 7)),
        ] {
            let c = tile_to_world_coords(x, y);
            app.world.spawn((Transform::from_xyz(c.x, c.y, 1.5),
                ZonePortal { target: ZoneId::Town, arrive_from: dir }));
        }
        app.add_systems(Update, discover_portals_in_fov);
        app.update();

        let markers = app.world.resource::<crate::modules::ui::minimap::DiscoveredMarkers>();
        use crate::modules::ui::minimap::MarkerKind;
        assert!(markers.0.iter().any(|m| m.kind == MarkerKind::StairDown));
        assert!(markers.0.iter().any(|m| m.kind == MarkerKind::StairUp));
        assert!(markers.0.iter().any(|m| m.kind == MarkerKind::Portal));
    }

    #[test]
    fn 시야안_포탈탐색은_보이지_않거나_범위밖_포탈은_마커로_등록하지_않는다() {
        let map = floor_map(40, 30); // 모든 타일 visible=false
        let mut app = harness(ZoneId::Forest, map);
        // 보이지 않는 포탈
        let c = tile_to_world_coords(3, 3);
        app.world.spawn((Transform::from_xyz(c.x, c.y, 1.5),
            ZonePortal { target: ZoneId::Town, arrive_from: PortalDirection::North }));
        // x 범위 밖 포탈 (tx >= map.width)
        let far_x = tile_to_world_coords(55, 5);
        app.world.spawn((Transform::from_xyz(far_x.x, far_x.y, 1.5),
            ZonePortal { target: ZoneId::Town, arrive_from: PortalDirection::North }));
        app.add_systems(Update, discover_portals_in_fov);
        app.update();
        assert!(app.world.resource::<crate::modules::ui::minimap::DiscoveredMarkers>().0.is_empty());
    }

    #[test]
    fn 시야안_포탈탐색은_x는_맵안이지만_y가_맵밖이면_등록하지_않는다() {
        // tx < width 이고 ty >= height 인 경로 (604:31 True). 좌표 클램프는 [0,79].
        let map = floor_map(40, 30);
        let mut app = harness(ZoneId::Forest, map);
        let c = tile_to_world_coords(5, 35); // x=5(<40), y=35(>=30, <80)
        app.world.spawn((Transform::from_xyz(c.x, c.y, 1.5),
            ZonePortal { target: ZoneId::Town, arrive_from: PortalDirection::North }));
        app.add_systems(Update, discover_portals_in_fov);
        app.update();
        assert!(app.world.resource::<crate::modules::ui::minimap::DiscoveredMarkers>().0.is_empty());
    }
}
