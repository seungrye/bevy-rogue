#![allow(non_snake_case)]
use super::*;
use crate::modules::map::{Map, MapResource, TileEntity, TileKind, tile_base_color, tile_to_world_coords};

// ── light_level (순수 광량 계산) ───────────────────────────────────────────────

#[test]
fn 광원_반경_안의_타일은_밝다() {
    // 광원(5,5) 반경 3 — (7,5)는 거리 2 로 반경 안.
    let sources = [((5usize, 5usize), 3i32)];
    assert_eq!(light_level((7, 5), &sources), LightLevel::Bright);
}

#[test]
fn 광원_반경_밖의_타일은_어둡다() {
    // 광원(5,5) 반경 3 — (10,5)는 거리 5 로 반경 밖.
    let sources = [((5usize, 5usize), 3i32)];
    assert_eq!(light_level((10, 5), &sources), LightLevel::Dark);
}

#[test]
fn 광원_반경_경계의_타일은_밝음에_포함된다() {
    // 거리² == 반경²(경계 포함) — (5,5) 반경 3, (8,5)는 거리 3.
    let sources = [((5usize, 5usize), 3i32)];
    assert_eq!(light_level((8, 5), &sources), LightLevel::Bright, "경계는 밝음에 포함");
    // 경계 바로 바깥(거리 4)은 어둠.
    assert_eq!(light_level((9, 5), &sources), LightLevel::Dark, "경계 한 칸 밖은 어둠");
}

#[test]
fn 광원_자기_타일은_반경0이라도_밝다() {
    // 반경 0 이면 dist²(0) <= 0 으로 자기 타일만 밝다.
    let sources = [((5usize, 5usize), 0i32)];
    assert_eq!(light_level((5, 5), &sources), LightLevel::Bright);
    assert_eq!(light_level((6, 5), &sources), LightLevel::Dark);
}

#[test]
fn 광원이_없으면_모든_타일은_어둡다() {
    let sources: [((usize, usize), i32); 0] = [];
    assert_eq!(light_level((3, 3), &sources), LightLevel::Dark);
}

#[test]
fn 여러_광원_중_하나라도_닿으면_타일은_밝다() {
    // (0,0) 반경 1 과 (20,20) 반경 2. (21,20)은 둘째 광원 반경 안.
    let sources = [((0usize, 0usize), 1i32), ((20usize, 20usize), 2i32)];
    assert_eq!(light_level((21, 20), &sources), LightLevel::Bright);
    // 두 광원 모두에서 먼 (10,10)은 어둠.
    assert_eq!(light_level((10, 10), &sources), LightLevel::Dark);
}

#[test]
fn 음수_반경_광원은_무시되어_밝히지_않는다() {
    // 방어: 음수 반경은 건너뛴다 → 해당 광원만 있으면 어둠.
    let sources = [((5usize, 5usize), -1i32)];
    assert_eq!(light_level((5, 5), &sources), LightLevel::Dark);
}

// ── effective_vision_radius (어둠 은신 보정) ──────────────────────────────────

#[test]
fn 어둠속_플레이어는_가드_탐지반경이_줄어든다() {
    // base 8, Dark → 8 - DARK_VISION_PENALTY(4) = 4.
    assert_eq!(effective_vision_radius(8, LightLevel::Dark), 8 - DARK_VISION_PENALTY);
    assert_eq!(effective_vision_radius(8, LightLevel::Dark), 4);
}

#[test]
fn 밝은곳_플레이어는_가드_탐지반경이_그대로_노출된다() {
    assert_eq!(effective_vision_radius(8, LightLevel::Bright), 8);
}

#[test]
fn 탐지반경_보정은_0_밑으로_내려가지_않는다() {
    // base 가 페널티보다 작아도 음수가 되지 않고 0 으로 클램프.
    assert_eq!(effective_vision_radius(2, LightLevel::Dark), 0);
    assert_eq!(effective_vision_radius(DARK_VISION_PENALTY, LightLevel::Dark), 0);
}

// ── tile_render_color (통합 디밍 색 결정) ─────────────────────────────────────

#[test]
fn 미탐험_타일은_색을_정하지_않는다() {
    let base = tile_base_color(TileKind::Floor);
    // distance 인자(d=0 / d=5) 와 turns_since_seen 과 무관하게 미탐험은 항상 None.
    assert_eq!(render_color_legacy(base, false, false, LightLevel::Bright, 0, None), None);
    assert_eq!(render_color_legacy(base, false, false, LightLevel::Bright, 5, None), None);
    assert_eq!(render_color_legacy(base, false, false, LightLevel::Bright, 5, Some(100)), None);
}

/// 기대 r — 클램프 없는 순수 lerp.
/// `lerp = base*factor + bg*(1-factor)` (bg=0.13).
fn expected_dim_r(base_r: f32, factor: f32) -> f32 {
    base_r * factor + 0.13 * (1.0 - factor)
}

/// 망각 분기와 동일 — 가독성용 별칭.
fn expected_lerp_r(base_r: f32, factor: f32) -> f32 {
    expected_dim_r(base_r, factor)
}

