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
    /// 동적으로 위치가 갱신될 수 있는 entity (예: NPC) 식별자.
    /// 같은 actor 의 마커가 이미 있으면 위치만 갱신된다.
    /// None 이면 정적 마커 (포털, 아이템 등) — add() 가 위치 중복만 검사.
    #[serde(default)]
    pub actor: Option<String>,
}

#[derive(Resource, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiscoveredMarkers(pub Vec<MapMarker>);

impl DiscoveredMarkers {
    /// 정적 마커 추가 — 같은 위치/종류/존이 이미 있으면 추가하지 않는다 (idempotent).
    pub fn add(&mut self, tile_x: usize, tile_y: usize, kind: MarkerKind, zone: ZoneId) {
        let already = self
            .0
            .iter()
            .any(|m| m.tile_x == tile_x && m.tile_y == tile_y && m.kind == kind && m.zone == zone);
        if !already {
            self.0.push(MapMarker { tile_x, tile_y, kind, zone, actor: None });
        }
    }

    /// 지정 위치/종류/존의 마커를 제거한다 (예: 퀘스트 아이템 획득 시).
    pub fn remove_at(&mut self, tile_x: usize, tile_y: usize, kind: MarkerKind, zone: &ZoneId) {
        self.0.retain(|m| !(m.tile_x == tile_x && m.tile_y == tile_y && m.kind == kind && m.zone == *zone));
    }

    /// 지정 actor 의 마커를 제거한다 (예: 퀘스트 종료/활성 해제 시).
    pub fn remove_actor(&mut self, actor: &str, kind: MarkerKind, zone: &ZoneId) {
        self.0.retain(|m| !(m.actor.as_deref() == Some(actor) && m.kind == kind && m.zone == *zone));
    }

    /// 동적 actor 마커의 위치를 갱신한다.
    /// 같은 (actor, kind, zone) 가 있으면 위치만 변경, 없으면 새로 추가한다.
    /// 이동 NPC 가 시야에 들어왔을 때 사용한다.
    pub fn update_actor_position(&mut self, actor: &str, kind: MarkerKind, zone: ZoneId, tile_x: usize, tile_y: usize) {
        if let Some(m) = self.0.iter_mut()
            .find(|m| m.actor.as_deref() == Some(actor) && m.kind == kind && m.zone == zone)
        {
            m.tile_x = tile_x;
            m.tile_y = tile_y;
        } else {
            self.0.push(MapMarker { tile_x, tile_y, kind, zone, actor: Some(actor.to_string()) });
        }
    }
}

pub const MINIMAP_RADIUS: i32 = 20;
pub const MINIMAP_SIDE: u32 = (MINIMAP_RADIUS * 2 + 1) as u32;
pub const MINIMAP_DISPLAY_SIZE: f32 = 180.0;

#[derive(Resource)]
pub struct MinimapImage(pub Handle<Image>);

#[derive(Component)]
pub struct MinimapOverlay;

#[derive(Component)]
pub(super) struct GeneratorNameText;

pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DiscoveredMarkers>()
            .init_resource::<FullMapOpen>()
            .add_systems(
                Startup,
                (
                    setup_minimap,
                    spawn_minimap_overlay.after(setup_minimap),
                    setup_full_map,
                    spawn_full_map_overlay.after(setup_full_map),
                ),
            )
            .add_systems(
                Update,
                (
                    update_minimap,
                    update_generator_name,
                    toggle_full_map,
                    update_full_map_visibility.after(toggle_full_map),
                    update_full_map_image.after(update_full_map_visibility),
                ),
            );
    }
}

// ── 전체화면 미니맵 ──────────────────────────────────────────────────────────

const FULL_MAP_W: u32 = crate::modules::map::MAP_WIDTH as u32;
const FULL_MAP_H: u32 = crate::modules::map::MAP_HEIGHT as u32;
/// 화면에서 차지할 크기 — 한 타일이 8px 으로 그려져 80x50 → 640x400
const FULL_MAP_DISPLAY_SCALE: f32 = 8.0;

#[derive(Resource)]
pub struct FullMapImage(pub Handle<Image>);

#[derive(Resource, Default)]
pub struct FullMapOpen(pub bool);

#[derive(Component)]
pub struct FullMapPanel;

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

fn setup_full_map(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let extent = Extent3d {
        width: FULL_MAP_W,
        height: FULL_MAP_H,
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
    commands.insert_resource(FullMapImage(handle));
}

fn spawn_full_map_overlay(
    mut commands: Commands,
    full_map: Res<FullMapImage>,
    asset_server: Res<AssetServer>,
) {
    let font = asset_server.load("fonts/NanumSquareNeo-bRg.ttf");
    let display_w = FULL_MAP_W as f32 * FULL_MAP_DISPLAY_SCALE;
    let display_h = FULL_MAP_H as f32 * FULL_MAP_DISPLAY_SCALE;
    commands
        .spawn((
            NodeBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(8.0),
                    ..default()
                },
                background_color: Color::rgba(0.0, 0.0, 0.0, 0.85).into(),
                z_index: ZIndex::Global(150),
                visibility: Visibility::Hidden,
                ..default()
            },
            FullMapPanel,
        ))
        .with_children(|parent| {
            parent.spawn(ImageBundle {
                style: Style {
                    width: Val::Px(display_w),
                    height: Val::Px(display_h),
                    ..default()
                },
                image: full_map.0.clone().into(),
                ..default()
            });
            parent.spawn(TextBundle::from_section(
                "[M] 닫기",
                TextStyle { font, font_size: 14.0, color: Color::GRAY },
            ));
        });
}

fn toggle_full_map(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut open: ResMut<FullMapOpen>,
    defeated_q: Query<(), With<crate::modules::combat::Defeated>>,
) {
    if !defeated_q.is_empty() { return; }
    // M — 전체 미니맵. 작은 미니맵은 항상 표시, M 은 큰 지도 toggle 전용.
    if keyboard.just_pressed(KeyCode::KeyM) {
        open.0 = !open.0;
    }
}

fn update_full_map_visibility(
    open: Res<FullMapOpen>,
    mut panel_q: Query<&mut Visibility, With<FullMapPanel>>,
) {
    if !open.is_changed() { return; }
    let Ok(mut vis) = panel_q.get_single_mut() else { return; };
    *vis = if open.0 { Visibility::Inherited } else { Visibility::Hidden };
}

