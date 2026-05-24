#![allow(non_snake_case)]
use super::*;
use crate::modules::trap::DISARM_CHANCE_NO_TOOL;
use crate::modules::map::{Map, MapType, Rect, TileKind, tile_to_world_coords};
use crate::modules::item::ConsumableKind;

// ── 순수 함수: 설치 가능 판정 ──────────────────────────────────────────────────

fn full_floor_map(w: usize, h: usize) -> Map {
    let mut m = Map::new(w, h);
    for y in 1..h - 1 { for x in 1..w - 1 { m.set_tile(x, y, TileKind::Floor); } }
    m.map_type = MapType::Dungeon;
    m.rooms.push(Rect::new(1, 1, w - 2, h - 2));
    m
}

#[test]
fn 빈_통과타일에는_함정을_설치할_수_있다() {
    let map = full_floor_map(10, 10);
    let occupied = HashSet::new();
    assert!(can_place_trap(&map, &occupied, 5, 5), "빈 바닥타일엔 설치 가능");
}

#[test]
fn 벽타일에는_함정을_설치할_수_없다() {
    let map = full_floor_map(10, 10); // 테두리(0,*)는 Wall
    let occupied = HashSet::new();
    assert!(!can_place_trap(&map, &occupied, 0, 0), "벽엔 설치 불가");
}

#[test]
fn 점유된_타일에는_함정을_설치할_수_없다() {
    let map = full_floor_map(10, 10);
    let mut occupied = HashSet::new();
    occupied.insert((5, 5));
    assert!(!can_place_trap(&map, &occupied, 5, 5), "점유 타일엔 설치 불가");
}

#[test]
fn 맵_범위_밖_좌표에는_함정을_설치할_수_없다() {
    let map = full_floor_map(10, 10);
    let occupied = HashSet::new();
    assert!(!can_place_trap(&map, &occupied, 10, 5), "x 가 width 이상이면 불가");
    assert!(!can_place_trap(&map, &occupied, 5, 10), "y 가 height 이상이면 불가");
}

// ── 순수 함수: 해제 성공 판정 ──────────────────────────────────────────────────

#[test]
fn 해제_도구가_있으면_확률과_무관하게_확정으로_성공한다() {
    assert!(disarm_succeeds(true, 0.0), "도구 있으면 roll 0.0 도 성공");
    assert!(disarm_succeeds(true, 0.99), "도구 있으면 roll 0.99 도 성공");
}

#[test]
fn 해제_도구가_없으면_roll이_임계값_미만일때만_성공한다() {
    assert!(disarm_succeeds(false, 0.0), "roll 0.0 < 임계 → 성공");
    assert!(disarm_succeeds(false, DISARM_CHANCE_NO_TOOL - 0.01), "임계 직전 → 성공");
    assert!(!disarm_succeeds(false, DISARM_CHANCE_NO_TOOL), "임계값 자체는 실패(미만 아님)");
    assert!(!disarm_succeeds(false, 0.99), "roll 0.99 → 실패");
}

// ── 순수 헬퍼: 소모품 개수 ─────────────────────────────────────────────────────

#[test]
fn 인벤토리에_없는_소모품의_개수는_0이다() {
    let inv = PlayerInventory::default();
    assert_eq!(consumable_count(&inv, TRAP_KIT_ID), 0);
}

#[test]
fn 인벤토리에_있는_소모품의_개수를_정확히_센다() {
    let mut inv = PlayerInventory::default();
    inv.consumables.push((ConsumableKind(TRAP_KIT_ID), 3));
    assert_eq!(consumable_count(&inv, TRAP_KIT_ID), 3);
}

// ── 발동 구분: 플레이어 함정은 플레이어를 안 밟게 ───────────────────────────────
// (기존 trigger_traps 경로 재사용 — PlayerTrap 마커만 추가)

