use bevy::{
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        render_asset::RenderAssetUsages,
    },
};
use crate::modules::{
    map::{MapResource, MapTile, MAP_HEIGHT, MAP_WIDTH},
    player::Player,
};

/// 미니맵의 가시 범위 (반경 타일 수)
pub const MINIMAP_RADIUS: i32 = 20;
/// 미니맵의 한 변의 길이 (타일 수, 지름)
pub const MINIMAP_SIDE: u32 = (MINIMAP_RADIUS * 2 + 1) as u32;

/// 미니맵을 위한 동적 Texture 이미지를 관리하는 리소스입니다.
#[derive(Resource)]
pub struct MinimapImage(pub Handle<Image>);

/// 미니맵 생성 및 실시간 픽셀 업데이트 시스템을 관리하는 플러그인입니다.
pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    /// 미니맵 텍스처 초기화 및 업데이트 시스템을 등록합니다.
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_minimap)
            .add_systems(Update, update_minimap);
    }
}

/// 미니맵을 위한 빈 RGBA 이미지를 생성하여 리소스로 등록합니다.
pub(crate) fn setup_minimap(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    // 텍스처 크기 설정
    let extent = Extent3d {
        width: MINIMAP_SIDE,
        height: MINIMAP_SIDE,
        ..default()
    };
    
    // 초기 검정색 채우기
    let image = Image::new_fill(
        extent,
        TextureDimension::D2,
        &[0, 0, 0, 255], 
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    
    let handle = images.add(image);
    commands.insert_resource(MinimapImage(handle));
}

/// 플레이어 위치 변화 시 미니맵 픽셀 데이터를 실시간으로 갱신하여 가시성을 반영합니다.
fn update_minimap(
    map_res: Res<MapResource>,
    minimap_res: Res<MinimapImage>,
    mut images: ResMut<Assets<Image>>,
    player_query: Query<&Transform, With<Player>>,
    mut last_pos: Local<Option<IVec2>>,
) {
    // 맵이 교체되면 강제 갱신
    if map_res.is_changed() {
        *last_pos = None;
    }

    if let Ok(player_transform) = player_query.get_single() {
        let (player_x, player_y) = crate::modules::map::world_to_tile_coords(player_transform.translation);
        let current_pos = IVec2::new(player_x as i32, player_y as i32);

        if Some(current_pos) == *last_pos {
            return;
        }
        *last_pos = Some(current_pos);

        let start = std::time::Instant::now();
        let map = map_res.map();
        let image_handle = &minimap_res.0;
        
        if let Some(image) = images.get_mut(image_handle) {
            // 모든 미니맵 타일 순회하며 색상 지정
            for ty in 0..MINIMAP_SIDE {
                for tx in 0..MINIMAP_SIDE {
                    let x = player_x as i32 + (tx as i32 - MINIMAP_RADIUS);
                    // y축은 Bevy 2D 좌표와 일치하도록 반전 처리
                    let y = player_y as i32 + (MINIMAP_RADIUS - ty as i32);
                    
                    let pixel_idx = (ty * MINIMAP_SIDE + tx) as usize * 4;
                    
                    // 맵 범위 이외 지역 처리
                    if x < 0 || x >= MAP_WIDTH as i32 || y < 0 || y >= MAP_HEIGHT as i32 {
                        image.data[pixel_idx..pixel_idx + 4].copy_from_slice(&[0, 0, 0, 255]);
                        continue;
                    }
                    
                    let x = x as usize;
                    let y = y as usize;
                    let idx = map.index(x, y);
                    let is_visible = map.visible_tiles[idx];
                    let is_revealed = map.revealed_tiles[idx];
                    
                    // 상태에 따른 픽셀 색상 결정 (RGBA)
                    let color = if x == player_x && y == player_y {
                        [255, 255, 0, 255] // 플레이어: 노랑
                    } else if is_visible {
                        match map.tiles[idx] {
                            MapTile::Wall => [255, 255, 255, 255], // 실시간 벽: 짙은 청회색 (대비 강화)
                            MapTile::Floor => [255, 255, 255, 125], // 실시간 바닥: 하양
                        }
                    } else if is_revealed {
                        match map.tiles[idx] {
                            MapTile::Wall => [255, 255, 255, 255],    // 탐험된 벽: 어두운 청회색 (시인성 증가)
                            MapTile::Floor => [255, 255, 255, 125], // 탐험된 바닥: 중간 밝기의 청회색
                        }
                    } else {
                        [0, 0, 0, 255] // 미정복 지역: 검정
                    };
                    
                    image.data[pixel_idx..pixel_idx + 4].copy_from_slice(&color);
                }
            }
            
            let elapsed = start.elapsed();
            if elapsed.as_micros() > 0 {
                info!("Minimap update took: {:?}", elapsed);
            }
        }
    }
}