fn update_full_map_image(
    open: Res<FullMapOpen>,
    map_res: Res<MapResource>,
    full_map: Res<FullMapImage>,
    mut images: ResMut<Assets<Image>>,
    player_query: Query<&Transform, With<Player>>,
    world_state: Res<WorldState>,
    markers: Res<DiscoveredMarkers>,
) {
    if !open.0 { return; }
    let Ok(player_transform) = player_query.get_single() else { return; };
    let (player_x, player_y) =
        crate::modules::map::world_to_tile_coords(player_transform.translation);
    let map = map_res.map();
    let Some(image) = images.get_mut(&full_map.0) else { return; };

    // 1) 타일 색칠 — 발견된 영역만.
    // 게임 좌표는 좌하단 원점(y 증가가 위), 이미지는 좌상단 원점이므로 y 를 뒤집는다.
    for ty in 0..FULL_MAP_H {
        for x in 0..FULL_MAP_W {
            let pixel_y = FULL_MAP_H - 1 - ty;
            let pixel_idx = (pixel_y * FULL_MAP_W + x) as usize * 4;
            let color = full_map_tile_color(map, x as i32, ty as i32, player_x, player_y);
            image.data[pixel_idx..pixel_idx + 4].copy_from_slice(&color);
        }
    }

    // 2) 마커 — 1 픽셀
    for marker in markers.0.iter().filter(|m| m.zone == world_state.current) {
        let (mx, my) = (marker.tile_x, marker.tile_y);
        if mx >= FULL_MAP_W as usize || my >= FULL_MAP_H as usize { continue; }
        let pixel_y = FULL_MAP_H as usize - 1 - my;
        let pixel_idx = (pixel_y as u32 * FULL_MAP_W + mx as u32) as usize * 4;
        image.data[pixel_idx..pixel_idx + 4].copy_from_slice(&marker.kind.color());
    }
}