use crate::modules::trap::{Trap, TrapKind, PlayerTrap, trigger_traps};
use crate::modules::combat::CombatStats;
use crate::modules::elemental::{ElementalApplyEvent, ElementalStatus};
use crate::modules::monster::{Monster, PlayerDetectedEvent};
use crate::modules::map::{MapResource, PlayerActedEvent};
use crate::modules::trap::TrapTriggeredEvent;

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
        Facing(IVec2::new(1, 0)),
        CombatStats { hp: 30, max_hp: 30, mp: 0, max_mp: 0, attack: 5, defense: 1 },
        ElementalStatus::default(),
    )).id()
}

fn triggered_kinds(app: &mut App) -> Vec<TrapKind> {
    let events = app.world.resource::<Events<TrapTriggeredEvent>>();
    let mut r = events.get_reader();
    r.read(events).map(|e| e.kind).collect()
}

#[test]
fn 플레이어함정은_플레이어가_같은타일에_있어도_발동하지_않는다() {
    let mut app = trigger_app();
    let p = spawn_player_at(&mut app, (5, 5));
    app.world.spawn((
        Trap { kind: TrapKind::Spike, tile_x: 5, tile_y: 5, hidden: false },
        Visibility::Visible,
        PlayerTrap,
    ));
    app.world.send_event(PlayerActedEvent);
    app.update();
    assert_eq!(app.world.get::<CombatStats>(p).unwrap().hp, 30, "아군 함정은 플레이어에게 피해 없음");
    assert!(triggered_kinds(&mut app).is_empty(), "플레이어 발동 없음");
}

#[test]
fn 플레이어함정을_몬스터가_밟으면_발동한다() {
    let mut app = trigger_app();
    spawn_player_at(&mut app, (1, 1)); // 함정과 다른 타일
    app.world.spawn(Monster {
        name: "고블린".into(), tile_x: 5, tile_y: 5,
        vision_radius: 6, alert_turns: 0, slot_idx: 0,
    });
    let t = app.world.spawn((
        Trap { kind: TrapKind::Spike, tile_x: 5, tile_y: 5, hidden: false },
        Visibility::Visible,
        PlayerTrap,
    )).id();
    app.world.send_event(PlayerActedEvent);
    app.update();
    assert_eq!(triggered_kinds(&mut app), vec![TrapKind::Spike], "몬스터 진입으로 발동");
    assert!(app.world.get_entity(t).is_none(), "가시(1회성) 함정은 발동 후 despawn");
}

#[test]
fn 일반함정은_여전히_플레이어가_밟으면_발동한다() {
    // PlayerTrap 마커가 없으면 기존대로 플레이어 발동(회귀 방지).
    let mut app = trigger_app();
    let p = spawn_player_at(&mut app, (5, 5));
    app.world.spawn((
        Trap { kind: TrapKind::Spike, tile_x: 5, tile_y: 5, hidden: false },
        Visibility::Visible,
    ));
    app.world.send_event(PlayerActedEvent);
    app.update();
    assert_eq!(app.world.get::<CombatStats>(p).unwrap().hp, 30 - TrapKind::Spike.spike_damage());
}

// ── 설치 시스템 하네스 ──────────────────────────────────────────────────────────

use crate::modules::map::{OccupiedTiles, MonsterTiles};
use crate::modules::item::EquipmentPanelOpen;
use crate::modules::ui::help::HelpPanelOpen;
use crate::modules::ui::shop::ShopPanelOpen;
use crate::modules::ui::guide_panel::GuidePanelOpen;

struct PlaceHarness {
    app: App,
}

impl PlaceHarness {
    fn new(player_tile: (usize, usize), facing: IVec2, kit_count: u32) -> Self {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app.init_resource::<OccupiedTiles>();
        app.init_resource::<MonsterTiles>();
        app.init_resource::<EquipmentPanelOpen>();
        app.init_resource::<ShopPanelOpen>();
        app.init_resource::<HelpPanelOpen>();
        app.init_resource::<GuidePanelOpen>();
        app.add_event::<PlayerActedEvent>();
        app.add_event::<LogMessage>();
        app.insert_resource(MapResource(full_floor_map(20, 20)));

        let mut inv = PlayerInventory::default();
        if kit_count > 0 {
            inv.consumables.push((ConsumableKind(TRAP_KIT_ID), kit_count));
        }
        app.insert_resource(inv);

        let pos = tile_to_world_coords(player_tile.0, player_tile.1);
        app.world.spawn((
            Player,
            Transform::from_xyz(pos.x, pos.y, 1.0),
            Facing(facing),
            CombatStats { hp: 30, max_hp: 30, mp: 0, max_mp: 0, attack: 5, defense: 1 },
        ));

        app.add_systems(Update, handle_place_trap);
        Self { app }
    }

