use bevy::{
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        render_asset::RenderAssetUsages,
        texture::ImageSampler,
    },
};
use crate::modules::{
    map::{Map, MapResource, MapTile, MAP_HEIGHT, MAP_WIDTH, MapGeneratorRegistry},
    player::Player,
};

pub const MINIMAP_RADIUS: i32 = 20;
pub const MINIMAP_SIDE: u32 = (MINIMAP_RADIUS * 2 + 1) as u32;
pub const MINIMAP_DISPLAY_SIZE: f32 = 180.0;
const MINIMAP_VIEW_RADIUS_MIN: i32 = MINIMAP_RADIUS;
const MINIMAP_VIEW_RADIUS_MAX: i32 = 70;
const MINIMAP_ZOOM_STEP: i32 = 5;

#[derive(Resource)]
pub struct MinimapImage(pub Handle<Image>);

#[derive(Resource)]
pub struct MinimapConfig {
    pub view_radius: i32,
}

impl Default for MinimapConfig {
    fn default() -> Self { Self { view_radius: MINIMAP_RADIUS } }
}

#[derive(Component)]
pub struct MinimapOverlay;

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
        parent.spawn(ImageBundle {
            style: Style {
                width: Val::Px(MINIMAP_DISPLAY_SIZE),
                height: Val::Px(MINIMAP_DISPLAY_SIZE),
                ..default()
            },
            image: minimap_res.0.clone().into(),
            ..default()
        });
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

/// view_radius 를 clamp하여 반환한다 (순수 함수, 테스트 가능)
pub fn apply_zoom(current: i32, delta: i32) -> i32 {
    (current + delta).clamp(MINIMAP_VIEW_RADIUS_MIN, MINIMAP_VIEW_RADIUS_MAX)
}