/// 테스트 어댑터 — 기존 (light, distance) 시그니처를 새 brightness state 모델로 매핑.
/// FOV 시스템이 갱신하는 brightness 의 값을 인자로부터 직접 계산해 넘긴다.
///
/// - 미탐험: brightness 무관(0.0).
/// - 시야 안: brightness = falloff(d)  (Bright) 또는 DARK_DIM_FACTOR × falloff(d)  (Dark).
/// - 망각(시야 밖 + revealed): brightness = 0.3 (기존 망각 cap 의 의미를 보존).
fn render_color_legacy(
    base: Color,
    visible: bool,
    revealed: bool,
    light: LightLevel,
    distance: i32,
    turns_since_seen: Option<u32>,
) -> Option<Color> {
    let brightness = if !visible {
        0.3
    } else {
        let falloff = distance_falloff_alpha(distance);
        match light {
            LightLevel::Bright => falloff,
            LightLevel::Dark => DARK_DIM_FACTOR * falloff,
        }
    };
    tile_render_color(base, visible, revealed, brightness, turns_since_seen)
}

#[test]
fn 탐험만_된_타일은_광량과_거리와_무관하게_0_3factor로_배경_lerp된다() {
    let base = Color::rgb(1.0, 1.0, 1.0);
    // turns_since_seen=None — 기존 동작(감퇴 없음).
    let dark   = render_color_legacy(base, false, true, LightLevel::Dark,   0, None).unwrap();
    let bright = render_color_legacy(base, false, true, LightLevel::Bright, 0, None).unwrap();
    let far    = render_color_legacy(base, false, true, LightLevel::Bright, 7, None).unwrap();
    // visible=false 면 광량·거리 무관 — 세 결과가 같고 0.3 factor 로 배경 lerp.
    assert_eq!(dark, bright, "기억 타일은 광량과 무관");
    assert_eq!(bright, far, "기억 타일은 거리와도 무관");
    assert!((dark.r() - expected_lerp_r(1.0, 0.3)).abs() < 1e-6, "기억 타일은 0.3 factor 배경 lerp");
}

#[test]
fn 보이고_밝은_타일은_거리1이면_기본색_그대로다() {
    // d=1 → falloff 1.0 → 밝음 분기는 base 그대로.
    let base = tile_base_color(TileKind::Water);
    assert_eq!(render_color_legacy(base, true, true, LightLevel::Bright, 1, None), Some(base));
}

#[test]
fn 보이지만_어두운_타일은_거리1이면_DARK_DIM_FACTOR로_배경_lerp된다() {
    // d=1 → falloff 1.0 → 어둠 분기는 factor = DARK_DIM_FACTOR(0.5) × 1.0 = 0.5
    let base = Color::rgb(1.0, 1.0, 1.0);
    let dimmed = render_color_legacy(base, true, true, LightLevel::Dark, 1, None).unwrap();
    let expected = expected_dim_r(1.0, DARK_DIM_FACTOR);
    assert!((dimmed.r() - expected).abs() < 1e-6, "어둠 d=1 은 DARK_DIM_FACTOR 로 배경 lerp");
    // factor<1 인 lerp 는 base 보다 어둡거나 같다(배경 쪽으로 lerp).
    assert!(dimmed.r() < base.r(), "lerp 결과는 base 보다 어둡다(배경에 가까움)");
}

#[test]
fn 디밍은_알파값을_보존한다() {
    let base = Color::rgba(0.8, 0.6, 0.4, 0.5);
    let dimmed = render_color_legacy(base, true, true, LightLevel::Dark, 1, None).unwrap();
    assert!((dimmed.a() - 0.5).abs() < 1e-6, "알파는 유지된다");
}

// ── distance_falloff_alpha / 시야 거리 감쇠 곡선 ──────────────────────────────

#[test]
fn 거리감쇠_d_1은_알파_1_0으로_완전한_밝기다() {
    // 사용자 곡선: d=1 → 1.0 (100%).
    assert!((distance_falloff_alpha(1) - 1.0).abs() < 1e-6);
}

#[test]
fn 거리감쇠_d_2는_알파_0_5로_절반이다() {
    // 1 / (1 + 1^3) = 0.5.
    assert!((distance_falloff_alpha(2) - 0.5).abs() < 1e-6);
}

#[test]
fn 거리감쇠_d_3은_알파_약_0_111이다() {
    // 1 / (1 + 2^3) = 1/9 ≈ 0.1111.
    let a = distance_falloff_alpha(3);
    assert!((a - (1.0 / 9.0)).abs() < 1e-6, "d=3 은 1/9 ≈ 0.111");
}

#[test]
fn 거리감쇠_d_0_이하는_방어적으로_알파_1_0이다() {
    // 플레이어 자기 타일(d=0) 과 방어적 음수 입력.
    assert!((distance_falloff_alpha(0)  - 1.0).abs() < 1e-6, "d=0 은 1.0");
    assert!((distance_falloff_alpha(-3) - 1.0).abs() < 1e-6, "음수 d 는 1.0");
}

#[test]
fn 거리감쇠는_큰_거리에서_0에_수렴한다() {
    // FOV_FRONT(8) 이상에서도 부드럽게 0 에 수렴.
    let a8 = distance_falloff_alpha(8);
    assert!(a8 > 0.0 && a8 < 0.01, "d=8 은 0 근처(0.003 수준)");
    let a20 = distance_falloff_alpha(20);
    assert!(a20 < a8, "거리가 커질수록 더 작아진다");
    assert!(a20 > 0.0, "여전히 양수");
}

#[test]
fn 거리감쇠는_단조감소_한다() {
    // d 가 커질수록 알파가 항상 감소(같음 포함).
    let mut prev = distance_falloff_alpha(1);
    for d in 2..=12 {
        let cur = distance_falloff_alpha(d);
        assert!(cur <= prev, "d={} 에서 단조감소 깨짐: {} > {}", d, cur, prev);
        prev = cur;
    }
}

// ── chebyshev_distance ────────────────────────────────────────────────────────