    fn press(&mut self, key: KeyCode) {
        self.app.world.resource_mut::<ButtonInput<KeyCode>>().press(key);
    }
    fn update(&mut self) { self.app.update(); }
    fn kit_count(&self) -> u32 {
        consumable_count(self.app.world.resource::<PlayerInventory>(), TRAP_KIT_ID)
    }
    fn player_traps(&mut self) -> Vec<(usize, usize)> {
        self.app.world.query::<(&Trap, &PlayerTrap)>()
            .iter(&self.app.world)
            .map(|(t, _)| (t.tile_x, t.tile_y))
            .collect()
    }
    fn acted_count(&mut self) -> usize {
        let events = self.app.world.resource::<Events<PlayerActedEvent>>();
        let mut r = events.get_reader();
        r.read(events).count()
    }
    fn last_log(&mut self) -> Option<String> {
        let events = self.app.world.resource::<Events<LogMessage>>();
        let mut r = events.get_reader();
        r.read(events).last().map(|m| m.0.clone())
    }
}

#[test]
fn 함정키트로_빈_정면타일에_설치하면_플레이어함정이_생기고_키트가_소모된다() {
    // (5,5) 에서 오른쪽을 보면 정면은 (6,5).
    let mut h = PlaceHarness::new((5, 5), IVec2::new(1, 0), 3);
    h.press(KEY_PLACE_TRAP);
    h.update();
    assert_eq!(h.player_traps(), vec![(6, 5)], "정면 타일에 플레이어 함정 설치");
    assert_eq!(h.kit_count(), 2, "키트 1개 소모");
    assert_eq!(h.acted_count(), 1, "턴 소비");
}

#[test]
fn 설치된_플레이어함정에는_PlayerTrap_마커가_붙고_노출상태다() {
    let mut h = PlaceHarness::new((5, 5), IVec2::new(1, 0), 1);
    h.press(KEY_PLACE_TRAP);
    h.update();
    let mut q = h.app.world.query::<(&Trap, &PlayerTrap, &Visibility)>();
    let (trap, _, vis) = q.iter(&h.app.world).next().expect("설치된 플레이어 함정");
    assert!(!trap.hidden, "플레이어는 위치를 알므로 노출 설치");
    assert_eq!(*vis, Visibility::Visible);
    assert_eq!(trap.kind, PLAYER_TRAP_KIND);
}

#[test]
fn 함정키트가_없으면_설치되지_않고_경고만_뜬다() {
    let mut h = PlaceHarness::new((5, 5), IVec2::new(1, 0), 0);
    h.press(KEY_PLACE_TRAP);
    h.update();
    assert!(h.player_traps().is_empty(), "키트 없으면 설치 안 됨");
    assert_eq!(h.acted_count(), 0, "턴 소비 없음");
    assert!(h.last_log().unwrap().contains("함정 키트가 없다"));
}

#[test]
fn 정면이_벽이면_설치되지_않고_키트도_유지된다() {
    // (1,5) 에서 왼쪽(-1,0)을 보면 정면은 (0,5) = 테두리 벽.
    let mut h = PlaceHarness::new((1, 5), IVec2::new(-1, 0), 2);
    h.press(KEY_PLACE_TRAP);
    h.update();
    assert!(h.player_traps().is_empty(), "벽엔 설치 불가");
    assert_eq!(h.kit_count(), 2, "실패 시 키트 유지");
    assert_eq!(h.acted_count(), 0, "턴 소비 없음");
    assert!(h.last_log().unwrap().contains("설치할 수 없다"));
}

