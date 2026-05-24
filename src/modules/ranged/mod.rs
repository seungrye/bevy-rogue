use bevy::prelude::*;
use crate::modules::{
    combat::Defeated,
    item::{EquipmentPanelOpen, PlayerEquipment, WeaponKind, weapon_attack},
    map::{
        MapResource, MAP_WIDTH, MAP_HEIGHT, TILE_SIZE,
        is_line_of_sight_clear, tile_to_world_coords, world_to_tile_coords, MonsterTiles,
        PlayerActedEvent,
    },
    player::Player,
    projectile::{FireProjectileEvent, BOW_RANGE},
    ui::{help::HelpPanelOpen, shop::ShopPanelOpen, LogMessage},
};

pub struct RangedPlugin;

impl Plugin for RangedPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RangedTargeting>()
            .add_systems(Update, (
                handle_ranged_input,
                handle_ranged_mouse,
                update_ranged_cursor,
            ).chain());
    }
}

#[derive(Resource, Default)]
pub struct RangedTargeting {
    pub active: bool,
    pub cursor: (usize, usize),
}

#[derive(Component)]
pub struct RangedCursor;

/// 한 발을 쏘려 할 때의 결정 결과 — 사거리/LoS 판정만 담은 순수 값.
/// (윈도우/카메라 viewport 변환은 헤드리스 재현이 불가하므로 시스템에 남기고,
/// 타일 좌표가 정해진 뒤의 판정 분기는 모두 이 순수 함수로 모아 단위 테스트한다.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RangedFireDecision {
    /// 사거리 밖 — "사거리를 벗어났다." 로그.
    OutOfRange,
    /// 장애물에 막힘 — "장애물에 막혔다." 로그.
    Blocked,
    /// 발사 가능.
    Fire,
}

/// 플레이어 타일 `(px, py)` 에서 목표 타일 `(tx, ty)` 로 발사 가능한지 판정한다.
/// 키보드 Enter 와 마우스 좌클릭이 동일하게 사용하는 순수 결정 로직.
fn decide_ranged_fire(
    px: usize, py: usize,
    tx: usize, ty: usize,
    map: &crate::modules::map::Map,
) -> RangedFireDecision {
    let dx = tx as i32 - px as i32;
    let dy = ty as i32 - py as i32;
    let in_range = dx * dx + dy * dy <= BOW_RANGE * BOW_RANGE;
    if !in_range {
        return RangedFireDecision::OutOfRange;
    }
    if !is_line_of_sight_clear(map, px as i32, py as i32, tx as i32, ty as i32) {
        return RangedFireDecision::Blocked;
    }
    RangedFireDecision::Fire
}

