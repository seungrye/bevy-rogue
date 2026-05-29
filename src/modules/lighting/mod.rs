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
    map::{GlobalTurn, MapResource, MapSystemSet, TILE_SIZE},
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

/// 시야 안 타일의 **거리 감쇠 알파**를 결정하는 순수 함수.
///
/// 플레이어로부터의 체비쇼프 거리 `d` (8방향 격자 게임 기준)에 따라
/// `alpha = 1 / (1 + (d-1)^3)` 곡선으로 감쇠한다.
///
/// 곡선 값:
///   - d ≤ 1 → 1.0       (플레이어 자기 타일 + 바로 옆은 완전한 밝기)
///   - d = 2 → 0.5
///   - d = 3 ≈ 0.111
///   - d = 4 → 1/28 ≈ 0.0357
///   - 큰 d 는 0 에 수렴 — 시야 끝(FOV_FRONT=8)에서도 부드럽게 사라진다.
///
/// 거리 0 / 음수는 1.0 으로 본다 (플레이어 자기 타일 보호 + 방어).
pub fn distance_falloff_alpha(distance: i32) -> f32 {
    if distance <= 1 {
        return 1.0;
    }
    let d_minus_1 = (distance - 1) as f32;
    1.0 / (1.0 + d_minus_1 * d_minus_1 * d_minus_1)
}

/// 두 타일 사이의 체비쇼프 거리 (max(|dx|, |dy|)). 8방향 격자 게임의 한 걸음
/// 거리와 일치하므로 시야 거리 감쇠 곱셈 인자로 그대로 쓴다.
pub fn chebyshev_distance(a: (i32, i32), b: (i32, i32)) -> i32 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}

/// 기억 타일의 **망각 감쇠 계수** — 시간이 흐를수록 0 에 수렴해 결국 배경에 묻힌다.
///
/// 곡선: `exp(-Δturn / 200)` — 사용자 디자인(500 은 너무 느려 망각이 거의 안 보임).
///   - Δturn = 0    → 1.0        (방금 본 — 감쇠 없음)
///   - Δturn = 50   ≈ 0.7788     (살짝 어둑)
///   - Δturn = 100  ≈ 0.6065     (약 40% 어두워짐)
///   - Δturn = 200  ≈ 0.3679     (약 절반)
///   - Δturn = 400  ≈ 0.1353
///   - Δturn = 800  ≈ 0.0183     (거의 배경)
///   - Δturn → ∞    → 0.0        (배경과 완전히 동일)
pub fn memory_fade_factor(turns_since_seen: u32) -> f32 {
    (-(turns_since_seen as f32) / 200.0).exp()
}

/// 타일의 **누적 가시화 상태(brightness)** 와 마지막 본 이후 경과 턴을 받아 최종 렌더
/// 색을 결정하는 단일 순수 함수.
///
/// brightness 는 FOV 시스템이 시야 닿을 때마다 `max(brightness * memory_fade(elapsed),
/// light_factor(d))` 로 갱신한 누적값 — 망각으로 감쇠된 이전 상태와 현재 시야 강도
/// 중 큰 값. 표시는 `brightness * memory_fade(elapsed)` — visible 인 동안엔 elapsed=0
/// 이라 brightness 그대로, 시야가 빠지면 시간 따라 부드럽게 어두워진다.
///
/// 이 모델은 분기(visible/!visible) 없이 누적 brightness 단일값으로 'state stays' 의도를
/// 표현 — 망각으로 30 인 타일에 시야 falloff 10 이 닿아도 FOV 갱신 단계에서
/// `max(30, 10) = 30` 이 되어 30 유지 (이전 분기 모델의 '시야 닿는 순간 망각 정보가
/// 사라져 10 으로 덮어쓰여지는' 어색함 해소).
///
/// - 미탐험(`!visible && !revealed`): `None` — 숨김(색 갱신 안 함).
/// - 그 외: `dim(base, brightness * memory_fade(turns_since_seen))`.
///
/// `turns_since_seen=None` 은 last_seen_turn 이 아직 없는 (직렬화 호환) 케이스 —
/// 감퇴 없음으로 간주(brightness 그대로).
pub fn tile_render_color(
    base: Color,
    visible: bool,
    revealed: bool,
    brightness: f32,
    turns_since_seen: Option<u32>,
) -> Option<Color> {
    if !visible && !revealed {
        return None;
    }
    // 시야 안: FOV 가 brightness 를 max-갱신해 최신값을 보장 — 표시 단계 fade 생략.
    // 시야 밖: last_seen 이후 경과 턴으로 memory_fade 진행. None 이면 감퇴 없음.
    let fade = if visible {
        1.0
    } else {
        turns_since_seen.map(memory_fade_factor).unwrap_or(1.0)
    };
    Some(dim(base, brightness * fade))
}

