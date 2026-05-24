use bevy::prelude::*;

pub mod generators;

// --- 트레이트 ---

pub trait MapGenerator: Send + Sync {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map;
    fn name(&self) -> &str;
}

// --- 컴포넌트 ---

#[derive(Component)]
pub struct TileEntity {
    pub x: usize,
    pub y: usize,
}

// --- 열거형 / 타입 ---

/// 타일의 종류를 나타내는 열거형.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum TileKind {
    #[default]
    Wall,
    Floor,
    Water,
    Sand,
    /// 파괴 가능한 벽(집/건물 구조물). 이동/시야상 일반 `Wall` 과 동일하지만
    /// 폭발 등으로 부술 수 있다 — 부서지면 `Rubble` 이 된다.
    DestructibleWall,
    /// 부서진 잔해. 통행 가능하고 시야가 통과한다.
    Rubble,
    /// 상점 가판대(카운터). 통행은 불가하지만 시야는 통과한다 — 플레이어가
    /// 카운터 앞에 서서 그 너머의 상인(vendor)을 보고 거래한다.
    Counter,
}

impl TileKind {
    /// 이동 가능 여부. `Floor`/`Sand`/`Rubble` 은 통과, `Wall`/`Water`/`DestructibleWall`/`Counter` 는 막힌다.
    /// 플레이어·몬스터·주민 이동과 경로탐색이 이 술어를 사용한다.
    pub fn is_walkable(self) -> bool {
        matches!(self, TileKind::Floor | TileKind::Sand | TileKind::Rubble)
    }

    /// 시야 차단 여부. `Wall`/`DestructibleWall` 이 시야를 막고, 나머지는 시야가 통과한다.
    /// FOV/시선(LoS) 계산이 이 술어를 사용한다(물·잔해·카운터 너머가 보인다).
    pub fn blocks_sight(self) -> bool {
        matches!(self, TileKind::Wall | TileKind::DestructibleWall)
    }

    /// 파괴 가능 여부. `DestructibleWall` 만 부술 수 있다.
    /// 일반 `Wall`(테두리·자연 암벽)은 파괴 불가다.
    pub fn is_destructible(self) -> bool {
        matches!(self, TileKind::DestructibleWall)
    }

    /// 상호작용 가능 여부. `Counter`(가판대)는 통행 불가지만 향해 이동하면
    /// 범프로 상점이 열린다. 그 외 타일은 상호작용 대상이 아니다.
    pub fn is_interactable(self) -> bool {
        matches!(self, TileKind::Counter)
    }
}

/// `TileKind::is_interactable` 의 자유 함수 형태(호출부 가독성용 thin wrapper).
pub fn is_interactable_tile(kind: TileKind) -> bool {
    kind.is_interactable()
}

/// `(x, y)` 가 `DestructibleWall` 이고 맵 테두리가 아니면 `Rubble` 로 바꾸고 `true`.
/// 그 외(경계·일반 벽·이미 잔해 등)는 보존하고 `false` 를 반환한다.
pub fn destroy_tile(map: &mut Map, x: usize, y: usize) -> bool {
    // 맵 테두리는 파괴 불가 (플레이어가 맵 밖으로 못 나가게 유지).
    if x == 0 || y == 0 || x >= map.width - 1 || y >= map.height - 1 {
        return false;
    }
    if map.get_tile(x, y).is_destructible() {
        map.set_tile(x, y, TileKind::Rubble);
        true
    } else {
        false
    }
}

/// 중심 `(cx, cy)` 에서 반경 `radius` 안(원형, `dist² ≤ radius²`)의 맵 내부 타일
/// 좌표를 모두 모은다. 맵 범위(`[0,w) × [0,h)`)를 벗어나는 좌표는 제외한다.
pub fn tiles_in_radius(
    center: (usize, usize),
    radius: i32,
    w: usize,
    h: usize,
) -> Vec<(usize, usize)> {
    let (cx, cy) = (center.0 as i32, center.1 as i32);
    let mut out = Vec::new();
    if radius < 0 {
        return out;
    }
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy > radius * radius {
                continue;
            }
            let (x, y) = (cx + dx, cy + dy);
            if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                continue;
            }
            out.push((x as usize, y as usize));
        }
    }
    out
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

// --- 맵 ---

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
    /// 마을 상점의 상인(vendor) 고정 위치 — 가판대(Counter) 뒤 바닥 타일.
    /// `Some` 이면 마을 생성기가 한 건물을 상점으로 만들어 여기에 상인을 배치한다.
    /// `#[serde(default)]` 로 기존 세이브(이 필드 없는 데이터)와 호환된다.
    #[serde(default)]
    pub shop_vendor: Option<(usize, usize)>,
}

impl Map {
    pub fn new(width: usize, height: usize) -> Self {
        let size = width * height;
        Self {
            width, height,
            tiles: vec![MapTile::new(TileKind::Wall); size],
            rooms: Vec::new(),
            map_type: MapType::Dungeon,
            seed: 0,
            algorithm: String::new(),
            shop_vendor: None,
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

// --- 리소스 ---

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
    pub fn select_by_name(&mut self, name: &str) -> bool {
        if let Some(idx) = self.generators.iter().position(|g| g.name() == name) {
            self.current = idx;
            true
        } else {
            false
        }
    }
    pub fn current_name(&self) -> &str {
        self.current().map(|g| g.name()).unwrap_or("없음")
    }
    pub fn generate_with(&self, algo: &str, width: usize, height: usize, seed: u64) -> Option<Map> {
        self.generators.iter()
            .find(|g| g.name() == algo)
            .map(|g| g.generate(width, height, seed))
    }
}

// --- 이벤트 ---

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

/// 폭발 요청 이벤트. 퀘스트·전투·마법 등 어떤 소스든 이 이벤트를 발행하면
/// `handle_explosion` 이 지형 파괴와 엔티티 피해를 공통으로 처리한다.
#[derive(Event)]
pub struct ExplosionEvent {
    pub center: (usize, usize),
    pub radius: i32,
    /// 반경 내 파괴 가능 지형을 부술지 여부.
    pub terrain: bool,
    /// 반경 내 몬스터·플레이어에게 줄 피해. 0 이하면 엔티티 피해 없음.
    pub entity_damage: i32,
}

/// 일부 타일의 종류가 런타임에 바뀌었음을 알리는 이벤트. 해당 좌표의 타일
/// 엔티티 글리프/색만 국소적으로 다시 그린다.
#[derive(Event)]
pub struct TilesChangedEvent {
    pub tiles: Vec<(usize, usize)>,
}

/// 몬스터 타일 위치 집합 — PreUpdate에서 동기화, 플레이어 이동 차단에 사용
#[derive(Resource, Default)]
pub struct MonsterTiles(pub std::collections::HashSet<(usize, usize)>);

/// 현재 맵에서 이미 스폰에 사용된 타일 집합.
/// 맵 교체 시 초기화되며, 아이템·포탈 스폰 시스템이 중복 배치를 피하기 위해 공유한다.
#[derive(Resource, Default)]
pub struct UsedSpawnTiles(pub std::collections::HashSet<(usize, usize)>);

// --- 시스템 세트 ---

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum MapSystemSet {
    ExecuteRegen,
}

// --- 플러그인 ---

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
        registry.register(Box::new(maze::MazeGenerator));
        registry.register(Box::new(maze_prim::MazePrimGenerator));
        registry.register(Box::new(recursive_division::RecursiveDivisionGenerator));
        registry.register(Box::new(voronoi_rooms::VoronoiRoomsGenerator));
        registry.register(Box::new(walled_town::WalledTownGenerator));
        registry.register(Box::new(voronoi_districts::VoronoiDistrictsGenerator));
        registry.register(Box::new(island::IslandGenerator));
        registry.register(Box::new(archipelago::ArchipelagoGenerator));
        registry.register(Box::new(coastal::CoastalGenerator));
        registry.register(Box::new(ocean::OceanGenerator));
        registry.register(Box::new(biome_world::BiomeWorldGenerator));
        registry.register(Box::new(wfc::WfcGenerator));

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
            .add_event::<ExplosionEvent>()
            .add_event::<TilesChangedEvent>()
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
                handle_explosion,
                apply_tile_changes.after(handle_explosion),
            ));
    }
}

