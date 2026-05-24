//! 조명/그림자 시스템 (specs/additional-systems.md §C).
//!
//! 광원(`LightSource`) 반경 기반으로 각 타일의 광량(`LightLevel`)을 계산한다.
//! 단일 광량 그리드(`LightMap` 리소스)를 한 경로로 계산해 **렌더(디밍)** 와
//! **탐지(가드 시야)** 양쪽이 같은 값을 쓰게 한다(일관성 규칙).
//!
//! 잠입 연계: 플레이어가 어둠에 있으면 가드 탐지 반경이 줄고(은신 보너스),
//! 밝은 곳에 있으면 기본 반경 그대로 노출된다. 보정은 순수 함수
//! `effective_vision_radius` 로 분리한다.

use bevy::prelude::*;
use crate::modules::{
    map::{MapResource, MapSystemSet, TILE_SIZE},
    player::{Player, PlayerSystemSet},
};

/// 타일의 광량 — 단순 2단계 모델(밝음/어둠).
/// 광원 반경 안(경계 포함)이면 `Bright`, 어떤 광원에도 닿지 않으면 `Dark`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightLevel {
    /// 어떤 광원의 반경에도 들지 않은 타일 — 디밍 대상, 은신 보너스.
    Dark,
    /// 하나 이상의 광원 반경 안에 든 타일 — 기본 색, 노출.
    Bright,
}

/// 광원 컴포넌트. `radius` 타일 이내(경계 포함)를 밝힌다.
/// 플레이어는 기본 시야광으로 이 컴포넌트를 보유하고, 횃불 등도 같은 컴포넌트로
/// 표현해 `light_level` 한 경로에서 일관되게 처리한다.
#[derive(Component, Debug, Clone, Copy)]
pub struct LightSource {
    /// 밝히는 반경(타일). 0 이면 자기 타일만 밝힌다.
    pub radius: i32,
}

impl LightSource {
    pub fn new(radius: i32) -> Self {
        Self { radius }
    }
}

/// 플레이어 기본 시야광 반경 — 플레이어가 든 횃불/등불 개념.
pub const PLAYER_LIGHT_RADIUS: i32 = 6;

/// 어둠 속 디밍 비율. 밝은 색을 이 비율로 곱해 어둡게 한다.
/// 기존 "탐험만 된 타일" 디밍(0.3)보다는 밝되 확실히 구분되는 0.5.
pub const DARK_DIM_FACTOR: f32 = 0.5;

/// 어둠 속 플레이어를 노리는 가드의 탐지 반경 감소량(타일).
/// 어둠 = 이만큼 가드가 가까워야만 탐지 → 은신 보너스.
pub const DARK_VISION_PENALTY: i32 = 4;

/// 한 타일의 광량을 광원 목록으로부터 계산하는 **순수 함수**.
///
/// 어떤 광원의 반경 안(`dist² <= radius²`, 경계 포함)에 들면 `Bright`,
/// 모든 광원 밖이면 `Dark`. 광원이 없으면 항상 `Dark`.
/// 거리 판정은 `tiles_in_radius`/`is_in_view` 와 동일한 원형 유클리드 기준이다.
pub fn light_level(tile: (usize, usize), sources: &[((usize, usize), i32)]) -> LightLevel {
    let (tx, ty) = (tile.0 as i32, tile.1 as i32);
    for &((sx, sy), radius) in sources {
        if radius < 0 {
            continue;
        }
        let dx = tx - sx as i32;
        let dy = ty - sy as i32;
        if dx * dx + dy * dy <= radius * radius {
            return LightLevel::Bright;
        }
    }
    LightLevel::Dark
}

/// 플레이어가 선 타일의 광량에 따라 가드의 **유효 탐지 반경**을 보정하는 순수 함수.
///
/// - 어둠(`Dark`): `base - DARK_VISION_PENALTY` (최소 0). 가드가 더 가까워야 탐지.
/// - 밝음(`Bright`): `base` 그대로 — 노출.
///
/// 렌더와 동일한 `light_level` 결과를 입력으로 받아 탐지·렌더가 같은 광량을 쓴다.
pub fn effective_vision_radius(base: i32, player_light: LightLevel) -> i32 {
    match player_light {
        LightLevel::Dark => (base - DARK_VISION_PENALTY).max(0),
        LightLevel::Bright => base,
    }
}

