use bevy::prelude::*;

pub mod generators;

// --- Trait ---

pub trait MapGenerator: Send + Sync {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map;
    fn name(&self) -> &str;
}

// --- Components ---

#[derive(Component)]
pub struct TileEntity {
    pub x: usize,
    pub y: usize,
}

// --- Enums / Types ---

/// 타일의 종류를 나타내는 열거형.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum TileKind {
    #[default]
    Wall,
    Floor,
}

/// 맵 타일 하나의 전체 상태를 담는 구조체.
/// 기존에 Map에서 별도 Vec<bool>로 관리하던 revealed/visible 상태를
/// 타일 자체에 포함시켜 데이터 응집도를 높였다.
#[derive(Copy, Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct MapTile {
    pub kind: TileKind,
    pub revealed: bool,
    pub visible: bool,
}

impl Default for MapTile {
    fn default() -> Self {
        Self { kind: TileKind::Wall, revealed: false, visible: false }
    }
}

impl MapTile {
    pub fn new(kind: TileKind) -> Self {
        Self { kind, revealed: false, visible: false }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum MapType {
    #[default]
    Dungeon,
    Village,
}

#[derive(Debug, Copy, Clone, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x1: usize,
    pub x2: usize,
    pub y1: usize,
    pub y2: usize,
}

impl Rect {
    pub fn new(x: usize, y: usize, w: usize, h: usize) -> Self {
        Self { x1: x, y1: y, x2: x + w, y2: y + h }
    }
    pub fn width(&self) -> usize { self.x2 - self.x1 }
    pub fn height(&self) -> usize { self.y2 - self.y1 }
    pub fn center(&self) -> (usize, usize) {
        ((self.x1 + self.x2) / 2, (self.y1 + self.y2) / 2)
    }
}

// --- Map ---

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Map {
    pub width: usize,
    pub height: usize,
    pub tiles: Vec<MapTile>,
    pub rooms: Vec<Rect>,
    pub map_type: MapType,
    #[serde(default)]
    pub seed: u64,
    #[serde(default)]
    pub algorithm: String,
}

impl Map {
    pub fn new(width: usize, height: usize) -> Self {
        let size = width * height;
        Self {
            width, height,
            tiles: vec![MapTile::default(); size],
            rooms: Vec::new(),
            map_type: MapType::Dungeon,
            seed: 0,
            algorithm: String::new(),
        }
    }
    pub fn index(&self, x: usize, y: usize) -> usize { y * self.width + x }
    pub fn set_tile(&mut self, x: usize, y: usize, kind: TileKind) {
        let idx = self.index(x, y);
        self.tiles[idx].kind = kind;
    }
    pub fn get_tile(&self, x: usize, y: usize) -> TileKind {
        self.tiles[self.index(x, y)].kind
    }
}

// --- Resources ---

#[derive(Resource)]
pub struct MapResource(pub Map);

impl MapResource {
    pub fn map(&self) -> &Map { &self.0 }
    pub fn map_mut(&mut self) -> &mut Map { &mut self.0 }
}

#[derive(Resource)]
pub struct MapGeneratorRegistry {
    generators: Vec<Box<dyn MapGenerator>>,
    current: usize,
}

impl MapGeneratorRegistry {
    pub fn new() -> Self {
        Self { generators: Vec::new(), current: 0 }
    }
    pub fn register(&mut self, gen: Box<dyn MapGenerator>) {
        self.generators.push(gen);
    }
    pub fn current(&self) -> Option<&dyn MapGenerator> {
        self.generators.get(self.current).map(|g| g.as_ref())
    }
    pub fn next(&mut self) {
        if self.generators.len() > 1 {
            self.current = (self.current + 1) % self.generators.len();
        }
    }
    pub fn select_by_name(&mut self, name: &str) {
        if let Some(idx) = self.generators.iter().position(|g| g.name() == name) {
            self.current = idx;
        }
    }
    pub fn current_name(&self) -> &str {
        self.current().map(|g| g.name()).unwrap_or("없음")
    }
    pub fn generate_with(&self, algo: &str, width: usize, height: usize, seed: u64) -> Map {
        self.generators.iter()
            .find(|g| g.name() == algo)
            .map(|g| g.generate(width, height, seed))
            .unwrap_or_else(|| Map::new(width, height))
    }
}

// --- Events ---

#[derive(Event)]
pub struct RegenerateMapEvent;

#[derive(Event)]
pub struct PlayerRespawnEvent(pub usize, pub usize);

#[derive(Event)]
pub struct VillagerRespawnEvent {
    pub map_type: MapType,
    pub rooms: Vec<Rect>,
}

#[derive(Resource, Default)]
pub struct OccupiedTiles(pub std::collections::HashSet<(usize, usize)>);

/// 플레이어가 이동하거나 주민과 부딪혀 턴을 소비했을 때 발행
#[derive(Event)]
pub struct PlayerActedEvent;

/// 플레이어가 주민이 점유한 타일로 이동을 시도했을 때 발행
#[derive(Event)]
pub struct BumpTileEvent(pub usize, pub usize);

/// 플레이어가 몬스터 타일로 이동을 시도했을 때 발행
#[derive(Event)]
pub struct AttackMonsterEvent(pub usize, pub usize);

/// 맵 재생성 시 몬스터 재스폰 트리거
#[derive(Event)]
pub struct MonsterRespawnEvent {
    pub map_type: MapType,
    pub rooms: Vec<Rect>,
}

/// 존 전환 시: 미리 준비된 맵을 그대로 적용 (재생성 없이 리드로우만)
#[derive(Event)]
pub struct ApplyMapEvent {
    pub map: Map,
    pub spawn_pos: Option<(usize, usize)>,
}

/// 몬스터 타일 위치 집합 — PreUpdate에서 동기화, 플레이어 이동 차단에 사용
#[derive(Resource, Default)]
pub struct MonsterTiles(pub std::collections::HashSet<(usize, usize)>);

/// 현재 맵에서 이미 스폰에 사용된 타일 집합.
/// 맵 교체 시 초기화되며, 아이템·포탈 스폰 시스템이 중복 배치를 피하기 위해 공유한다.
#[derive(Resource, Default)]
pub struct UsedSpawnTiles(pub std::collections::HashSet<(usize, usize)>);

// --- System Set ---

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum MapSystemSet {
    ExecuteRegen,
}

// --- Plugin ---

pub struct MapPlugin {
    pub initial_algorithm: Option<String>,
}

impl Default for MapPlugin {
    fn default() -> Self {
        Self { initial_algorithm: None }
    }
}

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        use generators::*;