#[test]
fn 체비쇼프거리는_8방향_한걸음을_1로_본다() {
    // 대각 한 칸도 1, 직선 한 칸도 1.
    assert_eq!(chebyshev_distance((5, 5), (5, 5)), 0);
    assert_eq!(chebyshev_distance((5, 5), (6, 5)), 1, "직선 1칸");
    assert_eq!(chebyshev_distance((5, 5), (6, 6)), 1, "대각 1칸");
    assert_eq!(chebyshev_distance((5, 5), (7, 8)), 3, "max(|dx|=2,|dy|=3)=3");
}

// ── tile_render_color × 거리 감쇠 통합 ────────────────────────────────────────

#[test]
fn 보이고_밝은_타일은_거리가_늘수록_배경에_더_가까워진다() {
    let base = Color::rgb(1.0, 1.0, 1.0);
    let r1 = render_color_legacy(base, true, true, LightLevel::Bright, 1, None).unwrap().r();
    let r2 = render_color_legacy(base, true, true, LightLevel::Bright, 2, None).unwrap().r();
    let r3 = render_color_legacy(base, true, true, LightLevel::Bright, 3, None).unwrap().r();
    assert!((r1 - 1.0).abs() < 1e-6);
    assert!((r2 - expected_dim_r(1.0, 0.5)).abs() < 1e-6, "d=2 → factor 0.5 로 배경 lerp");
    assert!((r3 - expected_dim_r(1.0, 1.0 / 9.0)).abs() < 1e-6, "d=3 → factor 1/9 로 배경 lerp");
}

#[test]
fn 보이는_어두운_타일도_거리_감쇠를_factor에_곱해_배경_lerp한다() {
    // 어둠 factor(0.5) × 거리 감쇠(0.5) = 0.25 factor 로 배경 lerp.
    let base = Color::rgb(1.0, 1.0, 1.0);
    let r2 = render_color_legacy(base, true, true, LightLevel::Dark, 2, None).unwrap().r();
    let expected = expected_dim_r(1.0, DARK_DIM_FACTOR * 0.5);
    assert!((r2 - expected).abs() < 1e-6, "어둠 d=2: 0.25 factor 로 배경 lerp");
}

// ── compute_light_levels / LightMap ───────────────────────────────────────────

#[test]
fn 광량그리드는_광원_주변만_밝게_채운다() {
    // 5x5 맵, 중앙(2,2) 반경 1.
    let levels = compute_light_levels(5, 5, &[((2usize, 2usize), 1i32)]);
    let at = |x: usize, y: usize| levels[y * 5 + x];
    assert_eq!(at(2, 2), LightLevel::Bright, "중앙은 밝다");
    assert_eq!(at(2, 1), LightLevel::Bright, "상하좌우 1칸은 밝다");
    assert_eq!(at(0, 0), LightLevel::Dark, "구석은 어둡다");
}

#[test]
fn LightMap_at은_범위밖_좌표를_어둠으로_본다() {
    let lm = LightMap { width: 3, height: 3, levels: vec![LightLevel::Bright; 9] };
    assert_eq!(lm.at(1, 1), LightLevel::Bright, "범위 안은 그대로");
    assert_eq!(lm.at(5, 0), LightLevel::Dark, "x 범위 밖은 어둠");
    assert_eq!(lm.at(0, 9), LightLevel::Dark, "y 범위 밖은 어둠");
}

// ── 시스템: update_light_map ──────────────────────────────────────────────────

fn map_app(w: usize, h: usize) -> App {
    let mut app = App::new();
    let mut map = Map::new(w, h);
    for y in 0..h { for x in 0..w { map.set_tile(x, y, TileKind::Floor); } }
    app.insert_resource(MapResource(map));
    app.init_resource::<LightMap>();
    app
}

#[test]
fn update_light_map은_광원_엔티티_위치로_광량을_채운다() {
    let mut app = map_app(10, 10);
    let coord = tile_to_world_coords(5, 5);
    app.world.spawn((Transform::from_xyz(coord.x, coord.y, 0.0), LightSource::new(2)));
    app.add_systems(Update, update_light_map);
    app.update();

    let lm = app.world.resource::<LightMap>();
    assert_eq!(lm.width, 10);
    assert_eq!(lm.at(5, 5), LightLevel::Bright, "광원 위치는 밝다");
    assert_eq!(lm.at(6, 5), LightLevel::Bright, "반경 안은 밝다");
    assert_eq!(lm.at(9, 9), LightLevel::Dark, "반경 밖은 어둡다");
}

#[test]
fn update_light_map은_광원이_없으면_전부_어둠으로_채운다() {
    let mut app = map_app(4, 4);
    app.add_systems(Update, update_light_map);
    app.update();
    let lm = app.world.resource::<LightMap>();
    assert!(lm.levels.iter().all(|&l| l == LightLevel::Dark), "광원 없으면 전부 어둠");
}

// ── 시스템: ensure_player_light ───────────────────────────────────────────────

#[test]
fn 플레이어에게_기본_시야광이_없으면_부여된다() {
    let mut app = App::new();
    let p = app.world.spawn(Player).id();
    app.add_systems(Update, ensure_player_light);
    app.update();
    let ls = app.world.get::<LightSource>(p).expect("플레이어에 LightSource 부여");
    assert_eq!(ls.radius, PLAYER_LIGHT_RADIUS);
}