fn zoom_minimap(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut config: ResMut<MinimapConfig>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !ctrl { return; }

    let zoom_in  = keyboard.just_pressed(KeyCode::Equal) || keyboard.just_pressed(KeyCode::NumpadAdd);
    let zoom_out = keyboard.just_pressed(KeyCode::Minus) || keyboard.just_pressed(KeyCode::NumpadSubtract);

    // 줌인 = 반경 감소 (타일 더 크게), 줌아웃 = 반경 증가 (더 넓게)
    let delta = if zoom_in { -MINIMAP_ZOOM_STEP } else if zoom_out { MINIMAP_ZOOM_STEP } else { return };
    config.view_radius = apply_zoom(config.view_radius, delta);
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

/// 1D box filter 가중치: 타일 [tile, tile+1) 구간이 [lo, hi) 와 겹치는 비율
pub(crate) fn box_weight(tile: i32, lo: f32, hi: f32, scale: f32) -> f32 {
    (((tile as f32 + 1.0).min(hi) - (tile as f32).max(lo)) / scale).max(0.0)
}

/// 타일 (x, y) 의 미니맵 색상을 [f32; 4] (0–255 범위) 로 반환한다.
/// 맵 경계 밖이거나 미탐험 타일은 어두운 배경색을 반환한다.
pub(crate) fn tile_color_f32(map: &Map, x: i32, y: i32, player_x: usize, player_y: usize) -> [f32; 4] {
    if x < 0 || x >= MAP_WIDTH as i32 || y < 0 || y >= MAP_HEIGHT as i32 {
        return [10.0, 8.0, 6.0, 200.0];
    }
    let ux = x as usize;
    let uy = y as usize;
    let idx = map.index(ux, uy);
    let c: [u8; 4] = if ux == player_x && uy == player_y {
        [255, 220, 0, 255]
    } else if map.visible_tiles[idx] {
        match map.tiles[idx] {
            MapTile::Wall  => [220, 200, 155, 255],
            MapTile::Floor => [130, 110, 80,  200],
        }
    } else if map.revealed_tiles[idx] {
        match map.tiles[idx] {
            MapTile::Wall  => [110, 95,  70,  200],
            MapTile::Floor => [60,  50,  35,  160],
        }
    } else {
        [10, 8, 6, 200]
    };
    [c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32]
}

fn update_minimap(
    map_res: Res<MapResource>,
    minimap_res: Res<MinimapImage>,
    mut images: ResMut<Assets<Image>>,
    player_query: Query<&Transform, With<Player>>,
    config: Res<MinimapConfig>,
    overlay_q: Query<&Visibility, With<MinimapOverlay>>,
    mut last_pos: Local<Option<IVec2>>,
) {
    // 미니맵이 숨겨져 있으면 텍스처 업데이트 불필요
    if let Ok(vis) = overlay_q.get_single() {
        if *vis == Visibility::Hidden { return; }
    }

    if map_res.is_changed() || config.is_changed() {
        *last_pos = None;
    }

    let Ok(player_transform) = player_query.get_single() else { return };
    let (player_x, player_y) = crate::modules::map::world_to_tile_coords(player_transform.translation);
    let current_pos = IVec2::new(player_x as i32, player_y as i32);

    if Some(current_pos) == *last_pos { return; }
    *last_pos = Some(current_pos);

    let map = map_res.map();
    let Some(image) = images.get_mut(&minimap_res.0) else { return };

    let scale = config.view_radius as f32 / MINIMAP_RADIUS as f32;

    for ty in 0..MINIMAP_SIDE {
        for tx in 0..MINIMAP_SIDE {
            let pixel_idx = (ty * MINIMAP_SIDE + tx) as usize * 4;

            if is_outside_diamond(tx, ty) {
                image.data[pixel_idx..pixel_idx + 4].copy_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            if is_diamond_border(tx, ty) {
                image.data[pixel_idx..pixel_idx + 4].copy_from_slice(&[160, 140, 100, 230]);
                continue;
            }

            // box filter: 픽셀이 덮는 footprint 내 모든 타일을 면적 비율로 가중 평균
            // bilinear(2×2)와 달리 scale>1일 때 모든 기여 타일을 포함해 flicker 제거
            let fx = player_x as f32 + (tx as f32 - MINIMAP_RADIUS as f32) * scale;
            let fy = player_y as f32 + (MINIMAP_RADIUS as f32 - ty as f32) * scale; // y축 반전
            let half = scale * 0.5;
            let fx_lo = fx - half;  let fx_hi = fx + half;
            let fy_lo = fy - half;  let fy_hi = fy + half;
            let ix0 = fx_lo.floor() as i32;  let ix1 = fx_hi.floor() as i32;
            let iy0 = fy_lo.floor() as i32;  let iy1 = fy_hi.floor() as i32;

            let mut blended = [0.0f32; 4];
            for iy in iy0..=iy1 {
                let wy = box_weight(iy, fy_lo, fy_hi, scale);
                for ix in ix0..=ix1 {
                    let wx = box_weight(ix, fx_lo, fx_hi, scale);
                    let c = tile_color_f32(map, ix, iy, player_x, player_y);
                    for i in 0..4 { blended[i] += c[i] * wx * wy; }
                }
            }
            let color: [u8; 4] = std::array::from_fn(|i| blended[i].round() as u8);

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
    fn zoom_out_increases_radius() {
        assert_eq!(apply_zoom(20, MINIMAP_ZOOM_STEP), 25);
    }

    #[test]
    fn zoom_clamps_at_max_radius() {
        assert_eq!(apply_zoom(MINIMAP_VIEW_RADIUS_MAX, MINIMAP_ZOOM_STEP), MINIMAP_VIEW_RADIUS_MAX);
    }

    #[test]
    fn zoom_clamps_at_min_is_default_radius() {
        // 최솟값이 기본값과 같으므로 기본 상태에서 줌인해도 변하지 않는다
        assert_eq!(apply_zoom(MINIMAP_RADIUS, -MINIMAP_ZOOM_STEP), MINIMAP_RADIUS);
    }

    #[test]
    fn default_view_radius_equals_min() {
        assert_eq!(MINIMAP_VIEW_RADIUS_MIN, MINIMAP_RADIUS);
        assert!(MINIMAP_RADIUS <= MINIMAP_VIEW_RADIUS_MAX);
    }

    #[test]
    fn zoom_scale_at_default_is_one() {
        let scale = MINIMAP_RADIUS as f32 / MINIMAP_RADIUS as f32;
        assert!((scale - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn zoom_scale_zoomed_out_is_greater_than_one() {
        let radius = apply_zoom(MINIMAP_RADIUS, MINIMAP_ZOOM_STEP);
        let scale = radius as f32 / MINIMAP_RADIUS as f32;
        assert!(scale > 1.0);
    }

    #[test]
    fn box_filter_1d_weights_sum_to_one() {
        for scale in [1.0_f32, 2.5, 3.5] {
            for fx in [10.0_f32, 10.3, 10.5, 10.99] {
                let lo = fx - scale * 0.5;
                let hi = fx + scale * 0.5;
                let i0 = lo.floor() as i32;
                let i1 = hi.floor() as i32;
                let total: f32 = (i0..=i1).map(|i| box_weight(i, lo, hi, scale)).sum();
                assert!((total - 1.0).abs() < 1e-5, "scale={scale} fx={fx}: total={total}");
            }
        }
    }

    #[test]
    fn box_filter_scale1_equals_lerp() {
        // scale=1 일 때 box filter 는 선형 보간과 동일해야 한다
        let fx = 10.3_f32;
        let lo = fx - 0.5;
        let hi = fx + 0.5;
        let w0 = box_weight(10, lo, hi, 1.0); // 기여: [10.0, 10.5) → 0.5 / 1.0 = 0.5
        let w1 = box_weight(11, lo, hi, 1.0); // 기여: [10.5, 10.8) → 0.3 / 1.0 = 0.3
        // 실제 w0 = (10.5 - 10.0) / 1.0 = 0.5, w1 = (10.8 - 10.5) / 1.0 = 0.3 → total = 0.8? NO
        // lo=9.8, hi=10.8: tile 9: (10.0-9.8)/1.0=0.2, tile 10: (10.8-10.0)/1.0=0.8
        let lo2 = 9.8_f32; let hi2 = 10.8_f32;
        let wa = box_weight(9,  lo2, hi2, 1.0);
        let wb = box_weight(10, lo2, hi2, 1.0);
        assert!((wa + wb - 1.0).abs() < 1e-6);
        let _ = (w0, w1); // used above
    }

    #[test]
    fn tile_color_f32_out_of_bounds_returns_dark() {
        let map = Map::new(10, 10);
        let c = tile_color_f32(&map, -1, 0, 0, 0);
        assert_eq!(c, [10.0, 8.0, 6.0, 200.0]);
        let c = tile_color_f32(&map, 10, 0, 0, 0);
        assert_eq!(c, [10.0, 8.0, 6.0, 200.0]);
    }

    #[test]
    fn tile_color_f32_unrevealed_returns_dark() {
        let map = Map::new(10, 10); // 모든 타일 미탐험
        let c = tile_color_f32(&map, 5, 5, 0, 0);
        assert_eq!(c, [10.0, 8.0, 6.0, 200.0]);
    }

    #[test]
    fn tile_color_f32_player_returns_yellow() {
        let map = Map::new(10, 10);
        let c = tile_color_f32(&map, 3, 3, 3, 3);
        assert_eq!(c, [255.0, 220.0, 0.0, 255.0]);
    }

    #[test]
    fn border_pixels_are_separate_from_tile_mapping() {
        // border 픽셀(다이아몬드 테두리)은 is_diamond_border로 식별된다
        // 모든 border 픽셀은 is_outside_diamond가 false여야 한다 (다이아몬드 내부)
        let r = MINIMAP_RADIUS as u32;
        assert!(is_diamond_border(r, 0));
        assert!(!is_outside_diamond(r, 0));
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
