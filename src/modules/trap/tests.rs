use super::*;
use bevy::prelude::*;
use rand::SeedableRng;
use crate::modules::map::{Map, MapResource, MapType, Rect, TileKind, UsedSpawnTiles};
use crate::modules::combat::CombatStats;
use crate::modules::elemental::{Element, ElementalStatus};
use crate::modules::monster::Monster;

// ── 순수 함수: TrapKind 메타 ───────────────────────────────────────────────────

#[test]
fn 모든_함정종류의_한글이름이_올바르게_반환된다() {
    assert_eq!(TrapKind::Spike.name_ko(),    "가시 함정");
    assert_eq!(TrapKind::Poison.name_ko(),   "독 함정");
    assert_eq!(TrapKind::Alarm.name_ko(),    "경보 함정");
    assert_eq!(TrapKind::Teleport.name_ko(), "전이 함정");
}

#[test]
fn 모든_함정종류의_글리프가_서로_다르다() {
    let glyphs = [
        TrapKind::Spike.glyph(),
        TrapKind::Poison.glyph(),
        TrapKind::Alarm.glyph(),
        TrapKind::Teleport.glyph(),
    ];
    for i in 0..glyphs.len() {
        for j in (i + 1)..glyphs.len() {
            assert_ne!(glyphs[i], glyphs[j], "글리프가 겹치면 안 된다");
        }
    }
}

#[test]
fn 모든_함정종류의_색은_각자_정의된_값을_반환한다() {
    // 색 함수의 모든 match arm 실행 보장.
    assert_eq!(TrapKind::Spike.color(),    Color::rgb(0.8, 0.8, 0.85));
    assert_eq!(TrapKind::Poison.color(),   Color::rgb(0.4, 0.85, 0.3));
    assert_eq!(TrapKind::Alarm.color(),    Color::rgb(1.0, 0.85, 0.1));
    assert_eq!(TrapKind::Teleport.color(), Color::rgb(0.7, 0.4, 1.0));
}

#[test]
fn 가시함정의_피해량은_양수이다() {
    assert!(TrapKind::Spike.spike_damage() > 0);
}

#[test]
fn 가시와_전이는_일회성이고_독과_경보는_지속된다() {
    assert!(TrapKind::Spike.is_one_shot(),     "가시는 1회성");
    assert!(TrapKind::Teleport.is_one_shot(),  "전이는 1회성");
    assert!(!TrapKind::Poison.is_one_shot(),   "독은 지속");
    assert!(!TrapKind::Alarm.is_one_shot(),    "경보는 지속");
}

// ── 순수 함수: 발동 판정 ───────────────────────────────────────────────────────

#[test]
fn 대상이_함정과_같은_타일에_있으면_발동한다() {
    assert!(trap_triggers_at(5, 5, 5, 5), "같은 타일이면 발동");
}

#[test]
fn 대상이_함정과_다른_타일에_있으면_발동하지_않는다() {
    assert!(!trap_triggers_at(5, 5, 6, 5), "x 다름 → 미발동");
    assert!(!trap_triggers_at(5, 5, 5, 6), "y 다름 → 미발동");
}

// ── 순수 함수: 노출 판정 ───────────────────────────────────────────────────────

#[test]
fn 이미_노출된_함정은_거리와_무관하게_노출_상태로_본다() {
    assert!(should_reveal(false, 0, 0, 40, 40, REVEAL_DIST), "노출 함정은 항상 노출");
}

#[test]
fn 숨김함정은_인접하면_노출되고_멀면_숨겨진다() {
    // 함정(5,5), 플레이어가 인접(6,5) → 노출, 멀리(8,8) → 숨김.
    assert!(should_reveal(true, 5, 5, 6, 5, REVEAL_DIST), "인접하면 노출");
    assert!(should_reveal(true, 5, 5, 6, 6, REVEAL_DIST), "대각 인접도 노출(체비쇼프)");
    assert!(!should_reveal(true, 5, 5, 8, 8, REVEAL_DIST), "멀면 숨김 유지");
}

// ── 순수 함수: 전이 목적지 ─────────────────────────────────────────────────────