#[test]
fn 이미_시야광이_있는_플레이어에게는_중복_부여하지_않는다() {
    let mut app = App::new();
    let p = app.world.spawn((Player, LightSource::new(99))).id();
    app.add_systems(Update, ensure_player_light);
    app.update();
    // 쿼리 필터(Without<LightSource>)에 걸리지 않아 기존 반경이 유지된다.
    assert_eq!(app.world.get::<LightSource>(p).unwrap().radius, 99);
}

// ── 시스템: handle_spawn_torch ────────────────────────────────────────────────

fn asset_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin::default());
    app.init_asset::<Font>();
    app.init_asset::<Image>();
    app
}

#[test]
fn 횃불스폰_이벤트는_광원을_가진_횃불_엔티티를_만든다() {
    let mut app = asset_app();
    app.add_event::<SpawnTorchEvent>();
    app.add_systems(Update, handle_spawn_torch);
    app.world.send_event(SpawnTorchEvent { tile_x: 3, tile_y: 4, radius: 5 });
    app.update();

    let mut q = app.world.query_filtered::<&LightSource, With<Torch>>();
    let sources: Vec<i32> = q.iter(&app.world).map(|ls| ls.radius).collect();
    assert_eq!(sources, vec![5], "반경 5 횃불 광원 하나가 생성된다");
}

// ── 시스템: apply_light_dimming (통합 디밍) ───────────────────────────────────

/// 타일 한 칸짜리 디밍 하네스 — 가시성과 brightness state 를 세팅한다.
/// 새 모델에서 광량/거리는 FOV 시스템이 brightness 로 결합해 두므로, dimming 시스템 단위
/// 테스트는 brightness 만 입력으로 받는다.
fn dimming_app(visible: bool, revealed: bool, brightness: f32, vis: Visibility) -> App {
    dimming_app_with_light(visible, revealed, brightness, LightLevel::Bright, vis)
}

/// LightMap 까지 명시할 수 있는 변종 — `광량이 바뀌면 색을 다시 칠한다` 같은
/// is_changed 검증용.
fn dimming_app_with_light(
    visible: bool,
    revealed: bool,
    brightness: f32,
    light: LightLevel,
    vis: Visibility,
) -> App {
    let mut app = App::new();
    let mut map = Map::new(1, 1);
    map.set_tile(0, 0, TileKind::Floor);
    map.tiles[0].visible = visible;
    map.tiles[0].revealed = revealed;
    map.tiles[0].brightness = brightness;
    app.insert_resource(MapResource(map));
    app.insert_resource(LightMap { width: 1, height: 1, levels: vec![light] });

    // 타일 엔티티 — 색은 일부러 임의 값으로 두고 시스템이 갱신하는지 본다.
    app.world.spawn((
        Text::from_section("x", TextStyle { color: Color::rgb(0.123, 0.123, 0.123), ..default() }),
        vis,
        TileEntity { x: 0, y: 0 },
    ));
    app.add_systems(Update, apply_light_dimming);
    app
}

fn tile_color(app: &mut App) -> Color {
    app.world.query::<&Text>().single(&app.world).sections[0].style.color
}

#[test]
fn 디밍시스템은_보이고_밝은_타일을_기본색으로_칠한다() {
    let mut app = dimming_app(true, true, 1.0, Visibility::Visible);
    app.update();
    assert_eq!(tile_color(&mut app), tile_base_color(TileKind::Floor));
}

#[test]
fn 디밍시스템은_보이지만_어두운_타일을_DARK_DIM_FACTOR로_배경_lerp한다() {
    let mut app = dimming_app(true, true, DARK_DIM_FACTOR, Visibility::Visible);
    app.update();
    let base = tile_base_color(TileKind::Floor);
    let f = DARK_DIM_FACTOR;
    // 시야 안 어둠 분기 — clamp 없는 순수 lerp.
    let lerp = |c: f32| c * f + 0.13 * (1.0 - f);
    let expected = Color::rgba(lerp(base.r()), lerp(base.g()), lerp(base.b()), base.a());
    assert_eq!(tile_color(&mut app), expected, "어둠 타일은 DARK_DIM_FACTOR 로 배경 lerp");
}

#[test]
fn 디밍시스템은_숨김_타일은_건드리지_않는다() {
    // Visibility::Hidden 이면 색을 그대로 둔다(가시성은 map 시스템이 결정).
    let mut app = dimming_app(false, false, 0.0, Visibility::Hidden);
    app.update();
    assert_eq!(tile_color(&mut app), Color::rgb(0.123, 0.123, 0.123), "숨김 타일 색 유지");
}

#[test]
fn 디밍시스템은_변경이_없으면_재적용을_건너뛴다() {
    // 첫 update 후 색을 수동으로 바꾸고, 변경 없는 두 번째 update 에서는 갱신 안 됨.
    let mut app = dimming_app(true, true, 1.0, Visibility::Visible);
    app.update();
    // 외부에서 색을 바꿔둔다.
    {
        let mut q = app.world.query::<&mut Text>();
        q.single_mut(&mut app.world).sections[0].style.color = Color::rgb(0.5, 0.5, 0.5);
    }
    // MapResource/LightMap 둘 다 변경 없음 → is_changed 거짓 → 건너뜀.
    app.update();
    assert_eq!(tile_color(&mut app), Color::rgb(0.5, 0.5, 0.5), "변경 없으면 색 유지(재적용 X)");
}

#[test]
fn 디밍시스템은_보임으로_표시됐지만_미탐험인_타일은_색을_정하지_않는다() {
    // 방어 경로: vis 가 Hidden 이 아닌데 visible/revealed 둘 다 false 면
    // tile_render_color 가 None 을 반환(if let Some False 분기). 색은 그대로 둔다.
    let mut app = dimming_app(false, false, 0.0, Visibility::Visible);
    app.update();
    assert_eq!(tile_color(&mut app), Color::rgb(0.123, 0.123, 0.123),
        "None 이면 색을 갱신하지 않는다");
}