fn handle_ranged_input(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut targeting: ResMut<RangedTargeting>,
    equipment: Res<PlayerEquipment>,
    equipment_open: Res<EquipmentPanelOpen>,
    shop_open: Res<ShopPanelOpen>,
    help_open: Res<HelpPanelOpen>,
    player_q: Query<&Transform, (With<Player>, Without<Defeated>)>,
    monster_tiles: Res<MonsterTiles>,
    map_res: Res<MapResource>,
    asset_server: Res<AssetServer>,
    cursor_q: Query<Entity, With<RangedCursor>>,
    mut fire_writer: EventWriter<FireProjectileEvent>,
    mut acted_writer: EventWriter<PlayerActedEvent>,
    mut log: EventWriter<LogMessage>,
    items: Res<crate::modules::item::ItemRegistry>,
) {
    if equipment_open.0 || shop_open.0 || help_open.0 { return; }

    let Ok(player_transform) = player_q.get_single() else { return };
    let (px, py) = world_to_tile_coords(player_transform.translation);

    // 진입
    if !targeting.active {
        if keyboard.just_pressed(KeyCode::KeyF) {
            if equipment.weapon != Some(WeaponKind::BOW) {
                log.send(LogMessage("활을 장착해야 원격 공격이 가능하다.".into()));
                return;
            }
            let initial = nearest_target(px, py, &monster_tiles, map_res.map())
                .unwrap_or((px, py));
            targeting.active = true;
            targeting.cursor = initial;
            spawn_cursor(&mut commands, &asset_server, initial);
        }
        return;
    }

    // 활성 상태 — 입력 처리
    if keyboard.just_pressed(KeyCode::Escape) {
        targeting.active = false;
        for e in cursor_q.iter() { commands.entity(e).despawn(); }
        return;
    }

    if keyboard.just_pressed(KeyCode::Tab) {
        let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
        let targets = sorted_targets(px, py, &monster_tiles, map_res.map());
        if !targets.is_empty() {
            // 현재 cursor 와 일치하는 위치 idx 찾고 다음/이전.
            let cur_idx = targets.iter().position(|&t| t == targeting.cursor);
            let next_idx = match cur_idx {
                Some(i) if shift => (i + targets.len() - 1) % targets.len(),
                Some(i)          => (i + 1) % targets.len(),
                None             => 0,
            };
            targeting.cursor = targets[next_idx];
        }
        return;
    }

    // 자유 커서 이동 — 한 키 이벤트당 한 칸 (just_pressed)
    let mut dx: i32 = 0;
    let mut dy: i32 = 0;
    if keyboard.just_pressed(KeyCode::ArrowLeft)  || keyboard.just_pressed(KeyCode::KeyA) { dx -= 1; }
    if keyboard.just_pressed(KeyCode::ArrowRight) || keyboard.just_pressed(KeyCode::KeyD) { dx += 1; }
    if keyboard.just_pressed(KeyCode::ArrowUp)    || keyboard.just_pressed(KeyCode::KeyW) { dy += 1; }
    if keyboard.just_pressed(KeyCode::ArrowDown)  || keyboard.just_pressed(KeyCode::KeyS) { dy -= 1; }
    if dx != 0 || dy != 0 {
        let nx = (targeting.cursor.0 as i32 + dx).clamp(0, MAP_WIDTH as i32 - 1) as usize;
        let ny = (targeting.cursor.1 as i32 + dy).clamp(0, MAP_HEIGHT as i32 - 1) as usize;
        targeting.cursor = (nx, ny);
        return;
    }

    if keyboard.just_pressed(KeyCode::Enter) {
        let (tx, ty) = targeting.cursor;
        match decide_ranged_fire(px, py, tx, ty, map_res.map()) {
            RangedFireDecision::OutOfRange => {
                log.send(LogMessage("사거리를 벗어났다.".into()));
            }
            RangedFireDecision::Blocked => {
                log.send(LogMessage("장애물에 막혔다.".into()));
            }
            RangedFireDecision::Fire => {
                fire_writer.send(FireProjectileEvent {
                    origin_tile: (px, py),
                    target_tile: (tx, ty),
                    damage: weapon_attack(WeaponKind::BOW, &items),
                    element: Some(crate::modules::elemental::Element::Lightning),
                });
                acted_writer.send(PlayerActedEvent);
                targeting.active = false;
                for e in cursor_q.iter() { commands.entity(e).despawn(); }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_ranged_mouse(
    mut commands: Commands,
    mut targeting: ResMut<RangedTargeting>,
    mouse_input: Res<ButtonInput<MouseButton>>,
    mut cursor_moved: EventReader<bevy::window::CursorMoved>,
    equipment_open: Res<EquipmentPanelOpen>,
    shop_open: Res<ShopPanelOpen>,
    help_open: Res<HelpPanelOpen>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<Camera>>,
    player_q: Query<&Transform, (With<Player>, Without<Defeated>)>,
    cursor_q: Query<Entity, With<RangedCursor>>,
    map_res: Res<MapResource>,
    mut fire_writer: EventWriter<FireProjectileEvent>,
    mut acted_writer: EventWriter<PlayerActedEvent>,
    mut log: EventWriter<LogMessage>,
    items: Res<crate::modules::item::ItemRegistry>,
) {
    if !targeting.active { return; }
    if equipment_open.0 || shop_open.0 || help_open.0 { return; }

    // 우클릭 — 취소.
    if mouse_input.just_pressed(MouseButton::Right) {
        targeting.active = false;
        for e in cursor_q.iter() { commands.entity(e).despawn(); }
        return;
    }

    // 테스트 불가: 헤드리스 viewport — Window/Camera 가 없는 테스트 환경에선
    // get_single() 이 Err 라 아래 가드에서 모두 조기 반환된다. 좌클릭 발사의 판정
    // 분기는 decide_ranged_fire 순수 함수로 분리해 단위 테스트로 모두 커버한다.
    let Ok(window) = windows.get_single() else { return };
    let Ok((camera, cam_transform)) = camera_q.get_single() else { return };
    let Ok(player_transform) = player_q.get_single() else { return };

    // 마우스 hover — 커서 갱신.
    // 테스트 불가: 헤드리스 viewport — viewport_to_world_2d 가 항상 None 이라
    // 안쪽 분기(타일 좌표 변환·클램프)는 헤드리스로 도달 불가하다.
    if cursor_moved.read().next().is_some() {
        if let Some(cursor_pos) = window.cursor_position() {
            if let Some(world_pos) = camera.viewport_to_world_2d(cam_transform, cursor_pos) {
                let world_vec3 = Vec3::new(world_pos.x, world_pos.y, 0.0);
                let (tx, ty) = world_to_tile_coords(world_vec3);
                if tx < MAP_WIDTH && ty < MAP_HEIGHT {
                    targeting.cursor = (tx, ty);
                }
            }
        }
    }

    // 좌클릭 — 발사.
    // 테스트 불가: 헤드리스 viewport — 위 Window 가드(line 201)에서 항상 조기 반환되어
    // 여기까지 도달하지 못한다. 발사 여부 판정(사거리/LoS)은 decide_ranged_fire 순수
    // 함수로 분리해 단위 테스트로 양방향 모두 커버했다.
    if mouse_input.just_pressed(MouseButton::Left) {
        let (px, py) = world_to_tile_coords(player_transform.translation);
        let (tx, ty) = targeting.cursor;
        match decide_ranged_fire(px, py, tx, ty, map_res.map()) {
            RangedFireDecision::OutOfRange => {
                log.send(LogMessage("사거리를 벗어났다.".into()));
            }
            RangedFireDecision::Blocked => {
                log.send(LogMessage("장애물에 막혔다.".into()));
            }
            RangedFireDecision::Fire => {
                fire_writer.send(FireProjectileEvent {
                    origin_tile: (px, py),
                    target_tile: (tx, ty),
                    damage: weapon_attack(WeaponKind::BOW, &items),
                    element: Some(crate::modules::elemental::Element::Lightning),
                });
                acted_writer.send(PlayerActedEvent);
                targeting.active = false;
                for e in cursor_q.iter() { commands.entity(e).despawn(); }
            }
        }
    }
}

fn update_ranged_cursor(
    targeting: Res<RangedTargeting>,
    map_res: Res<MapResource>,
    player_q: Query<&Transform, (With<Player>, Without<Defeated>)>,
    mut cursor_q: Query<(&mut Transform, &mut Text), (With<RangedCursor>, Without<Player>)>,
) {
    if !targeting.active { return; }
    let Ok((mut t, mut text)) = cursor_q.get_single_mut() else { return };
    let (cx, cy) = targeting.cursor;
    let coord = tile_to_world_coords(cx, cy);
    t.translation.x = coord.x;
    t.translation.y = coord.y;

    // 색상 — 사거리 + LoS 통과면 노랑, 실패면 빨강.
    let Ok(player_transform) = player_q.get_single() else { return };
    let (px, py) = world_to_tile_coords(player_transform.translation);
    let dx = cx as i32 - px as i32;
    let dy = cy as i32 - py as i32;
    let in_range = dx * dx + dy * dy <= BOW_RANGE * BOW_RANGE;
    let los = is_line_of_sight_clear(map_res.map(), px as i32, py as i32, cx as i32, cy as i32);
    let color = if in_range && los {
        Color::rgb(1.0, 0.95, 0.4)
    } else {
        Color::rgb(1.0, 0.3, 0.3)
    };
    if text.sections[0].style.color != color {
        text.sections[0].style.color = color;
    }
}

fn spawn_cursor(
    commands: &mut Commands,
    asset_server: &AssetServer,
    tile: (usize, usize),
) {
    let coord = tile_to_world_coords(tile.0, tile.1);
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");
    commands.spawn((
        Text2dBundle {
            text: Text::from_section("+", TextStyle {
                font,
                font_size: TILE_SIZE,
                color: Color::rgb(1.0, 0.95, 0.4),
            }),
            transform: Transform::from_xyz(coord.x, coord.y, 2.0),
            ..default()
        },
        RangedCursor,
    ));
}

/// FOV 안 + 사거리 안 적 위치를 player 와의 거리 오름차순으로 반환.
/// (tile_x, tile_y) 로 tie-break.
fn sorted_targets(
    px: usize, py: usize,
    monster_tiles: &MonsterTiles,
    map: &crate::modules::map::Map,
) -> Vec<(usize, usize)> {
    let mut v: Vec<(usize, usize)> = monster_tiles.0.iter()
        .copied()
        .filter(|&(x, y)| {
            let idx = y * MAP_WIDTH + x;
            if !map.tiles[idx].visible { return false; }
            let dx = x as i32 - px as i32;
            let dy = y as i32 - py as i32;
            dx * dx + dy * dy <= BOW_RANGE * BOW_RANGE
        })
        .collect();
    v.sort_by_key(|&(x, y)| {
        let dx = x as i32 - px as i32;
        let dy = y as i32 - py as i32;
        (dx * dx + dy * dy, x, y)
    });
    v
}

fn nearest_target(
    px: usize, py: usize,
    monster_tiles: &MonsterTiles,
    map: &crate::modules::map::Map,
) -> Option<(usize, usize)> {
    sorted_targets(px, py, monster_tiles, map).into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::map::{Map, TileKind};

    fn make_visible_map() -> Map {
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                map.set_tile(x, y, TileKind::Floor);
                let idx = y * MAP_WIDTH + x;
                map.tiles[idx].visible = true;
            }
        }
        map
    }

    #[test]
    fn 정렬된_타겟은_플레이어와의_거리_오름차순으로_나온다() {
        let map = make_visible_map();
        let mut mt = MonsterTiles::default();
        mt.0.insert((10, 10));
        mt.0.insert((6, 6));
        mt.0.insert((8, 8));
        let v = sorted_targets(5, 5, &mt, &map);
        assert_eq!(v[0], (6, 6));
        assert_eq!(v[1], (8, 8));
        assert_eq!(v[2], (10, 10));
    }

    #[test]
    fn 정렬된_타겟은_사거리_밖의_적을_제외한다() {
        let map = make_visible_map();
        let mut mt = MonsterTiles::default();
        mt.0.insert((20, 5));  // 거리 15 — BOW_RANGE(8) 밖
        mt.0.insert((6, 6));
        let v = sorted_targets(5, 5, &mt, &map);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0], (6, 6));
    }

    #[test]
    fn 정렬된_타겟은_보이지_않는_적을_제외한다() {
        let mut map = make_visible_map();
        // (6, 6) 만 invisible
        let idx = 6 * MAP_WIDTH + 6;
        map.tiles[idx].visible = false;
        let mut mt = MonsterTiles::default();
        mt.0.insert((6, 6));
        mt.0.insert((7, 7));
        let v = sorted_targets(5, 5, &mt, &map);
        assert_eq!(v, vec![(7, 7)]);
    }

    // ── nearest_target (순수 함수) ──────────────────────────────────────────
    #[test]
    fn 사거리_안에_보이는_적이_없으면_가장_가까운_타겟은_없음이다() {
        let map = make_visible_map();
        let mt = MonsterTiles::default(); // 적 없음
        assert_eq!(nearest_target(5, 5, &mt, &map), None);
    }

    #[test]
    fn 적이_여럿이면_가장_가까운_타겟을_돌려준다() {
        let map = make_visible_map();
        let mut mt = MonsterTiles::default();
        mt.0.insert((9, 9));
        mt.0.insert((6, 6)); // 가장 가까움
        mt.0.insert((8, 8));
        assert_eq!(nearest_target(5, 5, &mt, &map), Some((6, 6)));
    }

    // ── decide_ranged_fire (순수 판정 함수) ─────────────────────────────────
    #[test]
    fn 사거리_안이고_시야가_트이면_발사로_판정한다() {
        let map = make_visible_map();
        assert_eq!(decide_ranged_fire(5, 5, 8, 5, &map), RangedFireDecision::Fire);
    }

    #[test]
    fn 사거리_밖이면_사거리벗어남으로_판정한다() {
        let map = make_visible_map();
        // (5,5) → (20,5) 거리 15 > BOW_RANGE(8)
        assert_eq!(decide_ranged_fire(5, 5, 20, 5, &map), RangedFireDecision::OutOfRange);
    }

    #[test]
    fn 사거리_안이라도_벽에_막히면_장애물로_판정한다() {
        let mut map = make_visible_map();
        // 플레이어(5,5)와 목표(8,5) 사이 (6,5)를 벽으로
        map.set_tile(6, 5, TileKind::Wall);
        assert_eq!(decide_ranged_fire(5, 5, 8, 5, &map), RangedFireDecision::Blocked);
    }

    // ── App 하네스: 공통 셋업 ───────────────────────────────────────────────
    use crate::modules::item::build_test_registry;

    /// AssetServer(폰트) + 키보드 + ranged 입력 시스템에 필요한 리소스를 모두 갖춘 App.
    fn ranged_input_app(map: Map) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app.insert_resource(ButtonInput::<KeyCode>::default())
            .init_resource::<RangedTargeting>()
            .insert_resource(PlayerEquipment { weapon: Some(WeaponKind::BOW), armor: None, ..Default::default() })
            .init_resource::<EquipmentPanelOpen>()
            .init_resource::<ShopPanelOpen>()
            .init_resource::<HelpPanelOpen>()
            .init_resource::<MonsterTiles>()
            .insert_resource(MapResource(map))
            .insert_resource(build_test_registry())
            .add_event::<FireProjectileEvent>()
            .add_event::<PlayerActedEvent>()
            .add_event::<LogMessage>()
            .add_systems(Update, handle_ranged_input);
        app
    }

    fn spawn_player_at(app: &mut App, x: usize, y: usize) {
        let pos = tile_to_world_coords(x, y).extend(1.0);
        app.world.spawn((Player, Transform::from_translation(pos)));
    }

    fn press(app: &mut App, key: KeyCode) {
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(key);
    }

    fn last_log(app: &mut App) -> Option<String> {
        let events = app.world.resource::<Events<LogMessage>>();
        let mut reader = events.get_reader();
        reader.read(events).last().map(|m| m.0.clone())
    }

    // ── handle_ranged_input: 진입 ──────────────────────────────────────────
    #[test]
    fn 패널이_열려있으면_원격조준_입력을_무시한다() {
        let mut app = ranged_input_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        press(&mut app, KeyCode::KeyF);
        app.update();
        assert!(!app.world.resource::<RangedTargeting>().active, "패널 열림 시 진입 안 함");
    }

    #[test]
    fn 상점패널이_열려있으면_원격조준_입력을_무시한다() {
        let mut app = ranged_input_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        press(&mut app, KeyCode::KeyF);
        app.update();
        assert!(!app.world.resource::<RangedTargeting>().active, "상점 열림 시 진입 안 함");
    }

    #[test]
    fn 도움말패널이_열려있으면_원격조준_입력을_무시한다() {
        let mut app = ranged_input_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        app.world.resource_mut::<HelpPanelOpen>().0 = true;
        press(&mut app, KeyCode::KeyF);
        app.update();
        assert!(!app.world.resource::<RangedTargeting>().active, "도움말 열림 시 진입 안 함");
    }

    #[test]
    fn 플레이어가_없으면_원격조준_입력은_조용히_반환한다() {
        let mut app = ranged_input_app(make_visible_map());
        // 플레이어 미스폰
        press(&mut app, KeyCode::KeyF);
        app.update(); // get_single Err → 반환 (panic 없음)
        assert!(!app.world.resource::<RangedTargeting>().active);
    }

    #[test]
    fn 활을_안들고_F를_누르면_경고만_뜨고_진입하지_않는다() {
        let mut app = ranged_input_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        app.world.resource_mut::<PlayerEquipment>().weapon = Some(WeaponKind::SWORD);
        press(&mut app, KeyCode::KeyF);
        app.update();
        assert!(!app.world.resource::<RangedTargeting>().active);
        assert_eq!(last_log(&mut app).as_deref(), Some("활을 장착해야 원격 공격이 가능하다."));
    }

    #[test]
    fn 활을_들고_F를_누르면_조준에_진입하고_커서가_생긴다() {
        let mut app = ranged_input_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        // 사거리 안 적 → 초기 커서가 그 적 위치
        app.world.resource_mut::<MonsterTiles>().0.insert((7, 7));
        press(&mut app, KeyCode::KeyF);
        app.update();
        assert!(app.world.resource::<RangedTargeting>().active);
        assert_eq!(app.world.resource::<RangedTargeting>().cursor, (7, 7));
        assert_eq!(app.world.query::<&RangedCursor>().iter(&app.world).count(), 1, "커서 1개 스폰");
    }

    #[test]
    fn 적이_없으면_F진입시_커서는_플레이어_위치에서_시작한다() {
        let mut app = ranged_input_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        press(&mut app, KeyCode::KeyF);
        app.update();
        assert!(app.world.resource::<RangedTargeting>().active);
        assert_eq!(app.world.resource::<RangedTargeting>().cursor, (5, 5), "적 없으면 플레이어 위치");
    }

    #[test]
    fn 비활성_상태에서_F외의_키는_아무것도_하지_않는다() {
        let mut app = ranged_input_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        press(&mut app, KeyCode::Enter);
        app.update();
        assert!(!app.world.resource::<RangedTargeting>().active, "F 아닌 키는 진입 안 함");
    }

    // ── handle_ranged_input: 활성 상태 입력 ─────────────────────────────────
    /// 이미 조준 활성 + 커서 1개가 있는 상태로 만든다.
    fn activate(app: &mut App, cursor: (usize, usize)) {
        {
            let mut t = app.world.resource_mut::<RangedTargeting>();
            t.active = true;
            t.cursor = cursor;
        }
        let coord = tile_to_world_coords(cursor.0, cursor.1);
        app.world.spawn((
            Text::from_section("+", TextStyle::default()),
            Transform::from_xyz(coord.x, coord.y, 2.0),
            RangedCursor,
        ));
    }

    #[test]
    fn 조준중_ESC를_누르면_취소되고_커서가_사라진다() {
        let mut app = ranged_input_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        activate(&mut app, (6, 6));
        press(&mut app, KeyCode::Escape);
        app.update();
        assert!(!app.world.resource::<RangedTargeting>().active);
        assert_eq!(app.world.query::<&RangedCursor>().iter(&app.world).count(), 0, "커서 despawn");
    }

    #[test]
    fn 조준중_Tab을_누르면_다음_타겟으로_순환한다() {
        let mut app = ranged_input_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        app.world.resource_mut::<MonsterTiles>().0.insert((6, 6));
        app.world.resource_mut::<MonsterTiles>().0.insert((8, 8));
        activate(&mut app, (6, 6)); // 현재 커서가 첫 타겟
        press(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(app.world.resource::<RangedTargeting>().cursor, (8, 8), "다음 타겟으로");
    }

    #[test]
    fn 조준중_Shift_Tab은_이전_타겟으로_역순환한다() {
        let mut app = ranged_input_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        app.world.resource_mut::<MonsterTiles>().0.insert((6, 6));
        app.world.resource_mut::<MonsterTiles>().0.insert((8, 8));
        activate(&mut app, (6, 6)); // idx 0 → 역순환하면 마지막 (8,8)
        press(&mut app, KeyCode::ShiftLeft);
        press(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(app.world.resource::<RangedTargeting>().cursor, (8, 8), "이전(wrap)으로");
    }

    #[test]
    fn 커서가_타겟목록에_없을때_Tab은_첫_타겟으로_간다() {
        let mut app = ranged_input_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        app.world.resource_mut::<MonsterTiles>().0.insert((6, 6));
        app.world.resource_mut::<MonsterTiles>().0.insert((8, 8));
        activate(&mut app, (1, 1)); // 타겟 목록에 없는 위치
        press(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(app.world.resource::<RangedTargeting>().cursor, (6, 6), "None → 첫 타겟");
    }

    #[test]
    fn 타겟이_하나도_없으면_Tab은_커서를_유지한다() {
        let mut app = ranged_input_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        activate(&mut app, (6, 6)); // 적 없음
        press(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(app.world.resource::<RangedTargeting>().cursor, (6, 6), "타겟 없으면 유지");
    }

    #[test]
    fn 조준중_방향키로_커서를_한_칸씩_이동한다() {
        let mut app = ranged_input_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        activate(&mut app, (5, 5));
        press(&mut app, KeyCode::ArrowRight); // dx +1
        press(&mut app, KeyCode::ArrowUp);    // dy +1
        app.update();
        assert_eq!(app.world.resource::<RangedTargeting>().cursor, (6, 6));
    }

    #[test]
    fn 조준중_WASD키로도_커서를_이동한다() {
        // A/D/W/S 대체 키로 dx/dy 가 모두 갱신되는지 (각 || 의 우변 분기 커버)
        let mut app = ranged_input_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        activate(&mut app, (5, 5));
        press(&mut app, KeyCode::KeyA); // dx -1
        press(&mut app, KeyCode::KeyD); // dx +1 (상쇄)
        press(&mut app, KeyCode::KeyW); // dy +1
        press(&mut app, KeyCode::KeyS); // dy -1 (상쇄)
        app.update();
        // A+D 상쇄, W+S 상쇄 → dx=0, dy=0 → 이동 없음 (각 키의 just_pressed True 분기만 친다)
        assert_eq!(app.world.resource::<RangedTargeting>().cursor, (5, 5), "상쇄되어 제자리");
    }

    #[test]
    fn 조준중_상하_방향키만_누르면_세로로만_이동한다() {
        // dx==0 이고 dy!=0 인 경우 — `dx != 0 || dy != 0` 의 우변(dy != 0)을 평가시킨다.
        let mut app = ranged_input_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        activate(&mut app, (5, 5));
        press(&mut app, KeyCode::ArrowUp); // dy +1, dx=0
        app.update();
        assert_eq!(app.world.resource::<RangedTargeting>().cursor, (5, 6), "세로로만 이동");
    }

    #[test]
    fn 커서_이동은_맵_경계에서_클램프된다() {
        let mut app = ranged_input_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        activate(&mut app, (0, 0)); // 좌하단 모서리
        press(&mut app, KeyCode::ArrowLeft); // dx -1 → 0 으로 클램프
        press(&mut app, KeyCode::ArrowDown); // dy -1 → 0 으로 클램프
        app.update();
        assert_eq!(app.world.resource::<RangedTargeting>().cursor, (0, 0), "0 미만으로 안 내려감");
    }

    #[test]
    fn 조준중_Enter로_사거리_시야_통과시_발사된다() {
        let mut app = ranged_input_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        activate(&mut app, (8, 5)); // 사거리 안, 시야 트임
        press(&mut app, KeyCode::Enter);
        app.update();
        assert_eq!(app.world.resource::<Events<FireProjectileEvent>>().len(), 1, "발사 이벤트");
        assert_eq!(app.world.resource::<Events<PlayerActedEvent>>().len(), 1, "행동 이벤트");
        assert!(!app.world.resource::<RangedTargeting>().active, "발사 후 비활성");
        assert_eq!(app.world.query::<&RangedCursor>().iter(&app.world).count(), 0, "커서 despawn");
    }

    #[test]
    fn 조준중_Enter로_사거리_밖을_쏘면_경고뜨고_발사되지_않는다() {
        let mut app = ranged_input_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        activate(&mut app, (20, 5)); // 거리 15 > 8
        press(&mut app, KeyCode::Enter);
        app.update();
        assert_eq!(app.world.resource::<Events<FireProjectileEvent>>().len(), 0);
        assert_eq!(last_log(&mut app).as_deref(), Some("사거리를 벗어났다."));
        assert!(app.world.resource::<RangedTargeting>().active, "실패 시 조준 유지");
    }

    #[test]
    fn 조준중_Enter로_벽_너머를_쏘면_장애물_경고가_뜬다() {
        let mut map = make_visible_map();
        map.set_tile(6, 5, TileKind::Wall); // (5,5)→(8,5) 사이 차단
        let mut app = ranged_input_app(map);
        spawn_player_at(&mut app, 5, 5);
        activate(&mut app, (8, 5));
        press(&mut app, KeyCode::Enter);
        app.update();
        assert_eq!(app.world.resource::<Events<FireProjectileEvent>>().len(), 0);
        assert_eq!(last_log(&mut app).as_deref(), Some("장애물에 막혔다."));
    }

    // ── handle_ranged_mouse: 우클릭 취소 + 가드 ─────────────────────────────
    fn ranged_mouse_app(map: Map) -> App {
        let mut app = App::new();
        app.init_resource::<RangedTargeting>()
            .insert_resource(ButtonInput::<MouseButton>::default())
            .init_resource::<EquipmentPanelOpen>()
            .init_resource::<ShopPanelOpen>()
            .init_resource::<HelpPanelOpen>()
            .insert_resource(MapResource(map))
            .insert_resource(build_test_registry())
            .add_event::<bevy::window::CursorMoved>()
            .add_event::<FireProjectileEvent>()
            .add_event::<PlayerActedEvent>()
            .add_event::<LogMessage>()
            .add_systems(Update, handle_ranged_mouse);
        app
    }

    #[test]
    fn 마우스_조준중_우클릭하면_취소되고_커서가_사라진다() {
        let mut app = ranged_mouse_app(make_visible_map());
        activate(&mut app, (6, 6));
        app.world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        app.update();
        assert!(!app.world.resource::<RangedTargeting>().active);
        assert_eq!(app.world.query::<&RangedCursor>().iter(&app.world).count(), 0);
    }

    #[test]
    fn 마우스_비활성_상태면_마우스_시스템은_아무것도_안_한다() {
        let mut app = ranged_mouse_app(make_visible_map());
        // active=false (기본)
        app.world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        app.update();
        assert!(!app.world.resource::<RangedTargeting>().active);
    }

    #[test]
    fn 마우스_조준중_상점패널이_열려있으면_마우스_입력을_무시한다() {
        let mut app = ranged_mouse_app(make_visible_map());
        activate(&mut app, (6, 6));
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        app.update();
        assert!(app.world.resource::<RangedTargeting>().active, "상점 열림 시 취소 안 됨");
    }

    #[test]
    fn 마우스_조준중_장비패널이_열려있으면_마우스_입력을_무시한다() {
        let mut app = ranged_mouse_app(make_visible_map());
        activate(&mut app, (6, 6));
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        app.update();
        assert!(app.world.resource::<RangedTargeting>().active, "장비창 열림 시 취소 안 됨");
    }

    #[test]
    fn 마우스_조준중_도움말패널이_열려있으면_마우스_입력을_무시한다() {
        let mut app = ranged_mouse_app(make_visible_map());
        activate(&mut app, (6, 6));
        app.world.resource_mut::<HelpPanelOpen>().0 = true;
        app.world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        app.update();
        assert!(app.world.resource::<RangedTargeting>().active, "도움말 열림 시 취소 안 됨");
    }

    #[test]
    fn 마우스_조준중_윈도우_카메라가_없으면_가드에서_조용히_반환한다() {
        // 헤드리스라 Window/Camera 없음 → 좌클릭해도 get_single Err 가드로 반환.
        // (viewport 변환 분기는 도달 불가 — decide_ranged_fire 로 판정 분기는 별도 단위 테스트)
        let mut app = ranged_mouse_app(make_visible_map());
        activate(&mut app, (8, 5));
        app.world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        assert_eq!(app.world.resource::<Events<FireProjectileEvent>>().len(), 0, "윈도우 없으면 발사 불가");
    }

    // ── update_ranged_cursor: 위치/색 갱신 ──────────────────────────────────
    fn ranged_cursor_app(map: Map) -> App {
        let mut app = App::new();
        app.init_resource::<RangedTargeting>()
            .insert_resource(MapResource(map))
            .add_systems(Update, update_ranged_cursor);
        app
    }

    fn cursor_color(app: &mut App, e: Entity) -> Color {
        app.world.get::<Text>(e).unwrap().sections[0].style.color
    }

    #[test]
    fn 비활성_상태면_커서를_갱신하지_않는다() {
        let mut app = ranged_cursor_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        let e = app.world.spawn((
            Text::from_section("+", TextStyle::default()),
            Transform::from_xyz(0.0, 0.0, 2.0),
            RangedCursor,
        )).id();
        app.update(); // active=false → 반환
        assert_eq!(app.world.get::<Transform>(e).unwrap().translation.x, 0.0, "위치 그대로");
    }

    #[test]
    fn 조준중_사거리_시야_통과하면_커서가_노란색이_된다() {
        let mut app = ranged_cursor_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        let e = app.world.spawn((
            Text::from_section("+", TextStyle { color: Color::WHITE, ..default() }),
            Transform::from_xyz(0.0, 0.0, 2.0),
            RangedCursor,
        )).id();
        {
            let mut t = app.world.resource_mut::<RangedTargeting>();
            t.active = true;
            t.cursor = (8, 5); // 사거리 안, 시야 트임
        }
        app.update();
        assert_eq!(cursor_color(&mut app, e), Color::rgb(1.0, 0.95, 0.4), "통과 → 노랑");
        // 위치도 타깃 타일 좌표로 이동
        let coord = tile_to_world_coords(8, 5);
        assert_eq!(app.world.get::<Transform>(e).unwrap().translation.x, coord.x);
    }

    #[test]
    fn 조준중_사거리_밖이면_커서가_빨간색이_된다() {
        let mut app = ranged_cursor_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        let e = app.world.spawn((
            Text::from_section("+", TextStyle { color: Color::WHITE, ..default() }),
            Transform::from_xyz(0.0, 0.0, 2.0),
            RangedCursor,
        )).id();
        {
            let mut t = app.world.resource_mut::<RangedTargeting>();
            t.active = true;
            t.cursor = (20, 5); // 사거리 밖
        }
        app.update();
        assert_eq!(cursor_color(&mut app, e), Color::rgb(1.0, 0.3, 0.3), "실패 → 빨강");
    }

    #[test]
    fn 조준중_사거리_안이라도_벽에_막히면_커서가_빨간색이_된다() {
        // in_range=true 이지만 los=false → `in_range && los` 의 los 분기를 False 로 평가.
        let mut map = make_visible_map();
        map.set_tile(6, 5, TileKind::Wall); // (5,5)→(8,5) 사이 차단
        let mut app = ranged_cursor_app(map);
        spawn_player_at(&mut app, 5, 5);
        let e = app.world.spawn((
            Text::from_section("+", TextStyle { color: Color::WHITE, ..default() }),
            Transform::from_xyz(0.0, 0.0, 2.0),
            RangedCursor,
        )).id();
        {
            let mut t = app.world.resource_mut::<RangedTargeting>();
            t.active = true;
            t.cursor = (8, 5); // 사거리 안, 시야 막힘
        }
        app.update();
        assert_eq!(cursor_color(&mut app, e), Color::rgb(1.0, 0.3, 0.3), "막힘 → 빨강");
    }

    #[test]
    fn 색이_이미_같으면_커서색을_다시_쓰지_않는다() {
        // 두 번째 update 에서 색이 동일하면 `color != color` 분기가 False — 대입 생략.
        let mut app = ranged_cursor_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        let e = app.world.spawn((
            Text::from_section("+", TextStyle { color: Color::WHITE, ..default() }),
            Transform::from_xyz(0.0, 0.0, 2.0),
            RangedCursor,
        )).id();
        {
            let mut t = app.world.resource_mut::<RangedTargeting>();
            t.active = true;
            t.cursor = (8, 5);
        }
        app.update(); // 1회차: WHITE → 노랑 (다름 → 대입)
        assert_eq!(cursor_color(&mut app, e), Color::rgb(1.0, 0.95, 0.4));
        app.update(); // 2회차: 이미 노랑 → 동일 → 대입 생략
        assert_eq!(cursor_color(&mut app, e), Color::rgb(1.0, 0.95, 0.4), "동일 색 유지");
    }

    #[test]
    fn 조준중_커서_엔티티가_없으면_갱신은_조용히_반환한다() {
        let mut app = ranged_cursor_app(make_visible_map());
        spawn_player_at(&mut app, 5, 5);
        app.world.resource_mut::<RangedTargeting>().active = true;
        app.update(); // 커서 없음 → get_single_mut Err → 반환 (panic 없음)
    }

    #[test]
    fn 조준중_플레이어가_없으면_커서_위치만_갱신하고_색은_그대로다() {
        let mut app = ranged_cursor_app(make_visible_map());
        // 플레이어 미스폰
        let e = app.world.spawn((
            Text::from_section("+", TextStyle { color: Color::WHITE, ..default() }),
            Transform::from_xyz(0.0, 0.0, 2.0),
            RangedCursor,
        )).id();
        {
            let mut t = app.world.resource_mut::<RangedTargeting>();
            t.active = true;
            t.cursor = (8, 5);
        }
        app.update();
        // 위치는 갱신되지만 player get_single Err 로 색 분기 전에 반환
        let coord = tile_to_world_coords(8, 5);
        assert_eq!(app.world.get::<Transform>(e).unwrap().translation.x, coord.x);
        assert_eq!(cursor_color(&mut app, e), Color::WHITE, "플레이어 없으면 색 미변경");
    }

    // ── spawn_cursor: AssetServer 하네스 ────────────────────────────────────
    #[test]
    fn 커서_스폰은_RangedCursor_텍스트_엔티티를_만든다() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app.add_systems(Update, |mut commands: Commands, asset_server: Res<AssetServer>| {
            spawn_cursor(&mut commands, &asset_server, (4, 7));
        });
        app.update();
        let mut q = app.world.query::<(&RangedCursor, &Text, &Transform)>();
        let (_, text, transform) = q.iter(&app.world).next().expect("커서 엔티티가 있어야 한다");
        assert_eq!(text.sections[0].value, "+");
        let coord = tile_to_world_coords(4, 7);
        assert_eq!(transform.translation.x, coord.x);
        assert_eq!(transform.translation.y, coord.y);
    }

    // ── RangedPlugin::build ─────────────────────────────────────────────────
    #[test]
    fn 원격플러그인이_정상적으로_빌드된다() {
        let mut app = App::new();
        app.add_plugins(RangedPlugin);
        assert!(app.world.get_resource::<RangedTargeting>().is_some(), "리소스 초기화됨");
    }
}