/// 전체화면 미니맵 픽셀 색상 — 발견 안 된 곳은 완전 검정 (투명 X — 패널 자체가 어두움)
pub(crate) fn full_map_tile_color(map: &Map, x: i32, y: i32, player_x: usize, player_y: usize) -> [u8; 4] {
    if x < 0 || y < 0 || x >= map.width as i32 || y >= map.height as i32 {
        return [0, 0, 0, 255];
    }
    let ux = x as usize;
    let uy = y as usize;
    if ux == player_x && uy == player_y {
        return C_PLAYER;
    }
    let idx = map.index(ux, uy);
    if !map.tiles[idx].revealed {
        return [0, 0, 0, 255];  // 미탐험 — 검정
    }
    if map.tiles[idx].visible {
        match map.tiles[idx].kind {
            TileKind::Wall => C_VISIBLE_WALL,
            TileKind::Floor => C_VISIBLE_FLOOR,
            TileKind::Water => C_VISIBLE_WATER,
            TileKind::Sand => C_VISIBLE_SAND,
        }
    } else {
        match map.tiles[idx].kind {
            TileKind::Wall => C_REVEALED_WALL,
            TileKind::Floor => C_REVEALED_FLOOR,
            TileKind::Water => C_REVEALED_WATER,
            TileKind::Sand => C_REVEALED_SAND,
        }
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
const C_VISIBLE_WATER: [u8; 4] = [64, 128, 230, 255];
const C_VISIBLE_SAND: [u8; 4] = [216, 200, 128, 255];
const C_REVEALED_WALL: [u8; 4] = [84, 68, 48, 255];
const C_REVEALED_FLOOR: [u8; 4] = [34, 48, 42, 255];
const C_REVEALED_WATER: [u8; 4] = [26, 51, 92, 255];
const C_REVEALED_SAND: [u8; 4] = [86, 80, 51, 255];
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
            TileKind::Water => C_VISIBLE_WATER,
            TileKind::Sand => C_VISIBLE_SAND,
        }
    } else if map.tiles[idx].revealed {
        match map.tiles[idx].kind {
            TileKind::Wall => C_REVEALED_WALL,
            TileKind::Floor => C_REVEALED_FLOOR,
            TileKind::Water => C_REVEALED_WATER,
            TileKind::Sand => C_REVEALED_SAND,
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

    if map_res.is_changed() {
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

    // zoom 제거 — 항상 1:1 매핑 (한 픽셀 == 한 타일)
    let scale = 1.0_f32;

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
    use crate::modules::map::{tile_to_world_coords, MapResource};

    // ── AssetServer/이미지가 필요한 렌더 시스템용 App 하네스 ──
    fn 렌더_하네스() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app.init_asset::<Image>();
        app
    }

    /// 게임 좌표계에서 (tx, ty) 타일 중심에 놓이는 플레이어 Transform 을 만든다.
    fn 플레이어_트랜스폼(tx: usize, ty: usize) -> Transform {
        Transform::from_translation(tile_to_world_coords(tx, ty).extend(0.0))
    }

    /// 테스트용 빈 미니맵 이미지(MINIMAP_SIDE x MINIMAP_SIDE, Rgba8).
    fn 빈_미니맵_이미지() -> Image {
        Image::new_fill(
            Extent3d { width: MINIMAP_SIDE, height: MINIMAP_SIDE, ..default() },
            TextureDimension::D2,
            &[0, 0, 0, 0],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::MAIN_WORLD,
        )
    }

    fn 픽셀(image: &Image, x: u32, y: u32) -> [u8; 4] {
        let idx = (y * MINIMAP_SIDE + x) as usize * 4;
        [image.data[idx], image.data[idx + 1], image.data[idx + 2], image.data[idx + 3]]
    }

    // ── 다이아몬드 형태 판정 ──────────────────────────────────────────────

    #[test]
    fn 다이아몬드_중심점은_내부로_판정된다() {
        assert!(!is_outside_diamond(MINIMAP_RADIUS as u32, MINIMAP_RADIUS as u32));
    }

    #[test]
    fn 다이아몬드의_네_모서리는_바깥으로_판정된다() {
        assert!(is_outside_diamond(0, 0));
        assert!(is_outside_diamond(MINIMAP_SIDE - 1, 0));
        assert!(is_outside_diamond(0, MINIMAP_SIDE - 1));
        assert!(is_outside_diamond(MINIMAP_SIDE - 1, MINIMAP_SIDE - 1));
    }

    #[test]
    fn 상하좌우_끝점은_다이아몬드_바깥이_아니다() {
        let r = MINIMAP_RADIUS as u32;
        assert!(!is_outside_diamond(r, 0));
        assert!(!is_outside_diamond(r, MINIMAP_SIDE - 1));
        assert!(!is_outside_diamond(0, r));
        assert!(!is_outside_diamond(MINIMAP_SIDE - 1, r));
    }

    #[test]
    fn 상하좌우_끝점은_다이아몬드_테두리에_놓인다() {
        let r = MINIMAP_RADIUS as u32;
        assert!(is_diamond_border(r, 0));
        assert!(is_diamond_border(r, MINIMAP_SIDE - 1));
        assert!(is_diamond_border(0, r));
        assert!(is_diamond_border(MINIMAP_SIDE - 1, r));
    }

    #[test]
    fn 다이아몬드_중심점은_테두리가_아니다() {
        assert!(!is_diamond_border(MINIMAP_RADIUS as u32, MINIMAP_RADIUS as u32));
    }

    #[test]
    fn 테두리_픽셀은_다이아몬드_내부에_속한다() {
        // 테두리 픽셀은 is_diamond_border 가 참이면서 is_outside_diamond 는 거짓이어야 한다.
        let r = MINIMAP_RADIUS as u32;
        assert!(is_diamond_border(r, 0));
        assert!(!is_outside_diamond(r, 0));
    }

    #[test]
    fn 미니맵_표시크기와_변길이는_양수다() {
        assert!(MINIMAP_DISPLAY_SIZE > 0.0);
        assert!(MINIMAP_SIDE > 0);
    }

    // ── 픽셀↔타일 좌표 변환 ───────────────────────────────────────────────

    #[test]
    fn 미니맵_중앙픽셀은_플레이어_타일을_가리킨다() {
        let tile =
            minimap_pixel_to_world_tile(40, 50, MINIMAP_RADIUS as u32, MINIMAP_RADIUS as u32, 1.0);
        assert_eq!(tile, (40, 50));
    }

    #[test]
    fn 줌아웃하면_미니맵_픽셀은_더_먼_타일을_샘플링한다() {
        let tile = minimap_pixel_to_world_tile(
            40,
            50,
            (MINIMAP_RADIUS + 2) as u32,
            (MINIMAP_RADIUS - 3) as u32,
            2.0,
        );
        assert_eq!(tile, (44, 56));
    }

    // ── tile_color ───────────────────────────────────────────────────────

    #[test]
    fn 미니맵_범위밖_타일은_미탐험색이다() {
        let map = Map::new(10, 10);
        assert_eq!(tile_color(&map, -1, 0, 0, 0), C_UNEXPLORED);
        assert_eq!(tile_color(&map, 10, 0, 0, 0), C_UNEXPLORED);
    }

    #[test]
    fn 미니맵의_세로_범위밖_타일도_미탐험색이다() {
        // x 분기뿐 아니라 y 경계 분기도 양쪽으로 도달시킨다.
        let map = Map::new(10, 10);
        assert_eq!(tile_color(&map, 0, -1, 0, 0), C_UNEXPLORED);
        assert_eq!(tile_color(&map, 0, 10, 0, 0), C_UNEXPLORED);
    }

    #[test]
    fn 미니맵에서_미탐험_타일은_미탐험색이다() {
        let map = Map::new(10, 10);
        assert_eq!(tile_color(&map, 5, 5, 0, 0), C_UNEXPLORED);
    }

    #[test]
    fn 미니맵에서_플레이어_타일은_노란색이다() {
        let map = Map::new(10, 10);
        assert_eq!(tile_color(&map, 3, 3, 3, 3), C_PLAYER);
    }

    #[test]
    fn 미니맵에서_보이는_벽과_바닥은_색이_구분된다() {
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
    fn 미니맵에서_탐험된_벽과_바닥은_색이_구분된다() {
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
    fn 미니맵은_물과_모래에_고유한_색을_매핑한다() {
        let mut map = Map::new(10, 10);
        map.set_tile(2, 2, TileKind::Water);
        map.set_tile(3, 3, TileKind::Sand);
        let water_idx = map.index(2, 2);
        let sand_idx = map.index(3, 3);

        // 보이는 상태
        map.tiles[water_idx].visible = true;
        map.tiles[sand_idx].visible = true;
        assert_eq!(tile_color(&map, 2, 2, 0, 0), C_VISIBLE_WATER);
        assert_eq!(tile_color(&map, 3, 3, 0, 0), C_VISIBLE_SAND);

        // 탐험만 된 상태
        map.tiles[water_idx].visible = false;
        map.tiles[sand_idx].visible = false;
        map.tiles[water_idx].revealed = true;
        map.tiles[sand_idx].revealed = true;
        assert_eq!(tile_color(&map, 2, 2, 0, 0), C_REVEALED_WATER);
        assert_eq!(tile_color(&map, 3, 3, 0, 0), C_REVEALED_SAND);

        // 다른 타일들과 색이 구분돼야 한다
        assert_ne!(C_VISIBLE_WATER, C_VISIBLE_SAND);
        assert_ne!(C_VISIBLE_WATER, C_VISIBLE_FLOOR);
        assert_ne!(C_VISIBLE_SAND, C_VISIBLE_WALL);
    }

    #[test]
    fn 생성기_힌트_폰트크기는_미니맵보다_작다() {
        let name_font_size: f32 = 13.0;
        let hint_font_size: f32 = 11.0;
        assert!(name_font_size < MINIMAP_DISPLAY_SIZE);
        assert!(hint_font_size < name_font_size);
    }

    #[test]
    fn 전체맵_열림상태는_토글하면_뒤집힌다() {
        let mut open = FullMapOpen(false);
        open.0 = !open.0;
        assert!(open.0);
        open.0 = !open.0;
        assert!(!open.0);
    }

    // ── DiscoveredMarkers ────────────────────────────────────────────────

    #[test]
    fn 동일_위치종류존_마커는_중복으로_추가되지_않는다() {
        let mut dm = DiscoveredMarkers::default();
        dm.add(5, 5, MarkerKind::Portal, ZoneId::Town);
        dm.add(5, 5, MarkerKind::Portal, ZoneId::Town);
        assert_eq!(dm.0.len(), 1, "동일 위치·종류의 마커는 중복 추가되지 않아야 한다");
    }

    #[test]
    fn add는_같은_x라도_y가_다르면_각각_추가된다() {
        // add() 의 중복검사 클로저에서 tile_y 비교가 거짓이 되는 경로.
        let mut dm = DiscoveredMarkers::default();
        dm.add(5, 5, MarkerKind::Portal, ZoneId::Town);
        dm.add(5, 6, MarkerKind::Portal, ZoneId::Town);
        assert_eq!(dm.0.len(), 2);
    }

    #[test]
    fn remove_at은_같은_x라도_y가_다르면_제거하지_않는다() {
        // remove_at retain 클로저에서 tile_y 비교가 거짓이 되는 경로.
        let mut dm = DiscoveredMarkers::default();
        dm.add(5, 5, MarkerKind::Portal, ZoneId::Town);
        dm.remove_at(5, 9, MarkerKind::Portal, &ZoneId::Town);
        assert_eq!(dm.0.len(), 1, "y 가 다르면 남아 있어야 한다");
    }

    #[test]
    fn remove_actor는_actor가_같아도_종류가_다르면_제거하지_않는다() {
        // remove_actor retain 클로저에서 actor 는 일치하지만 kind 비교가 거짓이 되는 경로.
        let mut dm = DiscoveredMarkers::default();
        dm.update_actor_position("엘렌", MarkerKind::QuestGiver, ZoneId::Town, 5, 5);
        dm.remove_actor("엘렌", MarkerKind::QuestTarget, &ZoneId::Town);
        assert_eq!(dm.0.len(), 1, "종류가 다르면 남아 있어야 한다");
    }

    #[test]
    fn remove_actor는_actor도_종류도_같으면_제거한다() {
        // remove_actor retain 클로저의 kind 비교가 참이 되는 경로.
        let mut dm = DiscoveredMarkers::default();
        dm.update_actor_position("엘렌", MarkerKind::QuestGiver, ZoneId::Town, 5, 5);
        dm.remove_actor("엘렌", MarkerKind::QuestGiver, &ZoneId::Town);
        assert!(dm.0.is_empty(), "actor·종류·존이 모두 같으면 제거돼야 한다");
    }

    #[test]
    fn update_actor_position은_actor가_같아도_종류가_다르면_새_마커를_추가한다() {
        // find 클로저에서 actor 는 일치하지만 kind 비교가 거짓이 되어 새 마커가 추가되는 경로.
        let mut dm = DiscoveredMarkers::default();
        dm.update_actor_position("엘렌", MarkerKind::QuestGiver, ZoneId::Town, 5, 5);
        dm.update_actor_position("엘렌", MarkerKind::QuestTarget, ZoneId::Town, 9, 9);
        assert_eq!(dm.0.len(), 2, "같은 actor 라도 종류가 다르면 별도 마커");
    }

    #[test]
    fn 같은_위치라도_종류가_다르면_각각_추가된다() {
        let mut dm = DiscoveredMarkers::default();
        dm.add(5, 5, MarkerKind::Portal, ZoneId::Town);
        dm.add(5, 5, MarkerKind::QuestGiver, ZoneId::Town);
        dm.add(5, 5, MarkerKind::QuestTarget, ZoneId::Town);
        assert_eq!(dm.0.len(), 3);
    }

    #[test]
    fn 같은_위치종류라도_존이_다르면_각각_추가된다() {
        let mut dm = DiscoveredMarkers::default();
        dm.add(5, 5, MarkerKind::Portal, ZoneId::Town);
        dm.add(5, 5, MarkerKind::Portal, ZoneId::Forest);
        assert_eq!(dm.0.len(), 2);
    }

    #[test]
    fn remove_at은_위치종류가_정확히_일치하는_마커만_제거한다() {
        let mut dm = DiscoveredMarkers::default();
        dm.add(5, 5, MarkerKind::QuestTarget, ZoneId::Town);
        dm.add(5, 5, MarkerKind::QuestGiver, ZoneId::Town);
        dm.add(7, 7, MarkerKind::QuestTarget, ZoneId::Town);
        dm.remove_at(5, 5, MarkerKind::QuestTarget, &ZoneId::Town);
        assert_eq!(dm.0.len(), 2);
        assert!(!dm.0.iter().any(|m| m.tile_x == 5 && m.tile_y == 5 && m.kind == MarkerKind::QuestTarget));
        assert!(dm.0.iter().any(|m| m.tile_x == 5 && m.tile_y == 5 && m.kind == MarkerKind::QuestGiver));
    }

    #[test]
    fn update_actor_position은_같은_actor의_위치만_갱신한다() {
        let mut dm = DiscoveredMarkers::default();
        dm.update_actor_position("엘렌", MarkerKind::QuestGiver, ZoneId::Town, 5, 5);
        assert_eq!(dm.0.len(), 1);
        dm.update_actor_position("엘렌", MarkerKind::QuestGiver, ZoneId::Town, 7, 8);
        assert_eq!(dm.0.len(), 1, "같은 actor 는 위치만 갱신, 새 마커 추가 안 됨");
        assert_eq!(dm.0[0].tile_x, 7);
        assert_eq!(dm.0[0].tile_y, 8);
    }

    #[test]
    fn update_actor_position은_다른_actor면_새_마커를_추가한다() {
        let mut dm = DiscoveredMarkers::default();
        dm.update_actor_position("엘렌", MarkerKind::QuestGiver, ZoneId::Town, 5, 5);
        dm.update_actor_position("장로", MarkerKind::QuestGiver, ZoneId::Town, 8, 8);
        assert_eq!(dm.0.len(), 2);
    }

    #[test]
    fn remove_actor는_지정한_actor의_마커만_제거한다() {
        let mut dm = DiscoveredMarkers::default();
        dm.update_actor_position("엘렌", MarkerKind::QuestGiver, ZoneId::Town, 5, 5);
        dm.update_actor_position("장로", MarkerKind::QuestGiver, ZoneId::Town, 8, 8);
        dm.remove_actor("엘렌", MarkerKind::QuestGiver, &ZoneId::Town);
        assert_eq!(dm.0.len(), 1);
        assert_eq!(dm.0[0].actor.as_deref(), Some("장로"));
    }

    // ── full_map_tile_color ──────────────────────────────────────────────

    #[test]
    fn 전체맵에서_미탐험_타일은_검정이다() {
        let map = Map::new(10, 10);
        let c = full_map_tile_color(&map, 3, 3, 5, 5);
        assert_eq!(c, [0, 0, 0, 255]);
    }

    #[test]
    fn 전체맵에서_플레이어_타일은_플레이어색이다() {
        let map = Map::new(10, 10);
        let c = full_map_tile_color(&map, 5, 5, 5, 5);
        assert_eq!(c, C_PLAYER);
    }

    #[test]
    fn 전체맵에서_탐험된_바닥과_벽은_색이_구분된다() {
        let mut map = Map::new(10, 10);
        let f_idx = map.index(3, 3);
        map.tiles[f_idx].kind = TileKind::Floor;
        map.tiles[f_idx].revealed = true;
        let w_idx = map.index(4, 3);
        map.tiles[w_idx].revealed = true; // wall + revealed
        let cf = full_map_tile_color(&map, 3, 3, 0, 0);
        let cw = full_map_tile_color(&map, 4, 3, 0, 0);
        assert_ne!(cf, cw);
        assert_eq!(cf, C_REVEALED_FLOOR);
        assert_eq!(cw, C_REVEALED_WALL);
    }

    #[test]
    fn 전체맵에서_탐험된_물과_모래는_고유한_색이다() {
        let mut map = Map::new(10, 10);
        map.set_tile(2, 2, TileKind::Water);
        map.set_tile(3, 3, TileKind::Sand);
        let wi = map.index(2, 2);
        let si = map.index(3, 3);
        map.tiles[wi].revealed = true;
        map.tiles[si].revealed = true;
        assert_eq!(full_map_tile_color(&map, 2, 2, 0, 0), C_REVEALED_WATER);
        assert_eq!(full_map_tile_color(&map, 3, 3, 0, 0), C_REVEALED_SAND);
    }

    #[test]
    fn 전체맵에서_보이는_타일은_네_지형색을_모두_가진다() {
        // visible 분기의 Wall/Floor/Water/Sand arm 을 전부 도달시킨다.
        let mut map = Map::new(10, 10);
        let kinds = [
            (1, 1, TileKind::Wall, C_VISIBLE_WALL),
            (2, 2, TileKind::Floor, C_VISIBLE_FLOOR),
            (3, 3, TileKind::Water, C_VISIBLE_WATER),
            (4, 4, TileKind::Sand, C_VISIBLE_SAND),
        ];
        for &(x, y, kind, _) in &kinds {
            map.set_tile(x, y, kind);
            let idx = map.index(x, y);
            // full_map_tile_color 는 revealed 가 아니면 검정을 먼저 반환하므로 둘 다 켠다.
            map.tiles[idx].revealed = true;
            map.tiles[idx].visible = true;
        }
        for &(x, y, _, expected) in &kinds {
            assert_eq!(full_map_tile_color(&map, x as i32, y as i32, 0, 0), expected);
        }
    }

    #[test]
    fn 전체맵에서_범위밖_타일은_검정이다() {
        let map = Map::new(10, 10);
        // x<0, y<0, x>=width, y>=height 네 경계 분기를 모두 도달시킨다.
        assert_eq!(full_map_tile_color(&map, -1, 5, 0, 0), [0, 0, 0, 255]);
        assert_eq!(full_map_tile_color(&map, 5, -1, 0, 0), [0, 0, 0, 255]);
        assert_eq!(full_map_tile_color(&map, 10, 5, 0, 0), [0, 0, 0, 255]);
        assert_eq!(full_map_tile_color(&map, 5, 10, 0, 0), [0, 0, 0, 255]);
    }

    // ── MapMarker / MarkerKind ───────────────────────────────────────────

    #[test]
    fn actor필드가_없는_과거_마커_데이터도_파싱된다() {
        // actor 필드 없는 기존 저장 데이터 호환성 (#[serde(default)])
        let legacy = r#"(tile_x: 5, tile_y: 7, kind: Portal, zone: Town)"#;
        let parsed: MapMarker = ron::de::from_str(legacy).expect("legacy 파싱 성공");
        assert_eq!(parsed.tile_x, 5);
        assert_eq!(parsed.tile_y, 7);
        assert!(parsed.actor.is_none());
    }

    #[test]
    fn 퀘스트목표_마커는_퀘스트제공_마커와_색이_다르다() {
        assert_ne!(MarkerKind::QuestTarget.color(), MarkerKind::QuestGiver.color());
        assert_eq!(MarkerKind::QuestTarget.color(), [255, 0, 255, 255]);
    }

    #[test]
    fn 모든_마커종류는_서로_다른_고유색을_가진다() {
        // color() 의 다섯 arm 을 모두 도달시키고 색이 전부 다른지 확인한다.
        let colors = [
            MarkerKind::QuestGiver.color(),
            MarkerKind::QuestTarget.color(),
            MarkerKind::Portal.color(),
            MarkerKind::StairDown.color(),
            MarkerKind::StairUp.color(),
        ];
        assert_eq!(colors[0], [255, 255, 0, 255]);
        assert_eq!(colors[1], [255, 0, 255, 255]);
        assert_eq!(colors[2], [0, 255, 255, 255]);
        assert_eq!(colors[3], [255, 153, 0, 255]);
        assert_eq!(colors[4], [128, 255, 128, 255]);
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j], "마커 색은 서로 구분돼야 한다");
            }
        }
    }

    // ── marker_pixel_coords ──────────────────────────────────────────────

    #[test]
    fn 플레이어와_같은_타일_마커는_미니맵_중앙픽셀이다() {
        assert_eq!(
            marker_pixel_coords(40, 50, 40, 50, 1.0),
            Some((MINIMAP_RADIUS as u32, MINIMAP_RADIUS as u32)),
        );
    }

    #[test]
    fn 가로로_화면밖에_있는_마커는_좌표가_없다() {
        // mtx 가 음수가 되어 None.
        assert_eq!(marker_pixel_coords(40, 50, 0, 50, 1.0), None);
    }

    #[test]
    fn 세로로_화면밖에_있는_마커는_좌표가_없다() {
        // mty 가 음수가 되어 None — 위쪽 경계 분기.
        assert_eq!(marker_pixel_coords(40, 50, 40, 0, 1.0), None);
    }

    #[test]
    fn 미니맵_변길이를_넘는_위치의_마커는_좌표가_없다() {
        // mtx/mty 가 MINIMAP_SIDE 이상이 되는 분기 (오른쪽/아래쪽 경계).
        let far = (MINIMAP_RADIUS as usize) + (MINIMAP_SIDE as usize) + 5;
        assert_eq!(marker_pixel_coords(0, 100, far, 100, 1.0), None);
        assert_eq!(marker_pixel_coords(100, 0, 100, far, 1.0), None);
    }

    #[test]
    fn 다이아몬드_바깥에_떨어지는_마커는_좌표가_없다() {
        // 범위 안이지만 다이아몬드 밖이라 None — is_outside_diamond 분기.
        // 플레이어로부터 대각선으로 RADIUS 보다 멀리 떨어진 위치.
        let r = MINIMAP_RADIUS as usize;
        assert_eq!(marker_pixel_coords(40, 50, 40 + r, 50 + r, 1.0), None);
    }

    #[test]
    fn 다이아몬드_테두리에_정확히_놓이는_마커는_좌표가_없다() {
        // is_outside_diamond 는 거짓이지만 is_diamond_border 가 참이라 None 인 분기.
        // mtx = RADIUS + (mx-px), mty = RADIUS. 오른쪽 끝점(테두리)이 되도록 mx = px + RADIUS.
        let r = MINIMAP_RADIUS as usize;
        let (px, py) = (40, 50);
        assert!(is_diamond_border((MINIMAP_RADIUS + MINIMAP_RADIUS) as u32, MINIMAP_RADIUS as u32));
        assert_eq!(marker_pixel_coords(px, py, px + r, py, 1.0), None);
    }

    // ── write_minimap_pixel ──────────────────────────────────────────────

    #[test]
    fn 미니맵_내부_픽셀쓰기는_해당_픽셀을_색칠한다() {
        let mut image = 빈_미니맵_이미지();
        let c = MarkerKind::Portal.color();
        let center = MINIMAP_RADIUS as i32;
        write_minimap_pixel(&mut image, center, center, c);
        assert_eq!(픽셀(&image, center as u32, center as u32), c);
    }

    #[test]
    fn 음수좌표_픽셀쓰기는_무시된다() {
        let mut image = 빈_미니맵_이미지();
        write_minimap_pixel(&mut image, -1, 5, [9, 9, 9, 9]);
        write_minimap_pixel(&mut image, 5, -1, [9, 9, 9, 9]);
        // 아무 픽셀도 바뀌지 않아야 한다.
        assert!(image.data.iter().all(|&b| b == 0));
    }

    #[test]
    fn 변길이를_넘는_좌표_픽셀쓰기는_무시된다() {
        let mut image = 빈_미니맵_이미지();
        write_minimap_pixel(&mut image, MINIMAP_SIDE as i32, 5, [9, 9, 9, 9]);
        write_minimap_pixel(&mut image, 5, MINIMAP_SIDE as i32, [9, 9, 9, 9]);
        assert!(image.data.iter().all(|&b| b == 0));
    }

    #[test]
    fn 다이아몬드_바깥과_테두리_픽셀쓰기는_무시된다() {
        let mut image = 빈_미니맵_이미지();
        // 모서리(바깥) + 끝점(테두리)
        write_minimap_pixel(&mut image, 0, 0, [9, 9, 9, 9]);
        write_minimap_pixel(&mut image, MINIMAP_RADIUS, 0, [9, 9, 9, 9]);
        assert!(image.data.iter().all(|&b| b == 0));
    }

    // ── setup/spawn 렌더 시스템 (App 하네스) ─────────────────────────────

    #[test]
    fn 미니맵_셋업은_미니맵이미지_리소스를_삽입한다() {
        let mut app = 렌더_하네스();
        app.add_systems(Startup, setup_minimap);
        app.update();
        let res = app.world.resource::<MinimapImage>();
        let images = app.world.resource::<Assets<Image>>();
        let img = images.get(&res.0).expect("미니맵 이미지 존재");
        assert_eq!(img.texture_descriptor.size.width, MINIMAP_SIDE);
        assert_eq!(img.texture_descriptor.size.height, MINIMAP_SIDE);
    }

    #[test]
    fn 전체맵_셋업은_전체맵이미지_리소스를_삽입한다() {
        let mut app = 렌더_하네스();
        app.add_systems(Startup, setup_full_map);
        app.update();
        let res = app.world.resource::<FullMapImage>();
        let images = app.world.resource::<Assets<Image>>();
        let img = images.get(&res.0).expect("전체맵 이미지 존재");
        assert_eq!(img.texture_descriptor.size.width, FULL_MAP_W);
        assert_eq!(img.texture_descriptor.size.height, FULL_MAP_H);
    }

    #[test]
    fn 미니맵_오버레이_스폰은_오버레이와_생성기이름_노드를_만든다() {
        let mut app = 렌더_하네스();
        app.insert_resource(MapGeneratorRegistry::new());
        app.add_systems(Startup, (setup_minimap, spawn_minimap_overlay.after(setup_minimap)));
        app.update();
        assert_eq!(app.world.query::<&MinimapOverlay>().iter(&app.world).count(), 1);
        assert_eq!(app.world.query::<&GeneratorNameText>().iter(&app.world).count(), 1);
    }

    #[test]
    fn 전체맵_오버레이_스폰은_숨김상태의_패널을_만든다() {
        let mut app = 렌더_하네스();
        app.add_systems(Startup, (setup_full_map, spawn_full_map_overlay.after(setup_full_map)));
        app.update();
        let mut q = app.world.query::<(&FullMapPanel, &Visibility)>();
        let (_, vis) = q.single(&app.world);
        assert_eq!(*vis, Visibility::Hidden);
    }

    // ── toggle_full_map ──────────────────────────────────────────────────

    fn 토글_하네스() -> App {
        let mut app = App::new();
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app.init_resource::<FullMapOpen>();
        app.add_systems(Update, toggle_full_map);
        app
    }

    #[test]
    fn M키를_누르면_전체맵_열림상태가_뒤집힌다() {
        let mut app = 토글_하네스();
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyM);
        app.update();
        assert!(app.world.resource::<FullMapOpen>().0);
    }

    #[test]
    fn M키를_누르지_않으면_전체맵_열림상태는_유지된다() {
        let mut app = 토글_하네스();
        app.update();
        assert!(!app.world.resource::<FullMapOpen>().0);
    }

    #[test]
    fn 플레이어가_쓰러진_상태면_M키는_전체맵을_열지_않는다() {
        let mut app = 토글_하네스();
        app.world.spawn(crate::modules::combat::Defeated);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyM);
        app.update();
        assert!(!app.world.resource::<FullMapOpen>().0, "쓰러진 동안에는 토글되지 않아야 한다");
    }

    // ── update_full_map_visibility ───────────────────────────────────────

    fn 가시성_하네스(open: bool) -> App {
        let mut app = App::new();
        app.insert_resource(FullMapOpen(open));
        app.world.spawn((FullMapPanel, Visibility::Hidden));
        app.add_systems(Update, update_full_map_visibility);
        app
    }

    #[test]
    fn 전체맵이_열리면_패널_가시성이_표시로_바뀐다() {
        let mut app = 가시성_하네스(true);
        app.update();
        let mut q = app.world.query::<(&FullMapPanel, &Visibility)>();
        assert_eq!(*q.single(&app.world).1, Visibility::Inherited);
    }

    #[test]
    fn 전체맵이_닫히면_패널_가시성이_숨김으로_바뀐다() {
        let mut app = 가시성_하네스(false);
        // 변경 감지 트리거: 리소스를 한 번 건드린다.
        app.world.resource_mut::<FullMapOpen>().0 = false;
        app.update();
        let mut q = app.world.query::<(&FullMapPanel, &Visibility)>();
        assert_eq!(*q.single(&app.world).1, Visibility::Hidden);
    }

    #[test]
    fn 전체맵_열림상태가_변하지_않으면_가시성을_갱신하지_않는다() {
        let mut app = 가시성_하네스(true);
        app.update(); // 첫 update 에서 Inherited 로 변경
        // 두 번째 update 전에 패널을 숨김으로 직접 바꿔두고, open 은 그대로 둔다.
        {
            let mut q = app.world.query::<(&FullMapPanel, &mut Visibility)>();
            let mut e = q.iter_mut(&mut app.world);
            let (_, mut vis) = e.next().unwrap();
            *vis = Visibility::Hidden;
        }
        app.update(); // open 이 안 변했으므로 시스템은 일찍 return → 숨김 유지
        let mut q = app.world.query::<(&FullMapPanel, &Visibility)>();
        assert_eq!(*q.single(&app.world).1, Visibility::Hidden);
    }

    #[test]
    fn 패널이_없으면_가시성_갱신은_조용히_넘어간다() {
        // 도달 가능: 패널 엔티티가 아직 스폰되지 않은 프레임.
        let mut app = App::new();
        app.insert_resource(FullMapOpen(true));
        app.add_systems(Update, update_full_map_visibility);
        app.update(); // panic 없이 통과해야 한다
    }

    // ── update_generator_name ────────────────────────────────────────────

    #[test]
    fn 생성기가_바뀌면_생성기이름_텍스트가_갱신된다() {
        let mut app = 렌더_하네스();
        app.insert_resource(MapGeneratorRegistry::new());
        app.world.spawn((
            TextBundle::from_section("초기값", TextStyle::default()),
            GeneratorNameText,
        ));
        app.add_systems(Update, update_generator_name);
        app.update();
        let mut q = app.world.query_filtered::<&Text, With<GeneratorNameText>>();
        // 빈 레지스트리의 current_name() 은 "없음".
        assert_eq!(q.single(&app.world).sections[0].value, "없음");
    }

    #[test]
    fn 이름텍스트_엔티티가_없으면_생성기이름_갱신은_조용히_넘어간다() {
        // registry 는 막 삽입되어 is_changed 참이지만 GeneratorNameText 엔티티가 없는 경우.
        // get_single_mut() 이 Err → 내부 if let 의 Err 분기.
        let mut app = 렌더_하네스();
        app.insert_resource(MapGeneratorRegistry::new());
        app.add_systems(Update, update_generator_name);
        app.update(); // panic 없이 통과해야 한다
    }

    #[test]
    fn 생성기가_바뀌지_않은_프레임에는_이름텍스트를_건드리지_않는다() {
        let mut app = 렌더_하네스();
        app.insert_resource(MapGeneratorRegistry::new());
        app.world.spawn((
            TextBundle::from_section("그대로", TextStyle::default()),
            GeneratorNameText,
        ));
        app.add_systems(Update, update_generator_name);
        app.update(); // 첫 프레임: is_changed → 갱신
        // 텍스트를 다시 표식값으로 바꿔두고, 레지스트리는 건드리지 않는다.
        {
            let mut q = app.world.query_filtered::<&mut Text, With<GeneratorNameText>>();
            q.single_mut(&mut app.world).sections[0].value = "표식".into();
        }
        app.update(); // is_changed 거짓 → 시스템이 건드리지 않음
        let mut q = app.world.query_filtered::<&Text, With<GeneratorNameText>>();
        assert_eq!(q.single(&app.world).sections[0].value, "표식");
    }

    // ── update_minimap ───────────────────────────────────────────────────

    /// 미니맵 갱신 시스템에 필요한 리소스/엔티티를 모두 갖춘 App.
    fn 미니맵갱신_하네스(player_tile: (usize, usize)) -> App {
        let mut app = 렌더_하네스();
        app.add_systems(Startup, setup_minimap);
        app.update(); // MinimapImage 삽입
        let mut map = Map::new(crate::modules::map::MAP_WIDTH, crate::modules::map::MAP_HEIGHT);
        // 플레이어 주변 일부를 보이게/탐험되게 설정해 색칠 분기를 다양하게 탄다.
        let (px, py) = player_tile;
        map.set_tile(px + 1, py, TileKind::Floor);
        let idx = map.index(px + 1, py);
        map.tiles[idx].visible = true;
        map.set_tile(px, py + 1, TileKind::Wall);
        let idx2 = map.index(px, py + 1);
        map.tiles[idx2].revealed = true;
        app.insert_resource(MapResource(map));
        app.init_resource::<WorldState>();
        app.init_resource::<DiscoveredMarkers>();
        app.world.spawn((Player, 플레이어_트랜스폼(px, py)));
        app.add_systems(Update, update_minimap);
        app
    }

    #[test]
    fn 미니맵_갱신은_중앙에_플레이어_픽셀을_그린다() {
        let mut app = 미니맵갱신_하네스((40, 25));
        app.update();
        let res = app.world.resource::<MinimapImage>().0.clone();
        let images = app.world.resource::<Assets<Image>>();
        let img = images.get(&res).unwrap();
        let c = MINIMAP_RADIUS as u32;
        assert_eq!(픽셀(img, c, c), C_PLAYER, "중앙 픽셀은 플레이어색이어야 한다");
    }

    #[test]
    fn 미니맵_갱신은_현재존_마커를_단일픽셀로_그린다() {
        let mut app = 미니맵갱신_하네스((40, 25));
        // 플레이어 바로 옆(다이아몬드 내부)에 포털 마커를 둔다.
        app.world.resource_mut::<DiscoveredMarkers>()
            .add(41, 25, MarkerKind::Portal, ZoneId::Town);
        app.update();
        let res = app.world.resource::<MinimapImage>().0.clone();
        let images = app.world.resource::<Assets<Image>>();
        let img = images.get(&res).unwrap();
        let Some((mx, my)) = marker_pixel_coords(40, 25, 41, 25, 1.0) else {
            panic!("마커가 미니맵 범위 안에 있어야 한다");
        };
        assert_eq!(픽셀(img, mx, my), MarkerKind::Portal.color());
    }

    #[test]
    fn 다른_존의_마커는_미니맵에_그려지지_않는다() {
        let mut app = 미니맵갱신_하네스((40, 25));
        // 현재 존은 Town. Forest 마커는 무시되어야 한다.
        app.world.resource_mut::<DiscoveredMarkers>()
            .add(41, 25, MarkerKind::Portal, ZoneId::Forest);
        app.update();
        let res = app.world.resource::<MinimapImage>().0.clone();
        let images = app.world.resource::<Assets<Image>>();
        let img = images.get(&res).unwrap();
        let (mx, my) = marker_pixel_coords(40, 25, 41, 25, 1.0).unwrap();
        // 옆 타일은 미탐험이라 마커색이 아니라 C_UNEXPLORED 여야 한다.
        assert_ne!(픽셀(img, mx, my), MarkerKind::Portal.color());
    }

    #[test]
    fn 다이아몬드_범위밖의_마커는_미니맵에_그려지지_않는다() {
        // marker_pixel_coords 가 None 을 반환해 continue 하는 경로.
        let mut app = 미니맵갱신_하네스((40, 25));
        // 플레이어로부터 RADIUS 보다 훨씬 먼 마커 (화면 밖).
        app.world.resource_mut::<DiscoveredMarkers>()
            .add(0, 25, MarkerKind::Portal, ZoneId::Town);
        app.update(); // panic 없이 통과 (continue 분기)
        assert!(marker_pixel_coords(40, 25, 0, 25, 1.0).is_none());
    }

    #[test]
    fn 미니맵이_숨김상태면_텍스처를_갱신하지_않는다() {
        let mut app = 미니맵갱신_하네스((40, 25));
        // 숨김 오버레이 추가.
        app.world.spawn((MinimapOverlay, Visibility::Hidden));
        // 이미지를 표식 바이트로 채워두고, 시스템이 건드리지 않았는지 확인.
        let res = app.world.resource::<MinimapImage>().0.clone();
        {
            let mut images = app.world.resource_mut::<Assets<Image>>();
            let img = images.get_mut(&res).unwrap();
            for b in img.data.iter_mut() { *b = 7; }
        }
        app.update();
        let images = app.world.resource::<Assets<Image>>();
        let img = images.get(&res).unwrap();
        assert!(img.data.iter().all(|&b| b == 7), "숨김 상태면 텍스처가 그대로여야 한다");
    }

    #[test]
    fn 미니맵_오버레이가_보이는_상태면_텍스처를_갱신한다() {
        // 오버레이가 존재하고 Hidden 이 아니면 숨김 분기를 통과해 정상 갱신해야 한다.
        let mut app = 미니맵갱신_하네스((40, 25));
        app.world.spawn((MinimapOverlay, Visibility::Inherited));
        app.update();
        let res = app.world.resource::<MinimapImage>().0.clone();
        let images = app.world.resource::<Assets<Image>>();
        let img = images.get(&res).unwrap();
        let c = MINIMAP_RADIUS as u32;
        assert_eq!(픽셀(img, c, c), C_PLAYER, "보이는 상태면 정상 갱신돼야 한다");
    }

    #[test]
    fn 미니맵_이미지_핸들이_무효면_조용히_넘어간다() {
        // 도달 가능한 방어 분기: MinimapImage 가 가리키는 에셋이 제거된 경우.
        let mut app = 미니맵갱신_하네스((40, 25));
        let res = app.world.resource::<MinimapImage>().0.clone();
        app.world.resource_mut::<Assets<Image>>().remove(&res);
        app.update(); // images.get_mut 이 None → 일찍 return, panic 없음
    }

    #[test]
    fn 플레이어가_없으면_미니맵_갱신은_조용히_넘어간다() {
        let mut app = 렌더_하네스();
        app.add_systems(Startup, setup_minimap);
        app.update();
        app.insert_resource(MapResource(Map::new(
            crate::modules::map::MAP_WIDTH,
            crate::modules::map::MAP_HEIGHT,
        )));
        app.init_resource::<WorldState>();
        app.init_resource::<DiscoveredMarkers>();
        app.add_systems(Update, update_minimap);
        app.update(); // 플레이어 쿼리 실패 → 일찍 return, panic 없음
    }

    #[test]
    fn 위치가_같아도_마커가_바뀌면_미니맵을_다시_그린다() {
        // 535 행: Some(current_pos) == last_pos 는 참이지만 마커가 바뀌어
        // !markers.is_changed() 가 거짓 → early return 하지 않고 다시 그린다.
        let mut app = 미니맵갱신_하네스((40, 25));
        app.update(); // 첫 갱신: last_pos 설정
        // 같은 위치에서 마커 추가 (markers is_changed) 후 텍스처를 표식으로 덮어둔다.
        app.world.resource_mut::<DiscoveredMarkers>()
            .add(41, 25, MarkerKind::Portal, ZoneId::Town);
        let res = app.world.resource::<MinimapImage>().0.clone();
        {
            let mut images = app.world.resource_mut::<Assets<Image>>();
            for b in images.get_mut(&res).unwrap().data.iter_mut() { *b = 9; }
        }
        app.update(); // 마커 변경 감지 → 다시 그림
        let (mx, my) = marker_pixel_coords(40, 25, 41, 25, 1.0).unwrap();
        let images = app.world.resource::<Assets<Image>>();
        let img = images.get(&res).unwrap();
        assert_eq!(픽셀(img, mx, my), MarkerKind::Portal.color(), "마커가 새로 그려져야 한다");
    }

    #[test]
    fn 플레이어가_같은_타일에_머물면_미니맵을_다시_그리지_않는다() {
        let mut app = 미니맵갱신_하네스((40, 25));
        app.update(); // 첫 갱신 (last_pos 설정)
        // 텍스처를 표식으로 덮어쓰고, 같은 위치에서 다시 update.
        let res = app.world.resource::<MinimapImage>().0.clone();
        {
            let mut images = app.world.resource_mut::<Assets<Image>>();
            for b in images.get_mut(&res).unwrap().data.iter_mut() { *b = 3; }
        }
        app.update(); // 위치 동일 + 마커 불변 → 일찍 return
        let images = app.world.resource::<Assets<Image>>();
        assert!(images.get(&res).unwrap().data.iter().all(|&b| b == 3));
    }

    // ── update_full_map_image ────────────────────────────────────────────

    fn 전체맵갱신_하네스(open: bool, player_tile: (usize, usize)) -> App {
        let mut app = 렌더_하네스();
        app.add_systems(Startup, setup_full_map);
        app.update(); // FullMapImage 삽입
        app.insert_resource(FullMapOpen(open));
        let mut map = Map::new(crate::modules::map::MAP_WIDTH, crate::modules::map::MAP_HEIGHT);
        let (px, py) = player_tile;
        let idx = map.index(px + 1, py);
        map.set_tile(px + 1, py, TileKind::Floor);
        map.tiles[idx].revealed = true;
        app.insert_resource(MapResource(map));
        app.init_resource::<WorldState>();
        app.init_resource::<DiscoveredMarkers>();
        app.world.spawn((Player, 플레이어_트랜스폼(px, py)));
        app.add_systems(Update, update_full_map_image);
        app
    }

    #[test]
    fn 전체맵이_열려있으면_플레이어_픽셀을_그린다() {
        let (px, py) = (40, 25);
        let mut app = 전체맵갱신_하네스(true, (px, py));
        app.update();
        let res = app.world.resource::<FullMapImage>().0.clone();
        let images = app.world.resource::<Assets<Image>>();
        let img = images.get(&res).unwrap();
        let pixel_y = FULL_MAP_H as usize - 1 - py;
        let idx = (pixel_y * FULL_MAP_W as usize + px) * 4;
        assert_eq!(&img.data[idx..idx + 4], &C_PLAYER);
    }

    #[test]
    fn 전체맵이_열려있으면_현재존_마커를_그린다() {
        let (px, py) = (40, 25);
        let mut app = 전체맵갱신_하네스(true, (px, py));
        app.world.resource_mut::<DiscoveredMarkers>()
            .add(10, 10, MarkerKind::StairDown, ZoneId::Town);
        app.update();
        let res = app.world.resource::<FullMapImage>().0.clone();
        let images = app.world.resource::<Assets<Image>>();
        let img = images.get(&res).unwrap();
        let pixel_y = FULL_MAP_H as usize - 1 - 10;
        let idx = (pixel_y * FULL_MAP_W as usize + 10) * 4;
        assert_eq!(&img.data[idx..idx + 4], &MarkerKind::StairDown.color());
    }

    #[test]
    fn 전체맵의_범위밖_마커는_무시된다() {
        // marker.tile_x/y 가 FULL_MAP 범위를 넘으면 continue.
        let mut app = 전체맵갱신_하네스(true, (40, 25));
        app.world.resource_mut::<DiscoveredMarkers>().0.push(MapMarker {
            tile_x: FULL_MAP_W as usize + 10,
            tile_y: 5,
            kind: MarkerKind::Portal,
            zone: ZoneId::Town,
            actor: None,
        });
        app.world.resource_mut::<DiscoveredMarkers>().0.push(MapMarker {
            tile_x: 5,
            tile_y: FULL_MAP_H as usize + 10,
            kind: MarkerKind::Portal,
            zone: ZoneId::Town,
            actor: None,
        });
        app.update(); // panic 없이 통과 (범위 검사 continue)
    }

    #[test]
    fn 전체맵이_닫혀있으면_이미지를_갱신하지_않는다() {
        let mut app = 전체맵갱신_하네스(false, (40, 25));
        let res = app.world.resource::<FullMapImage>().0.clone();
        {
            let mut images = app.world.resource_mut::<Assets<Image>>();
            for b in images.get_mut(&res).unwrap().data.iter_mut() { *b = 5; }
        }
        app.update(); // open.0 == false → 일찍 return
        let images = app.world.resource::<Assets<Image>>();
        assert!(images.get(&res).unwrap().data.iter().all(|&b| b == 5));
    }

    #[test]
    fn 전체맵_이미지_핸들이_무효면_조용히_넘어간다() {
        // 도달 가능한 방어 분기: FullMapImage 가 가리키는 에셋이 제거된 경우.
        let mut app = 전체맵갱신_하네스(true, (40, 25));
        let res = app.world.resource::<FullMapImage>().0.clone();
        app.world.resource_mut::<Assets<Image>>().remove(&res);
        app.update(); // images.get_mut 이 None → 일찍 return, panic 없음
    }

    #[test]
    fn 전체맵에서_플레이어가_없으면_조용히_넘어간다() {
        let mut app = 렌더_하네스();
        app.add_systems(Startup, setup_full_map);
        app.update();
        app.insert_resource(FullMapOpen(true));
        app.insert_resource(MapResource(Map::new(
            crate::modules::map::MAP_WIDTH,
            crate::modules::map::MAP_HEIGHT,
        )));
        app.init_resource::<WorldState>();
        app.init_resource::<DiscoveredMarkers>();
        app.add_systems(Update, update_full_map_image);
        app.update(); // 플레이어 쿼리 실패 → 일찍 return
    }

    // ── 플러그인 빌드 ─────────────────────────────────────────────────────

    #[test]
    fn 미니맵_플러그인이_정상적으로_빌드된다() {
        let mut app = App::new();
        app.add_plugins(MinimapPlugin);
        assert!(app.world.get_resource::<DiscoveredMarkers>().is_some());
        assert!(app.world.get_resource::<FullMapOpen>().is_some());
    }
}