#[test]
fn 디밍시스템은_이미_같은_색이면_재대입하지_않는다() {
    // text 색을 미리 목표색(기본색)으로 맞춰 두면 color != color 가 거짓(재대입 안 함).
    let mut app = App::new();
    let mut map = Map::new(1, 1);
    map.set_tile(0, 0, TileKind::Floor);
    map.tiles[0].visible = true;
    map.tiles[0].revealed = true;
    map.tiles[0].brightness = 1.0;
    app.insert_resource(MapResource(map));
    app.insert_resource(LightMap { width: 1, height: 1, levels: vec![LightLevel::Bright] });
    app.world.spawn((
        Text::from_section("x", TextStyle { color: tile_base_color(TileKind::Floor), ..default() }),
        Visibility::Visible,
        TileEntity { x: 0, y: 0 },
    ));
    app.add_systems(Update, apply_light_dimming);
    app.update();
    assert_eq!(tile_color(&mut app), tile_base_color(TileKind::Floor), "같은 색이면 그대로");
}

#[test]
fn 디밍시스템은_플레이어_거리에_따라_시야_안_타일을_더_어둡게_칠한다() {
    // 3x1 맵 — 플레이어를 (0,0) 에, 타일은 (2,0) 한 칸만 검사한다.
    // d = max(|2-0|, |0-0|) = 2 → falloff = 0.5. 밝음 분기는 base × 0.5.
    let mut app = App::new();
    let mut map = Map::new(3, 1);
    for x in 0..3 {
        map.set_tile(x, 0, TileKind::Floor);
        map.tiles[x].visible = true;
        map.tiles[x].revealed = true;
    }
    // 새 모델: FOV 가 brightness 를 거리별 falloff 로 세팅한 결과를 시뮬레이션.
    // (2,0) 의 plays 거리=2 → falloff=0.5 → brightness=0.5.
    map.tiles[2].brightness = 0.5;
    app.insert_resource(MapResource(map));
    app.insert_resource(LightMap { width: 3, height: 1, levels: vec![LightLevel::Bright; 3] });
    // 플레이어 (0,0) — Transform 으로 표현.
    let p_coord = tile_to_world_coords(0, 0);
    app.world.spawn((Player, Transform::from_xyz(p_coord.x, p_coord.y, 0.0)));
    // 멀리 있는 타일(2,0) 만 엔티티로 검사.
    app.world.spawn((
        Text::from_section("x", TextStyle { color: Color::rgb(0.0, 0.0, 0.0), ..default() }),
        Visibility::Visible,
        TileEntity { x: 2, y: 0 },
    ));
    app.add_systems(Update, apply_light_dimming);
    app.update();

    let got = app.world.query::<&Text>().single(&app.world).sections[0].style.color;
    let base = tile_base_color(TileKind::Floor);
    // d=2 → falloff 0.5 → 각 채널이 factor 0.5 로 배경(0.13) 과 lerp.
    let lerp = |c: f32| c * 0.5 + 0.13 * 0.5;
    assert!((got.r() - lerp(base.r())).abs() < 1e-6, "d=2 R 채널 0.5 factor 로 배경 lerp");
    assert!((got.g() - lerp(base.g())).abs() < 1e-6, "G 채널도 동일");
    assert!((got.b() - lerp(base.b())).abs() < 1e-6, "B 채널도 동일");
}

#[test]
fn 디밍시스템은_플레이어가_없으면_거리_감쇠_없이_그린다() {
    // 플레이어 엔티티가 없으면 None → 거리 0 으로 보아 falloff 1.0 (기존 동작).
    let mut app = dimming_app(true, true, 1.0, Visibility::Visible);
    app.update();
    assert_eq!(tile_color(&mut app), tile_base_color(TileKind::Floor),
        "플레이어 없을 때는 falloff 1.0 — 기존 동작 보존");
}

#[test]
fn 디밍시스템은_brightness가_바뀌면_색을_다시_칠한다() {
    // 새 모델 — 광량/거리는 FOV 시스템이 brightness 로 통합한다. dimming 시스템은
    // brightness 만 보므로, MapResource 의 brightness 가 변하면 색을 다시 칠한다.
    let mut app = dimming_app(true, true, 1.0, Visibility::Visible);
    app.update();
    let full_brightness_color = tile_color(&mut app);

    // brightness 를 낮추면(망각·어둠 시뮬레이션) 다시 칠해진다.
    app.world.resource_mut::<MapResource>().0.tiles[0].brightness = 0.3;
    app.update();
    let dim_color = tile_color(&mut app);
    assert!(dim_color.r() < full_brightness_color.r(), "brightness 가 낮아지면 더 어둡게 다시 칠한다");
}

// ── memory_fade_factor (기억 감퇴 곡선) ───────────────────────────────────────

#[test]
fn 기억감퇴_델타_0턴이면_factor는_1_0이다() {
    // exp(0) = 1.0 — 방금 본 타일은 감퇴 없음.
    assert!((memory_fade_factor(0) - 1.0).abs() < 1e-6);
}

#[test]
fn 기억감퇴_델타_50턴이면_factor는_약_0_3679이다() {
    // exp(-1) ≈ 0.36788 — 50 턴이면 약 절반쯤 어두워진다.
    let f = memory_fade_factor(50);
    assert!((f - (-1.0f32).exp()).abs() < 1e-6);
    assert!((f - 0.36788).abs() < 1e-3, "50턴은 약 0.37");
}