#[test]
fn 정면이_몬스터로_점유되면_설치되지_않는다() {
    let mut h = PlaceHarness::new((5, 5), IVec2::new(1, 0), 2);
    h.app.world.resource_mut::<MonsterTiles>().0.insert((6, 5));
    h.press(KEY_PLACE_TRAP);
    h.update();
    assert!(h.player_traps().is_empty(), "몬스터 점유 타일엔 설치 불가");
    assert_eq!(h.kit_count(), 2, "실패 시 키트 유지");
}

#[test]
fn 정면이_주민으로_점유되면_설치되지_않는다() {
    let mut h = PlaceHarness::new((5, 5), IVec2::new(1, 0), 2);
    h.app.world.resource_mut::<OccupiedTiles>().0.insert((6, 5));
    h.press(KEY_PLACE_TRAP);
    h.update();
    assert!(h.player_traps().is_empty(), "주민 점유 타일엔 설치 불가");
}

#[test]
fn 설치키가_아니면_아무것도_설치되지_않는다() {
    let mut h = PlaceHarness::new((5, 5), IVec2::new(1, 0), 2);
    h.press(KeyCode::KeyZ);
    h.update();
    assert!(h.player_traps().is_empty());
    assert_eq!(h.kit_count(), 2);
}

#[test]
fn 모달패널이_열려있으면_설치입력을_무시한다() {
    let mut h = PlaceHarness::new((5, 5), IVec2::new(1, 0), 2);
    h.app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
    h.press(KEY_PLACE_TRAP);
    h.update();
    assert!(h.player_traps().is_empty(), "패널 열림 시 설치 무시");
    assert_eq!(h.kit_count(), 2);
}

#[test]
fn 상점패널이_열려있으면_설치입력을_무시한다() {
    let mut h = PlaceHarness::new((5, 5), IVec2::new(1, 0), 2);
    h.app.world.resource_mut::<ShopPanelOpen>().0 = true;
    h.press(KEY_PLACE_TRAP);
    h.update();
    assert!(h.player_traps().is_empty());
}

#[test]
fn 도움말패널이_열려있으면_설치입력을_무시한다() {
    let mut h = PlaceHarness::new((5, 5), IVec2::new(1, 0), 2);
    h.app.world.resource_mut::<HelpPanelOpen>().0 = true;
    h.press(KEY_PLACE_TRAP);
    h.update();
    assert!(h.player_traps().is_empty());
}

#[test]
fn 사망상태면_설치입력을_무시한다() {
    let mut h = PlaceHarness::new((5, 5), IVec2::new(1, 0), 2);
    let p = h.app.world.query_filtered::<Entity, With<Player>>()
        .iter(&h.app.world).next().unwrap();
    h.app.world.entity_mut(p).insert(crate::modules::combat::Defeated);
    h.press(KEY_PLACE_TRAP);
    h.update();
    assert!(h.player_traps().is_empty(), "Defeated 면 query 비어 무시");
}

// ── 해제 시스템 하네스 ──────────────────────────────────────────────────────────

struct DisarmHarness {
    app: App,
}

impl DisarmHarness {
    fn new(player_tile: (usize, usize), tool_count: u32) -> Self {
        let mut app = App::new();
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app.init_resource::<EquipmentPanelOpen>();
        app.init_resource::<ShopPanelOpen>();
        app.init_resource::<HelpPanelOpen>();
        app.init_resource::<GuidePanelOpen>();
        app.add_event::<PlayerActedEvent>();
        app.add_event::<LogMessage>();

        let mut inv = PlayerInventory::default();
        if tool_count > 0 {
            inv.consumables.push((ConsumableKind(DISARM_TOOL_ID), tool_count));
        }
        app.insert_resource(inv);

        let pos = tile_to_world_coords(player_tile.0, player_tile.1);
        app.world.spawn((
            Player,
            Transform::from_xyz(pos.x, pos.y, 1.0),
        ));

        app.add_systems(Update, handle_disarm_trap);
        Self { app }
    }