fn full_floor_map(w: usize, h: usize) -> Map {
    let mut m = Map::new(w, h);
    for y in 1..h - 1 { for x in 1..w - 1 { m.set_tile(x, y, TileKind::Floor); } }
    m.map_type = MapType::Dungeon;
    m.rooms.push(Rect::new(1, 1, w - 2, h - 2));
    m
}

#[test]
fn 전이목적지는_현재위치를_제외한_통과타일을_반환한다() {
    let map = full_floor_map(10, 10);
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    for _ in 0..30 {
        let (x, y) = random_teleport_destination(&map, (3, 3), &mut rng).unwrap();
        assert!(map.get_tile(x, y).is_walkable(), "목적지는 통과 타일");
        assert_ne!((x, y), (3, 3), "현재 위치는 제외");
    }
}

#[test]
fn 통과타일이_하나도_없으면_전이목적지는_없다() {
    let map = Map::new(8, 8); // 전부 Wall
    let mut rng = rand::rngs::StdRng::seed_from_u64(1);
    assert!(random_teleport_destination(&map, (0, 0), &mut rng).is_none());
}

// ── 시스템 하네스 ──────────────────────────────────────────────────────────────

/// trigger_traps 단독 실행용 App. 전부 Floor 인 던전 맵을 둔다.
fn trigger_app() -> App {
    let mut app = App::new();
    app.add_event::<PlayerActedEvent>()
        .add_event::<ElementalApplyEvent>()
        .add_event::<PlayerDetectedEvent>()
        .add_event::<TrapTriggeredEvent>()
        .add_event::<LogMessage>();
    app.insert_resource(MapResource(full_floor_map(20, 20)));
    app.add_systems(Update, trigger_traps);
    app
}

fn spawn_player_at(app: &mut App, tile: (usize, usize)) -> Entity {
    let pos = tile_to_world_coords(tile.0, tile.1);
    app.world.spawn((
        Player,
        Transform::from_xyz(pos.x, pos.y, 1.0),
        CombatStats { hp: 30, max_hp: 30, mp: 0, max_mp: 0, attack: 5, defense: 1 },
        ElementalStatus::default(),
    )).id()
}

fn spawn_trap_at(app: &mut App, kind: TrapKind, tile: (usize, usize), hidden: bool) -> Entity {
    let vis = if hidden { Visibility::Hidden } else { Visibility::Visible };
    app.world.spawn((Trap { kind, tile_x: tile.0, tile_y: tile.1, hidden }, vis)).id()
}

fn detected_count(app: &App) -> usize {
    app.world.resource::<Events<PlayerDetectedEvent>>().len()
}

fn elemental_targets(app: &mut App) -> Vec<(Entity, Element)> {
    let events = app.world.resource::<Events<ElementalApplyEvent>>();
    let mut r = events.get_reader();
    r.read(events).map(|e| (e.target, e.element)).collect()
}

fn triggered_kinds(app: &mut App) -> Vec<TrapKind> {
    let events = app.world.resource::<Events<TrapTriggeredEvent>>();
    let mut r = events.get_reader();
    r.read(events).map(|e| e.kind).collect()
}

// ── trigger_traps: 턴 이벤트 게이트 ─────────────────────────────────────────────

#[test]
fn 턴_이벤트가_없으면_함정은_발동하지_않는다() {
    let mut app = trigger_app();
    spawn_player_at(&mut app, (5, 5));
    spawn_trap_at(&mut app, TrapKind::Spike, (5, 5), false);
    app.update(); // PlayerActedEvent 없음
    assert!(triggered_kinds(&mut app).is_empty(), "턴 이벤트 없으면 미발동");
}

#[test]
fn 플레이어가_함정타일에_없으면_발동하지_않는다() {
    let mut app = trigger_app();
    spawn_player_at(&mut app, (5, 5));
    spawn_trap_at(&mut app, TrapKind::Spike, (8, 8), false);
    app.world.send_event(PlayerActedEvent);
    app.update();
    assert!(triggered_kinds(&mut app).is_empty(), "다른 타일이면 미발동");
}

// ── trigger_traps: 종류별 효과 ──────────────────────────────────────────────────