#[test]
fn 기억감퇴_델타_100턴이면_factor는_약_0_1353이다() {
    // exp(-2) ≈ 0.13534
    let f = memory_fade_factor(100);
    assert!((f - 0.13534).abs() < 1e-3, "100턴은 약 0.135");
}

#[test]
fn 기억감퇴_델타_200턴이면_factor는_약_0_0183이다() {
    // exp(-4) ≈ 0.01832 — 거의 배경.
    let f = memory_fade_factor(200);
    assert!((f - 0.01832).abs() < 1e-3, "200턴은 약 0.018");
}

#[test]
fn 기억감퇴는_매우_큰_델타에서_0에_수렴한다() {
    // u32 범위 안에서 큰 값들도 0 에 매끄럽게 수렴해야 한다.
    let a = memory_fade_factor(500);
    let b = memory_fade_factor(1_000);
    let c = memory_fade_factor(10_000);
    assert!(a < 1e-3, "Δ=500 이면 거의 0");
    assert!(b < a, "Δ가 커질수록 더 작아진다");
    assert!(c < b);
    assert!(c >= 0.0, "음수가 되지 않는다");
}

#[test]
fn 기억감퇴는_델타가_커질수록_단조감소한다() {
    let mut prev = memory_fade_factor(0);
    for d in (10..=500).step_by(10) {
        let cur = memory_fade_factor(d);
        assert!(cur < prev, "Δ={} 에서 단조감소 깨짐: {} >= {}", d, cur, prev);
        prev = cur;
    }
}

// ── tile_render_color × 기억 감퇴 통합 ────────────────────────────────────────

#[test]
fn 기억타일은_turns_since_seen_None이면_기존_0_3_factor_동작을_유지한다() {
    let base = Color::rgb(1.0, 1.0, 1.0);
    let none = render_color_legacy(base, false, true, LightLevel::Bright, 0, None).unwrap();
    let zero = render_color_legacy(base, false, true, LightLevel::Bright, 0, Some(0)).unwrap();
    // Δ=0 도 1.0 이라 None 과 같은 결과.
    assert_eq!(none, zero, "None 과 Δ=0 은 동일(감퇴 없음)");
    assert!((none.r() - expected_lerp_r(1.0, 0.3)).abs() < 1e-6, "None 은 0.3 factor 그대로");
}

#[test]
fn 기억타일은_시간이_흐를수록_배경에_더_가까워진다() {
    // 망각 분기는 클램프 없는 순수 lerp — 밝은 base 도 시간 흐를수록 배경 쪽으로 어두워진다.
    let base = Color::rgb(1.0, 1.0, 1.0);
    let r0   = render_color_legacy(base, false, true, LightLevel::Bright, 0, Some(0)).unwrap().r();
    let r50  = render_color_legacy(base, false, true, LightLevel::Bright, 0, Some(50)).unwrap().r();
    let r100 = render_color_legacy(base, false, true, LightLevel::Bright, 0, Some(100)).unwrap().r();
    let r200 = render_color_legacy(base, false, true, LightLevel::Bright, 0, Some(200)).unwrap().r();
    // 시간이 흐를수록 base(1.0) 에서 멀어져 배경(0.13) 쪽으로 단조 감소.
    assert!(r0 > r50,  "Δ=0 > Δ=50");
    assert!(r50 > r100, "Δ=50 > Δ=100");
    assert!(r100 > r200, "Δ=100 > Δ=200");
    // Δ=100 에서는 factor = 0.3 × 0.1353 ≈ 0.0406 — 거의 배경.
    let expected_100 = expected_lerp_r(1.0, 0.3 * memory_fade_factor(100));
    assert!((r100 - expected_100).abs() < 1e-6, "Δ=100 은 0.3 × 0.135 factor");
}

#[test]
fn 기억타일은_시간이_무한히_흐르면_정확히_배경색이_된다() {
    // 매우 큰 Δ → memory_fade_factor → 0 → 순수 lerp dim(base, 0.0) = BACKGROUND_COLOR.
    // 망각 분기는 클램프하지 않으므로 base 가 어떤 색이든 결국 배경으로 수렴.
    let base = Color::rgb(0.8, 0.2, 0.4);
    let faded = render_color_legacy(base, false, true, LightLevel::Bright, 0, Some(10_000)).unwrap();
    let bg = BACKGROUND_COLOR;
    assert!((faded.r() - bg.r()).abs() < 1e-4, "R 채널이 배경");
    assert!((faded.g() - bg.g()).abs() < 1e-4, "G 채널이 배경");
    assert!((faded.b() - bg.b()).abs() < 1e-4, "B 채널이 배경");
    // 검정이 아니라 배경 회색이어야 한다(중요 — 어색한 검은 페이드 회피).
    assert!(faded.r() > 0.1, "검정이 아닌 배경 회색");
}

#[test]
fn 기억타일은_광량과_거리는_여전히_무시한다_turns가_있어도() {
    // turns_since_seen 이 있어도 visible=false 분기는 광량·거리 무시.
    let base = Color::rgb(1.0, 1.0, 1.0);
    let a = render_color_legacy(base, false, true, LightLevel::Dark,   5, Some(30)).unwrap();
    let b = render_color_legacy(base, false, true, LightLevel::Bright, 0, Some(30)).unwrap();
    assert_eq!(a, b, "기억 타일은 광량/거리 무관 (turns 만 반영)");
}