    fn spawn_trap(&mut self, tile: (usize, usize), hidden: bool, player_owned: bool) -> Entity {
        let mut e = self.app.world.spawn((
            Trap { kind: TrapKind::Spike, tile_x: tile.0, tile_y: tile.1, hidden },
            if hidden { Visibility::Hidden } else { Visibility::Visible },
        ));
        if player_owned { e.insert(PlayerTrap); }
        e.id()
    }

    fn press(&mut self, key: KeyCode) {
        self.app.world.resource_mut::<ButtonInput<KeyCode>>().press(key);
    }
    fn update(&mut self) { self.app.update(); }
    fn kit_count(&self) -> u32 {
        consumable_count(self.app.world.resource::<PlayerInventory>(), TRAP_KIT_ID)
    }
    fn trap_alive(&self, e: Entity) -> bool {
        self.app.world.get_entity(e).is_some()
    }
    fn acted_count(&mut self) -> usize {
        let events = self.app.world.resource::<Events<PlayerActedEvent>>();
        let mut r = events.get_reader();
        r.read(events).count()
    }
    fn last_log(&mut self) -> Option<String> {
        let events = self.app.world.resource::<Events<LogMessage>>();
        let mut r = events.get_reader();
        r.read(events).last().map(|m| m.0.clone())
    }
}

#[test]
fn 해제도구가_있으면_인접한_노출함정을_확정으로_해제하고_키트를_회수한다() {
    let mut h = DisarmHarness::new((5, 5), 1);
    let t = h.spawn_trap((6, 5), false, true); // 인접, 노출
    h.press(KEY_DISARM_TRAP);
    h.update();
    assert!(!h.trap_alive(t), "도구 있으면 확정 해제(despawn)");
    assert_eq!(h.kit_count(), 1, "회수형 키트 +1");
    assert_eq!(h.acted_count(), 1, "턴 소비");
    assert!(h.last_log().unwrap().contains("해제 성공"));
}

#[test]
fn 현재타일의_노출함정도_해제할_수_있다() {
    let mut h = DisarmHarness::new((5, 5), 1);
    let t = h.spawn_trap((5, 5), false, true); // 같은 타일
    h.press(KEY_DISARM_TRAP);
    h.update();
    assert!(!h.trap_alive(t), "현재 타일 함정도 해제");
}

#[test]
fn 숨김함정은_해제대상에서_제외된다() {
    let mut h = DisarmHarness::new((5, 5), 1);
    let t = h.spawn_trap((6, 5), true, false); // 숨김
    h.press(KEY_DISARM_TRAP);
    h.update();
    assert!(h.trap_alive(t), "숨김(미탐지) 함정은 해제 안 됨");
    assert_eq!(h.acted_count(), 0, "대상 없으면 턴 소비도 없음");
    assert!(h.last_log().unwrap().contains("주변에 없다"));
}

#[test]
fn 사거리_밖의_함정은_해제대상이_아니다() {
    let mut h = DisarmHarness::new((5, 5), 1);
    let t = h.spawn_trap((9, 9), false, false); // 멀리
    h.press(KEY_DISARM_TRAP);
    h.update();
    assert!(h.trap_alive(t), "사거리 밖이면 해제 안 됨");
    assert!(h.last_log().unwrap().contains("주변에 없다"));
}

#[test]
fn 가장_가까운_노출함정부터_해제한다() {
    // 인접(6,5)과 같은타일(5,5) 둘 다 있으면 더 가까운(거리 0) 것을 먼저.
    let mut h = DisarmHarness::new((5, 5), 1);
    let near = h.spawn_trap((5, 5), false, true);
    let far = h.spawn_trap((6, 5), false, true);
    h.press(KEY_DISARM_TRAP);
    h.update();
    assert!(!h.trap_alive(near), "거리 0 함정을 해제");
    assert!(h.trap_alive(far), "더 먼 함정은 남는다");
}

// attempt_disarm 의 roll 분기는 시스템(`rand::random`)으로는 비결정적이라,
// roll 을 고정으로 주입할 수 있는 얇은 테스트 시스템으로 검증한다.
#[derive(Resource, Clone, Copy)]
struct DisarmProbe { target: Entity, roll: f32 }