#[test]
fn 가시함정을_밟으면_플레이어가_피해를_입고_함정은_사라진다() {
    let mut app = trigger_app();
    let p = spawn_player_at(&mut app, (5, 5));
    let t = spawn_trap_at(&mut app, TrapKind::Spike, (5, 5), false);
    app.world.send_event(PlayerActedEvent);
    app.update();
    assert_eq!(app.world.get::<CombatStats>(p).unwrap().hp, 30 - TrapKind::Spike.spike_damage());
    assert!(app.world.get_entity(t).is_none(), "가시(1회성)는 발동 후 despawn");
    assert_eq!(triggered_kinds(&mut app), vec![TrapKind::Spike]);
}

#[test]
fn 가시함정으로_체력이_0이되면_플레이어가_사망한다() {
    let mut app = trigger_app();
    let p = spawn_player_at(&mut app, (5, 5));
    app.world.get_mut::<CombatStats>(p).unwrap().hp = 3; // < spike_damage
    spawn_trap_at(&mut app, TrapKind::Spike, (5, 5), false);
    app.world.send_event(PlayerActedEvent);
    app.update();
    assert!(app.world.entity(p).contains::<Defeated>(), "치명타면 Defeated");
}

#[test]
fn 독함정을_밟으면_플레이어에게_독_원소가_부여된다() {
    let mut app = trigger_app();
    let p = spawn_player_at(&mut app, (5, 5));
    let t = spawn_trap_at(&mut app, TrapKind::Poison, (5, 5), false);
    app.world.send_event(PlayerActedEvent);
    app.update();
    assert_eq!(elemental_targets(&mut app), vec![(p, Element::Poison)], "독 원소 부여");
    assert!(app.world.get_entity(t).is_some(), "독(지속)은 발동 후에도 남는다");
}

#[test]
fn 경보함정을_밟으면_탐지이벤트가_발행된다() {
    // stealth_blown 은 quest 모듈의 handle_player_detected 가 PlayerDetectedEvent 로 set.
    let mut app = trigger_app();
    spawn_player_at(&mut app, (5, 5));
    spawn_trap_at(&mut app, TrapKind::Alarm, (5, 5), false);
    app.world.send_event(PlayerActedEvent);
    app.update();
    assert_eq!(detected_count(&app), 1, "경보 → PlayerDetectedEvent 발행(가드 경계/stealth_blown)");
}

#[test]
fn 전이함정을_밟으면_플레이어가_다른_통과타일로_이동한다() {
    let mut app = trigger_app();
    let p = spawn_player_at(&mut app, (5, 5));
    let before = app.world.get::<Transform>(p).unwrap().translation;
    spawn_trap_at(&mut app, TrapKind::Teleport, (5, 5), false);
    app.world.send_event(PlayerActedEvent);
    app.update();
    let after = app.world.get::<Transform>(p).unwrap().translation;
    assert_ne!(before, after, "전이는 위치를 바꾼다");
    let (nx, ny) = world_to_tile_coords(after);
    assert!(app.world.resource::<MapResource>().map().get_tile(nx, ny).is_walkable(),
        "전이 목적지는 통과 타일");
}

#[test]
fn 전이할_빈타일이_없으면_플레이어는_제자리에_남는다() {
    // 통과 가능한 타일이 플레이어가 선 단 한 칸뿐이면 목적지 None → 이동 없음.
    let mut app = trigger_app();
    let mut map = Map::new(5, 5); // 전부 Wall
    map.map_type = MapType::Dungeon;
    map.set_tile(2, 2, TileKind::Floor); // 유일한 통과 타일
    app.insert_resource(MapResource(map));
    let p = spawn_player_at(&mut app, (2, 2));
    let before = app.world.get::<Transform>(p).unwrap().translation;
    spawn_trap_at(&mut app, TrapKind::Teleport, (2, 2), false);
    app.world.send_event(PlayerActedEvent);
    app.update();
    let after = app.world.get::<Transform>(p).unwrap().translation;
    assert_eq!(before, after, "갈 곳이 없으면 제자리");
}

// ── trigger_traps: 몬스터 발동 ──────────────────────────────────────────────────