        let mut registry = MapGeneratorRegistry::new();
        registry.register(Box::new(bsp::BspGenerator));
        registry.register(Box::new(rooms::SimpleRoomsGenerator));
        registry.register(Box::new(drunkard::DrunkardWalkGenerator));
        registry.register(Box::new(cellular_automata::CellularAutomataGenerator));
        registry.register(Box::new(dla::DlaGenerator));
        registry.register(Box::new(bsp_indoor::BspIndoorGenerator));
        registry.register(Box::new(prefab::PrefabGenerator));
        registry.register(Box::new(organic_village::OrganicVillageGenerator));
        registry.register(Box::new(grid_village::GridVillageGenerator));
        registry.register(Box::new(forest::ForestGenerator));
        registry.register(Box::new(perlin::PerlinNoiseGenerator));

        if let Some(name) = &self.initial_algorithm {
            registry.select_by_name(name);
        }

        app.insert_resource(registry)
            .insert_resource(GlobalSeed(rand::random()))
            .init_resource::<GlobalTurn>()
            .init_resource::<OccupiedTiles>()
            .init_resource::<MonsterTiles>()
            .init_resource::<UsedSpawnTiles>()
            .add_event::<RegenerateMapEvent>()
            .add_event::<ApplyMapEvent>()
            .add_event::<PlayerRespawnEvent>()
            .add_event::<VillagerRespawnEvent>()
            .add_event::<MonsterRespawnEvent>()
            .add_event::<PlayerActedEvent>()
            .add_event::<BumpTileEvent>()
            .add_event::<AttackMonsterEvent>()
            .add_systems(Startup, (
                create_and_store_map,
                draw_map.after(create_and_store_map),
            ))
            .add_systems(Update, (
                cycle_map_generator,
                execute_regen
                    .after(cycle_map_generator)
                    .in_set(MapSystemSet::ExecuteRegen),
                execute_apply
                    .in_set(MapSystemSet::ExecuteRegen),
                update_tile_visibility.after(MapSystemSet::ExecuteRegen),
                increment_global_turn,
            ));
    }
}

// --- Systems ---

fn increment_global_turn(
    mut events: EventReader<PlayerActedEvent>,
    mut turn: ResMut<GlobalTurn>,
) {
    for _ in events.read() { turn.0 += 1; }
}

