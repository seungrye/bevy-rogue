use bevy::prelude::*;

pub mod bsp;
pub mod rooms;
pub mod drunkard;

// --- Components ---

/// 맵 타일 엔티티를 식별하고 좌표를 저장하는 컴포넌트입니다.
#[derive(Component)]
pub struct TileEntity {
    /// 맵의 가로 인덱스 (0 .. MAP_WIDTH-1)
    pub x: usize,
    /// 맵의 세로 인덱 (0 .. MAP_HEIGHT-1)
    pub y: usize,
}

// --- Enum Types ---

/// 맵의 각 타일 종류를 정의하는 열거형입니다.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum MapTile {
    /// 벽 (이동 불가능, 시야 차단)
    Wall,
    /// 바닥 (이동 가능, 시야 투과)
    Floor,
}

/// 사용할 맵 생성 알고리즘을 선택하기 위한 열거형 리소스입니다.
#[derive(Clone, Copy, Default, Debug, Resource)]
pub enum MapAlgorithm {
    /// Binary Space Partitioning (구조적인 방 배치)
    #[default]
    Bsp,
    /// Simple Rooms (무작위 방 배치 및 복도 연결)
    SimpleRooms,
    /// Drunkard's Walk (유기적이고 동굴 같은 구조)
    DrunkardWalk,
}

// --- Data Structures ---

/// 사각형 영역을 정의하는 구조체로, 방 생성 시 사용됩니다.
#[derive(Debug, Copy, Clone)]
pub struct Rect {
    /// 왼쪽 위 x 좌표
    pub x1: usize,
    /// 오른쪽 아래 x 좌표
    pub x2: usize,
    /// 왼쪽 위 y 좌표
    pub y1: usize,
    /// 오른쪽 아래 y 좌표
    pub y2: usize,
}

impl Rect {
    /// 새로운 Rect를 생성합니다.
    ///
    /// # Arguments
    /// * `x` - 시작 x 좌표
    /// * `y` - 시작 y 좌표
    /// * `w` - 너비
    /// * `h` - 높이
    ///
    /// # Returns
    /// 생성된 Rect 인스턴스
    pub fn new(x: usize, y: usize, w: usize, h: usize) -> Self {
        Self { x1: x, y1: y, x2: x + w, y2: y + h }
    }

    /// 영역의 너비를 반환합니다.
    pub fn width(&self) -> usize { self.x2 - self.x1 }

    /// 영역의 높이를 반환합니다.
    pub fn height(&self) -> usize { self.y2 - self.y1 }

    /// 영역의 중앙 좌표를 반환합니다.
    ///
    /// # Returns
    /// (x, y) 형태의 튜플
    pub fn center(&self) -> (usize, usize) {
        ((self.x1 + self.x2) / 2, (self.y1 + self.y2) / 2)
    }
}

/// 맵의 전체 데이터(타일, 방 목록, 가시성 등)를 관리하는 구조체입니다.
pub struct Map {
    /// 전체 너비 (타일 수)
    pub width: usize,
    /// 전체 높이 (타일 수)
    pub height: usize,
    /// 타일 배열 (1차원 벡터)
    pub tiles: Vec<MapTile>,
    /// 생성된 방들의 목록
    pub rooms: Vec<Rect>,
    /// 플레이어에게 밝혀진 타일들 (기억된 안개)
    pub revealed_tiles: Vec<bool>,
    /// 현재 플레이어 시야에 들어오는 타일들
    pub visible_tiles: Vec<bool>,
}

impl Map {
    /// 지정된 크기의 새로운 맵을 생성하며, 모든 타일을 벽으로 초기화합니다.
    ///
    /// # Arguments
    /// * `width` - 맵 너비
    /// * `height` - 맵 높이
    ///
    /// # Returns
    /// 초기화된 Map 인스턴스
    pub fn new(width: usize, height: usize) -> Self {
        let size = width * height;
        Self {
            width,
            height,
            tiles: vec![MapTile::Wall; size],
            rooms: Vec::new(),
            revealed_tiles: vec![false; size],
            visible_tiles: vec![false; size],
        }
    }

    /// (x, y) 좌표를 1차원 배열 인덱스로 변환합니다.
    pub fn index(&self, x: usize, y: usize) -> usize { y * self.width + x }

    /// 특정 좌표의 타일 종류를 설정합니다.
    pub fn set_tile(&mut self, x: usize, y: usize, tile: MapTile) {
        let idx = self.index(x, y);
        self.tiles[idx] = tile;
    }

    /// 특정 좌표의 타일 종류를 가져옵니다.
    pub fn get_tile(&self, x: usize, y: usize) -> MapTile {
        self.tiles[self.index(x, y)]
    }
}

// --- Resources ---

/// Map 구조체를 Bevy 리소스로 래핑한 구조체입니다.
#[derive(Resource)]
pub struct MapResource(pub Map);

impl MapResource {
    /// 맵 데이터에 대한 참조를 반환합니다.
    pub fn map(&self) -> &Map { &self.0 }
    /// 맵 데이터에 대한 가변 참조를 반환합니다.
    pub fn map_mut(&mut self) -> &mut Map { &mut self.0 }
}

