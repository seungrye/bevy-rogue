use bevy::prelude::*;
use rand::prelude::*;

use super::tile_to_world_coords;

/// 타일 엔티티를 추적하기 위한 컴포넌트입니다.
/// 각 타일 엔티티가 맵의 어느 위치에 있는지 저장합니다.
#[derive(Component)]
pub struct TileEntity {
    pub x: usize,
    pub y: usize,
}

/// 맵 생성을 담당하는 Bevy 플러그인입니다.
///
/// Bevy에서 플러그인은 관련 있는 로직(컴포넌트, 리소스, 시스템 등)을
/// 하나의 단위로 묶어주는 역할을 합니다. 이 플러그인을 앱에 추가하면,
/// 게임 시작(`Startup`) 시점에 맵을 생성하고 화면에 그리는 시스템들이 자동으로 등록됩니다.
pub struct BspMapPlugin;

impl Plugin for BspMapPlugin {
    fn build(&self, app: &mut App) {
        // 앱이 시작될 때 (Startup 스케줄에서) 실행될 시스템들을 등록합니다.
        app.add_systems(Startup, (
            // 1. BSP 알고리즘을 사용해 맵 데이터를 생성하고, Bevy의 리소스(Resource)로 저장합니다.
            create_and_store_map,
            // 2. `create_and_store_map` 시스템이 완료된 '후에' 실행되도록 순서를 지정합니다.
            //    `.after()`를 사용함으로써, `draw_map`이 실행될 때 맵 리소스가 항상 존재함을 보장할 수 있습니다.
            draw_map.after(create_and_store_map)
        ))
        .add_systems(Update, update_tile_visibility);
    }
}


/// 맵을 구성하는 각 타일의 종류를 나타내는 열거형입니다.
/// `Copy`와 `Clone`을 derive하여 값 타입처럼 간단하게 복사해서 사용할 수 있습니다.
/// `PartialEq`와 `Eq`는 타일 종류를 비교할 때 필요합니다 (예: `tile == MapTile::Wall`).
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum MapTile {
    /// 이동할 수 없는 벽 타일입니다.
    Wall,
    /// 플레이어나 다른 개체들이 이동할 수 있는 바닥 타일입니다.
    Floor,
}

/// 맵 상의 사각형 영역을 정의하는 구조체입니다.
/// BSP(Binary Space Partitioning) 알고리즘에서 공간을 분할하거나,
/// 생성된 방의 위치와 크기를 나타내는 데 사용됩니다.
/// `Debug`는 디버깅 시 `println!("{:?}", rect)`와 같이 출력하기 위해,
/// `Copy`와 `Clone`은 구조체를 쉽게 복사하기 위해 추가되었습니다.
#[derive(Debug, Copy, Clone)]
pub struct Rect {
    pub x1: usize,
    pub x2: usize,
    pub y1: usize,
    pub y2: usize,
}

impl Rect {
    /// 새로운 `Rect` 인스턴스를 생성합니다.
    ///
    /// # Arguments
    /// * `x`, `y`: 사각형의 왼쪽 위 모서리 좌표
    /// * `w`, `h`: 사각형의 너비와 높이
    pub fn new(x: usize, y: usize, w: usize, h: usize) -> Self {
        Self {
            x1: x,
            y1: y,
            x2: x + w,
            y2: y + h,
        }
    }

    /// 사각형의 너비(width)를 계산하여 반환합니다.
    pub fn width(&self) -> usize {
        self.x2 - self.x1
    }

    /// 사각형의 높이(height)를 계산하여 반환합니다.
    pub fn height(&self) -> usize {
        self.y2 - self.y1
    }

    /// 사각형의 중심 좌표를 `(x, y)` 튜플로 반환합니다.
    /// 방과 방을 연결하는 복도를 만들 때 유용하게 사용됩니다.
    pub fn center(&self) -> (usize, usize) {
        ((self.x1 + self.x2) / 2, (self.y1 + self.y2) / 2)
    }
}

/// 게임 맵 전체의 데이터를 담는 구조체입니다.
pub struct Map {
    /// 맵의 너비 (타일 단위)
    pub width: usize,
    /// 맵의 높이 (타일 단위)
    pub height: usize,
    /// 맵의 모든 타일 데이터.
    /// 2차원 맵을 1차원 `Vec`으로 저장하면, 인덱스 계산은 조금 더 필요하지만
    /// 메모리 상에서 연속적으로 배치되어 캐시 효율성이 높아져 성능에 이점이 있을 수 있습니다.
    pub tiles: Vec<MapTile>,
    /// 맵 생성 과정에서 만들어진 모든 방의 `Rect` 정보를 저장합니다.
    /// 이 정보는 나중에 복도를 연결하거나 플레이어, 아이템 등을 배치할 때 사용됩니다.
    pub rooms: Vec<Rect>,
    /// 플레이어가 한 번이라도 본 타일들을 기록합니다. 미니맵 표시에 사용됩니다.
    pub revealed_tiles: Vec<bool>,
    /// 현재 플레이어의 시야에 보이는 타일들을 기록합니다.
    pub visible_tiles: Vec<bool>,
}

