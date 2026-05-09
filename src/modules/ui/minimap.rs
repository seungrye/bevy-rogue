use crate::modules::{
    map::{Map, MapGeneratorRegistry, MapResource, TileKind},
    player::Player,
    zone::{WorldState, ZoneId},
};
use bevy::{
    prelude::*,
    render::{
        render_asset::RenderAssetUsages,
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        texture::ImageSampler,
    },
};

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MarkerKind {
    QuestGiver,
    QuestTarget,
    Portal,
    StairDown,
    StairUp,
}

impl MarkerKind {
    pub fn color(&self) -> [u8; 4] {
        match self {
            MarkerKind::QuestGiver => [255, 255, 0, 255],
            MarkerKind::QuestTarget => [255, 0, 255, 255],
            MarkerKind::Portal => [0, 255, 255, 255],
            MarkerKind::StairDown => [255, 153, 0, 255],
            MarkerKind::StairUp => [128, 255, 128, 255],
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct MapMarker {
    pub tile_x: usize,
    pub tile_y: usize,
    pub kind: MarkerKind,
    pub zone: ZoneId,
}

#[derive(Resource, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiscoveredMarkers(pub Vec<MapMarker>);

impl DiscoveredMarkers {
    pub fn add(&mut self, tile_x: usize, tile_y: usize, kind: MarkerKind, zone: ZoneId) {
        let already = self
            .0
            .iter()
            .any(|m| m.tile_x == tile_x && m.tile_y == tile_y && m.kind == kind && m.zone == zone);
        if !already {
            self.0.push(MapMarker {
                tile_x,
                tile_y,
                kind,
                zone,
            });
        }
    }
}

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
    fn default() -> Self {
        Self {
            view_radius: MINIMAP_RADIUS,
        }
    }
}

#[derive(Component)]
pub struct MinimapOverlay;

#[derive(Component)]
pub(super) struct GeneratorNameText;

pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MinimapConfig>()
            .init_resource::<DiscoveredMarkers>()
            .add_systems(
                Startup,
                (setup_minimap, spawn_minimap_overlay.after(setup_minimap)),
            )
            .add_systems(
                Update,
                (
                    update_minimap,
                    toggle_minimap,
                    update_generator_name,
                    zoom_minimap,
                ),
            );
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
    commands
        .spawn((
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
        ))
        .with_children(|parent| {
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
                    TextStyle {
                        font: font.clone(),
                        font_size: 13.0,
                        color: Color::CYAN,
                    },
                ),
                GeneratorNameText,
            ));
            parent.spawn(TextBundle::from_section(
                "[F1] 맵 전환",
                TextStyle {
                    font,
                    font_size: 11.0,
                    color: Color::GRAY,
                },
            ));
        });
}

/// view_radius 를 clamp하여 반환한다 (순수 함수, 테스트 가능)
pub fn apply_zoom(current: i32, delta: i32) -> i32 {
    (current + delta).clamp(MINIMAP_VIEW_RADIUS_MIN, MINIMAP_VIEW_RADIUS_MAX)
}

fn zoom_minimap(keyboard: Res<ButtonInput<KeyCode>>, mut config: ResMut<MinimapConfig>) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !ctrl {
        return;
    }

    let zoom_in =
        keyboard.just_pressed(KeyCode::Equal) || keyboard.just_pressed(KeyCode::NumpadAdd);
    let zoom_out =
        keyboard.just_pressed(KeyCode::Minus) || keyboard.just_pressed(KeyCode::NumpadSubtract);

    // 줌인 = 반경 감소 (타일 더 크게), 줌아웃 = 반경 증가 (더 넓게)
    let delta = if zoom_in {
        -MINIMAP_ZOOM_STEP
    } else if zoom_out {
        MINIMAP_ZOOM_STEP
    } else {
        return;
    };
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

const C_TRANSPARENT: [u8; 4] = [0, 0, 0, 0];
const C_BORDER: [u8; 4] = [168, 132, 72, 255];
const C_UNEXPLORED: [u8; 4] = [7, 8, 10, 255];
const C_VISIBLE_WALL: [u8; 4] = [210, 176, 116, 255];
const C_VISIBLE_FLOOR: [u8; 4] = [92, 128, 92, 255];
const C_REVEALED_WALL: [u8; 4] = [84, 68, 48, 255];
const C_REVEALED_FLOOR: [u8; 4] = [34, 48, 42, 255];
const C_PLAYER: [u8; 4] = [255, 228, 64, 255];