// --- Plugin ---

/// 맵 생성 및 렌더링 시스템을 관리하는 플러그인입니다.
#[derive(Default)]
pub struct MapPlugin {
    /// 게임 시작 시 사용할 알고리즘입니다.
    pub algorithm: MapAlgorithm,
}

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.algorithm);
        app.add_systems(Startup, (
            create_and_store_map,
            draw_map.after(create_and_store_map)
        ))
        .add_systems(Update, update_tile_visibility);
    }
}

// --- Systems ---

/// 설정된 알고리즘을 사용하여 맵을 생성하고 MapResource로 저장합니다.
fn create_and_store_map(mut commands: Commands, algorithm: Res<MapAlgorithm>) {
    let map = match *algorithm {
        MapAlgorithm::Bsp => bsp::generate_bsp_map(MAP_WIDTH, MAP_HEIGHT),
        MapAlgorithm::SimpleRooms => rooms::generate_rooms_map(MAP_WIDTH, MAP_HEIGHT),
        MapAlgorithm::DrunkardWalk => drunkard::generate_drunkard_map(MAP_WIDTH, MAP_HEIGHT),
    };
    commands.insert_resource(MapResource(map));
}

/// 맵 데이터를 기반으로 시각적인 타일 엔티티(@, #, . 등)를 화면에 스폰합니다.
pub fn draw_map(mut commands: Commands, asset_server: Res<AssetServer>, map_res: Res<MapResource>) {
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");
    let map = map_res.map();
    for y in 0..map.height {
        for x in 0..map.width {
            let tile = map.get_tile(x, y);
            let glyph = match tile {
                MapTile::Wall => "#",
                MapTile::Floor => ".",
            };
            let coord = tile_to_world_coords(x, y);
            commands.spawn((
                Text2dBundle {
                    text: Text::from_section(
                        glyph,
                        TextStyle {
                            font: font.clone(),
                            font_size: TILE_SIZE,
                            color: Color::WHITE,
                        },
                    ),
                    transform: Transform::from_xyz(coord.x, coord.y, 0.0),
                    ..default()
                },
                TileEntity { x, y },
            ));
        }
    }
}

/// 플레이어 시야(FOV)와 공개(Revealed) 상태에 따라 각 타일 엔티티의 가시성과 색상을 업데이트합니다.
pub fn update_tile_visibility(map_res: Res<MapResource>, mut tile_query: Query<(&TileEntity, &mut Text, &mut Visibility)>) {
    let map = map_res.map();
    for (tile_entity, mut text, mut visibility) in tile_query.iter_mut() {
        let idx = map.index(tile_entity.x, tile_entity.y);
        let is_visible = map.visible_tiles[idx];
        let is_revealed = map.revealed_tiles[idx];
        if is_visible {
            text.sections[0].style.color = Color::WHITE;
            *visibility = Visibility::Visible;
        } else if is_revealed {
            text.sections[0].style.color = Color::rgb(0.3, 0.3, 0.3);
            *visibility = Visibility::Visible;
        } else {
            *visibility = Visibility::Hidden;
        }
    }
}

// --- Constants ---

/// 맵의 가로 너비 (160 타일)
pub const MAP_WIDTH: usize = 160;
/// 맵의 세로 높이 (100 타일)
pub const MAP_HEIGHT: usize = 100;
/// 타일의 픽셀 크기 (16px)
pub const TILE_SIZE: f32 = 16.0;

// --- Coords Conversion ---

/// 타일 좌표(그리드)를 Bevy 월드 좌표(픽셀 위치)로 변환합니다.
pub fn tile_to_world_coords(x: usize, y: usize) -> Vec2 {
    let screen_width_offset = (MAP_WIDTH as f32 * TILE_SIZE) / 2.0 - TILE_SIZE / 2.0;
    let screen_height_offset = (MAP_HEIGHT as f32 * TILE_SIZE) / 2.0 - TILE_SIZE / 2.0;
    Vec2::new(x as f32 * TILE_SIZE - screen_width_offset, y as f32 * TILE_SIZE - screen_height_offset)
}

/// Bevy 월드 좌표를 가장 가까운 타일 그리드 좌표로 변환합니다.
pub fn world_to_tile_coords(world_pos: Vec3) -> (usize, usize) {
    let screen_width_offset = (MAP_WIDTH as f32 * TILE_SIZE) / 2.0 - TILE_SIZE / 2.0;
    let screen_height_offset = (MAP_HEIGHT as f32 * TILE_SIZE) / 2.0 - TILE_SIZE / 2.0;
    let x = ((world_pos.x + screen_width_offset + TILE_SIZE / 2.0) / TILE_SIZE).floor() as usize;
    let y = ((world_pos.y + screen_height_offset + TILE_SIZE / 2.0) / TILE_SIZE).floor() as usize;
    (x.clamp(0, MAP_WIDTH - 1), y.clamp(0, MAP_HEIGHT - 1))
}