impl Map {
    /// 지정된 너비와 높이로 새로운 `Map`을 생성하고, 모든 타일을 벽(`Wall`)으로 초기화합니다.
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

    /// 2차원 맵 좌표 `(x, y)`를 1차원 `tiles` 벡터의 인덱스로 변환합니다.
    /// 이 계산은 'Row-major' 순서를 따릅니다. (y 좌표가 행, x 좌표가 열)
    pub fn index(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    /// 지정된 `(x, y)` 좌표에 원하는 `MapTile`을 설정합니다.
    pub fn set_tile(&mut self, x: usize, y: usize, tile: MapTile) {
        let idx = self.index(x, y);
        self.tiles[idx] = tile;
    }

    /// 지정된 `(x, y)` 좌표의 `MapTile`을 가져옵니다.
    pub fn get_tile(&self, x: usize, y: usize) -> MapTile {
        self.tiles[self.index(x, y)]
    }
}

/// BSP(Binary Space Partitioning) 알고리즘을 사용하여 절차적으로 던전 맵을 생성합니다.
fn generate_bsp_map(width: usize, height: usize) -> Map {
    // 1. 초기화
    // 모든 타일이 벽으로 채워진 맵을 생성합니다.
    let mut map = Map::new(width, height);
    // 난수 생성을 위한 `rng`를 가져옵니다.
    let mut rng = thread_rng();

    // `leaves`는 분할된 공간(사각형 영역)들을 담는 벡터입니다.
    let mut leaves = Vec::new();

    // 2. 루트 영역 생성
    // 전체 맵에서 테두리 1칸을 제외한 영역을 최초의 분할 공간(루트)으로 설정합니다.
    // 테두리를 남겨두면 맵이 항상 벽으로 둘러싸이게 됩니다.
    let root = Rect::new(1, 1, width - 2, height - 2);
    leaves.push(root);

    // 3. 공간 분할 (Partitioning)
    // `MIN_LEAF_SIZE`는 분할을 멈출 최소 크기를 정의합니다. 이 값보다 작아지면 더 이상 쪼개지 않습니다.
    // 이 값을 조절하여 생성되는 방의 최소 크기와 밀도를 조절할 수 있습니다.
    const MIN_LEAF_SIZE: usize = 8;
    // `MIN_ROOM_SIZE`는 방의 최소 너비/높이를 정의합니다.
    const MIN_ROOM_SIZE: usize = 3;

    let mut did_split = true;
    // 분할이 더 이상 일어나지 않을 때까지(모든 `leaf`가 `MIN_LEAF_SIZE` 이하가 될 때까지) 반복합니다.
    while did_split {
        did_split = false;
        let mut new_leaves = Vec::new();

        for leaf in leaves.iter() {
            // 현재 `leaf`가 이미 충분히 작다면, 분할하지 않고 그대로 다음 단계로 넘깁니다.
            if leaf.width() <= MIN_LEAF_SIZE && leaf.height() <= MIN_LEAF_SIZE {
                new_leaves.push(*leaf);
                continue;
            }

            // 개선된 분할 방향 결정: 영역이 길쭉한 방향으로 분할하여 더 정사각형에 가까운 방을 만듭니다.
            let split_horizontally = if leaf.width() > leaf.height() && leaf.width() as f32 / leaf.height() as f32 >= 1.25 {
                false // 너비가 높이보다 1.25배 이상 크면 무조건 수직 분할
            } else if leaf.height() > leaf.width() && leaf.height() as f32 / leaf.width() as f32 >= 1.25 {
                true // 높이가 너비보다 1.25배 이상 크면 무조건 수평 분할
            } else {
                rng.gen_bool(0.5) // 그 외에는 50% 확률
            };

            if split_horizontally {
                // 수평으로 분할합니다. (y축 기준)
                // 분할 지점을 랜덤하게 선택하되, 양쪽 영역이 너무 작아지지 않도록 범위를 제한합니다.
                let split = rng.gen_range(leaf.y1 + MIN_LEAF_SIZE / 2..=leaf.y2 - MIN_LEAF_SIZE / 2);
                let top_rect = Rect::new(leaf.x1, leaf.y1, leaf.width(), split - leaf.y1);
                let bottom_rect = Rect::new(leaf.x1, split, leaf.width(), leaf.y2 - split);
                // 생성된 영역이 최소 방 크기보다 크거나 같을 때만 유효한 영역으로 추가합니다.
                if top_rect.height() >= MIN_ROOM_SIZE { new_leaves.push(top_rect); }
                if bottom_rect.height() >= MIN_ROOM_SIZE { new_leaves.push(bottom_rect); }
            } else {
                // 수직으로 분할합니다. (x축 기준)
                let split = rng.gen_range(leaf.x1 + MIN_LEAF_SIZE / 2..=leaf.x2 - MIN_LEAF_SIZE / 2);
                let left_rect = Rect::new(leaf.x1, leaf.y1, split - leaf.x1, leaf.height());
                let right_rect = Rect::new(split, leaf.y1, leaf.x2 - split, leaf.height());
                if left_rect.width() >= MIN_ROOM_SIZE { new_leaves.push(left_rect); }
                if right_rect.width() >= MIN_ROOM_SIZE { new_leaves.push(right_rect); }
            }
            did_split = true;
        }
        // 분할이 한 번이라도 일어났다면, `leaves` 벡터를 새로 생성된 `new_leaves`로 교체합니다.
        if did_split {
            leaves = new_leaves;
        }
    }

    // 4. 방 생성 (Room Generation)
    // 최종적으로 분할된 모든 `leaf` 영역에 대해 방을 생성합니다.
    for leaf in leaves.iter() {
        // // 디버깅용 로그: 현재 leaf의 크기를 출력합니다.
        // // 최종 맵 생성 시에는 주석 처리하거나 삭제하는 것이 좋습니다.
        // println!("Leaf: {:?}", leaf);

        // 방을 생성하기에 충분한 공간이 있는지 다시 한번 확인합니다.
        // 방은 최소 `MIN_ROOM_SIZE` 크기에, 양 옆으로 1칸씩 벽이 있어야 하므로 `+ 2`를 해줍니다.
        if leaf.width() < MIN_ROOM_SIZE + 2 || leaf.height() < MIN_ROOM_SIZE + 2 {
            continue; // 공간이 부족하면 이 leaf는 건너뜁니다.
        }

        // `leaf` 내부에 생성될 방의 크기를 랜덤하게 결정합니다.
        // 방의 최대 크기는 `leaf` 크기에서 벽 2칸을 뺀 값입니다.
        let max_width = leaf.width() - 2;
        let room_w = rng.gen_range(MIN_ROOM_SIZE..=max_width);
        let max_height = leaf.height() - 2;
        let room_h = rng.gen_range(MIN_ROOM_SIZE..=max_height);

        // `leaf` 내에서 방이 위치할 좌표를 랜덤하게 결정합니다.
        let min_x = leaf.x1 + 1;
        let max_x = leaf.x2 - room_w - 1;
        let room_x = rng.gen_range(min_x..=max_x);

        let min_y = leaf.y1 + 1;
        let max_y = leaf.y2 - room_h - 1;
        let room_y = rng.gen_range(min_y..=max_y);

        let room = Rect::new(room_x, room_y, room_w, room_h);

        // 생성된 방의 영역을 `MapTile::Floor`로 채웁니다.
        for y in room.y1..room.y2 {
            for x in room.x1..room.x2 {
                map.set_tile(x, y, MapTile::Floor);
            }
        }
        // 생성된 방 정보를 `map.rooms`에 추가합니다.
        map.rooms.push(room);
    }

    // 5. 복도 생성 (Corridor Generation)
    // 생성된 방들을 순서대로 하나씩 연결하여 모든 방이 이어지도록 합니다.
    for i in 0..map.rooms.len() - 1 {
        let (x1, y1) = map.rooms[i].center();
        let (x2, y2) = map.rooms[i+1].center();

        // L자 형태의 복도를 만듭니다. 먼저 수평으로 이동하고, 그 다음 수직으로 이동합니다.
        // 이 방식은 모든 방이 연결됨을 보장하는 가장 간단한 방법 중 하나입니다.
        for x in x1.min(x2)..=x1.max(x2) { map.set_tile(x, y1, MapTile::Floor); }
        for y in y1.min(y2)..=y1.max(y2) { map.set_tile(x2, y, MapTile::Floor); }
    }

    map
}

/// `Map` 데이터를 Bevy의 ECS 월드에 리소스로 저장하기 위한 래퍼 구조체입니다.
/// `#[derive(Resource)]` 어트리뷰트를 통해 이 구조체를 Bevy 리소스로 사용할 수 있음을 알립니다.
/// 리소스로 등록하면, 다른 시스템에서 `Res<MapResource>`나 `ResMut<MapResource>`를 통해
/// 월드 어디에서든 이 데이터에 접근할 수 있습니다.
#[derive(Resource)]
pub struct MapResource(Map);

impl MapResource {
    /// 맵 데이터에 대한 불변 참조를 반환하는 공개 접근자(public accessor)입니다.
    pub fn map(&self) -> &Map {
        &self.0
    }

