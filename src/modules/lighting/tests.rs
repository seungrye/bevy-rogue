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
    assert_eq!(tile_render_color(base, false, false, LightLevel::Bright), None);
}

#[test]
fn 탐험만_된_타일은_광량과_무관하게_0_3배로_디밍된다() {
    let base = Color::rgb(1.0, 1.0, 1.0);
    let dark = tile_render_color(base, false, true, LightLevel::Dark).unwrap();
    let bright = tile_render_color(base, false, true, LightLevel::Bright).unwrap();
    // visible=false 면 광량 무관 — 두 결과가 같고 0.3 디밍.
    assert_eq!(dark, bright, "기억 타일은 광량과 무관");
    assert!((dark.r() - 0.3).abs() < 1e-6, "기억 타일은 0.3 디밍");
}

#[test]
fn 보이고_밝은_타일은_기본색_그대로다() {
    let base = tile_base_color(TileKind::Water);
    assert_eq!(tile_render_color(base, true, true, LightLevel::Bright), Some(base));
}

#[test]
fn 보이지만_어두운_타일은_기본색보다_어둡게_디밍된다() {
    let base = Color::rgb(1.0, 1.0, 1.0);
    let dimmed = tile_render_color(base, true, true, LightLevel::Dark).unwrap();
    assert!((dimmed.r() - DARK_DIM_FACTOR).abs() < 1e-6, "어둠은 DARK_DIM_FACTOR 디밍");
    assert!(dimmed.r() < base.r(), "어둠 타일은 기본색보다 어둡다");
}

#[test]
fn 디밍은_알파값을_보존한다() {
    let base = Color::rgba(0.8, 0.6, 0.4, 0.5);
    let dimmed = tile_render_color(base, true, true, LightLevel::Dark).unwrap();
    assert!((dimmed.a() - 0.5).abs() < 1e-6, "알파는 유지된다");
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

/// 타일 한 칸짜리 디밍 하네스 — 가시성/광량을 세팅하고 시스템을 한 번 돌린다.
fn dimming_app(visible: bool, revealed: bool, light: LightLevel, vis: Visibility) -> App {
    let mut app = App::new();
    let mut map = Map::new(1, 1);
    map.set_tile(0, 0, TileKind::Floor);
    map.tiles[0].visible = visible;
    map.tiles[0].revealed = revealed;
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
    let mut app = dimming_app(true, true, LightLevel::Bright, Visibility::Visible);
    app.update();
    assert_eq!(tile_color(&mut app), tile_base_color(TileKind::Floor));
}

#[test]
fn 디밍시스템은_보이지만_어두운_타일을_어둡게_칠한다() {
    let mut app = dimming_app(true, true, LightLevel::Dark, Visibility::Visible);
    app.update();
    let expected = Color::rgba(
        tile_base_color(TileKind::Floor).r() * DARK_DIM_FACTOR,
        tile_base_color(TileKind::Floor).g() * DARK_DIM_FACTOR,
        tile_base_color(TileKind::Floor).b() * DARK_DIM_FACTOR,
        tile_base_color(TileKind::Floor).a(),
    );
    assert_eq!(tile_color(&mut app), expected, "어둠 타일은 DARK_DIM_FACTOR 디밍");
}

#[test]
fn 디밍시스템은_숨김_타일은_건드리지_않는다() {
    // Visibility::Hidden 이면 색을 그대로 둔다(가시성은 map 시스템이 결정).
    let mut app = dimming_app(false, false, LightLevel::Bright, Visibility::Hidden);
    app.update();
    assert_eq!(tile_color(&mut app), Color::rgb(0.123, 0.123, 0.123), "숨김 타일 색 유지");
}

#[test]
fn 디밍시스템은_변경이_없으면_재적용을_건너뛴다() {
    // 첫 update 후 색을 수동으로 바꾸고, 변경 없는 두 번째 update 에서는 갱신 안 됨.
    let mut app = dimming_app(true, true, LightLevel::Bright, Visibility::Visible);
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
    let mut app = dimming_app(false, false, LightLevel::Bright, Visibility::Visible);
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
fn 디밍시스템은_광량이_바뀌면_색을_다시_칠한다() {
    let mut app = dimming_app(true, true, LightLevel::Bright, Visibility::Visible);
    app.update();
    assert_eq!(tile_color(&mut app), tile_base_color(TileKind::Floor), "처음엔 밝은 색");

    // 광량을 어둠으로 바꾸면(LightMap 변경) 다시 칠해진다.
    app.world.resource_mut::<LightMap>().levels[0] = LightLevel::Dark;
    app.update();
    assert!(tile_color(&mut app).r() < tile_base_color(TileKind::Floor).r(),
        "어둠으로 바뀌면 더 어둡게 다시 칠한다");
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