/// 가시성(visible/revealed)과 광량을 함께 받아 타일의 최종 렌더 색을 결정하는
/// **단일 순수 함수** — 디밍은 여기 한 곳에서만 결정한다(이중 처리/예외 분기 금지).
///
/// - 미탐험(`!visible && !revealed`): `None` — 숨김(색 갱신 안 함).
/// - 탐험만 됨(`!visible`): 기존대로 0.3 배 디밍(광량 무관 — 안 보이는 기억).
/// - 보이고 밝음: 기본 색.
/// - 보이고 어둠: `DARK_DIM_FACTOR` 배 디밍(광량 그림자).
pub fn tile_render_color(
    base: Color,
    visible: bool,
    revealed: bool,
    light: LightLevel,
) -> Option<Color> {
    if !visible && !revealed {
        return None;
    }
    if !visible {
        // 탐험만 된(기억) 타일 — 광량과 무관하게 기존 0.3 디밍 유지.
        return Some(dim(base, 0.3));
    }
    match light {
        LightLevel::Bright => Some(base),
        LightLevel::Dark => Some(dim(base, DARK_DIM_FACTOR)),
    }
}

/// 색을 균일 비율로 어둡게 만든다(알파 유지). map 모듈 `dim_color` 와 동형이나
/// 조명 디밍을 같은 규칙으로 묶기 위해 여기 둔다.
fn dim(c: Color, factor: f32) -> Color {
    Color::rgba(c.r() * factor, c.g() * factor, c.b() * factor, c.a())
}

/// 맵 전체의 광량을 보관하는 리소스 — 렌더와 탐지가 공유하는 단일 정본.
/// `update_light_map` 이 매 프레임 모든 `LightSource` 위치로부터 재계산한다.
#[derive(Resource, Default)]
pub struct LightMap {
    pub width: usize,
    pub height: usize,
    /// 행 우선(`y * width + x`) 광량 그리드.
    pub levels: Vec<LightLevel>,
}

impl LightMap {
    /// 타일 좌표의 광량을 조회한다. 범위 밖은 `Dark`(빛이 없는 것으로 간주).
    pub fn at(&self, x: usize, y: usize) -> LightLevel {
        if x >= self.width || y >= self.height {
            return LightLevel::Dark;
        }
        self.levels[y * self.width + x]
    }
}

/// 모든 `LightSource` 의 (타일좌표, 반경) 목록과 맵 크기로부터 광량 그리드를
/// 통째로 계산하는 순수 함수. `light_level` 을 타일마다 적용한다.
pub fn compute_light_levels(
    width: usize,
    height: usize,
    sources: &[((usize, usize), i32)],
) -> Vec<LightLevel> {
    let mut levels = vec![LightLevel::Dark; width * height];
    for y in 0..height {
        for x in 0..width {
            levels[y * width + x] = light_level((x, y), sources);
        }
    }
    levels
}

/// 모든 `LightSource` 엔티티의 현재 타일 위치를 모아 `LightMap` 을 재계산한다.
/// 플레이어/횃불 이동, 광원 추가·제거를 매 프레임 반영해 항상 현재 광량을 유지한다.
fn update_light_map(
    map_res: Res<MapResource>,
    mut light_map: ResMut<LightMap>,
    source_query: Query<(&Transform, &LightSource)>,
) {
    let map = map_res.map();
    let sources: Vec<((usize, usize), i32)> = source_query.iter()
        .map(|(t, ls)| (crate::modules::map::world_to_tile_coords(t.translation), ls.radius))
        .collect();
    light_map.width = map.width;
    light_map.height = map.height;
    light_map.levels = compute_light_levels(map.width, map.height, &sources);
}