    /// 맵 데이터에 대한 가변 참조를 반환하는 공개 접근자입니다.
    pub fn map_mut(&mut self) -> &mut Map {
        &mut self.0
    }
}

/// `Startup` 시점에 실행되는 시스템으로, 맵을 생성하고 `MapResource`에 담아 리소스로 추가합니다.
/// `mut commands: Commands`는 Bevy의 ECS에 명령을 내리기 위한 파라미터입니다.
/// 여기서는 `insert_resource` 명령을 사용하여 `MapResource`를 월드에 추가하고 있습니다.
fn create_and_store_map(mut commands: Commands) {
    // `generate_bsp_map` 함수를 호출하여 맵 데이터를 생성합니다.
    let map = generate_bsp_map(super::MAP_WIDTH, super::MAP_HEIGHT);
    // 생성된 맵을 `MapResource`로 감싸서 Bevy의 리소스로 등록합니다.
    commands.insert_resource(MapResource(map));
}

/// `Startup` 시스템: `MapResource`에 저장된 맵 데이터를 읽어와 화면에 텍스트로 그립니다.
pub fn draw_map(
    mut commands: Commands,         // 엔티티 생성/삭제 등 월드에 대한 명령을 내립니다.
    asset_server: Res<AssetServer>, // 에셋(폰트, 이미지 등)을 로드하기 위한 리소스입니다.
    map_res: Res<MapResource>,      // `create_and_store_map`에서 생성된 맵 리소스에 접근합니다.
) {
    // 맵을 그릴 폰트를 로드합니다. `assets/fonts` 폴더에 해당 폰트 파일이 있어야 합니다.
    // `asset_server.load`는 에셋에 대한 핸들(`Handle`)을 반환합니다.
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");

    // 리소스에서 맵 데이터에 대한 참조를 가져옵니다.
    let map = map_res.map();

    // 맵의 모든 타일 좌표(y, x)를 순회합니다.
    for y in 0..map.height {
        for x in 0..map.width {
            // 현재 좌표의 타일 종류를 가져옵니다.
            let tile = map.get_tile(x, y);
            // 타일 종류에 따라 화면에 표시할 문자(glyph)를 결정합니다.
            let glyph = match tile {
                MapTile::Wall => "#",   // 벽은 '#'으로
                MapTile::Floor => ".", // 바닥은 '.'으로
            };

            let coord = tile_to_world_coords(x, y);

            // 각 타일에 해당하는 텍스트 엔티티를 생성(spawn)합니다.
            commands.spawn((
                Text2dBundle {
                    // 표시할 텍스트와 스타일을 설정합니다.
                    text: Text::from_section(
                        glyph,
                        TextStyle {
                            // `font` 핸들을 복제하여 사용합니다. 핸들은 가벼워서 복제 비용이 저렴합니다.
                            font: font.clone(),
                            font_size: super::TILE_SIZE, // 타일 크기를 폰트 크기로 설정합니다.
                            color: Color::WHITE,  // 텍스트 색상을 흰색으로 설정합니다.
                        },
                    ),
                    // 텍스트의 월드 좌표를 설정합니다.
                    transform: Transform::from_xyz(
                        coord.x, // x 좌표
                        coord.y, // y 좌표
                        // z 좌표: 2D 렌더링에서는 보통 0.0으로 설정하여 렌더링 순서를 제어합니다.
                        0.0,
                    ),
                    // `..default()`는 나머지 필드를 기본값으로 채웁니다.
                    ..default()
                },
                // 타일 위치 정보를 저장하는 컴포넌트를 추가합니다.
                TileEntity { x, y },
            ));
        }
    }
}

/// 타일의 가시성에 따라 색상과 표시 여부를 업데이트하는 시스템입니다.
fn update_tile_visibility(
    map_res: Res<MapResource>,
    mut tile_query: Query<(&TileEntity, &mut Text, &mut Visibility)>,
) {
    let map = map_res.map();
    
    for (tile_entity, mut text, mut visibility) in tile_query.iter_mut() {
        let idx = map.index(tile_entity.x, tile_entity.y);
        let is_visible = map.visible_tiles[idx];
        let is_revealed = map.revealed_tiles[idx];
        
        if is_visible {
            // 현재 보이는 타일은 정상 색상으로 표시
            text.sections[0].style.color = Color::WHITE;
            *visibility = Visibility::Visible;
        } else if is_revealed {
            // 본 적 있지만 현재 보이지 않는 타일은 다크 그레이로 표시
            text.sections[0].style.color = Color::rgb(0.3, 0.3, 0.3);
            *visibility = Visibility::Visible;
        } else {
            // 본 적 없는 타일은 숨김
            *visibility = Visibility::Hidden;
        }
    }
}
