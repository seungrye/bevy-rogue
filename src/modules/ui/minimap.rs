use bevy::{
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        render_asset::RenderAssetUsages,
        texture::ImageSampler,
    },
};
use crate::modules::{
    map::{MapResource, MapTile, MAP_HEIGHT, MAP_WIDTH, MapGeneratorRegistry},
    player::Player,
};

pub const MINIMAP_RADIUS: i32 = 20;
pub const MINIMAP_SIDE: u32 = (MINIMAP_RADIUS * 2 + 1) as u32;
pub const MINIMAP_DISPLAY_SIZE: f32 = 180.0;
const MINIMAP_MIN_SIZE: f32 = 80.0;
const MINIMAP_MAX_SIZE: f32 = 280.0;
const MINIMAP_ZOOM_STEP: f32 = 20.0;

#[derive(Resource)]
pub struct MinimapImage(pub Handle<Image>);

#[derive(Resource)]
pub struct MinimapConfig {
    pub display_size: f32,
}

impl Default for MinimapConfig {
    fn default() -> Self { Self { display_size: MINIMAP_DISPLAY_SIZE } }
}

#[derive(Component)]
pub struct MinimapOverlay;

#[derive(Component)]
struct MinimapImageNode;

#[derive(Component)]
pub(super) struct GeneratorNameText;

pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MinimapConfig>()
            .add_systems(Startup, (
                setup_minimap,
                spawn_minimap_overlay.after(setup_minimap),
            ))
            .add_systems(Update, (update_minimap, toggle_minimap, update_generator_name, zoom_minimap));
    }
}

fn toggle_visibility(vis: Visibility) -> Visibility {
    match vis {
        Visibility::Hidden => Visibility::Inherited,
        _ => Visibility::Hidden,
    }
}