fn create_and_store_map(
    mut commands: Commands,
    registry: Res<MapGeneratorRegistry>,
    global_seed: Res<GlobalSeed>,
) {
    let seed = zone_seed_from_idx(global_seed.0, 0); // Town = index 0
    let algo = registry.current_name().to_string();
    let mut map = registry.current()
        .map(|g| g.generate(MAP_WIDTH, MAP_HEIGHT, seed))
        .unwrap_or_else(|| Map::new(MAP_WIDTH, MAP_HEIGHT));
    map.algorithm = algo;
    commands.insert_resource(MapResource(map));
}

pub fn draw_map(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    map_res: Res<MapResource>,
) {
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");
    let map = map_res.map();
    for y in 0..map.height {
        for x in 0..map.width {
            let glyph = match map.get_tile(x, y) {
                TileKind::Wall => "#",
                TileKind::Floor => ".",
            };
            let coord = tile_to_world_coords(x, y);
            commands.spawn((
                Text2dBundle {
                    text: Text::from_section(glyph, TextStyle {
                        font: font.clone(),
                        font_size: TILE_SIZE,
                        color: Color::WHITE,
                    }),
                    transform: Transform::from_xyz(coord.x, coord.y, 0.0),
                    ..default()
                },
                TileEntity { x, y },
            ));
        }
    }
}

fn cycle_map_generator(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut registry: ResMut<MapGeneratorRegistry>,
    mut events: EventWriter<RegenerateMapEvent>,
) {
    if keyboard_input.just_pressed(KeyCode::F1) {
        registry.next();
        info!("맵 생성기 전환: {}", registry.current_name());
        events.send(RegenerateMapEvent);
    }
}

fn execute_regen(
    mut commands: Commands,
    mut events: EventReader<RegenerateMapEvent>,
    tile_query: Query<Entity, With<TileEntity>>,
    asset_server: Res<AssetServer>,
    registry: Res<MapGeneratorRegistry>,
    mut player_respawn: EventWriter<PlayerRespawnEvent>,
    mut villager_respawn: EventWriter<VillagerRespawnEvent>,
    mut monster_respawn: EventWriter<MonsterRespawnEvent>,
) {
    for _ in events.read() {
        for entity in tile_query.iter() {
            commands.entity(entity).despawn();
        }

        let seed: u64 = rand::random();
        let algo = registry.current_name().to_string();
        let mut map = registry.current()
            .map(|g| g.generate(MAP_WIDTH, MAP_HEIGHT, seed))
            .unwrap_or_else(|| Map::new(MAP_WIDTH, MAP_HEIGHT));
        map.algorithm = algo;

        let font = asset_server.load("fonts/FiraMono-Medium.ttf");
        for y in 0..map.height {
            for x in 0..map.width {
                let glyph = match map.get_tile(x, y) {
                    TileKind::Wall => "#",
                    TileKind::Floor => ".",
                };
                let coord = tile_to_world_coords(x, y);
                commands.spawn((
                    Text2dBundle {
                        text: Text::from_section(glyph, TextStyle {
                            font: font.clone(),
                            font_size: TILE_SIZE,
                            color: Color::WHITE,
                        }),
                        transform: Transform::from_xyz(coord.x, coord.y, 0.0),
                        ..default()
                    },
                    TileEntity { x, y },
                ));
            }
        }

        let (sx, sy) = find_spawn_point(&map);
        let rooms = map.rooms.clone();
        let map_type = map.map_type;
        commands.insert_resource(MapResource(map));

        player_respawn.send(PlayerRespawnEvent(sx, sy));
        villager_respawn.send(VillagerRespawnEvent { map_type, rooms: rooms.clone() });
        monster_respawn.send(MonsterRespawnEvent { map_type, rooms });
    }
}

/// 카메라 중심 기준으로 타일이 뷰포트 내에 있는지 확인한다 (+ 2타일 여백 포함)
pub(crate) fn tile_in_viewport(tx: i32, ty: i32, cx: i32, cy: i32) -> bool {
    const HALF_W: i32 = 22; // 40타일 / 2 + 2 여백
    const HALF_H: i32 = 15; // 25타일 / 2 + 2 여백
    (tx - cx).abs() <= HALF_W && (ty - cy).abs() <= HALF_H
}