fn run_attempt_disarm(
    mut commands: Commands,
    mut inv: ResMut<PlayerInventory>,
    probe: Res<DisarmProbe>,
    mut acted: EventWriter<PlayerActedEvent>,
    mut log: EventWriter<LogMessage>,
) {
    attempt_disarm(&mut commands, &mut inv, probe.target, TrapKind::Spike,
        probe.roll, &mut acted, &mut log);
}

fn probe_app(roll: f32, tool_count: u32) -> (App, Entity) {
    let mut app = App::new();
    app.add_event::<PlayerActedEvent>();
    app.add_event::<LogMessage>();
    let mut inv = PlayerInventory::default();
    if tool_count > 0 {
        inv.consumables.push((ConsumableKind(DISARM_TOOL_ID), tool_count));
    }
    app.insert_resource(inv);
    let t = app.world.spawn(Trap { kind: TrapKind::Spike, tile_x: 1, tile_y: 1, hidden: false }).id();
    app.insert_resource(DisarmProbe { target: t, roll });
    app.add_systems(Update, run_attempt_disarm);
    (app, t)
}

#[test]
fn 해제도구가_없고_roll이_낮으면_확률로_성공하고_키트를_회수한다() {
    // 도구 없음 + roll 0.0 < 0.5 → 성공.
    let (mut app, t) = probe_app(0.0, 0);
    app.update();
    assert!(app.world.get_entity(t).is_none(), "성공 시 함정 despawn");
    assert_eq!(consumable_count(app.world.resource::<PlayerInventory>(), TRAP_KIT_ID), 1,
        "성공 시 회수형 키트 +1");
}

#[test]
fn 해제도구가_없고_roll이_높으면_실패하고_함정이_남으며_키트도_없다() {
    // 도구 없음 + roll 0.99 ≥ 0.5 → 실패.
    let (mut app, t) = probe_app(0.99, 0);
    app.update();
    assert!(app.world.get_entity(t).is_some(), "실패 시 함정 유지");
    assert_eq!(consumable_count(app.world.resource::<PlayerInventory>(), TRAP_KIT_ID), 0,
        "실패 시 키트 회수 없음");
}

#[test]
fn 해제도구가_있으면_roll이_높아도_성공한다() {
    // 도구 있음 + roll 0.99 → has_tool 단락으로 성공.
    let (mut app, t) = probe_app(0.99, 1);
    app.update();
    assert!(app.world.get_entity(t).is_none(), "도구 있으면 roll 무관 성공");
}

#[test]
fn 모달패널이_열려있으면_해제입력을_무시한다() {
    let mut h = DisarmHarness::new((5, 5), 1);
    h.app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
    let t = h.spawn_trap((6, 5), false, true);
    h.press(KEY_DISARM_TRAP);
    h.update();
    assert!(h.trap_alive(t), "패널 열림 시 해제 무시");
}

#[test]
fn 해제키가_아니면_아무것도_해제되지_않는다() {
    let mut h = DisarmHarness::new((5, 5), 1);
    let t = h.spawn_trap((6, 5), false, true);
    h.press(KeyCode::KeyZ);
    h.update();
    assert!(h.trap_alive(t));
    assert_eq!(h.acted_count(), 0);
}

#[test]
fn 사망상태면_해제입력을_무시한다() {
    let mut h = DisarmHarness::new((5, 5), 1);
    let p = h.app.world.query_filtered::<Entity, With<Player>>()
        .iter(&h.app.world).next().unwrap();
    h.app.world.entity_mut(p).insert(crate::modules::combat::Defeated);
    let t = h.spawn_trap((6, 5), false, true);
    h.press(KEY_DISARM_TRAP);
    h.update();
    assert!(h.trap_alive(t), "Defeated 면 query 비어 무시");
}

// ── 플러그인 빌드 ───────────────────────────────────────────────────────────────

#[test]
fn 플레이어함정플러그인이_정상적으로_빌드된다() {
    let mut app = App::new();
    app.add_plugins(PlayerTrapPlugin);
    // 시스템만 추가하는 얇은 플러그인 — 빌드가 panic 없이 끝나면 성공.
    let _ = app;
}