// --- 시스템 ---

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
    let seed = zone_seed_from_idx(global_seed.0, 0); // 마을 = 인덱스 0
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
            let kind = map.get_tile(x, y);
            let glyph = tile_glyph(kind);
            let coord = tile_to_world_coords(x, y);
            commands.spawn((
                Text2dBundle {
                    text: Text::from_section(glyph, TextStyle {
                        font: font.clone(),
                        font_size: TILE_SIZE,
                        color: tile_base_color(kind),
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
    defeated_q: Query<(), With<crate::modules::combat::Defeated>>,
) {
    if !defeated_q.is_empty() { return; }
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
                let kind = map.get_tile(x, y);
                let glyph = tile_glyph(kind);
                let coord = tile_to_world_coords(x, y);
                commands.spawn((
                    Text2dBundle {
                        text: Text::from_section(glyph, TextStyle {
                            font: font.clone(),
                            font_size: TILE_SIZE,
                            color: tile_base_color(kind),
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

pub fn update_tile_visibility(
    map_res: Res<MapResource>,
    mut tile_query: Query<(&TileEntity, &mut Text, &mut Visibility)>,
) {
    if !map_res.is_changed() { return; }

    let map = map_res.map();
    for (tile, mut text, mut vis) in tile_query.iter_mut() {
        let idx = map.index(tile.x, tile.y);
        let target_vis = if map.tiles[idx].visible || map.tiles[idx].revealed {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };

        if *vis != target_vis { *vis = target_vis; }

        if target_vis == Visibility::Visible {
            let base = tile_base_color(map.tiles[idx].kind);
            // 보이는 타일은 기본 색, 탐험만 된 타일은 동일 색을 0.3 배로 어둡게.
            // Wall/Floor(흰색)는 (1,1,1)→(0.3,0.3,0.3) 으로 기존 동작과 동일.
            let new_color = if map.tiles[idx].visible { base } else { dim_color(base, 0.3) };
            if text.sections[0].style.color != new_color {
                text.sections[0].style.color = new_color;
            }
        }
    }
}

/// 폭발을 처리한다.
/// - `terrain` 이면 반경 내 `destroy_tile` 로 파괴 가능 지형을 부수고, 실제로
///   바뀐 좌표를 모아 `TilesChangedEvent` 로 국소 재렌더를 요청한다.
/// - `entity_damage > 0` 이면 반경 내 몬스터·플레이어의 `CombatStats.hp` 를 깎는다
///   (사망 처리는 기존 cleanup/Defeated 흐름이 그대로 담당).
fn handle_explosion(
    mut events: EventReader<ExplosionEvent>,
    mut map_res: ResMut<MapResource>,
    mut tiles_changed: EventWriter<TilesChangedEvent>,
    mut monster_q: Query<(&crate::modules::monster::Monster, &mut crate::modules::combat::CombatStats), Without<crate::modules::player::Player>>,
    mut player_q: Query<(&Transform, &mut crate::modules::combat::CombatStats), With<crate::modules::player::Player>>,
) {
    for ev in events.read() {
        let (w, h) = (map_res.map().width, map_res.map().height);
        let area = tiles_in_radius(ev.center, ev.radius, w, h);

        if ev.terrain {
            let mut changed: Vec<(usize, usize)> = Vec::new();
            for &(x, y) in &area {
                if destroy_tile(map_res.map_mut(), x, y) {
                    changed.push((x, y));
                }
            }
            if !changed.is_empty() {
                tiles_changed.send(TilesChangedEvent { tiles: changed });
            }
        }

        if ev.entity_damage > 0 {
            let in_area = |tx: usize, ty: usize| area.iter().any(|&(x, y)| x == tx && y == ty);
            // 몬스터 피해 — Monster 가 자기 타일을 들고 있으므로 좌표로 판정.
            for (monster, mut stats) in monster_q.iter_mut() {
                if in_area(monster.tile_x, monster.tile_y) {
                    stats.hp -= ev.entity_damage;
                }
            }
            // 플레이어 피해 — Transform 으로 현재 타일을 계산해 판정.
            for (transform, mut stats) in player_q.iter_mut() {
                let (px, py) = world_to_tile_coords(transform.translation);
                if in_area(px, py) {
                    stats.hp -= ev.entity_damage;
                }
            }
        }
    }
}

/// 일부 타일만 글리프/색을 다시 그린다(국소 재렌더). 전체 맵을 다시 스폰하지
/// 않고 바뀐 좌표의 `TileEntity` 텍스트만 갱신한다.
fn apply_tile_changes(
    mut events: EventReader<TilesChangedEvent>,
    map_res: Res<MapResource>,
    mut tile_query: Query<(&TileEntity, &mut Text)>,
) {
    let map = map_res.map();
    // 한 프레임에 도착한 변경 좌표를 모은다.
    let mut changed: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    for ev in events.read() {
        for &t in &ev.tiles {
            changed.insert(t);
        }
    }
    if changed.is_empty() {
        return;
    }
    for (tile, mut text) in tile_query.iter_mut() {
        if !changed.contains(&(tile.x, tile.y)) {
            continue;
        }
        let kind = map.get_tile(tile.x, tile.y);
        text.sections[0].value = tile_glyph(kind).to_string();
        text.sections[0].style.color = tile_base_color(kind);
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
                let kind = map.get_tile(x, y);
                let glyph = tile_glyph(kind);
                let coord = tile_to_world_coords(x, y);
                commands.spawn((
                    Text2dBundle {
                        text: Text::from_section(glyph, TextStyle {
                            font: font.clone(),
                            font_size: TILE_SIZE,
                            color: tile_base_color(kind),
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
            if map.get_tile(x, y).is_walkable() {
                return (x, y);
            }
        }
    }
    (map.width / 2, map.height / 2)
}

// --- 렌더 헬퍼 ---

/// 타일 종류별 ascii 글리프.
pub fn tile_glyph(kind: TileKind) -> &'static str {
    match kind {
        TileKind::Wall => "#",
        TileKind::Floor => ".",
        TileKind::Water => "~",
        TileKind::Sand => ",",
        TileKind::DestructibleWall => "▒",
        TileKind::Rubble => "%",
        TileKind::Counter => "=",
    }
}

/// 타일 종류별 기본(밝은) 렌더 색.
/// Wall/Floor 는 기존 동작과 동일하게 흰색을 유지하고,
/// Water 는 파랑 계열, Sand 는 모래색을 쓴다.
/// DestructibleWall 은 일반 Wall 과 살짝 구분되는 밝은 회색,
/// Rubble 은 칙칙한 회갈색이다.
pub fn tile_base_color(kind: TileKind) -> Color {
    match kind {
        TileKind::Wall => Color::WHITE,
        TileKind::Floor => Color::WHITE,
        TileKind::Water => Color::rgb(0.25, 0.5, 0.9),
        TileKind::Sand => Color::rgb(0.85, 0.78, 0.5),
        TileKind::DestructibleWall => Color::rgb(0.75, 0.75, 0.78),
        TileKind::Rubble => Color::rgb(0.5, 0.45, 0.4),
        // 카운터는 따뜻한 나무색으로 — 상점임을 시각적으로 구분.
        TileKind::Counter => Color::rgb(0.72, 0.52, 0.28),
    }
}

/// 색을 균일 비율로 어둡게 만든다(탐험만 된 타일 표시용). 알파는 유지.
fn dim_color(c: Color, factor: f32) -> Color {
    Color::rgba(c.r() * factor, c.g() * factor, c.b() * factor, c.a())
}

// --- 상수 ---

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

// --- 좌표 ---

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
/// room 경계가 맵 범위를 벗어나면 자동으로 clamp 한다.
pub fn random_floor_tile_in_room(
    room: &Rect,
    map: &Map,
    used: &mut std::collections::HashSet<(usize, usize)>,
    rng: &mut impl rand::Rng,
) -> Option<(usize, usize)> {
    use rand::seq::SliceRandom;
    let x_max = (room.x2.min(map.width.saturating_sub(1))).max(room.x1);
    let y_max = (room.y2.min(map.height.saturating_sub(1))).max(room.y1);
    let mut candidates: Vec<(usize, usize)> = (room.x1..=x_max)
        .flat_map(|x| (room.y1..=y_max).map(move |y| (x, y)))
        .filter(|&(x, y)| x < map.width && y < map.height
            && map.get_tile(x, y).is_walkable()
            && !used.contains(&(x, y)))
        .collect();
    candidates.shuffle(rng);
    let &(x, y) = candidates.first()?;
    used.insert((x, y));
    Some((x, y))
}

/// rooms 중에서 무작위 room 을 골라 그 안의 Floor 타일을 반환한다.
/// 한 room 에 빈 자리가 없으면 다음 room 시도. 모든 room 실패 시 맵 전체에서
/// 선형 탐색으로 Floor 타일을 찾는다 (견고한 fallback).
///
/// 퀘스트 아이템·몬스터 스폰 등 "어디든 Floor 라면 OK" 인 경우에 사용한다.
pub fn random_floor_tile_anywhere(
    rooms: &[Rect],
    map: &Map,
    used: &mut std::collections::HashSet<(usize, usize)>,
    rng: &mut impl rand::Rng,
) -> Option<(usize, usize)> {
    use rand::seq::SliceRandom;
    // 1) room 무작위 순서로 시도 → 단일 room 집중 방지
    let mut order: Vec<usize> = (0..rooms.len()).collect();
    order.shuffle(rng);
    for idx in order {
        if let Some(p) = random_floor_tile_in_room(&rooms[idx], map, used, rng) {
            return Some(p);
        }
    }
    // 2) 마지막 fallback — 맵 전체에서 Floor 타일 선형 검색
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    for y in 0..map.height {
        for x in 0..map.width {
            if map.get_tile(x, y).is_walkable() && !used.contains(&(x, y)) {
                candidates.push((x, y));
            }
        }
    }
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
        if (x != x0 || y != y0) && map.tiles[map.index(x as usize, y as usize)].kind.blocks_sight() {
            return false;
        }
        let e2 = 2 * err;
        if e2 > -dy { err -= dy; x += sx; }
        if e2 < dx  { err += dx; y += sy; }
    }
}

/// 방향 시야 기본 반경. 정면(facing 쪽)으로는 멀리(`FOV_FRONT`),
/// 등 뒤로는 가깝게(`FOV_BACK`) 본다. 두 값을 분리해 두면 시야 feel 을
/// 데이터처럼 튜닝할 수 있다.
pub const FOV_FRONT: i32 = 8;
pub const FOV_BACK: i32 = 3;

/// `facing` 을 바라보는 주체(`px`,`py`)가 대상 타일(`tx`,`ty`)을 볼 수 있는지
/// 판정하는 순수 함수.
///
/// - `front = dot(t - p, facing) >= 0` 이면 정면(수직 dot==0 도 관대하게 정면).
///   정면이면 반경 `front_r`, 등 뒤면 반경 `back_r` 을 쓴다.
/// - `facing` 이 0 벡터(초기/정지 직후 등)면 방향 개념이 없으므로 전방향
///   원형(항상 `front_r`)으로 폴백한다.
/// - 반경 안(`dist² <= r²`)이고 `is_line_of_sight_clear` 면 가시.
pub fn is_in_view(
    px: i32, py: i32,
    facing: IVec2,
    tx: i32, ty: i32,
    front_r: i32, back_r: i32,
    map: &Map,
) -> bool {
    let dx = tx - px;
    let dy = ty - py;

    // facing 0 벡터면 방향이 없으니 전방향 원형(front_r)으로 폴백.
    let radius = if facing == IVec2::ZERO {
        front_r
    } else if dx * facing.x + dy * facing.y >= 0 {
        front_r // 정면(수직 dot==0 포함)
    } else {
        back_r // 등 뒤
    };

    if dx * dx + dy * dy > radius * radius { return false; }
    is_line_of_sight_clear(map, px, py, tx, ty)
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;
    use bevy::prelude::Color;
    use rand::SeedableRng;

    struct NamedGen(&'static str);
    impl MapGenerator for NamedGen {
        fn generate(&self, width: usize, height: usize, _seed: u64) -> Map { Map::new(width, height) }
        fn name(&self) -> &str { self.0 }
    }

    /// 내부를 모두 바닥으로 채우고 방 하나를 두는 테스트용 생성기.
    struct FloorGen;
    impl MapGenerator for FloorGen {
        fn generate(&self, width: usize, height: usize, _seed: u64) -> Map {
            let mut m = Map::new(width, height);
            for y in 1..height - 1 { for x in 1..width - 1 { m.set_tile(x, y, TileKind::Floor); } }
            m.rooms.push(Rect::new(2, 2, 4, 4));
            m
        }
        fn name(&self) -> &str { "floor" }
    }

    fn registry_with(names: &[&'static str]) -> MapGeneratorRegistry {
        let mut r = MapGeneratorRegistry::new();
        for &n in names { r.register(Box::new(NamedGen(n))); }
        r
    }

    #[test]
    fn 빈_레지스트리는_현재_생성기가_없다() {
        let r = MapGeneratorRegistry::new();
        assert!(r.current().is_none());
    }

    #[test]
    fn 생성기가_하나뿐이면_다음으로_넘겨도_그대로다() {
        let mut r = registry_with(&["A"]);
        r.next();
        assert_eq!(r.current_name(), "A");
    }

    #[test]
    fn 다음_호출은_모든_생성기를_순환하고_마지막에서_처음으로_돌아온다() {
        let mut r = registry_with(&["A", "B", "C"]);
        assert_eq!(r.current_name(), "A");
        r.next(); assert_eq!(r.current_name(), "B");
        r.next(); assert_eq!(r.current_name(), "C");
        r.next(); assert_eq!(r.current_name(), "A"); // 처음으로 순환
    }

    #[test]
    fn 이름으로_선택하면_해당_생성기가_현재가_된다() {
        let mut r = registry_with(&["A", "B", "C"]);
        assert!(r.select_by_name("C"));
        assert_eq!(r.current_name(), "C");
    }

    #[test]
    fn 없는_이름으로_선택하면_아무것도_바뀌지_않는다() {
        let mut r = registry_with(&["A", "B"]);
        assert!(!r.select_by_name("Z"));
        assert_eq!(r.current_name(), "A");
    }

    #[test]
    fn 없는_알고리즘으로_생성요청하면_None을_반환한다() {
        let r = registry_with(&["A"]);
        assert!(r.generate_with("missing", 10, 10, 1).is_none());
    }

    #[test]
    fn 어디든_바닥타일찾기는_바닥타일만_반환한다() {
        // 두 room — 첫 room 은 모두 wall, 두 번째는 모두 floor
        let mut map = Map::new(20, 20);
        // 두 번째 room 영역만 floor 로 변경
        for y in 10..15 { for x in 10..15 { map.set_tile(x, y, TileKind::Floor); } }
        let rooms = vec![
            Rect::new(2, 2, 5, 5),    // 모두 Wall
            Rect::new(10, 10, 5, 5),  // 모두 Floor
        ];
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let (x, y) = random_floor_tile_anywhere(&rooms, &map, &mut used, &mut rng).unwrap();
        // 무조건 두 번째 room 안에서 나와야 함 (첫 room 은 wall 뿐)
        assert!(x >= 10 && x <= 14 && y >= 10 && y <= 14);
        assert_eq!(map.get_tile(x, y), TileKind::Floor);
    }

    #[test]
    fn 어디든_바닥타일찾기는_여러_방에_고르게_분산된다() {
        // 두 room, 모두 충분한 floor — 여러 번 호출 시 두 room 모두 사용됨
        let mut map = Map::new(40, 20);
        for y in 1..19 { for x in 1..39 { map.set_tile(x, y, TileKind::Floor); } }
        let rooms = vec![
            Rect::new(1, 1, 10, 10),    // 좌측
            Rect::new(20, 1, 10, 10),   // 우측
        ];
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let mut left_count = 0;
        let mut right_count = 0;
        for _ in 0..20 {
            let (x, _) = random_floor_tile_anywhere(&rooms, &map, &mut used, &mut rng).unwrap();
            if x < 12 { left_count += 1; } else { right_count += 1; }
        }
        // 두 room 모두 한 번 이상 선택돼야 한다 (단일 room 집중 회피)
        assert!(left_count > 0 && right_count > 0,
            "left={}, right={}: 한쪽으로 집중되면 안 된다", left_count, right_count);
    }

    #[test]
    fn 어디든_바닥타일찾기는_맵범위를_넘는_방을_경계안으로_클램프한다() {
        // room.x2 가 map.width 를 넘어도 영역 밖 좌표를 반환하지 않는다
        let mut map = Map::new(10, 10);
        for y in 0..10 { for x in 0..10 { map.set_tile(x, y, TileKind::Floor); } }
        let rooms = vec![Rect::new(5, 5, 100, 100)];  // 의도적으로 영역 밖 boundary
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        for _ in 0..30 {
            let (x, y) = random_floor_tile_anywhere(&rooms, &map, &mut used, &mut rng).unwrap();
            assert!(x < map.width && y < map.height, "({},{}) 가 영역 밖이면 안 된다", x, y);
        }
    }

    #[test]
    fn 어디든_바닥타일찾기는_바닥이_없으면_None을_반환한다() {
        let map = Map::new(10, 10);  // 전체 wall
        let rooms = vec![Rect::new(1, 1, 8, 8)];
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        assert!(random_floor_tile_anywhere(&rooms, &map, &mut used, &mut rng).is_none());
    }

    #[test]
    fn 어디든_바닥타일찾기는_혼합맵에서도_벽좌표를_반환하지_않는다() {
        // bsp 처럼 room boundary 안에 wall 이 섞인 맵에서도 wall 좌표 안 반환
        let mut map = Map::new(20, 20);
        // 체스판 패턴 — 절반은 floor, 절반은 wall
        for y in 0..20 {
            for x in 0..20 {
                if (x + y) % 2 == 0 {
                    map.set_tile(x, y, TileKind::Floor);
                }
            }
        }
        let rooms = vec![Rect::new(1, 1, 18, 18)];  // 전체 영역
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(99);
        for _ in 0..50 {
            let (x, y) = random_floor_tile_anywhere(&rooms, &map, &mut used, &mut rng).unwrap();
            assert_eq!(map.get_tile(x, y), TileKind::Floor,
                "({},{}) 가 wall 인데 반환됨 — wall 위 spawn 버그", x, y);
        }
    }

    // --- 타일 술어 ---

    #[test]
    fn 바닥과_모래만_이동가능하고_벽과_물은_막힌다() {
        assert!(TileKind::Floor.is_walkable(), "Floor 는 이동 가능해야 한다");
        assert!(TileKind::Sand.is_walkable(), "Sand 는 이동 가능해야 한다");
        assert!(!TileKind::Wall.is_walkable(), "Wall 은 이동 불가여야 한다");
        assert!(!TileKind::Water.is_walkable(), "Water 는 이동 불가여야 한다");
    }

    #[test]
    fn 벽만_시야를_막고_바닥물모래는_시야가_통과한다() {
        assert!(TileKind::Wall.blocks_sight(), "Wall 은 시야를 막아야 한다");
        assert!(!TileKind::Floor.blocks_sight(), "Floor 는 시야가 통과해야 한다");
        assert!(!TileKind::Water.blocks_sight(), "Water 는 시야가 통과해야 한다(물 너머가 보임)");
        assert!(!TileKind::Sand.blocks_sight(), "Sand 는 시야가 통과해야 한다");
    }

    // --- 파괴 가능 지형 술어 ---

    #[test]
    fn 파괴가능벽은_이동상_일반벽처럼_막히고_시야도_막는다() {
        // DestructibleWall 은 이동/시야상 Wall 과 동일해야 한다(파괴만 추가).
        assert!(!TileKind::DestructibleWall.is_walkable(), "파괴가능벽은 이동 불가여야 한다");
        assert!(TileKind::DestructibleWall.blocks_sight(), "파괴가능벽은 시야를 막아야 한다");
    }

    #[test]
    fn 잔해는_통행가능하고_시야가_통과한다() {
        assert!(TileKind::Rubble.is_walkable(), "Rubble 은 통행 가능해야 한다");
        assert!(!TileKind::Rubble.blocks_sight(), "Rubble 은 시야가 통과해야 한다");
    }

    // --- 상점 가판대(Counter) 술어 ---

    #[test]
    fn 카운터는_통행불가지만_시야는_통과한다() {
        // 플레이어가 카운터 앞에 서서 그 너머 상인을 볼 수 있어야 한다.
        assert!(!TileKind::Counter.is_walkable(), "카운터는 통행 불가여야 한다");
        assert!(!TileKind::Counter.blocks_sight(), "카운터 너머가 보여야 한다(시야 통과)");
    }

    #[test]
    fn 카운터는_파괴불가이고_상호작용_가능하다() {
        assert!(!TileKind::Counter.is_destructible(), "카운터는 폭발로 부술 수 없다");
        assert!(TileKind::Counter.is_interactable(), "카운터는 향해 이동하면 상호작용한다");
        assert!(is_interactable_tile(TileKind::Counter), "자유 함수도 동일하게 판정");
    }

    #[test]
    fn 카운터를_제외한_타일은_상호작용_대상이_아니다() {
        for k in [TileKind::Wall, TileKind::Floor, TileKind::Water,
                  TileKind::Sand, TileKind::DestructibleWall, TileKind::Rubble] {
            assert!(!k.is_interactable(), "{:?} 는 상호작용 대상이 아니어야 한다", k);
            assert!(!is_interactable_tile(k));
        }
    }

    #[test]
    fn 카운터는_고유한_글리프와_색을_가진다() {
        assert_eq!(tile_glyph(TileKind::Counter), "=", "카운터 글리프는 '='");
        let c = tile_base_color(TileKind::Counter);
        // 다른 지형색과 구분돼야 한다.
        assert_ne!(c, tile_base_color(TileKind::Floor));
        assert_ne!(c, tile_base_color(TileKind::Wall));
        assert_ne!(c, tile_base_color(TileKind::DestructibleWall));
    }

    #[test]
    fn 새_맵은_상점없음으로_초기화된다() {
        let m = Map::new(10, 10);
        assert!(m.shop_vendor.is_none(), "기본 맵은 상점 vendor 위치가 없어야 한다");
    }

    #[test]
    fn 상점위치는_직렬화_왕복으로_보존된다() {
        let mut m = Map::new(3, 3);
        m.shop_vendor = Some((1, 2));
        let s = ron::ser::to_string(&m).expect("직렬화 성공");
        let parsed: Map = ron::de::from_str(&s).expect("역직렬화 성공");
        assert_eq!(parsed.shop_vendor, Some((1, 2)), "상점 위치가 왕복으로 보존된다");
    }

    #[test]
    fn shop_vendor_필드가_없는_기존_맵_RON도_None으로_파싱된다() {
        // 과거 세이브(이 필드 없는 데이터)는 #[serde(default)] 로 None 이 된다.
        // 필드를 일부러 뺀 RON 을 직접 구성한다.
        let ron_text = "(width:1,height:1,tiles:[(kind:Floor,revealed:false,visible:false)],rooms:[],map_type:Dungeon)";
        let parsed: Map = ron::de::from_str(ron_text).expect("과거 형식 파싱 성공");
        assert!(parsed.shop_vendor.is_none(), "필드 없으면 None 기본값");
    }

    #[test]
    fn 파괴가능벽만_파괴할_수_있고_나머지는_파괴_불가다() {
        assert!(TileKind::DestructibleWall.is_destructible(), "파괴가능벽만 파괴 가능");
        assert!(!TileKind::Wall.is_destructible(), "일반 벽은 파괴 불가");
        assert!(!TileKind::Floor.is_destructible(), "바닥은 파괴 불가");
        assert!(!TileKind::Rubble.is_destructible(), "잔해는 더 이상 파괴 불가");
        assert!(!TileKind::Water.is_destructible(), "물은 파괴 불가");
        assert!(!TileKind::Sand.is_destructible(), "모래는 파괴 불가");
    }

    // --- destroy_tile ---

    #[test]
    fn 파괴가능벽을_부수면_잔해로_바뀌고_참을_반환한다() {
        let mut map = Map::new(10, 10);
        map.set_tile(5, 5, TileKind::DestructibleWall);
        assert!(destroy_tile(&mut map, 5, 5), "파괴 성공 시 true");
        assert_eq!(map.get_tile(5, 5), TileKind::Rubble, "부수면 잔해가 된다");
    }

    #[test]
    fn 일반벽은_부수려_해도_보존되고_거짓을_반환한다() {
        let mut map = Map::new(10, 10);
        map.set_tile(5, 5, TileKind::Wall);
        assert!(!destroy_tile(&mut map, 5, 5), "일반 벽은 파괴 불가 → false");
        assert_eq!(map.get_tile(5, 5), TileKind::Wall, "일반 벽은 그대로 보존");
    }

    #[test]
    fn 맵_테두리의_파괴가능벽은_부수지_못하고_보존된다() {
        // 네 테두리(x=0, x=w-1, y=0, y=h-1)에 파괴가능벽을 둬도 파괴 불가여야 한다.
        let mut map = Map::new(6, 6);
        let edges = [(0, 3), (5, 3), (3, 0), (3, 5)];
        for &(x, y) in &edges {
            map.set_tile(x, y, TileKind::DestructibleWall);
            assert!(!destroy_tile(&mut map, x, y), "테두리 ({},{}) 는 파괴 불가", x, y);
            assert_eq!(map.get_tile(x, y), TileKind::DestructibleWall, "테두리는 보존");
        }
    }

    // --- tiles_in_radius ---

    #[test]
    fn 반경0의_타일목록은_중심한칸만_포함한다() {
        let tiles = tiles_in_radius((5, 5), 0, 10, 10);
        assert_eq!(tiles, vec![(5, 5)], "반경 0 은 중심만");
    }

    #[test]
    fn 반경내_타일은_원형으로_모서리를_제외한다() {
        // 반경 1: 중심 + 상하좌우 4칸(=5칸). 대각선(dist²=2>1)은 제외.
        let tiles = tiles_in_radius((5, 5), 1, 10, 10);
        assert_eq!(tiles.len(), 5, "반경 1 원형은 십자 5칸");
        assert!(tiles.contains(&(5, 5)));
        assert!(tiles.contains(&(4, 5)) && tiles.contains(&(6, 5)));
        assert!(tiles.contains(&(5, 4)) && tiles.contains(&(5, 6)));
        assert!(!tiles.contains(&(4, 4)), "대각(4,4)는 반경 밖이라 제외돼야 한다");
    }

    #[test]
    fn 반경계산은_맵_경계_밖_좌표를_제외한다() {
        // 좌상 모서리(0,0) 중심 반경 2 → 음수 좌표(x<0, y<0)는 제외.
        let lo = tiles_in_radius((0, 0), 2, 4, 4);
        for &(x, y) in &lo {
            assert!(x < 4 && y < 4, "({},{}) 가 맵 범위를 벗어났다", x, y);
        }
        assert!(lo.contains(&(0, 0)));
        assert!(lo.contains(&(2, 0)) && lo.contains(&(0, 2)));

        // 우하 모서리(3,3) 중심 반경 2 → 범위 초과 좌표(x>=w, y>=h)도 제외.
        let hi = tiles_in_radius((3, 3), 2, 4, 4);
        for &(x, y) in &hi {
            assert!(x < 4 && y < 4, "({},{}) 가 맵 범위를 벗어났다(우하)", x, y);
        }
        assert!(hi.contains(&(3, 3)));
        assert!(hi.contains(&(1, 3)) && hi.contains(&(3, 1)));
    }

    #[test]
    fn 음수_반경은_빈_목록을_반환한다() {
        // 방어: 음수 반경은 도달할 수 없는 입력이지만 안전하게 빈 목록.
        assert!(tiles_in_radius((5, 5), -1, 10, 10).is_empty());
    }

    // --- 렌더 글리프/색 매핑 ---

    #[test]
    fn 타일별_글리프는_명세대로_매핑된다() {
        assert_eq!(tile_glyph(TileKind::Wall), "#");
        assert_eq!(tile_glyph(TileKind::Floor), ".");
        assert_eq!(tile_glyph(TileKind::Water), "~");
        assert_eq!(tile_glyph(TileKind::Sand), ",");
        assert_eq!(tile_glyph(TileKind::DestructibleWall), "▒");
        assert_eq!(tile_glyph(TileKind::Rubble), "%");
    }

    #[test]
    fn 파괴가능벽과_잔해는_고유한_색으로_그려진다() {
        let dwall = tile_base_color(TileKind::DestructibleWall);
        let rubble = tile_base_color(TileKind::Rubble);
        // 일반 Wall(흰색)과 살짝 구분돼야 한다.
        assert_ne!(dwall, Color::WHITE, "파괴가능벽은 일반 벽과 색이 구분돼야 한다");
        // 잔해는 칙칙한 회갈색 — 파괴가능벽보다 어둡고 둘은 서로 다르다.
        assert_ne!(dwall, rubble, "파괴가능벽과 잔해 색은 서로 달라야 한다");
        assert!(rubble.r() < dwall.r(), "잔해는 파괴가능벽보다 어두워야 한다");
    }

    #[test]
    fn 벽과_바닥의_기본색은_기존대로_흰색이다() {
        // 지상맵 동작 불변: Wall/Floor 는 흰색을 유지해야 한다.
        assert_eq!(tile_base_color(TileKind::Wall), Color::WHITE);
        assert_eq!(tile_base_color(TileKind::Floor), Color::WHITE);
    }

    #[test]
    fn 물은_파랑계열_모래는_모래색으로_그려진다() {
        let water = tile_base_color(TileKind::Water);
        // 파랑 계열: 파랑 성분이 빨강보다 확실히 크다
        assert!(water.b() > water.r(), "Water 색은 파랑 성분이 우세해야 한다: {:?}", water);
        assert!(water.b() > water.g(), "Water 색은 파랑이 초록보다 커야 한다: {:?}", water);

        let sand = tile_base_color(TileKind::Sand);
        // 모래색: 빨강·초록이 높고 파랑이 낮은 따뜻한 톤
        assert!(sand.r() > sand.b() && sand.g() > sand.b(),
            "Sand 색은 파랑이 가장 낮은 따뜻한 톤이어야 한다: {:?}", sand);
    }

    // --- LoS/FOV ---

    #[test]
    fn 시선은_물타일_너머를_본다() {
        // 한 줄을 모두 Water 로 두면 시선이 끝까지 통과해야 한다.
        let mut map = Map::new(10, 3);
        for x in 0..10 { map.set_tile(x, 1, TileKind::Water); }
        assert!(is_line_of_sight_clear(&map, 0, 1, 9, 1),
            "물 타일은 시야를 막지 않으므로 끝까지 보여야 한다");
    }

    #[test]
    fn 시선은_벽타일에서_막힌다() {
        // 중간에 Wall 이 있으면 그 너머는 보이지 않는다.
        let mut map = Map::new(10, 3);
        for x in 0..10 { map.set_tile(x, 1, TileKind::Floor); }
        map.set_tile(5, 1, TileKind::Wall);
        assert!(!is_line_of_sight_clear(&map, 0, 1, 9, 1),
            "벽이 시선을 가로막으면 너머가 보이면 안 된다");
    }

    #[test]
    fn 시선은_모래타일_너머도_본다() {
        let mut map = Map::new(10, 3);
        for x in 0..10 { map.set_tile(x, 1, TileKind::Sand); }
        assert!(is_line_of_sight_clear(&map, 0, 1, 9, 1),
            "모래 타일은 시야를 막지 않는다");
    }

    #[test]
    fn 시선은_아래에서_위로도_정상_판정한다() {
        // y0 < y1 인 세로 시선 — y 증가 방향(sy=+1) 분기를 탄다.
        let mut map = Map::new(3, 10);
        for y in 0..10 { map.set_tile(1, y, TileKind::Floor); }
        assert!(is_line_of_sight_clear(&map, 1, 0, 1, 9),
            "세로 시선(아래→위)도 통과해야 한다");
    }

    #[test]
    fn 시선이_맵_경계_밖으로_나가면_차단된다() {
        // 끝점이 맵 밖이면 경계 검사 분기에서 false 를 반환한다.
        let mut map = Map::new(5, 5);
        for y in 0..5 { for x in 0..5 { map.set_tile(x, y, TileKind::Floor); } }
        assert!(!is_line_of_sight_clear(&map, 0, 0, 10, 0),
            "오른쪽 맵 밖으로 향하는 시선은 차단돼야 한다(x>=width)");
        assert!(!is_line_of_sight_clear(&map, 0, 0, 0, 10),
            "위쪽 맵 밖으로 향하는 시선도 차단돼야 한다(y>=height)");
    }

    #[test]
    fn 시선의_시작점이_음수좌표면_즉시_차단된다() {
        // 시작점이 (-1,*) 또는 (*,-1) 이면 첫 루프에서 x<0/y<0 경계 분기로 false.
        let mut map = Map::new(5, 5);
        for y in 0..5 { for x in 0..5 { map.set_tile(x, y, TileKind::Floor); } }
        assert!(!is_line_of_sight_clear(&map, -1, 0, 3, 0),
            "시작 x 가 음수면 차단돼야 한다(x<0)");
        assert!(!is_line_of_sight_clear(&map, 0, -1, 0, 3),
            "시작 y 가 음수면 차단돼야 한다(y<0)");
    }

    // --- is_in_view (방향 시야 순수 함수) ---

    /// 모서리 한 칸만 Wall 인 충분히 큰 빈 맵.
    fn open(w: usize, h: usize) -> Map {
        let mut m = Map::new(w, h);
        for y in 0..h { for x in 0..w { m.set_tile(x, y, TileKind::Floor); } }
        m
    }

    #[test]
    fn 방향시야는_정면_반경_안의_타일을_본다() {
        let map = open(40, 40);
        // 오른쪽(+x)을 보는 주체(20,20). 정면 7칸은 front_r(8) 이내.
        assert!(is_in_view(20, 20, IVec2::new(1, 0), 27, 20, 8, 3, &map),
            "정면 반경 안의 타일은 보여야 한다");
    }

    #[test]
    fn 방향시야는_정면이라도_반경을_넘으면_보지_못한다() {
        let map = open(40, 40);
        // 정면 9칸은 front_r(8) 초과.
        assert!(!is_in_view(20, 20, IVec2::new(1, 0), 29, 20, 8, 3, &map),
            "정면이라도 front_r 를 넘으면 보이지 않아야 한다");
    }

    #[test]
    fn 방향시야는_등_뒤_먼_타일을_보지_못한다() {
        let map = open(40, 40);
        // 오른쪽을 보는데 왼쪽(-x) 5칸은 back_r(3) 초과.
        assert!(!is_in_view(20, 20, IVec2::new(1, 0), 15, 20, 8, 3, &map),
            "등 뒤로 back_r 를 넘는 타일은 보이지 않아야 한다");
    }

    #[test]
    fn 방향시야는_등_뒤라도_back_r_이내면_본다() {
        let map = open(40, 40);
        // 왼쪽(-x) 3칸은 back_r(3) 이내.
        assert!(is_in_view(20, 20, IVec2::new(1, 0), 17, 20, 8, 3, &map),
            "등 뒤라도 back_r 이내면 보여야 한다");
    }

    #[test]
    fn 방향시야는_dot이_0인_측면은_정면으로_간주한다() {
        // 오른쪽(+x)을 볼 때 위(+y) 측면은 dot==0 → 정면(front_r) 적용.
        let map = open(40, 40);
        // 위로 5칸: front_r(8) 이내라 보임. back_r(3) 라면 안 보일 거리.
        assert!(is_in_view(20, 20, IVec2::new(1, 0), 20, 25, 8, 3, &map),
            "수직(dot==0) 측면은 정면으로 간주해 front_r 를 써야 한다");
    }

    #[test]
    fn 방향시야는_정면_경계_거리에서_정확히_보인다() {
        // 정확히 front_r(8) 거리 — dist²(64) <= r²(64) 경계 포함.
        let map = open(40, 40);
        assert!(is_in_view(20, 20, IVec2::new(1, 0), 28, 20, 8, 3, &map),
            "정확히 front_r 거리는 경계 포함이라 보여야 한다");
    }

    #[test]
    fn 방향시야는_등_뒤_경계_거리에서_정확히_보인다() {
        // 등 뒤 정확히 back_r(3) 거리 — 경계 포함.
        let map = open(40, 40);
        assert!(is_in_view(20, 20, IVec2::new(1, 0), 17, 20, 8, 3, &map),
            "정확히 back_r 거리는 경계 포함이라 보여야 한다");
    }

    #[test]
    fn 방향시야는_벽이_가로막으면_정면_반경_안이어도_못_본다() {
        let mut map = open(40, 40);
        for y in 0..40 { map.set_tile(23, y, TileKind::Wall); } // 정면을 막는 벽 열
        assert!(!is_in_view(20, 20, IVec2::new(1, 0), 27, 20, 8, 3, &map),
            "정면 반경 안이라도 LoS 가 막히면 보이지 않아야 한다");
    }

    #[test]
    fn 방향시야는_facing이_0이면_전방향_원형으로_폴백한다() {
        let map = open(40, 40);
        // facing 0 → 모든 방향이 front_r(8) 원형. 왼쪽 7칸도 보인다.
        assert!(is_in_view(20, 20, IVec2::ZERO, 13, 20, 8, 3, &map),
            "facing 0 이면 왼쪽도 front_r 원형으로 보여야 한다");
        assert!(is_in_view(20, 20, IVec2::ZERO, 27, 20, 8, 3, &map),
            "facing 0 이면 오른쪽도 front_r 원형으로 보여야 한다");
        // 원형 반경 밖(9칸)은 방향과 무관하게 안 보인다.
        assert!(!is_in_view(20, 20, IVec2::ZERO, 11, 20, 8, 3, &map),
            "facing 0 이라도 front_r 원형 반경 밖은 보이지 않아야 한다");
    }

    #[test]
    fn 방향시야는_네_방향_facing에서_각자_정면을_본다() {
        let map = open(40, 40);
        let cases = [
            (IVec2::new(1, 0), 26, 20),   // 오른쪽
            (IVec2::new(-1, 0), 14, 20),  // 왼쪽
            (IVec2::new(0, 1), 20, 26),   // 위
            (IVec2::new(0, -1), 20, 14),  // 아래
        ];
        for (f, tx, ty) in cases {
            assert!(is_in_view(20, 20, f, tx, ty, 8, 3, &map),
                "facing {:?} 의 정면 타일({},{})은 보여야 한다", f, tx, ty);
            // 정반대 방향 같은 거리(6칸)는 등 뒤라 back_r(3) 초과 → 안 보임
            let bx = 20 - (tx - 20);
            let by = 20 - (ty - 20);
            assert!(!is_in_view(20, 20, f, bx, by, 8, 3, &map),
                "facing {:?} 의 등 뒤 타일({},{})은 보이지 않아야 한다", f, bx, by);
        }
    }

    #[test]
    fn 방향시야는_여덟_방향_대각_facing에서도_정면을_본다() {
        let map = open(40, 40);
        // 대각 facing 4종. 정면 대각 타일은 보이고, 정반대 대각은 등 뒤라 멀면 안 보임.
        let diags = [
            IVec2::new(1, 1), IVec2::new(-1, 1),
            IVec2::new(1, -1), IVec2::new(-1, -1),
        ];
        for f in diags {
            // 정면 대각 4칸 (dist²=32 <= 64)
            let (tx, ty) = (20 + f.x * 4, 20 + f.y * 4);
            assert!(is_in_view(20, 20, f, tx, ty, 8, 3, &map),
                "대각 facing {:?} 의 정면 타일은 보여야 한다", f);
            // 정반대 대각 4칸 — dot<0 등 뒤, dist²=32 > back_r²(9) → 안 보임
            let (bx, by) = (20 - f.x * 4, 20 - f.y * 4);
            assert!(!is_in_view(20, 20, f, bx, by, 8, 3, &map),
                "대각 facing {:?} 의 등 뒤 먼 타일은 보이지 않아야 한다", f);
        }
    }

    // --- 좌표 변환 ---

    #[test]
    fn 타일과_월드_좌표_변환은_왕복해도_같은_타일이다() {
        for &(x, y) in &[(0usize, 0usize), (40, 25), (MAP_WIDTH - 1, MAP_HEIGHT - 1)] {
            let world = tile_to_world_coords(x, y);
            let (rx, ry) = world_to_tile_coords(world.extend(0.0));
            assert_eq!((rx, ry), (x, y), "({},{}) 왕복 변환이 어긋났다", x, y);
        }
    }

    #[test]
    fn 월드좌표가_범위를_벗어나면_타일_경계로_클램프된다() {
        // 아주 큰 음수/양수 좌표는 [0, MAP_*) 로 클램프된다(양쪽 clamp 분기).
        let lo = world_to_tile_coords(Vec3::new(-1_000_000.0, -1_000_000.0, 0.0));
        assert_eq!(lo, (0, 0), "큰 음수는 (0,0) 으로 클램프돼야 한다");
        let hi = world_to_tile_coords(Vec3::new(1_000_000.0, 1_000_000.0, 0.0));
        assert_eq!(hi, (MAP_WIDTH - 1, MAP_HEIGHT - 1),
            "큰 양수는 우하단 모서리로 클램프돼야 한다");
    }

    // --- 존 시드 파생 ---

    #[test]
    fn 존시드는_같은_입력에_대해_결정론적이고_존마다_다르다() {
        let a = zone_seed_from_idx(12345, 0);
        let b = zone_seed_from_idx(12345, 0);
        assert_eq!(a, b, "같은 입력은 같은 시드를 낸다");
        let c = zone_seed_from_idx(12345, 1);
        assert_ne!(a, c, "다른 존 인덱스는 다른 시드를 낸다");
    }

    // --- 방 사각형 헬퍼 ---

    #[test]
    fn 방사각형은_너비높이중심을_정확히_계산한다() {
        let r = Rect::new(10, 20, 6, 4);
        assert_eq!(r.width(), 6);
        assert_eq!(r.height(), 4);
        assert_eq!(r.center(), (13, 22));
    }

    #[test]
    fn 맵타일_기본값은_숨겨진_벽이다() {
        let t = MapTile::default();
        assert_eq!(t.kind, TileKind::Wall);
        assert!(!t.revealed);
        assert!(!t.visible);
    }

    #[test]
    fn 맵타일_생성자는_지정한_종류로_숨겨진_타일을_만든다() {
        let t = MapTile::new(TileKind::Floor);
        assert_eq!(t.kind, TileKind::Floor);
        assert!(!t.revealed && !t.visible);
    }

    #[test]
    fn 맵리소스는_가변_불변_접근자를_제공한다() {
        let mut res = MapResource(Map::new(5, 5));
        res.map_mut().set_tile(2, 2, TileKind::Floor);
        assert_eq!(res.map().get_tile(2, 2), TileKind::Floor);
    }

    // --- 한 방 안 무작위 바닥 ---

    #[test]
    fn 한_방안_무작위_바닥찾기는_바닥만_반환하고_중복을_피한다() {
        let mut map = Map::new(20, 20);
        for y in 5..10 { for x in 5..10 { map.set_tile(x, y, TileKind::Floor); } }
        let room = Rect::new(5, 5, 5, 5);
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..10 {
            if let Some((x, y)) = random_floor_tile_in_room(&room, &map, &mut used, &mut rng) {
                assert_eq!(map.get_tile(x, y), TileKind::Floor);
                assert!(seen.insert((x, y)), "({},{}) 가 중복 반환됐다", x, y);
            }
        }
    }

    #[test]
    fn 한_방안_무작위_바닥찾기는_방의_시작좌표가_맵밖이면_None을_반환한다() {
        // room.x1 이 map.width 이상이면 후보 좌표가 맵 범위(x<width) 검사를 통과 못 한다.
        let mut map = Map::new(10, 10);
        for y in 0..10 { for x in 0..10 { map.set_tile(x, y, TileKind::Floor); } }
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(5);
        // x 가 맵 밖(x<width 거짓), 그리고 x 는 안이고 y 만 맵 밖(y<height 거짓)인 두 경우.
        let x_outside = Rect::new(20, 2, 3, 3);
        let y_outside = Rect::new(2, 20, 3, 3);
        assert!(random_floor_tile_in_room(&x_outside, &map, &mut used, &mut rng).is_none(),
            "방 x 가 맵 밖이면 None");
        assert!(random_floor_tile_in_room(&y_outside, &map, &mut used, &mut rng).is_none(),
            "방 y 가 맵 밖이면 None");
    }

    #[test]
    fn 무작위_바닥찾기는_threadrng로도_동일하게_동작한다() {
        // 프로덕션 스폰(quest)은 thread_rng 를 쓴다. 그 단형화(monomorphization)의
        // 분기들(맵밖 거부, fallback 경로)을 함께 실행한다.
        let mut map = Map::new(10, 10);
        for y in 0..10 { for x in 0..10 { map.set_tile(x, y, TileKind::Floor); } }
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::thread_rng();
        // 방이 맵 밖 → x<width / y<height 거짓 분기.
        assert!(random_floor_tile_in_room(&Rect::new(20, 2, 3, 3), &map, &mut used, &mut rng).is_none());
        assert!(random_floor_tile_in_room(&Rect::new(2, 20, 3, 3), &map, &mut used, &mut rng).is_none());
        // 방 안에서 정상 반환도 한 번.
        assert!(random_floor_tile_in_room(&Rect::new(2, 2, 3, 3), &map, &mut used, &mut rng).is_some());

        // anywhere fallback: 방은 전부 맵 밖(실패) → 맵 전체 fallback 에서 바닥을 찾는다.
        // 바닥 두 칸 중 하나는 미리 used 에 넣어 fallback 의 `!used.contains` 양쪽을 탄다.
        let mut small = Map::new(10, 10);
        small.set_tile(7, 7, TileKind::Floor);
        small.set_tile(8, 8, TileKind::Floor);
        let mut used2 = std::collections::HashSet::new();
        used2.insert((7, 7));
        let bad_rooms = vec![Rect::new(50, 50, 3, 3)];
        let found = random_floor_tile_anywhere(&bad_rooms, &small, &mut used2, &mut rng);
        assert_eq!(found, Some((8, 8)), "사용된 (7,7) 은 건너뛰고 (8,8) 을 반환");
        // 전부 벽인 맵에서 fallback 도 실패하는 경로(645 의 walkable False 쪽).
        let wall_map = Map::new(10, 10);
        let mut used3 = std::collections::HashSet::new();
        assert!(random_floor_tile_anywhere(&bad_rooms, &wall_map, &mut used3, &mut rng).is_none());
    }

    // --- 어디든 바닥찾기 fallback 경로 ---

    #[test]
    fn 어디든_바닥타일찾기는_모든_방이_벽이면_맵전체_fallback으로_바닥을_찾는다() {
        // 방 영역은 전부 벽, 방 밖에만 바닥 한 칸 → 1)방 시도 전부 실패 후 2)전체 스캔.
        let mut map = Map::new(20, 20);
        map.set_tile(18, 18, TileKind::Floor); // 방 밖 바닥
        let rooms = vec![Rect::new(2, 2, 3, 3), Rect::new(8, 8, 3, 3)]; // 모두 벽
        let mut used = std::collections::HashSet::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(11);
        let p = random_floor_tile_anywhere(&rooms, &map, &mut used, &mut rng);
        assert_eq!(p, Some((18, 18)), "fallback 으로 방 밖 바닥을 찾아야 한다");
    }

    #[test]
    fn 어디든_바닥타일찾기_fallback은_이미_사용된_바닥은_건너뛴다() {
        // 방 밖 바닥 두 칸 중 하나를 미리 used 에 넣어 둔다 → fallback 스캔에서
        // 그 칸은 `!used.contains` 거짓으로 제외되고 나머지 한 칸만 반환된다.
        let mut map = Map::new(20, 20);
        map.set_tile(17, 17, TileKind::Floor);
        map.set_tile(18, 18, TileKind::Floor);
        let rooms = vec![Rect::new(2, 2, 3, 3)]; // 전부 벽
        let mut used = std::collections::HashSet::new();
        used.insert((17, 17)); // 이미 사용됨
        let mut rng = rand::rngs::StdRng::seed_from_u64(2);
        let p = random_floor_tile_anywhere(&rooms, &map, &mut used, &mut rng);
        assert_eq!(p, Some((18, 18)), "사용된 (17,17) 은 건너뛰고 (18,18) 을 반환");
    }

    // --- 스폰 지점 찾기 ---

    #[test]
    fn 스폰지점은_방이_있으면_첫_방의_중심이다() {
        let mut map = Map::new(20, 20);
        map.rooms.push(Rect::new(4, 4, 6, 6));
        assert_eq!(find_spawn_point(&map), (7, 7), "첫 방 중심이어야 한다");
    }

    #[test]
    fn 스폰지점은_방이_없으면_첫_통과타일을_스캔한다() {
        let mut map = Map::new(20, 20);
        map.set_tile(5, 3, TileKind::Floor);
        // 방 없음 → 내부를 스캔해 첫 통과타일을 반환.
        assert_eq!(find_spawn_point(&map), (5, 3));
    }

    #[test]
    fn 스폰지점은_방도_통과타일도_없으면_맵_중앙으로_폴백한다() {
        let map = Map::new(20, 20); // 전부 벽, 방 없음
        assert_eq!(find_spawn_point(&map), (10, 10), "중앙으로 폴백해야 한다");
    }

    // --- 색 어둡게(dim) ---

    #[test]
    fn 색어둡게는_RGB를_비율로_줄이고_알파는_유지한다() {
        let c = Color::rgba(1.0, 0.8, 0.4, 0.6);
        let d = dim_color(c, 0.5);
        assert!((d.r() - 0.5).abs() < 1e-6);
        assert!((d.g() - 0.4).abs() < 1e-6);
        assert!((d.b() - 0.2).abs() < 1e-6);
        assert!((d.a() - 0.6).abs() < 1e-6, "알파는 유지돼야 한다");
    }

    // --- App 하네스: 시스템 ---

    /// AssetServer(폰트) 를 제공하는 기본 App.
    fn asset_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app
    }

    /// rooms[0] 의 중앙이 스폰점이 되도록 작은 바닥 맵을 만든다.
    fn floor_map(w: usize, h: usize) -> Map {
        let mut m = Map::new(w, h);
        for y in 1..h - 1 { for x in 1..w - 1 { m.set_tile(x, y, TileKind::Floor); } }
        m.rooms.push(Rect::new(2, 2, 4, 4));
        m
    }

    #[test]
    fn 맵그리기_시스템은_타일마다_텍스트_엔티티를_스폰한다() {
        let mut app = asset_app();
        let w = 6; let h = 5;
        app.insert_resource(MapResource(floor_map(w, h)));
        app.add_systems(Update, draw_map);
        app.update();
        let count = app.world.query::<&TileEntity>().iter(&app.world).count();
        assert_eq!(count, w * h, "모든 타일에 엔티티가 하나씩 스폰돼야 한다");
    }

    #[test]
    fn 전역턴_증가_시스템은_행동_이벤트만큼_턴을_올린다() {
        let mut app = App::new();
        app.add_event::<PlayerActedEvent>();
        app.init_resource::<GlobalTurn>();
        app.add_systems(Update, increment_global_turn);
        app.world.send_event(PlayerActedEvent);
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.resource::<GlobalTurn>().0, 2);
        // 이벤트가 없으면 그대로 유지.
        app.update();
        assert_eq!(app.world.resource::<GlobalTurn>().0, 2);
    }

    #[test]
    fn 맵저장_시스템은_현재_생성기로_맵리소스를_삽입한다() {
        let mut app = App::new();
        let mut registry = MapGeneratorRegistry::new();
        registry.register(Box::new(NamedGen("bsp")));
        app.insert_resource(registry);
        app.insert_resource(GlobalSeed(42));
        app.add_systems(Startup, create_and_store_map);
        app.update();
        let res = app.world.resource::<MapResource>();
        assert_eq!(res.map().algorithm, "bsp", "알고리즘 이름이 기록돼야 한다");
        assert_eq!(res.map().width, MAP_WIDTH);
    }

    #[test]
    fn 맵저장_시스템은_빈_레지스트리면_기본_빈맵을_삽입한다() {
        let mut app = App::new();
        app.insert_resource(MapGeneratorRegistry::new());
        app.insert_resource(GlobalSeed(1));
        app.add_systems(Startup, create_and_store_map);
        app.update();
        let res = app.world.resource::<MapResource>();
        assert_eq!(res.map().width, MAP_WIDTH);
        // 빈 레지스트리 → unwrap_or_else 의 Map::new 경로.
        assert!(res.map().rooms.is_empty());
    }

    #[test]
    fn 생성기전환_시스템은_F1을_누르면_다음_생성기로_바꾸고_재생성을_발행한다() {
        let mut app = App::new();
        let mut registry = MapGeneratorRegistry::new();
        registry.register(Box::new(NamedGen("a")));
        registry.register(Box::new(NamedGen("b")));
        app.insert_resource(registry);
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app.add_event::<RegenerateMapEvent>();
        app.add_systems(Update, cycle_map_generator);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::F1);
        app.update();
        assert_eq!(app.world.resource::<MapGeneratorRegistry>().current_name(), "b");
        let events = app.world.resource::<Events<RegenerateMapEvent>>();
        assert_eq!(events.len(), 1, "재생성 이벤트가 한 번 발행돼야 한다");
    }

    #[test]
    fn 생성기전환_시스템은_F1을_안누르면_아무것도_안한다() {
        let mut app = App::new();
        let mut registry = MapGeneratorRegistry::new();
        registry.register(Box::new(NamedGen("a")));
        registry.register(Box::new(NamedGen("b")));
        app.insert_resource(registry);
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app.add_event::<RegenerateMapEvent>();
        app.add_systems(Update, cycle_map_generator);
        app.update();
        assert_eq!(app.world.resource::<MapGeneratorRegistry>().current_name(), "a");
        assert_eq!(app.world.resource::<Events<RegenerateMapEvent>>().len(), 0);
    }

    #[test]
    fn 생성기전환_시스템은_플레이어가_패배상태면_입력을_무시한다() {
        let mut app = App::new();
        let mut registry = MapGeneratorRegistry::new();
        registry.register(Box::new(NamedGen("a")));
        registry.register(Box::new(NamedGen("b")));
        app.insert_resource(registry);
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app.add_event::<RegenerateMapEvent>();
        app.add_systems(Update, cycle_map_generator);
        app.world.spawn(crate::modules::combat::Defeated);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::F1);
        app.update();
        // 패배 중이면 전환 안 함.
        assert_eq!(app.world.resource::<MapGeneratorRegistry>().current_name(), "a");
        assert_eq!(app.world.resource::<Events<RegenerateMapEvent>>().len(), 0);
    }

    /// 재생성/적용 시스템이 발행하는 리스폰 이벤트를 받는 App 을 만든다.
    fn regen_app() -> App {
        let mut app = asset_app();
        app.add_event::<RegenerateMapEvent>();
        app.add_event::<ApplyMapEvent>();
        app.add_event::<PlayerRespawnEvent>();
        app.add_event::<VillagerRespawnEvent>();
        app.add_event::<MonsterRespawnEvent>();
        app.init_resource::<UsedSpawnTiles>();
        app
    }

    #[test]
    fn 재생성_시스템은_기존타일을_지우고_새맵을_그린_뒤_리스폰을_발행한다() {
        let mut app = regen_app();
        let mut registry = MapGeneratorRegistry::new();
        registry.register(Box::new(FloorGen));
        app.insert_resource(registry);
        // 기존 타일 엔티티 하나 — despawn 되어야 한다.
        let old = app.world.spawn(TileEntity { x: 0, y: 0 }).id();
        app.add_systems(Update, execute_regen);
        app.world.send_event(RegenerateMapEvent);
        app.update();
        assert!(app.world.get_entity(old).is_none(), "기존 타일은 despawn 돼야 한다");
        let count = app.world.query::<&TileEntity>().iter(&app.world).count();
        assert_eq!(count, MAP_WIDTH * MAP_HEIGHT, "새 맵 전체가 그려져야 한다");
        assert_eq!(app.world.resource::<Events<PlayerRespawnEvent>>().len(), 1);
        assert_eq!(app.world.resource::<Events<MonsterRespawnEvent>>().len(), 1);
        assert!(app.world.contains_resource::<MapResource>());
    }

    #[test]
    fn 재생성_시스템은_빈_레지스트리면_기본_빈맵을_그린다() {
        let mut app = regen_app();
        app.insert_resource(MapGeneratorRegistry::new());
        app.add_systems(Update, execute_regen);
        app.world.send_event(RegenerateMapEvent);
        app.update();
        // 빈 레지스트리 → Map::new fallback. 전부 벽이지만 타일은 그려진다.
        let count = app.world.query::<&TileEntity>().iter(&app.world).count();
        assert_eq!(count, MAP_WIDTH * MAP_HEIGHT);
    }

    #[test]
    fn 적용_시스템은_준비된_맵을_그리고_스폰타일을_예약하며_리스폰을_발행한다() {
        let mut app = regen_app();
        let old = app.world.spawn(TileEntity { x: 1, y: 1 }).id();
        app.add_systems(Update, execute_apply);
        let map = floor_map(8, 6);
        app.world.send_event(ApplyMapEvent { map, spawn_pos: Some((3, 3)) });
        app.update();
        assert!(app.world.get_entity(old).is_none(), "기존 타일 despawn");
        let count = app.world.query::<&TileEntity>().iter(&app.world).count();
        assert_eq!(count, 8 * 6);
        // 스폰 타일이 예약됐는지 확인.
        assert!(app.world.resource::<UsedSpawnTiles>().0.contains(&(3, 3)));
        assert_eq!(app.world.resource::<Events<PlayerRespawnEvent>>().len(), 1);
        assert_eq!(app.world.resource::<Events<VillagerRespawnEvent>>().len(), 1);
    }

    #[test]
    fn 적용_시스템은_스폰위치가_없으면_맵에서_스폰지점을_찾는다() {
        let mut app = regen_app();
        app.add_systems(Update, execute_apply);
        let map = floor_map(8, 6); // rooms[0] = Rect::new(2,2,4,4) → center (4,4)
        app.world.send_event(ApplyMapEvent { map, spawn_pos: None });
        app.update();
        // spawn_pos None → find_spawn_point → 첫 방 중심 (4,4) 예약.
        assert!(app.world.resource::<UsedSpawnTiles>().0.contains(&(4, 4)),
            "spawn_pos 가 없으면 맵 스폰지점이 예약돼야 한다");
    }

    /// 타일 엔티티를 흰색 Text + Inherited Visibility 로 직접 스폰한다.
    /// (draw_map 을 거치지 않아 중복 스폰/AssetServer 없이 가시성 시스템만 검증)
    fn spawn_tile_entities(app: &mut App, map: &Map) {
        for y in 0..map.height {
            for x in 0..map.width {
                let kind = map.get_tile(x, y);
                app.world.spawn((
                    Text::from_section(tile_glyph(kind), TextStyle {
                        font: default(),
                        font_size: TILE_SIZE,
                        color: tile_base_color(kind),
                    }),
                    Visibility::default(),
                    TileEntity { x, y },
                ));
            }
        }
    }

    #[test]
    fn 타일가시성_시스템은_보임_탐험_숨김_상태를_타일에_반영한다() {
        let mut app = App::new();
        let mut map = Map::new(3, 1);
        // 0: visible, 1: revealed only, 2: 아무것도 아님.
        map.set_tile(0, 0, TileKind::Floor); map.tiles[0].visible = true;
        map.set_tile(1, 0, TileKind::Floor); map.tiles[1].revealed = true;
        map.set_tile(2, 0, TileKind::Floor);
        spawn_tile_entities(&mut app, &map);
        app.insert_resource(MapResource(map));
        app.add_systems(Update, update_tile_visibility);
        app.update(); // MapResource 가 막 삽입됨 → is_changed → 처리

        // 좌표별 Visibility 확인.
        let mut vis_by_x = std::collections::HashMap::new();
        let mut q = app.world.query::<(&TileEntity, &Visibility)>();
        for (t, v) in q.iter(&app.world) {
            vis_by_x.insert(t.x, *v);
        }
        assert_eq!(vis_by_x[&0], Visibility::Visible, "보이는 타일");
        assert_eq!(vis_by_x[&1], Visibility::Visible, "탐험만 된 타일도 표시(어둡게)");
        assert_eq!(vis_by_x[&2], Visibility::Hidden, "미탐험 타일은 숨김");

        // 탐험만 된 타일은 어둡게(기본색의 0.3 배), 보이는 타일은 기본색.
        let mut color_by_x = std::collections::HashMap::new();
        let mut q2 = app.world.query::<(&TileEntity, &Text)>();
        for (t, txt) in q2.iter(&app.world) {
            color_by_x.insert(t.x, txt.sections[0].style.color);
        }
        assert_eq!(color_by_x[&0], Color::WHITE, "보이는 바닥은 흰색");
        assert_eq!(color_by_x[&1], dim_color(Color::WHITE, 0.3), "탐험만 된 바닥은 어둡게");

        // 리소스를 다시 변경시켜 시스템을 한 번 더 돌린다. 이번엔 Visibility/색이
        // 이미 목표값과 같으므로 변경-없음(`!=` 거짓) 분기를 탄다.
        app.world.resource_mut::<MapResource>().set_changed();
        app.update();
        let mut q3 = app.world.query::<(&TileEntity, &Visibility)>();
        let mut vis2 = std::collections::HashMap::new();
        for (t, v) in q3.iter(&app.world) { vis2.insert(t.x, *v); }
        assert_eq!(vis2[&0], Visibility::Visible, "재실행 후에도 가시성 유지");
        assert_eq!(vis2[&2], Visibility::Hidden, "재실행 후에도 숨김 유지");
    }

    #[test]
    fn 타일가시성_시스템은_맵이_안바뀌었으면_조기_종료한다() {
        let mut app = App::new();
        let mut map = Map::new(2, 1);
        map.set_tile(0, 0, TileKind::Floor); map.tiles[0].visible = true;
        spawn_tile_entities(&mut app, &map);
        app.insert_resource(MapResource(map));
        app.add_systems(Update, update_tile_visibility);
        app.update(); // 첫 실행 — is_changed
        app.update(); // 변경 없음 → 조기 종료
        // 조기 종료해도 패닉/변형 없이 타일 엔티티는 유지된다.
        let count = app.world.query::<&TileEntity>().iter(&app.world).count();
        assert_eq!(count, 2);
    }

    // --- 플러그인 빌드 ---

    #[test]
    fn 맵플러그인_기본값은_초기_알고리즘이_없다() {
        let p = MapPlugin::default();
        assert!(p.initial_algorithm.is_none());
    }

    #[test]
    fn 맵플러그인을_추가하면_레지스트리와_시스템이_등록된다() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_plugins(MapPlugin { initial_algorithm: Some("bsp".into()) });
        // build() 가 레지스트리를 삽입하고 시작 알고리즘을 선택했는지 확인.
        let reg = app.world.resource::<MapGeneratorRegistry>();
        assert_eq!(reg.current_name(), "bsp");
        // 핵심 리소스/이벤트가 등록됐는지.
        assert!(app.world.contains_resource::<GlobalTurn>());
        assert!(app.world.contains_resource::<OccupiedTiles>());
        assert!(app.world.contains_resource::<UsedSpawnTiles>());
    }

    #[test]
    fn 맵플러그인은_초기알고리즘이_없으면_기본_생성기를_쓴다() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        // initial_algorithm: None → select_by_name 을 호출하지 않는 분기.
        app.add_plugins(MapPlugin::default());
        let reg = app.world.resource::<MapGeneratorRegistry>();
        // 첫 등록 생성기(bsp)가 그대로 현재 생성기다.
        assert_eq!(reg.current_name(), "bsp");
    }

    // --- 시스템 세트 트레이트(파생) ---

    #[test]
    fn 맵시스템세트는_복제와_동등성을_지원한다() {
        let a = MapSystemSet::ExecuteRegen;
        let b = a.clone();
        assert_eq!(a, b);
        // 스케줄에 set 을 사용해 dyn SystemSet 경로(dyn_hash/as_dyn_eq/dyn_clone)를 탄다.
        let mut app = App::new();
        app.configure_sets(Update, MapSystemSet::ExecuteRegen);
        app.add_systems(Update, increment_global_turn.in_set(MapSystemSet::ExecuteRegen));
        app.add_event::<PlayerActedEvent>();
        app.init_resource::<GlobalTurn>();
        app.update();
    }

    // --- 폭발 (handle_explosion) ---

    use crate::modules::combat::CombatStats;
    use crate::modules::monster::Monster;
    use crate::modules::player::Player;

    /// 내부가 모두 파괴가능벽인 맵(테두리는 일반 Wall)을 만든다.
    fn dwall_map(w: usize, h: usize) -> Map {
        let mut m = Map::new(w, h);
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                m.set_tile(x, y, TileKind::DestructibleWall);
            }
        }
        m
    }

    fn monster_at(x: usize, y: usize, hp: i32) -> (Monster, CombatStats) {
        (
            Monster { name: "고블린".into(), tile_x: x, tile_y: y, vision_radius: 5, alert_turns: 0, slot_idx: 0 },
            CombatStats { hp, max_hp: hp, mp: 0, max_mp: 0, attack: 1, defense: 0 },
        )
    }

    fn explosion_app(map: Map) -> App {
        let mut app = App::new();
        app.insert_resource(MapResource(map));
        app.add_event::<ExplosionEvent>();
        app.add_event::<TilesChangedEvent>();
        app.add_systems(Update, handle_explosion);
        app
    }

    #[test]
    fn 폭발은_반경내_파괴가능지형을_부수고_타일변경_이벤트를_발행한다() {
        let mut app = explosion_app(dwall_map(10, 10));
        app.world.send_event(ExplosionEvent { center: (5, 5), radius: 1, terrain: true, entity_damage: 0 });
        app.update();
        // 반경 1 십자 5칸이 잔해가 됐는지.
        let map = app.world.resource::<MapResource>().map().clone();
        for &(x, y) in &[(5, 5), (4, 5), (6, 5), (5, 4), (5, 6)] {
            assert_eq!(map.get_tile(x, y), TileKind::Rubble, "({},{}) 가 잔해가 돼야 한다", x, y);
        }
        // 대각은 반경 밖이라 그대로 파괴가능벽.
        assert_eq!(map.get_tile(4, 4), TileKind::DestructibleWall, "대각은 부서지지 않아야 한다");
        // TilesChangedEvent 가 발행됐고 좌표 5칸을 담는다.
        let events = app.world.resource::<Events<TilesChangedEvent>>();
        let mut cursor = events.get_reader();
        let all: Vec<&TilesChangedEvent> = cursor.read(events).collect();
        assert_eq!(all.len(), 1, "변경 이벤트 한 번");
        assert_eq!(all[0].tiles.len(), 5, "바뀐 좌표 5칸");
    }

    #[test]
    fn 폭발은_부술_지형이_없으면_타일변경_이벤트를_발행하지_않는다() {
        // 전부 일반 Wall(파괴 불가) → 아무것도 안 바뀜 → 이벤트 없음.
        let mut app = explosion_app(Map::new(10, 10));
        app.world.send_event(ExplosionEvent { center: (5, 5), radius: 2, terrain: true, entity_damage: 0 });
        app.update();
        assert_eq!(app.world.resource::<Events<TilesChangedEvent>>().len(), 0,
            "부순 타일이 없으면 변경 이벤트도 없어야 한다");
    }

    #[test]
    fn 폭발은_terrain이_거짓이면_지형을_부수지_않는다() {
        let mut app = explosion_app(dwall_map(10, 10));
        app.world.send_event(ExplosionEvent { center: (5, 5), radius: 2, terrain: false, entity_damage: 0 });
        app.update();
        assert_eq!(app.world.resource::<MapResource>().map().get_tile(5, 5), TileKind::DestructibleWall,
            "terrain=false 면 지형이 그대로여야 한다");
        assert_eq!(app.world.resource::<Events<TilesChangedEvent>>().len(), 0);
    }

    #[test]
    fn 폭발은_반경내_몬스터에게_피해를_준다() {
        let mut app = explosion_app(Map::new(20, 20));
        let near = app.world.spawn(monster_at(5, 5, 10)).id();
        let far = app.world.spawn(monster_at(15, 15, 10)).id();
        app.world.send_event(ExplosionEvent { center: (5, 5), radius: 2, terrain: false, entity_damage: 4 });
        app.update();
        assert_eq!(app.world.get::<CombatStats>(near).unwrap().hp, 6, "반경 내 몬스터는 피해");
        assert_eq!(app.world.get::<CombatStats>(far).unwrap().hp, 10, "반경 밖 몬스터는 무사");
    }

    #[test]
    fn 폭발은_반경내_플레이어에게는_피해를_주고_반경밖은_무사하다() {
        let mut app = explosion_app(Map::new(20, 20));
        let near = app.world.spawn((
            Player,
            Transform::from_translation(tile_to_world_coords(8, 8).extend(0.0)),
            CombatStats { hp: 30, max_hp: 30, mp: 0, max_mp: 0, attack: 5, defense: 1 },
        )).id();
        // 반경 밖(멀리 떨어진) 플레이어 — in_area 의 False 분기.
        let far = app.world.spawn((
            Player,
            Transform::from_translation(tile_to_world_coords(18, 18).extend(0.0)),
            CombatStats { hp: 30, max_hp: 30, mp: 0, max_mp: 0, attack: 5, defense: 1 },
        )).id();
        app.world.send_event(ExplosionEvent { center: (8, 8), radius: 1, terrain: false, entity_damage: 7 });
        app.update();
        assert_eq!(app.world.get::<CombatStats>(near).unwrap().hp, 23, "반경 내 플레이어는 피해");
        assert_eq!(app.world.get::<CombatStats>(far).unwrap().hp, 30, "반경 밖 플레이어는 무사");
    }

    #[test]
    fn 폭발은_엔티티피해가_0이면_아무도_다치지_않는다() {
        let mut app = explosion_app(Map::new(20, 20));
        let m = app.world.spawn(monster_at(5, 5, 10)).id();
        app.world.send_event(ExplosionEvent { center: (5, 5), radius: 3, terrain: false, entity_damage: 0 });
        app.update();
        assert_eq!(app.world.get::<CombatStats>(m).unwrap().hp, 10, "피해 0 이면 hp 변화 없음");
    }

    // --- 국소 재렌더 (apply_tile_changes) ---

    #[test]
    fn 타일변경_시스템은_바뀐_타일의_글리프와_색만_갱신한다() {
        let mut app = App::new();
        let mut map = Map::new(4, 1);
        // (1,0) 을 잔해로 바꿔 두고, 타일 엔티티는 옛 글리프(파괴가능벽)로 스폰.
        map.set_tile(1, 0, TileKind::Rubble);
        app.world.spawn((
            Text::from_section(tile_glyph(TileKind::DestructibleWall), TextStyle::default()),
            TileEntity { x: 1, y: 0 },
        ));
        // 안 바뀐 타일도 하나 — 그대로 유지돼야 한다.
        let other = app.world.spawn((
            Text::from_section("#", TextStyle::default()),
            TileEntity { x: 2, y: 0 },
        )).id();
        app.insert_resource(MapResource(map));
        app.add_event::<TilesChangedEvent>();
        app.add_systems(Update, apply_tile_changes);
        app.world.send_event(TilesChangedEvent { tiles: vec![(1, 0)] });
        app.update();

        // (1,0) 타일은 Rubble 글리프/색으로 갱신.
        let mut q = app.world.query::<(&TileEntity, &Text)>();
        let mut by_x = std::collections::HashMap::new();
        for (t, txt) in q.iter(&app.world) {
            by_x.insert(t.x, (txt.sections[0].value.clone(), txt.sections[0].style.color));
        }
        assert_eq!(by_x[&1].0, tile_glyph(TileKind::Rubble), "바뀐 타일 글리프 갱신");
        assert_eq!(by_x[&1].1, tile_base_color(TileKind::Rubble), "바뀐 타일 색 갱신");
        // 변경 목록에 없는 (2,0)은 손대지 않음.
        assert_eq!(by_x[&2].0, "#", "변경 안 된 타일은 글리프 유지");
        let _ = other;
    }

    #[test]
    fn 타일변경_시스템은_변경이_없으면_조기_종료한다() {
        let mut app = App::new();
        app.insert_resource(MapResource(Map::new(3, 1)));
        app.world.spawn((
            Text::from_section("#", TextStyle::default()),
            TileEntity { x: 0, y: 0 },
        ));
        app.add_event::<TilesChangedEvent>();
        app.add_systems(Update, apply_tile_changes);
        app.update(); // 이벤트 없음 → changed 비어 조기 종료.
        let mut q = app.world.query::<&Text>();
        let txt = q.iter(&app.world).next().unwrap();
        assert_eq!(txt.sections[0].value, "#", "변경 이벤트 없으면 글리프 유지");
    }
}