pub fn update_tile_visibility(
    map_res: Res<MapResource>,
    camera_q: Query<&Transform, With<Camera>>,
    mut tile_query: Query<(&TileEntity, &mut Text, &mut Visibility)>,
) {
    if !map_res.is_changed() { return; }

    let (cx, cy) = camera_q.get_single()
        .map(|t| world_to_tile_coords(t.translation))
        .map(|(x, y)| (x as i32, y as i32))
        .unwrap_or((MAP_WIDTH as i32 / 2, MAP_HEIGHT as i32 / 2));

    let map = map_res.map();
    for (tile, mut text, mut vis) in tile_query.iter_mut() {
        let idx = map.index(tile.x, tile.y);
        let in_vp = tile_in_viewport(tile.x as i32, tile.y as i32, cx, cy);

        let target_vis = if in_vp && map.tiles[idx].visible {
            Visibility::Visible
        } else if in_vp && map.tiles[idx].revealed {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };

        if *vis != target_vis { *vis = target_vis; }

        if target_vis == Visibility::Visible {
            let new_color = if map.tiles[idx].visible { Color::WHITE } else { Color::rgb(0.3, 0.3, 0.3) };
            if text.sections[0].style.color != new_color {
                text.sections[0].style.color = new_color;
            }
        }
    }
}

/// 존 전환: 사전 생성된 Map 을 적용하고 관련 리스폰 이벤트를 발행한다
fn execute_apply(
    mut commands: Commands,
    mut events: EventReader<ApplyMapEvent>,
    tile_query: Query<Entity, With<TileEntity>>,
    asset_server: Res<AssetServer>,
    mut player_respawn: EventWriter<PlayerRespawnEvent>,
    mut villager_respawn: EventWriter<VillagerRespawnEvent>,
    mut monster_respawn: EventWriter<MonsterRespawnEvent>,
    mut used_spawn: ResMut<UsedSpawnTiles>,
) {
    for ev in events.read() {
        for entity in tile_query.iter() {
            commands.entity(entity).despawn();
        }

        let map = &ev.map;
        let font = asset_server.load("fonts/FiraMono-Medium.ttf");
        for y in 0..map.height {
            for x in 0..map.width {
                let glyph = match map.get_tile(x, y) {
                    TileKind::Wall  => "#",
                    TileKind::Floor => ".",
                };
                let coord = tile_to_world_coords(x, y);
                commands.spawn((
                    Text2dBundle {
                        text: Text::from_section(glyph, TextStyle {
                            font: font.clone(),
                            font_size: TILE_SIZE,
                            color: Color::WHITE,
                        }),
                        transform: Transform::from_xyz(coord.x, coord.y, 0.0),
                        ..default()
                    },
                    TileEntity { x, y },
                ));
            }
        }

        let (sx, sy) = ev.spawn_pos.unwrap_or_else(|| find_spawn_point(map));

        // 맵 교체마다 스폰 타일 집합 초기화 — 플레이어 위치는 미리 예약
        used_spawn.0.clear();
        used_spawn.0.insert((sx, sy));

        let rooms = map.rooms.clone();
        let map_type = map.map_type;
        commands.insert_resource(MapResource(map.clone()));

        player_respawn.send(PlayerRespawnEvent(sx, sy));
        villager_respawn.send(VillagerRespawnEvent { map_type, rooms: rooms.clone() });
        monster_respawn.send(MonsterRespawnEvent { map_type, rooms });
    }
}

fn find_spawn_point(map: &Map) -> (usize, usize) {
    if let Some(r) = map.rooms.first() {
        return r.center();
    }
    for y in 1..map.height - 1 {
        for x in 1..map.width - 1 {
            if map.get_tile(x, y) == TileKind::Floor {
                return (x, y);
            }
        }
    }
    (map.width / 2, map.height / 2)
}

// --- Constants ---

pub const MAP_WIDTH: usize = 80;
pub const MAP_HEIGHT: usize = 50;
pub const TILE_SIZE: f32 = 16.0;

#[derive(Resource, Default)]
pub struct GlobalTurn(pub u64);

#[derive(Resource, Clone)]
pub struct GlobalSeed(pub u64);

