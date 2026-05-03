use bevy::prelude::*;

pub mod generators;

// --- Trait ---

pub trait MapGenerator: Send + Sync {
    fn generate(&self, width: usize, height: usize) -> Map;
    fn name(&self) -> &str;
}

// --- Components ---

#[derive(Component)]
pub struct TileEntity {
    pub x: usize,
    pub y: usize,
}

// --- Enums / Types ---

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum MapTile {
    #[default]
    Wall,
    Floor,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum MapType {
    #[default]
    Dungeon,
    Village,
}

#[derive(Debug, Copy, Clone)]
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

pub struct Map {
    pub width: usize,
    pub height: usize,
    pub tiles: Vec<MapTile>,
    pub rooms: Vec<Rect>,
    pub revealed_tiles: Vec<bool>,
    pub visible_tiles: Vec<bool>,
    pub map_type: MapType,
}

impl Map {
    pub fn new(width: usize, height: usize) -> Self {
        let size = width * height;
        Self {
            width, height,
            tiles: vec![MapTile::Wall; size],
            rooms: Vec::new(),
            revealed_tiles: vec![false; size],
            visible_tiles: vec![false; size],
            map_type: MapType::Dungeon,
        }
    }
    pub fn index(&self, x: usize, y: usize) -> usize { y * self.width + x }
    pub fn set_tile(&mut self, x: usize, y: usize, tile: MapTile) {
        let idx = self.index(x, y);
        self.tiles[idx] = tile;
    }
    pub fn get_tile(&self, x: usize, y: usize) -> MapTile {
        self.tiles[self.index(x, y)]
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
}

// --- Events ---

#[derive(Event)]
pub struct RegenerateMapEvent;

#[derive(Event)]
pub struct PlayerRespawnEvent(pub usize, pub usize);

#[derive(Event)]
pub struct TriggerRespawnEvent(pub Vec<Rect>);

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

/// 몬스터 타일 위치 집합 — PreUpdate에서 동기화, 플레이어 이동 차단에 사용
#[derive(Resource, Default)]
pub struct MonsterTiles(pub std::collections::HashSet<(usize, usize)>);

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
            .init_resource::<OccupiedTiles>()
            .init_resource::<MonsterTiles>()
            .add_event::<RegenerateMapEvent>()
            .add_event::<PlayerRespawnEvent>()
            .add_event::<TriggerRespawnEvent>()
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
                update_tile_visibility.after(MapSystemSet::ExecuteRegen),
            ));
    }
}

// --- Systems ---

fn create_and_store_map(mut commands: Commands, registry: Res<MapGeneratorRegistry>) {
    let map = registry.current()
        .map(|g| g.generate(MAP_WIDTH, MAP_HEIGHT))
        .unwrap_or_else(|| Map::new(MAP_WIDTH, MAP_HEIGHT));
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
                MapTile::Wall => "#",
                MapTile::Floor => ".",
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
    if keyboard_input.just_pressed(KeyCode::Tab) {
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
    mut trigger_respawn: EventWriter<TriggerRespawnEvent>,
    mut villager_respawn: EventWriter<VillagerRespawnEvent>,
    mut monster_respawn: EventWriter<MonsterRespawnEvent>,
) {
    for _ in events.read() {
        for entity in tile_query.iter() {
            commands.entity(entity).despawn();
        }

        let map = registry.current()
            .map(|g| g.generate(MAP_WIDTH, MAP_HEIGHT))
            .unwrap_or_else(|| Map::new(MAP_WIDTH, MAP_HEIGHT));

        let font = asset_server.load("fonts/FiraMono-Medium.ttf");
        for y in 0..map.height {
            for x in 0..map.width {
                let glyph = match map.get_tile(x, y) {
                    MapTile::Wall => "#",
                    MapTile::Floor => ".",
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
        trigger_respawn.send(TriggerRespawnEvent(rooms.clone()));
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

        let target_vis = if in_vp && map.visible_tiles[idx] {
            Visibility::Visible
        } else if in_vp && map.revealed_tiles[idx] {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };

        if *vis != target_vis { *vis = target_vis; }

        if target_vis == Visibility::Visible {
            let new_color = if map.visible_tiles[idx] { Color::WHITE } else { Color::rgb(0.3, 0.3, 0.3) };
            if text.sections[0].style.color != new_color {
                text.sections[0].style.color = new_color;
            }
        }
    }
}

fn find_spawn_point(map: &Map) -> (usize, usize) {
    if let Some(r) = map.rooms.first() {
        return r.center();
    }
    for y in 1..map.height - 1 {
        for x in 1..map.width - 1 {
            if map.get_tile(x, y) == MapTile::Floor {
                return (x, y);
            }
        }
    }
    (map.width / 2, map.height / 2)
}

// --- Constants ---

pub const MAP_WIDTH: usize = 160;
pub const MAP_HEIGHT: usize = 100;
pub const TILE_SIZE: f32 = 16.0;

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
        fn generate(&self, width: usize, height: usize) -> Map { Map::new(width, height) }
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