#[test]
fn 보이는_타일은_turns_since_seen을_무시한다() {
    // visible=true 분기는 turns_since_seen 을 안 본다 — 보는 동안엔 망각이 멈춤.
    let base = Color::rgb(1.0, 1.0, 1.0);
    let fresh = render_color_legacy(base, true, true, LightLevel::Bright, 1, None).unwrap();
    let stale = render_color_legacy(base, true, true, LightLevel::Bright, 1, Some(10_000)).unwrap();
    assert_eq!(fresh, stale, "visible 분기는 turns 무시");
}

// ── 시스템: update_fov 가 last_seen_turn 을 갱신 ──────────────────────────────

#[test]
fn fov는_보인_타일의_last_seen_turn을_현재_글로벌턴으로_갱신한다() {
    use crate::modules::map::{Map, MapResource, GlobalTurn};
    use crate::modules::player::{Facing, Player};
    let mut map = Map::new(30, 30);
    for y in 5..15 { for x in 5..15 { map.set_tile(x, y, TileKind::Floor); } }
    let mut app = App::new();
    app.insert_resource(MapResource(map));
    app.insert_resource(GlobalTurn(42));
    let pos = tile_to_world_coords(10, 10);
    app.world.spawn((Player, Transform::from_xyz(pos.x, pos.y, 1.0), Facing::default()));
    app.add_systems(Update, crate::modules::player::update_fov_for_test);
    app.update();

    let map = &app.world.resource::<MapResource>().0;
    let idx = map.index(10, 10);
    assert!(map.tiles[idx].visible, "플레이어 위치는 보임");
    assert_eq!(map.tiles[idx].last_seen_turn, Some(42), "보인 타일은 GlobalTurn 으로 갱신");
}

#[test]
fn fov는_보이지_않는_타일의_last_seen_turn을_유지한다() {
    use crate::modules::map::{Map, MapResource, GlobalTurn};
    use crate::modules::player::{Facing, Player};
    let mut map = Map::new(30, 30);
    for y in 0..30 { for x in 0..30 { map.set_tile(x, y, TileKind::Floor); } }
    // 멀리 떨어진 타일에 이전에 본 기록을 심어 둔다.
    let far_idx = map.index(29, 29);
    map.tiles[far_idx].revealed = true;
    map.tiles[far_idx].last_seen_turn = Some(10);
    let mut app = App::new();
    app.insert_resource(MapResource(map));
    app.insert_resource(GlobalTurn(100));
    let pos = tile_to_world_coords(0, 0);
    app.world.spawn((Player, Transform::from_xyz(pos.x, pos.y, 1.0), Facing::default()));
    app.add_systems(Update, crate::modules::player::update_fov_for_test);
    app.update();

    let map = &app.world.resource::<MapResource>().0;
    // (29,29) 는 (0,0) 시야 밖이라 visible=false, last_seen_turn 은 이전 값 유지.
    assert!(!map.tiles[far_idx].visible, "멀리 있는 타일은 안 보임");
    assert_eq!(map.tiles[far_idx].last_seen_turn, Some(10),
        "안 보이는 타일은 last_seen_turn 유지");
}

// ── 시스템: apply_light_dimming × 기억 감퇴 ──────────────────────────────────

#[test]
fn 디밍시스템은_기억타일의_경과_턴에_따라_더_어둡게_칠한다() {
    use crate::modules::map::GlobalTurn;
    // 1x1 맵 — visible=false, revealed=true(기억), 마지막 본 턴 0, 현재 100 → Δ=100.
    let mut app = App::new();
    let mut map = Map::new(1, 1);
    map.set_tile(0, 0, TileKind::Floor);
    map.tiles[0].visible = false;
    map.tiles[0].revealed = true;
    map.tiles[0].last_seen_turn = Some(0);
    map.tiles[0].brightness = 0.3;
    app.insert_resource(MapResource(map));
    app.insert_resource(LightMap { width: 1, height: 1, levels: vec![LightLevel::Bright] });
    app.insert_resource(GlobalTurn(100));
    app.world.spawn((
        Text::from_section("x", TextStyle { color: Color::rgb(0.0, 0.0, 0.0), ..default() }),
        Visibility::Visible,
        TileEntity { x: 0, y: 0 },
    ));
    app.add_systems(Update, apply_light_dimming);
    app.update();

    let got = tile_color(&mut app);
    let base = tile_base_color(TileKind::Floor);
    // factor = 0.3 × memory_fade_factor(100) ≈ 0.3 × 0.1353 ≈ 0.0406
    let f = 0.3 * memory_fade_factor(100);
    let lerp = |c: f32| c * f + 0.13 * (1.0 - f);
    assert!((got.r() - lerp(base.r())).abs() < 1e-5, "R: 기억 감퇴된 lerp");
    assert!((got.g() - lerp(base.g())).abs() < 1e-5, "G");
    assert!((got.b() - lerp(base.b())).abs() < 1e-5, "B");
}