/// global_seed 와 zone 인덱스로부터 해당 존의 맵 시드를 결정론적으로 파생한다.
/// splitmix64 방식 — Rust 버전에 무관하게 안정적.
pub fn zone_seed_from_idx(global_seed: u64, zone_idx: u64) -> u64 {
    let mut x = global_seed.wrapping_add(zone_idx).wrapping_add(0x9e3779b97f4a7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}

// --- Coords ---

pub fn tile_to_world_coords(x: usize, y: usize) -> Vec2 {
    let ow = (MAP_WIDTH as f32 * TILE_SIZE) / 2.0 - TILE_SIZE / 2.0;
    let oh = (MAP_HEIGHT as f32 * TILE_SIZE) / 2.0 - TILE_SIZE / 2.0;
    Vec2::new(x as f32 * TILE_SIZE - ow, y as f32 * TILE_SIZE - oh)
}

pub fn world_to_tile_coords(world_pos: Vec3) -> (usize, usize) {
    let ow = (MAP_WIDTH as f32 * TILE_SIZE) / 2.0 - TILE_SIZE / 2.0;
    let oh = (MAP_HEIGHT as f32 * TILE_SIZE) / 2.0 - TILE_SIZE / 2.0;
    let x = ((world_pos.x + ow + TILE_SIZE / 2.0) / TILE_SIZE).floor() as usize;
    let y = ((world_pos.y + oh + TILE_SIZE / 2.0) / TILE_SIZE).floor() as usize;
    (x.clamp(0, MAP_WIDTH - 1), y.clamp(0, MAP_HEIGHT - 1))
}

/// 방 안의 Floor 타일 중 `used` 에 없는 타일을 무작위로 하나 골라 반환한다.
/// 선택된 타일은 `used` 에 추가되어 이후 호출에서 중복 선택이 방지된다.
pub fn random_floor_tile_in_room(
    room: &Rect,
    map: &Map,
    used: &mut std::collections::HashSet<(usize, usize)>,
    rng: &mut impl rand::Rng,
) -> Option<(usize, usize)> {
    use rand::seq::SliceRandom;
    let mut candidates: Vec<(usize, usize)> = (room.x1..=room.x2)
        .flat_map(|x| (room.y1..=room.y2).map(move |y| (x, y)))
        .filter(|&(x, y)| map.get_tile(x, y) == TileKind::Floor && !used.contains(&(x, y)))
        .collect();
    candidates.shuffle(rng);
    let &(x, y) = candidates.first()?;
    used.insert((x, y));
    Some((x, y))
}

pub fn is_line_of_sight_clear(map: &Map, x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
    let (dx, dy) = ((x1 - x0).abs(), (y1 - y0).abs());
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let mut err = dx - dy;
    let (mut x, mut y) = (x0, y0);
    loop {
        if x < 0 || x >= map.width as i32 || y < 0 || y >= map.height as i32 { return false; }
        if x == x1 && y == y1 { return true; }
        if (x != x0 || y != y0) && map.tiles[map.index(x as usize, y as usize)].kind == TileKind::Wall {
            return false;
        }
        let e2 = 2 * err;
        if e2 > -dy { err -= dy; x += sx; }
        if e2 < dx  { err += dx; y += sy; }
    }
}

#[cfg(test)]
mod tests {
    use super::{MapGenerator, MapGeneratorRegistry, Map, tile_in_viewport};

    #[test]
    fn viewport_contains_camera_center() {
        assert!(tile_in_viewport(10, 10, 10, 10));
    }

    #[test]
    fn viewport_includes_tiles_at_boundary() {
        // HALF_W=22, HALF_H=15
        assert!(tile_in_viewport(22, 0, 0, 0));
        assert!(tile_in_viewport(0, 15, 0, 0));
    }

    #[test]
    fn viewport_excludes_tiles_beyond_boundary() {
        assert!(!tile_in_viewport(23, 0, 0, 0));
        assert!(!tile_in_viewport(0, 16, 0, 0));
    }

    struct NamedGen(&'static str);
    impl MapGenerator for NamedGen {
        fn generate(&self, width: usize, height: usize, _seed: u64) -> Map { Map::new(width, height) }
        fn name(&self) -> &str { self.0 }
    }

    fn registry_with(names: &[&'static str]) -> MapGeneratorRegistry {
        let mut r = MapGeneratorRegistry::new();
        for &n in names { r.register(Box::new(NamedGen(n))); }
        r
    }

    #[test]
    fn empty_registry_current_returns_none() {
        let r = MapGeneratorRegistry::new();
        assert!(r.current().is_none());
    }

    #[test]
    fn single_generator_next_stays_same() {
        let mut r = registry_with(&["A"]);
        r.next();
        assert_eq!(r.current_name(), "A");
    }

    #[test]
    fn next_cycles_through_all() {
        let mut r = registry_with(&["A", "B", "C"]);
        assert_eq!(r.current_name(), "A");
        r.next(); assert_eq!(r.current_name(), "B");
        r.next(); assert_eq!(r.current_name(), "C");
        r.next(); assert_eq!(r.current_name(), "A"); // wrap
    }

    #[test]
    fn select_by_name_picks_correct() {
        let mut r = registry_with(&["A", "B", "C"]);
        r.select_by_name("C");
        assert_eq!(r.current_name(), "C");
    }

    #[test]
    fn select_by_name_unknown_is_noop() {
        let mut r = registry_with(&["A", "B"]);
        r.select_by_name("Z");
        assert_eq!(r.current_name(), "A");
    }
}