/// 게임 캔버스 배경색 (Bevy 기본 ClearColor 와 같은 짙은 회색).
/// 거리 감쇠 시 타일 색을 이 배경색으로 lerp 해 자연스럽게 흐려진다.
/// (검정 곱하기로 알파만 줄이면 검은색으로 페이드되어 어색했음.)
pub(crate) const BACKGROUND_COLOR: Color = Color::rgb(0.13, 0.13, 0.13);

/// `base` 와 배경색을 `factor` 비율로 섞는다. `factor=1.0` 이면 base 그대로,
/// `factor=0.0` 이면 배경색 그대로. RGB 만 lerp 하고 알파는 base 그대로 유지한다.
///
/// 단순 `dim`(RGB 곱) 은 모든 색을 검은색으로 가게 해 어색하므로, 배경색(짙은 회색)
/// 쪽으로 lerp 해 멀어질수록 타일이 배경에 자연스럽게 녹아드는 효과를 낸다.
fn dim(base: Color, factor: f32) -> Color {
    let t = factor.clamp(0.0, 1.0);
    let bg = BACKGROUND_COLOR;
    Color::rgba(
        base.r() * t + bg.r() * (1.0 - t),
        base.g() * t + bg.g() * (1.0 - t),
        base.b() * t + bg.b() * (1.0 - t),
        base.a(),
    )
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

/// 타일 스프라이트 색을 가시성 + 광량 + 플레이어 거리 + 기억 경과 시간으로 한 번에
/// 결정해 디밍한다. 기존 map 의 `update_tile_visibility` 가 가시성/숨김을 담당하고,
/// 여기서는 타일의 누적 `brightness` 와 `last_seen_turn` 을 `tile_render_color` 에
/// 넘겨 색을 결정한다 — visible 인 동안엔 elapsed=0 이라 brightness 그대로,
/// 시야가 빠지면 매 프레임 fade 가 진행. 거리/광량 결합은 FOV 시스템에서 이미 완료.
fn apply_light_dimming(
    map_res: Res<MapResource>,
    light_map: Res<LightMap>,
    global_turn: Option<Res<GlobalTurn>>,
    mut tile_query: Query<(&crate::modules::map::TileEntity, &mut Text, &Visibility)>,
) {
    // 맵(가시성·brightness)·광량 둘 중 하나라도 바뀌어야 재적용한다.
    if !map_res.is_changed() && !light_map.is_changed() {
        return;
    }
    let map = map_res.map();
    let now_turn: Option<u32> = global_turn.as_ref().map(|t| t.0 as u32);
    for (tile, mut text, vis) in tile_query.iter_mut() {
        // 숨김 타일은 색을 건드리지 않는다(가시성은 map 시스템이 결정).
        if *vis == Visibility::Hidden {
            continue;
        }
        let idx = map.index(tile.x, tile.y);
        let base = crate::modules::map::tile_base_color(map.tiles[idx].kind);
        // Δturn = now - last_seen — 둘 중 하나라도 없으면 None(감퇴 없음).
        let turns_since_seen: Option<u32> = match (now_turn, map.tiles[idx].last_seen_turn) {
            (Some(now), Some(last)) => Some(now.saturating_sub(last)),
            _ => None,
        };
        if let Some(color) = tile_render_color(
            base,
            map.tiles[idx].visible,
            map.tiles[idx].revealed,
            map.tiles[idx].brightness,
            turns_since_seen,
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
