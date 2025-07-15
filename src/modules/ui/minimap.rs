use bevy::{
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        render_asset::RenderAssetUsages,
    },
};
use crate::modules::{
    map::{bsp::MapResource, bsp::MapTile, MAP_HEIGHT, MAP_WIDTH},
    player::Player,
};

/// 미니맵 이미지를 관리하는 리소스입니다.
#[derive(Resource)]
pub struct MinimapImage(pub Handle<Image>);


pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_minimap)
            .add_systems(Update, update_minimap);
    }
}

pub(crate) fn setup_minimap(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    // 160x100 크기의 초기 텍스처를 생성합니다.
    let extent = Extent3d {
        width: MAP_WIDTH as u32,
        height: MAP_HEIGHT as u32,
        ..default()
    };
    
    let image = Image::new_fill(
        extent,
        TextureDimension::D2,
        &[0, 0, 0, 255], // 초기값: 검은색 (알파 255)
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    
    let handle = images.add(image);
    commands.insert_resource(MinimapImage(handle));
}

fn update_minimap(
    map_res: Res<MapResource>,
    minimap_res: Res<MinimapImage>,
    mut images: ResMut<Assets<Image>>,
    player_query: Query<&Transform, With<Player>>,
) {
    let map = map_res.map();
    let image_handle = &minimap_res.0;
    
    if let Some(image) = images.get_mut(image_handle) {
        let (player_x, player_y) = if let Ok(player_transform) = player_query.get_single() {
            crate::modules::map::world_to_tile_coords(player_transform.translation)
        } else {
            (0, 0)
        };

        // 데이터 업데이트 (RGBA8 형식)
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                let idx = map.index(x, y);
                let pixel_idx = (y * MAP_WIDTH + x) * 4;
                
                let is_visible = map.visible_tiles[idx];
                let is_revealed = map.revealed_tiles[idx];
                
                let color = if x == player_x && y == player_y {
                    [255, 255, 0, 255] // Player: Yellow
                } else if is_visible {
                    match map.tiles[idx] {
                        MapTile::Wall => [150, 150, 150, 255], // Visible Wall: Light Gray
                        MapTile::Floor => [255, 255, 255, 255], // Visible Floor: White
                    }
                } else if is_revealed {
                    match map.tiles[idx] {
                        MapTile::Wall => [40, 40, 40, 255],   // Revealed Wall: Dark Gray
                        MapTile::Floor => [80, 80, 80, 255],  // Revealed Floor: Gray
                    }
                } else {
                    [0, 0, 0, 255] // Not revealed: Black
                };
                
                image.data[pixel_idx..pixel_idx + 4].copy_from_slice(&color);
            }
        }
    }
}