#[test]
fn 몬스터가_경보함정을_밟아도_경계가_울린다() {
    let mut app = trigger_app();
    spawn_player_at(&mut app, (1, 1)); // 함정과 다른 타일
    app.world.spawn(Monster {
        name: "고블린".into(), tile_x: 5, tile_y: 5,
        vision_radius: 6, alert_turns: 0, slot_idx: 0,
    });
    spawn_trap_at(&mut app, TrapKind::Alarm, (5, 5), false);
    app.world.send_event(PlayerActedEvent);
    app.update();
    assert_eq!(detected_count(&app), 1, "몬스터가 밟은 경보도 탐지 발행");
    assert_eq!(triggered_kinds(&mut app), vec![TrapKind::Alarm]);
}

#[test]
fn 플레이어가_없어도_몬스터가_밟은_경보는_울린다() {
    // player_info == None 분기: 플레이어 엔티티가 없을 때도 몬스터 발동 경보는 동작.
    let mut app = trigger_app();
    app.world.spawn(Monster {
        name: "고블린".into(), tile_x: 5, tile_y: 5,
        vision_radius: 6, alert_turns: 0, slot_idx: 0,
    });
    spawn_trap_at(&mut app, TrapKind::Alarm, (5, 5), false);
    app.world.send_event(PlayerActedEvent);
    app.update();
    assert_eq!(detected_count(&app), 1, "플레이어 없어도 몬스터 경보 발동");
}

#[test]
fn 몬스터가_가시함정을_밟으면_함정은_사라지지만_플레이어는_피해없음() {
    let mut app = trigger_app();
    let p = spawn_player_at(&mut app, (1, 1));
    app.world.spawn(Monster {
        name: "고블린".into(), tile_x: 5, tile_y: 5,
        vision_radius: 6, alert_turns: 0, slot_idx: 0,
    });
    let t = spawn_trap_at(&mut app, TrapKind::Spike, (5, 5), false);
    app.world.send_event(PlayerActedEvent);
    app.update();
    assert_eq!(app.world.get::<CombatStats>(p).unwrap().hp, 30, "플레이어는 멀리 있어 피해 없음");
    assert!(app.world.get_entity(t).is_none(), "몬스터 발동도 1회성 함정을 소비");
}

// ── trigger_traps: 발동 시 숨김 노출 ───────────────────────────────────────────

#[test]
fn 숨김함정도_밟으면_노출되며_발동한다() {
    let mut app = trigger_app();
    spawn_player_at(&mut app, (5, 5));
    let t = spawn_trap_at(&mut app, TrapKind::Poison, (5, 5), true);
    app.world.send_event(PlayerActedEvent);
    app.update();
    assert!(!app.world.get::<Trap>(t).unwrap().hidden, "밟으면 노출된다");
    assert_eq!(triggered_kinds(&mut app), vec![TrapKind::Poison]);
}

// ── reveal_hidden_traps ────────────────────────────────────────────────────────

fn reveal_app() -> App {
    let mut app = App::new();
    app.add_systems(Update, reveal_hidden_traps);
    app
}

#[test]
fn 숨김함정에_플레이어가_근접하면_노출된다() {
    let mut app = reveal_app();
    // 플레이어 (6,5), 함정 (5,5) → 인접.
    let pos = tile_to_world_coords(6, 5);
    app.world.spawn((Player, Transform::from_xyz(pos.x, pos.y, 1.0)));
    let t = app.world.spawn((
        Trap { kind: TrapKind::Spike, tile_x: 5, tile_y: 5, hidden: true },
        Visibility::Hidden,
    )).id();
    app.update();
    assert!(!app.world.get::<Trap>(t).unwrap().hidden, "근접하면 노출");
    assert_eq!(*app.world.get::<Visibility>(t).unwrap(), Visibility::Visible);
}

#[test]
fn 멀리있는_숨김함정은_노출되지_않는다() {
    let mut app = reveal_app();
    let pos = tile_to_world_coords(15, 15);
    app.world.spawn((Player, Transform::from_xyz(pos.x, pos.y, 1.0)));
    let t = app.world.spawn((
        Trap { kind: TrapKind::Spike, tile_x: 5, tile_y: 5, hidden: true },
        Visibility::Hidden,
    )).id();
    app.update();
    assert!(app.world.get::<Trap>(t).unwrap().hidden, "멀면 숨김 유지");
}

