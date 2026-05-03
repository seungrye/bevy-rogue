use bevy::prelude::*;
use std::collections::HashMap;
use crate::modules::map::{
    Map, MapResource, MapGeneratorRegistry, ApplyMapEvent,
    MAP_WIDTH, MAP_HEIGHT, MapTile, tile_to_world_coords, TILE_SIZE,
};

// ── ZoneId ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ZoneId {
    Town,
    Forest,
    Dungeon(u32),
}

impl ZoneId {
    pub fn display_name(&self) -> String {
        match self {
            ZoneId::Town      => "마을".into(),
            ZoneId::Forest    => "숲".into(),
            ZoneId::Dungeon(n) => format!("던전 {}층", n),
        }
    }

    pub fn algorithm(&self) -> &'static str {
        match self {
            ZoneId::Town      => "organic_village",
            ZoneId::Forest    => "forest",
            ZoneId::Dungeon(_) => "bsp",
        }
    }
}

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
            .add_event::<ZoneTransitionEvent>()
            .add_systems(Startup, cache_initial_map.after(crate::modules::map::draw_map))
            .add_systems(Update, (
                check_portal_collision,
                handle_zone_transition,
                spawn_portals_after_apply,
            ).chain());
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
    player_q: Query<&Transform, With<crate::modules::player::Player>>,
    portal_q: Query<(&Transform, &ZonePortal)>,
    mut ev: EventWriter<ZoneTransitionEvent>,
    mut triggered: Local<bool>,
    // 이동 이벤트가 발생한 프레임에만 체크
    acted: EventReader<crate::modules::map::PlayerActedEvent>,
) {
    if acted.is_empty() {
        *triggered = false;
        return;
    }
    if *triggered { return; }

    let Ok(pt) = player_q.get_single() else { return };
    let (px, py) = crate::modules::map::world_to_tile_coords(pt.translation);

    for (portal_t, portal) in portal_q.iter() {
        let (tx, ty) = crate::modules::map::world_to_tile_coords(portal_t.translation);
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
) {
    for transition in ev.read() {
        // 현재 맵 캐시에 저장
        world.cache_current(map_res.0.clone());

        let target = transition.target.clone();

        // 캐시된 맵 사용 or 새로 생성
        let map = if let Some(cached) = world.maps.get(&target) {
            cached.clone()
        } else {
            registry.select_by_name(target.algorithm());
            registry.current()
                .map(|g| g.generate(MAP_WIDTH, MAP_HEIGHT))
                .unwrap_or_else(|| Map::new(MAP_WIDTH, MAP_HEIGHT))
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
) {
    if !map_res.is_changed() { return; }
    if !portal_q.is_empty() { return; }  // 이미 스폰됨

    let map = &map_res.0;
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");

    let portals = zone_portals(&world.current);
    for (dir, target) in portals {
        if let Some((px, py)) = portal_tile(map, &dir) {
            let coord = tile_to_world_coords(px, py);
            let glyph = dir.glyph();
            let color = match dir {
                PortalDirection::StairDown => Color::YELLOW,
                PortalDirection::StairUp   => Color::CYAN,
                _                          => Color::rgba(0.5, 1.0, 0.5, 0.7),
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
    }
}

/// PortalDirection 에 따른 포털 타일 위치를 찾는다
fn portal_tile(map: &Map, dir: &PortalDirection) -> Option<(usize, usize)> {
    match dir {
        PortalDirection::North => {
            let cx = map.width / 2;
            for y in 0..map.height {
                if map.get_tile(cx, y) == MapTile::Floor {
                    return Some((cx, y));
                }
            }
            None
        }
        PortalDirection::South => {
            let cx = map.width / 2;
            for y in (0..map.height).rev() {
                if map.get_tile(cx, y) == MapTile::Floor {
                    return Some((cx, y));
                }
            }
            None
        }
        PortalDirection::StairDown | PortalDirection::StairUp => {
            // 마지막 방의 중앙에 배치
            let room = if *dir == PortalDirection::StairDown {
                map.rooms.last()
            } else {
                map.rooms.first()
            };
            room.map(|r| r.center())
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
                if map.get_tile(cx, y) == MapTile::Floor {
                    let y2 = (y + 1).min(map.height - 1);
                    if map.get_tile(cx, y2) == MapTile::Floor { return (cx, y2); }
                    return (cx, y);
                }
            }
            (cx, map.height / 2)
        }
        PortalDirection::South => {
            // 북쪽에서 내려옴 → 맵 북쪽 첫 Floor
            let cx = map.width / 2;
            for y in 0..map.height {
                if map.get_tile(cx, y) == MapTile::Floor {
                    let y2 = y.saturating_sub(1);
                    if map.get_tile(cx, y2) == MapTile::Floor { return (cx, y2); }
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
        for x in 0..20 { map.set_tile(x, 18, crate::modules::map::MapTile::Floor); }
        let (_, y) = arrival_pos(&map, &PortalDirection::North);
        assert!(y >= 10, "남쪽 스폰이어야 함: y={}", y);
    }
}