/// 미니맵 픽셀 하나가 대표할 월드 타일 좌표를 반환한다.
/// 보간 없이 가장 가까운 타일 하나만 선택해 픽셀 아트처럼 또렷하게 그린다.
pub(crate) fn minimap_pixel_to_world_tile(
    player_x: usize,
    player_y: usize,
    tx: u32,
    ty: u32,
    scale: f32,
) -> (i32, i32) {
    let wx = player_x as f32 + (tx as f32 - MINIMAP_RADIUS as f32) * scale;
    let wy = player_y as f32 + (MINIMAP_RADIUS as f32 - ty as f32) * scale;
    (wx.round() as i32, wy.round() as i32)
}

/// 타일 하나의 상태를 미니맵 팔레트 색상으로 변환한다.
/// 경계 밖과 미탐험 타일은 같은 어두운 색으로 처리해 지도를 읽기 쉽게 유지한다.
pub(crate) fn tile_color(map: &Map, x: i32, y: i32, player_x: usize, player_y: usize) -> [u8; 4] {
    if x < 0 || y < 0 || x >= map.width as i32 || y >= map.height as i32 {
        return C_UNEXPLORED;
    }

    let ux = x as usize;
    let uy = y as usize;
    if ux == player_x && uy == player_y {
        return C_PLAYER;
    }

    let idx = map.index(ux, uy);
    if map.tiles[idx].visible {
        match map.tiles[idx].kind {
            TileKind::Wall => C_VISIBLE_WALL,
            TileKind::Floor => C_VISIBLE_FLOOR,
        }
    } else if map.tiles[idx].revealed {
        match map.tiles[idx].kind {
            TileKind::Wall => C_REVEALED_WALL,
            TileKind::Floor => C_REVEALED_FLOOR,
        }
    } else {
        C_UNEXPLORED
    }
}

/// 월드 타일 좌표를 미니맵 픽셀 좌표로 변환한다.
/// 화면 밖·다이아몬드 바깥·테두리 위 마커는 렌더링하지 않도록 None을 반환한다.
pub(crate) fn marker_pixel_coords(
    player_x: usize,
    player_y: usize,
    marker_x: usize,
    marker_y: usize,
    scale: f32,
) -> Option<(u32, u32)> {
    let mtx = MINIMAP_RADIUS + ((marker_x as f32 - player_x as f32) / scale).round() as i32;
    let mty = MINIMAP_RADIUS - ((marker_y as f32 - player_y as f32) / scale).round() as i32;
    if mtx < 0 || mty < 0 {
        return None;
    }

    let (mtx, mty) = (mtx as u32, mty as u32);
    if mtx >= MINIMAP_SIDE || mty >= MINIMAP_SIDE {
        return None;
    }
    if is_outside_diamond(mtx, mty) || is_diamond_border(mtx, mty) {
        return None;
    }

    Some((mtx, mty))
}

// (이전에 5 픽셀 십자 stamp 가 있었으나, 미니맵에서 너무 큰 영역을 차지해 제거.
// 색상이 충분히 구분되므로 1 픽셀로도 가독성이 충분하다.)

/// 미니맵 이미지의 단일 픽셀에 색상을 기록한다.
/// 스탬프 일부가 경계에 걸릴 때는 다이아몬드 내부 픽셀만 남긴다.
fn write_minimap_pixel(image: &mut Image, x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 {
        return;
    }

    let (x, y) = (x as u32, y as u32);
    if x >= MINIMAP_SIDE || y >= MINIMAP_SIDE {
        return;
    }
    if is_outside_diamond(x, y) || is_diamond_border(x, y) {
        return;
    }

    let pixel_idx = (y * MINIMAP_SIDE + x) as usize * 4;
    image.data[pixel_idx..pixel_idx + 4].copy_from_slice(&color);
}