#[test]
fn 디밍시스템은_last_seen_turn_None이면_기존_0_3_factor를_유지한다() {
    use crate::modules::map::GlobalTurn;
    // last_seen_turn=None(아직 한 번도 본 적 없지만 revealed=true 인 인공적 케이스).
    // GlobalTurn 이 있어도 None 이면 감퇴 없음 — 기존 동작.
    let mut app = App::new();
    let mut map = Map::new(1, 1);
    map.set_tile(0, 0, TileKind::Floor);
    map.tiles[0].visible = false;
    map.tiles[0].revealed = true;
    map.tiles[0].last_seen_turn = None;
    map.tiles[0].brightness = 0.3;
    app.insert_resource(MapResource(map));
    app.insert_resource(LightMap { width: 1, height: 1, levels: vec![LightLevel::Bright] });
    app.insert_resource(GlobalTurn(999));
    app.world.spawn((
        Text::from_section("x", TextStyle { color: Color::rgb(0.0, 0.0, 0.0), ..default() }),
        Visibility::Visible,
        TileEntity { x: 0, y: 0 },
    ));
    app.add_systems(Update, apply_light_dimming);
    app.update();

    let got = tile_color(&mut app);
    let base = tile_base_color(TileKind::Floor);
    let f = 0.3;
    let lerp = |c: f32| c * f + 0.13 * (1.0 - f);
    assert!((got.r() - lerp(base.r())).abs() < 1e-6, "R: 기존 0.3 factor 그대로");
}

#[test]
fn 디밍시스템은_GlobalTurn_리소스가_없어도_기존_0_3_factor를_유지한다() {
    // GlobalTurn 없음 → 모든 기억 타일은 turns_since_seen=None → 감퇴 없음.
    let mut app = App::new();
    let mut map = Map::new(1, 1);
    map.set_tile(0, 0, TileKind::Floor);
    map.tiles[0].visible = false;
    map.tiles[0].revealed = true;
    map.tiles[0].last_seen_turn = Some(0);
    map.tiles[0].brightness = 0.3;
    app.insert_resource(MapResource(map));
    app.insert_resource(LightMap { width: 1, height: 1, levels: vec![LightLevel::Bright] });
    // GlobalTurn 의도적으로 미삽입.
    app.world.spawn((
        Text::from_section("x", TextStyle { color: Color::rgb(0.0, 0.0, 0.0), ..default() }),
        Visibility::Visible,
        TileEntity { x: 0, y: 0 },
    ));
    app.add_systems(Update, apply_light_dimming);
    app.update();

    let got = tile_color(&mut app);
    let base = tile_base_color(TileKind::Floor);
    let f = 0.3;
    let lerp = |c: f32| c * f + 0.13 * (1.0 - f);
    assert!((got.r() - lerp(base.r())).abs() < 1e-6, "GlobalTurn 없으면 감퇴 없음");
}

#[test]
fn 디밍시스템은_last_seen이_현재턴보다_미래여도_방어적으로_0턴으로_본다() {
    use crate::modules::map::GlobalTurn;
    // saturating_sub 방어: last_seen=200 > now=100 → Δ=0 → factor 1.0 → 기존 0.3 그대로.
    let mut app = App::new();
    let mut map = Map::new(1, 1);
    map.set_tile(0, 0, TileKind::Floor);
    map.tiles[0].visible = false;
    map.tiles[0].revealed = true;
    map.tiles[0].last_seen_turn = Some(200);
    map.tiles[0].brightness = 0.3;
    app.insert_resource(MapResource(map));
    app.insert_resource(LightMap { width: 1, height: 1, levels: vec![LightLevel::Bright] });
    app.insert_resource(GlobalTurn(100));
    app.world.spawn((
        Text::from_section("x", TextStyle { color: Color::rgb(0.0, 0.0, 0.0), ..default() }),
        Visibility::Visible,
        TileEntity { x: 0, y: 0 },
    ));
    app.add_systems(Update, apply_light_dimming);
    app.update();

    let got = tile_color(&mut app);
    let base = tile_base_color(TileKind::Floor);
    let f = 0.3;
    let lerp = |c: f32| c * f + 0.13 * (1.0 - f);
    assert!((got.r() - lerp(base.r())).abs() < 1e-6, "미래 last_seen 은 Δ=0 으로 방어");
}

// ── 세이브 호환 (last_seen_turn 누락 → None) ─────────────────────────────────

#[test]
fn 기존_세이브의_MapTile_직렬화에_last_seen_turn이_없으면_None으로_복원된다() {
    use crate::modules::map::{MapTile, TileKind};
    // 기존 세이브 포맷(필드 3개) 모방한 JSON.
    let legacy = r#"{"kind":"Floor","revealed":true,"visible":false}"#;
    let tile: MapTile = serde_json::from_str(legacy)
        .expect("기존 세이브 호환: last_seen_turn 누락도 디시리얼라이즈");
    assert_eq!(tile.kind, TileKind::Floor);
    assert!(tile.revealed);
    assert!(!tile.visible);
    assert_eq!(tile.last_seen_turn, None, "누락 시 None 으로 복원");
}

#[test]
fn 신규_MapTile은_last_seen_turn_필드를_포함해_round_trip한다() {
    use crate::modules::map::{MapTile, TileKind};
    let t = MapTile { kind: TileKind::Floor, revealed: true, visible: true, last_seen_turn: Some(7), brightness: 0.5 };
    let s = serde_json::to_string(&t).unwrap();
    let back: MapTile = serde_json::from_str(&s).unwrap();
    assert_eq!(back, t, "round-trip 보존");
    assert!(s.contains("last_seen_turn"), "필드가 직렬화에 포함");
}

// ── 플러그인 build ────────────────────────────────────────────────────────────

#[test]
fn 라이팅플러그인은_LightMap_리소스와_횃불이벤트를_등록한다() {
    let mut app = App::new();
    app.add_plugins(LightingPlugin);
    assert!(app.world.get_resource::<LightMap>().is_some(), "LightMap 리소스 등록");
    // 이벤트 등록 여부 — 발행이 패닉 없이 동작하면 등록된 것.
    app.world.send_event(SpawnTorchEvent { tile_x: 0, tile_y: 0, radius: 1 });
}