/// 타일 스프라이트 색을 가시성 + 광량으로 한 번에 결정해 디밍한다(통합 디밍).
/// 기존 map 의 `update_tile_visibility` 가 가시성/숨김을 담당하고, 여기서는
/// 광량에 따른 색만 `tile_render_color` 로 일관 적용한다 — 보이는 타일에 한해.
fn apply_light_dimming(
    map_res: Res<MapResource>,
    light_map: Res<LightMap>,
    mut tile_query: Query<(&crate::modules::map::TileEntity, &mut Text, &Visibility)>,
) {
    // 맵(가시성)·광량 둘 중 하나라도 바뀌어야 재적용한다.
    if !map_res.is_changed() && !light_map.is_changed() {
        return;
    }
    let map = map_res.map();
    for (tile, mut text, vis) in tile_query.iter_mut() {
        // 숨김 타일은 색을 건드리지 않는다(가시성은 map 시스템이 결정).
        if *vis == Visibility::Hidden {
            continue;
        }
        let idx = map.index(tile.x, tile.y);
        let base = crate::modules::map::tile_base_color(map.tiles[idx].kind);
        let light = light_map.at(tile.x, tile.y);
        if let Some(color) = tile_render_color(
            base,
            map.tiles[idx].visible,
            map.tiles[idx].revealed,
            light,
        ) {
            if text.sections[0].style.color != color {
                text.sections[0].style.color = color;
            }
        }
    }
}

/// 플레이어 엔티티에 기본 시야광(`LightSource`)이 없으면 부여한다.
/// 플레이어 스폰 시점(다른 모듈)에서 컴포넌트를 추가하지 않아도, 이 모듈이
/// 자기 책임으로 기본 광원을 보장한다 — 어둠/밝음 판정의 기준점.
fn ensure_player_light(
    mut commands: Commands,
    player_query: Query<Entity, (With<Player>, Without<LightSource>)>,
) {
    for entity in player_query.iter() {
        commands.entity(entity).insert(LightSource::new(PLAYER_LIGHT_RADIUS));
    }
}

/// 횃불 광원 엔티티 마커. 맵/퀘스트가 배치하는 정적 광원을 시각화한다.
#[derive(Component)]
pub struct Torch;

/// 특정 타일에 횃불(정적 광원)을 배치하라는 요청 이벤트.
/// (선택 기능) 맵 생성기/퀘스트가 발행해 잠입 구역에 광원을 깐다.
#[derive(Event)]
pub struct SpawnTorchEvent {
    pub tile_x: usize,
    pub tile_y: usize,
    pub radius: i32,
}

/// `SpawnTorchEvent` 를 받아 해당 타일에 횃불 광원 엔티티를 스폰한다.
/// 횃불은 `LightSource` 를 가지므로 `update_light_map` 이 자동으로 광량에 반영한다.
fn handle_spawn_torch(
    mut commands: Commands,
    mut events: EventReader<SpawnTorchEvent>,
    asset_server: Res<AssetServer>,
) {
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");
    for ev in events.read() {
        let coord = crate::modules::map::tile_to_world_coords(ev.tile_x, ev.tile_y);
        commands.spawn((
            Text2dBundle {
                text: Text::from_section("†", TextStyle {
                    font: font.clone(),
                    font_size: TILE_SIZE,
                    color: Color::rgb(1.0, 0.8, 0.3),
                }),
                transform: Transform::from_xyz(coord.x, coord.y, 0.35),
                ..default()
            },
            LightSource::new(ev.radius),
            Torch,
        ));
    }
}

pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LightMap>()
            .add_event::<SpawnTorchEvent>()
            .add_systems(Update, (
                ensure_player_light,
                handle_spawn_torch,
                // 광량 계산은 플레이어 이동 완료 후, 디밍은 그 뒤에.
                update_light_map.after(PlayerSystemSet::MovementComplete),
                // 가시성(update_tile_visibility, ExecuteRegen 이후)을 깐 뒤 광량 디밍을 얹는다.
                apply_light_dimming
                    .after(update_light_map)
                    .after(MapSystemSet::ExecuteRegen),
            ));
    }
}

#[cfg(test)]
mod tests;