fn update_minimap(
    map_res: Res<MapResource>,
    minimap_res: Res<MinimapImage>,
    mut images: ResMut<Assets<Image>>,
    player_query: Query<&Transform, With<Player>>,
    config: Res<MinimapConfig>,
    overlay_q: Query<&Visibility, With<MinimapOverlay>>,
    mut last_pos: Local<Option<IVec2>>,
    world_state: Res<WorldState>,
    markers: Res<DiscoveredMarkers>,
) {
    // 미니맵이 숨겨져 있으면 텍스처 업데이트를 건너뛴다.
    if let Ok(vis) = overlay_q.get_single() {
        if *vis == Visibility::Hidden {
            return;
        }
    }

    if map_res.is_changed() || config.is_changed() {
        *last_pos = None;
    }

    let Ok(player_transform) = player_query.get_single() else {
        return;
    };
    let (player_x, player_y) =
        crate::modules::map::world_to_tile_coords(player_transform.translation);
    let current_pos = IVec2::new(player_x as i32, player_y as i32);

    if Some(current_pos) == *last_pos && !markers.is_changed() {
        return;
    }
    *last_pos = Some(current_pos);

    let map = map_res.map();
    let Some(image) = images.get_mut(&minimap_res.0) else {
        return;
    };

    let scale = config.view_radius as f32 / MINIMAP_RADIUS as f32;

    for ty in 0..MINIMAP_SIDE {
        for tx in 0..MINIMAP_SIDE {
            let pixel_idx = (ty * MINIMAP_SIDE + tx) as usize * 4;

            if is_outside_diamond(tx, ty) {
                image.data[pixel_idx..pixel_idx + 4].copy_from_slice(&C_TRANSPARENT);
                continue;
            }
            if is_diamond_border(tx, ty) {
                image.data[pixel_idx..pixel_idx + 4].copy_from_slice(&C_BORDER);
                continue;
            }

            // 보간 없이 대표 타일 하나만 샘플링해 미니맵을 또렷한 픽셀 스타일로 유지한다.
            let (wx, wy) = minimap_pixel_to_world_tile(player_x, player_y, tx, ty, scale);
            let color = tile_color(map, wx, wy, player_x, player_y);
            image.data[pixel_idx..pixel_idx + 4].copy_from_slice(&color);
        }
    }

    // 마커는 단일 픽셀 — 색상으로 종류 구분.
    for marker in markers.0.iter().filter(|m| m.zone == world_state.current) {
        let Some((mtx, mty)) =
            marker_pixel_coords(player_x, player_y, marker.tile_x, marker.tile_y, scale)
        else {
            continue;
        };
        write_minimap_pixel(image, mtx as i32, mty as i32, marker.kind.color());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_is_inside_diamond() {
        assert!(!is_outside_diamond(
            MINIMAP_RADIUS as u32,
            MINIMAP_RADIUS as u32
        ));
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
        assert!(!is_diamond_border(
            MINIMAP_RADIUS as u32,
            MINIMAP_RADIUS as u32
        ));
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
        assert_eq!(
            apply_zoom(MINIMAP_VIEW_RADIUS_MAX, MINIMAP_ZOOM_STEP),
            MINIMAP_VIEW_RADIUS_MAX
        );
    }

    #[test]
    fn zoom_clamps_at_min_is_default_radius() {
        // 최솟값이 기본값과 같으므로 기본 상태에서 줌인해도 변하지 않는다
        assert_eq!(
            apply_zoom(MINIMAP_RADIUS, -MINIMAP_ZOOM_STEP),
            MINIMAP_RADIUS
        );
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
    fn minimap_pixel_to_world_tile_center_returns_player_tile() {
        let tile =
            minimap_pixel_to_world_tile(40, 50, MINIMAP_RADIUS as u32, MINIMAP_RADIUS as u32, 1.0);
        assert_eq!(tile, (40, 50));
    }

    #[test]
    fn minimap_pixel_to_world_tile_zoomed_out_samples_farther_tile() {
        let tile = minimap_pixel_to_world_tile(
            40,
            50,
            (MINIMAP_RADIUS + 2) as u32,
            (MINIMAP_RADIUS - 3) as u32,
            2.0,
        );
        assert_eq!(tile, (44, 56));
    }

    #[test]
    fn tile_color_out_of_bounds_returns_unexplored() {
        let map = Map::new(10, 10);
        assert_eq!(tile_color(&map, -1, 0, 0, 0), C_UNEXPLORED);
        assert_eq!(tile_color(&map, 10, 0, 0, 0), C_UNEXPLORED);
    }

    #[test]
    fn tile_color_unrevealed_returns_unexplored() {
        let map = Map::new(10, 10);
        assert_eq!(tile_color(&map, 5, 5, 0, 0), C_UNEXPLORED);
    }

    #[test]
    fn tile_color_player_returns_yellow() {
        let map = Map::new(10, 10);
        assert_eq!(tile_color(&map, 3, 3, 3, 3), C_PLAYER);
    }

    #[test]
    fn tile_color_visible_wall_and_floor_are_distinct() {
        let mut map = Map::new(10, 10);
        map.set_tile(4, 4, TileKind::Wall);
        map.set_tile(5, 5, TileKind::Floor);
        let wall_idx = map.index(4, 4);
        let floor_idx = map.index(5, 5);
        map.tiles[wall_idx].visible = true;
        map.tiles[floor_idx].visible = true;

        assert_eq!(tile_color(&map, 4, 4, 0, 0), C_VISIBLE_WALL);
        assert_eq!(tile_color(&map, 5, 5, 0, 0), C_VISIBLE_FLOOR);
        assert_ne!(C_VISIBLE_WALL, C_VISIBLE_FLOOR);
    }

    #[test]
    fn tile_color_revealed_wall_and_floor_are_distinct() {
        let mut map = Map::new(10, 10);
        map.set_tile(4, 4, TileKind::Wall);
        map.set_tile(5, 5, TileKind::Floor);
        let wall_idx = map.index(4, 4);
        let floor_idx = map.index(5, 5);
        map.tiles[wall_idx].revealed = true;
        map.tiles[floor_idx].revealed = true;

        assert_eq!(tile_color(&map, 4, 4, 0, 0), C_REVEALED_WALL);
        assert_eq!(tile_color(&map, 5, 5, 0, 0), C_REVEALED_FLOOR);
        assert_ne!(C_REVEALED_WALL, C_REVEALED_FLOOR);
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

    #[test]
    fn discovered_markers_no_duplicate() {
        let mut dm = DiscoveredMarkers::default();
        dm.add(5, 5, MarkerKind::Portal, ZoneId::Town);
        dm.add(5, 5, MarkerKind::Portal, ZoneId::Town);
        assert_eq!(
            dm.0.len(),
            1,
            "동일 위치·종류의 마커는 중복 추가되지 않아야 한다"
        );
    }

    #[test]
    fn discovered_markers_different_kind_allowed() {
        let mut dm = DiscoveredMarkers::default();
        dm.add(5, 5, MarkerKind::Portal, ZoneId::Town);
        dm.add(5, 5, MarkerKind::QuestGiver, ZoneId::Town);
        dm.add(5, 5, MarkerKind::QuestTarget, ZoneId::Town);
        assert_eq!(dm.0.len(), 3);
    }

    #[test]
    fn discovered_markers_different_zone_allowed() {
        let mut dm = DiscoveredMarkers::default();
        dm.add(5, 5, MarkerKind::Portal, ZoneId::Town);
        dm.add(5, 5, MarkerKind::Portal, ZoneId::Forest);
        assert_eq!(dm.0.len(), 2);
    }

    #[test]
    fn quest_target_marker_uses_distinct_color() {
        assert_ne!(
            MarkerKind::QuestTarget.color(),
            MarkerKind::QuestGiver.color()
        );
        assert_eq!(MarkerKind::QuestTarget.color(), [255, 0, 255, 255]);
    }

    #[test]
    fn marker_pixel_coords_in_range_for_center_tile() {
        // 플레이어와 동일 위치 마커는 미니맵 정중앙 픽셀에 놓인다.
        assert_eq!(
            marker_pixel_coords(40, 50, 40, 50, 1.0),
            Some((MINIMAP_RADIUS as u32, MINIMAP_RADIUS as u32)),
        );
    }

    #[test]
    fn marker_pixel_coords_outside_range_returns_none() {
        assert_eq!(marker_pixel_coords(40, 50, 0, 50, 1.0), None);
    }

    #[test]
    fn marker_pixel_at_player_position_is_center() {
        // 마커가 플레이어 위치에 있으면 미니맵 중앙 픽셀이 된다 — 1 픽셀 마커
        let center = MINIMAP_RADIUS as u32;
        assert_eq!(
            marker_pixel_coords(40, 50, 40, 50, 1.0),
            Some((center, center))
        );
    }
}