pub(crate) fn setup_minimap(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let extent = Extent3d {
        width: MINIMAP_SIDE,
        height: MINIMAP_SIDE,
        ..default()
    };

    let mut image = Image::new_fill(
        extent,
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();

    let handle = images.add(image);
    commands.insert_resource(MinimapImage(handle));
}

fn spawn_minimap_overlay(
    mut commands: Commands,
    minimap_res: Res<MinimapImage>,
    asset_server: Res<AssetServer>,
    registry: Res<MapGeneratorRegistry>,
    config: Res<MinimapConfig>,
) {
    let font = asset_server.load("fonts/NanumSquareNeo-bRg.ttf");
    commands.spawn((
        NodeBundle {
            style: Style {
                position_type: PositionType::Absolute,
                right: Val::Px(5.0),
                top: Val::Px(10.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                row_gap: Val::Px(4.0),
                ..default()
            },
            z_index: ZIndex::Global(50),
            ..default()
        },
        MinimapOverlay,
    )).with_children(|parent| {
        parent.spawn((
            ImageBundle {
                style: Style {
                    width: Val::Px(config.display_size),
                    height: Val::Px(config.display_size),
                    ..default()
                },
                image: minimap_res.0.clone().into(),
                ..default()
            },
            MinimapImageNode,
        ));
        parent.spawn((
            TextBundle::from_section(
                registry.current_name(),
                TextStyle { font: font.clone(), font_size: 13.0, color: Color::CYAN },
            ),
            GeneratorNameText,
        ));
        parent.spawn(TextBundle::from_section(
            "[Tab] 맵 전환",
            TextStyle { font, font_size: 11.0, color: Color::GRAY },
        ));
    });
}

pub fn apply_zoom(current: f32, delta: f32) -> f32 {
    (current + delta).clamp(MINIMAP_MIN_SIZE, MINIMAP_MAX_SIZE)
}

fn zoom_minimap(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut config: ResMut<MinimapConfig>,
    mut q: Query<&mut Style, With<MinimapImageNode>>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !ctrl { return; }

    let zoom_in  = keyboard.just_pressed(KeyCode::Equal) || keyboard.just_pressed(KeyCode::NumpadAdd);
    let zoom_out = keyboard.just_pressed(KeyCode::Minus) || keyboard.just_pressed(KeyCode::NumpadSubtract);

    let delta = if zoom_in { MINIMAP_ZOOM_STEP } else if zoom_out { -MINIMAP_ZOOM_STEP } else { return };
    config.display_size = apply_zoom(config.display_size, delta);

    if let Ok(mut style) = q.get_single_mut() {
        style.width  = Val::Px(config.display_size);
        style.height = Val::Px(config.display_size);
    }
}

fn update_generator_name(
    registry: Res<MapGeneratorRegistry>,
    mut q: Query<&mut Text, With<GeneratorNameText>>,
) {
    if registry.is_changed() {
        if let Ok(mut text) = q.get_single_mut() {
            text.sections[0].value = registry.current_name().to_string();
        }
    }
}

fn toggle_minimap(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut q: Query<&mut Visibility, With<MinimapOverlay>>,
) {
    if keyboard.just_pressed(KeyCode::KeyM) {
        if let Ok(mut vis) = q.get_single_mut() {
            *vis = toggle_visibility(*vis);
        }
    }
}

fn is_outside_diamond(tx: u32, ty: u32) -> bool {
    let dx = (tx as i32 - MINIMAP_RADIUS).abs();
    let dy = (ty as i32 - MINIMAP_RADIUS).abs();
    dx + dy > MINIMAP_RADIUS
}

fn is_diamond_border(tx: u32, ty: u32) -> bool {
    let dx = (tx as i32 - MINIMAP_RADIUS).abs();
    let dy = (ty as i32 - MINIMAP_RADIUS).abs();
    dx + dy == MINIMAP_RADIUS
}

fn update_minimap(
    map_res: Res<MapResource>,
    minimap_res: Res<MinimapImage>,
    mut images: ResMut<Assets<Image>>,
    player_query: Query<&Transform, With<Player>>,
    mut last_pos: Local<Option<IVec2>>,
) {
    if map_res.is_changed() {
        *last_pos = None;
    }

    let Ok(player_transform) = player_query.get_single() else { return };
    let (player_x, player_y) = crate::modules::map::world_to_tile_coords(player_transform.translation);
    let current_pos = IVec2::new(player_x as i32, player_y as i32);

    if Some(current_pos) == *last_pos { return; }
    *last_pos = Some(current_pos);

    let map = map_res.map();
    let Some(image) = images.get_mut(&minimap_res.0) else { return };

    for ty in 0..MINIMAP_SIDE {
        for tx in 0..MINIMAP_SIDE {
            let pixel_idx = (ty * MINIMAP_SIDE + tx) as usize * 4;
            // 다이아몬드 바깥은 완전 투명
            if is_outside_diamond(tx, ty) {
                image.data[pixel_idx..pixel_idx + 4].copy_from_slice(&[0, 0, 0, 0]);
                continue;
            }

            let x = player_x as i32 + (tx as i32 - MINIMAP_RADIUS);
            // y축 반전: 이미지 상단 = 게임 북쪽
            let y = player_y as i32 + (MINIMAP_RADIUS - ty as i32);

            // 맵 경계 밖
            if x < 0 || x >= MAP_WIDTH as i32 || y < 0 || y >= MAP_HEIGHT as i32 {
                image.data[pixel_idx..pixel_idx + 4].copy_from_slice(&[10, 8, 6, 200]);
                continue;
            }

            let x = x as usize;
            let y = y as usize;
            let idx = map.index(x, y);

            let color: [u8; 4] = if is_diamond_border(tx, ty) {
                [160, 140, 100, 230] // 다이아몬드 테두리
            } else if x == player_x && y == player_y {
                [255, 220, 0, 255]   // 플레이어
            } else if map.visible_tiles[idx] {
                match map.tiles[idx] {
                    MapTile::Wall  => [220, 200, 155, 255], // 시야 내 벽
                    MapTile::Floor => [130, 110, 80,  200], // 시야 내 바닥
                }
            } else if map.revealed_tiles[idx] {
                match map.tiles[idx] {
                    MapTile::Wall  => [110, 95,  70,  200], // 탐험된 벽
                    MapTile::Floor => [60,  50,  35,  160], // 탐험된 바닥
                }
            } else {
                [10, 8, 6, 200] // 미탐험
            };

            image.data[pixel_idx..pixel_idx + 4].copy_from_slice(&color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_is_inside_diamond() {
        assert!(!is_outside_diamond(MINIMAP_RADIUS as u32, MINIMAP_RADIUS as u32));
    }

    #[test]
    fn corner_is_outside_diamond() {
        assert!(is_outside_diamond(0, 0));
        assert!(is_outside_diamond(MINIMAP_SIDE - 1, 0));
        assert!(is_outside_diamond(0, MINIMAP_SIDE - 1));
        assert!(is_outside_diamond(MINIMAP_SIDE - 1, MINIMAP_SIDE - 1));
    }

    #[test]
    fn cardinal_tips_are_inside_diamond() {
        let r = MINIMAP_RADIUS as u32;
        // 상하좌우 끝점은 경계(border)에 해당
        assert!(!is_outside_diamond(r, 0));
        assert!(!is_outside_diamond(r, MINIMAP_SIDE - 1));
        assert!(!is_outside_diamond(0, r));
        assert!(!is_outside_diamond(MINIMAP_SIDE - 1, r));
    }

    #[test]
    fn cardinal_tips_are_on_border() {
        let r = MINIMAP_RADIUS as u32;
        assert!(is_diamond_border(r, 0));
        assert!(is_diamond_border(r, MINIMAP_SIDE - 1));
        assert!(is_diamond_border(0, r));
        assert!(is_diamond_border(MINIMAP_SIDE - 1, r));
    }

    #[test]
    fn center_is_not_border() {
        assert!(!is_diamond_border(MINIMAP_RADIUS as u32, MINIMAP_RADIUS as u32));
    }

    #[test]
    fn display_size_is_positive() {
        assert!(MINIMAP_DISPLAY_SIZE > 0.0);
        assert!(MINIMAP_SIDE > 0);
    }

    #[test]
    fn zoom_increases_by_step() {
        assert_eq!(apply_zoom(180.0, MINIMAP_ZOOM_STEP), 200.0);
    }

    #[test]
    fn zoom_decreases_by_step() {
        assert_eq!(apply_zoom(180.0, -MINIMAP_ZOOM_STEP), 160.0);
    }

    #[test]
    fn zoom_clamps_at_max() {
        assert_eq!(apply_zoom(MINIMAP_MAX_SIZE, MINIMAP_ZOOM_STEP), MINIMAP_MAX_SIZE);
    }

    #[test]
    fn zoom_clamps_at_min() {
        assert_eq!(apply_zoom(MINIMAP_MIN_SIZE, -MINIMAP_ZOOM_STEP), MINIMAP_MIN_SIZE);
    }

    #[test]
    fn default_size_is_within_bounds() {
        assert!(MINIMAP_MIN_SIZE <= MINIMAP_DISPLAY_SIZE && MINIMAP_DISPLAY_SIZE <= MINIMAP_MAX_SIZE);
    }

    #[test]
    fn generator_hint_font_sizes_are_smaller_than_minimap() {
        // 생성기 이름·Tab 힌트는 미니맵 이미지보다 작아야 한다
        let name_font_size: f32 = 13.0;
        let hint_font_size: f32 = 11.0;
        assert!(name_font_size < MINIMAP_DISPLAY_SIZE);
        assert!(hint_font_size < name_font_size);
    }

    #[test]
    fn toggle_visible_to_hidden() {
        assert_eq!(toggle_visibility(Visibility::Inherited), Visibility::Hidden);
        assert_eq!(toggle_visibility(Visibility::Visible), Visibility::Hidden);
    }

    #[test]
    fn toggle_hidden_to_visible() {
        assert_eq!(toggle_visibility(Visibility::Hidden), Visibility::Inherited);
    }

    #[test]
    fn double_toggle_restores_visible() {
        let original = Visibility::Inherited;
        let after_two_toggles = toggle_visibility(toggle_visibility(original));
        assert_eq!(after_two_toggles, Visibility::Inherited);
    }
}