#[test]
fn 이미_노출된_함정은_노출처리를_건너뛴다() {
    // hidden=false 인 함정은 continue 분기 — 상태 변화 없음.
    let mut app = reveal_app();
    let pos = tile_to_world_coords(15, 15);
    app.world.spawn((Player, Transform::from_xyz(pos.x, pos.y, 1.0)));
    let t = app.world.spawn((
        Trap { kind: TrapKind::Spike, tile_x: 5, tile_y: 5, hidden: false },
        Visibility::Visible,
    )).id();
    app.update();
    assert!(!app.world.get::<Trap>(t).unwrap().hidden);
}

#[test]
fn 플레이어가_없으면_노출처리는_조용히_종료된다() {
    let mut app = reveal_app();
    let t = app.world.spawn((
        Trap { kind: TrapKind::Spike, tile_x: 5, tile_y: 5, hidden: true },
        Visibility::Hidden,
    )).id();
    app.update(); // 플레이어 없음 → get_single Err → 조기 종료
    assert!(app.world.get::<Trap>(t).unwrap().hidden, "플레이어 없으면 변화 없음");
}

// ── handle_spawn_trap (배치/스폰) ──────────────────────────────────────────────

fn spawn_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin::default());
    app.init_asset::<Font>();
    app.add_event::<SpawnTrapEvent>();
    app.init_resource::<UsedSpawnTiles>();
    app.insert_resource(MapResource(full_floor_map(20, 20)));
    app.add_systems(Update, handle_spawn_trap);
    app
}

fn trap_count(app: &mut App) -> usize {
    app.world.query::<&Trap>().iter(&app.world).count()
}

#[test]
fn 함정스폰이벤트는_요청한_개수만큼_함정을_배치한다() {
    let mut app = spawn_app();
    app.world.send_event(SpawnTrapEvent { kind: TrapKind::Alarm, count: 3, hidden: true });
    app.update();
    assert_eq!(trap_count(&mut app), 3, "3개 요청 → 3개 배치");
}

#[test]
fn 스폰된_경보함정은_경보종류로_배치된다() {
    let mut app = spawn_app();
    app.world.send_event(SpawnTrapEvent { kind: TrapKind::Alarm, count: 1, hidden: false });
    app.update();
    let kind = app.world.query::<&Trap>().iter(&app.world).next().map(|t| t.kind);
    assert_eq!(kind, Some(TrapKind::Alarm));
}

#[test]
fn 숨김으로_스폰한_함정은_숨김상태이고_글리프가_가려진다() {
    let mut app = spawn_app();
    app.world.send_event(SpawnTrapEvent { kind: TrapKind::Spike, count: 1, hidden: true });
    app.update();
    let mut q = app.world.query::<(&Trap, &Visibility)>();
    let (trap, vis) = q.iter(&app.world).next().unwrap();
    assert!(trap.hidden, "숨김 스폰 → hidden");
    assert_eq!(*vis, Visibility::Hidden, "숨김 함정은 글리프 가림");
}

#[test]
fn 통과타일이_없으면_함정은_배치되지_않는다() {
    let mut app = spawn_app();
    app.insert_resource(MapResource(Map::new(8, 8))); // 전부 Wall, rooms 없음
    app.world.send_event(SpawnTrapEvent { kind: TrapKind::Spike, count: 2, hidden: false });
    app.update();
    assert_eq!(trap_count(&mut app), 0, "통과타일 없으면 배치 실패");
}

// ── 플러그인 빌드 ───────────────────────────────────────────────────────────────

#[test]
fn 함정플러그인이_정상적으로_빌드된다() {
    let mut app = App::new();
    app.add_plugins(TrapPlugin);
    assert!(app.world.get_resource::<Events<SpawnTrapEvent>>().is_some());
    assert!(app.world.get_resource::<Events<TrapTriggeredEvent>>().is_some());
}
