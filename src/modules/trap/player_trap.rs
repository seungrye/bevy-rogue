//! 플레이어 함정 설치/해제 (§B-2).
//!
//! 기본 함정 시스템(`super`)의 `Trap` 컴포넌트·`trigger_traps` 발동 로직을
//! 그대로 재사용하고, "플레이어가 설치한 아군 함정"이라는 점만 `PlayerTrap`
//! 마커로 구분한다. 설치 가능 판정(`can_place_trap`)과 해제 성공 판정
//! (`disarm_succeeds`)은 `super` 의 순수 함수로 분리해 단독 테스트한다.
//!
//! - 설치(`KEY_PLACE_TRAP`): "함정 키트"(회수형 소모품)로 정면 타일에 배치.
//!   비어(통과 가능)·점유되지 않은 타일에만 설치. 키트 1개 소모.
//! - 해제(`KEY_DISARM_TRAP`): 인접/현재의 노출된 함정을 제거. 해제 도구가
//!   있으면 확정, 없으면 확률(`DISARM_CHANCE_NO_TOOL`). 성공 시 회수형 키트
//!   가 회복된다(키트 +1).

use bevy::prelude::*;
use std::collections::HashSet;
use crate::modules::{
    map::{
        MapResource, OccupiedTiles, MonsterTiles, PlayerActedEvent,
        world_to_tile_coords,
    },
    player::{Player, Facing},
    combat::Defeated,
    item::{PlayerInventory, ConsumableKind},
    ui::{LogMessage, help::HelpPanelOpen, shop::ShopPanelOpen, guide_panel::GuidePanelOpen},
    item::EquipmentPanelOpen,
};
use super::{Trap, TrapKind, PlayerTrap, spawn_trap_entity, can_place_trap, disarm_succeeds};

/// 함정 키트 아이템 id(assets/items/consumables.ron). 회수형 소모품으로 관리.
pub const TRAP_KIT_ID: &str = "trap_kit";
/// 해제 도구 아이템 id(assets/items/consumables.ron). 보유 시 확정 해제.
pub const DISARM_TOOL_ID: &str = "disarm_tool";

/// 함정 키트가 깔아 두는 플레이어 함정의 종류. 가시(피해형)로 둔다.
pub const PLAYER_TRAP_KIND: TrapKind = TrapKind::Spike;

/// 설치 단축키.
pub const KEY_PLACE_TRAP: KeyCode = KeyCode::KeyT;
/// 해제 단축키.
pub const KEY_DISARM_TRAP: KeyCode = KeyCode::KeyY;

/// 해제 가능한 함정 탐색 반경(체비쇼프). 인접/현재 타일까지.
pub const DISARM_REACH: i32 = 1;

// ── 플러그인 ─────────────────────────────────────────────────────────────────

pub struct PlayerTrapPlugin;

impl Plugin for PlayerTrapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
            handle_place_trap,
            handle_disarm_trap,
        ));
    }
}

// ── 순수 헬퍼 ────────────────────────────────────────────────────────────────

/// 인벤토리의 특정 소모품 보유 개수를 센다(없으면 0).
pub fn consumable_count(inv: &PlayerInventory, id: &'static str) -> u32 {
    inv.consumables.iter()
        .find(|(k, _)| k.0 == id)
        .map(|(_, n)| *n)
        .unwrap_or(0)
}

// ── 설치 ─────────────────────────────────────────────────────────────────────

/// 모달 패널이 열려 있으면 설치/해제 입력을 막는다(스킬 모듈과 동일 가드).
fn any_panel_open(eq: &EquipmentPanelOpen, shop: &ShopPanelOpen, help: &HelpPanelOpen, guide: &GuidePanelOpen) -> bool {
    eq.0 || shop.0 || help.0 || guide.0
}

#[allow(clippy::too_many_arguments)]
fn handle_place_trap(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    eq: Res<EquipmentPanelOpen>,
    shop: Res<ShopPanelOpen>,
    help: Res<HelpPanelOpen>,
    guide: Res<GuidePanelOpen>,
    mut inventory: ResMut<PlayerInventory>,
    map_res: Res<MapResource>,
    occupied: Res<OccupiedTiles>,
    monster_tiles: Res<MonsterTiles>,
    asset_server: Res<AssetServer>,
    player_q: Query<(&Transform, &Facing), (With<Player>, Without<Defeated>)>,
    mut acted: EventWriter<PlayerActedEvent>,
    mut log: EventWriter<LogMessage>,
) {
    if any_panel_open(&eq, &shop, &help, &guide) { return; }
    if !keyboard.just_pressed(KEY_PLACE_TRAP) { return; }
    let Ok((transform, facing)) = player_q.get_single() else { return };

    place_player_trap(
        &mut commands, &mut inventory, &map_res, &occupied, &monster_tiles,
        &asset_server, transform.translation, facing.0, &mut acted, &mut log,
    );
}

/// 정면 타일에 플레이어 함정을 설치하는 핵심 로직(시스템에서 분리해 단독 호출 가능).
///
/// 키트가 없으면 실패 로그만. 정면 타일이 설치 불가(벽/점유)면 실패 로그만.
/// 설치 성공 시 키트 1개 소모 + PlayerTrap 함정 스폰 + 턴 소비.
#[allow(clippy::too_many_arguments)]
pub fn place_player_trap(
    commands: &mut Commands,
    inventory: &mut PlayerInventory,
    map_res: &MapResource,
    occupied: &OccupiedTiles,
    monster_tiles: &MonsterTiles,
    asset_server: &AssetServer,
    player_world: Vec3,
    facing: IVec2,
    acted: &mut EventWriter<PlayerActedEvent>,
    log: &mut EventWriter<LogMessage>,
) {
    let kit = ConsumableKind(TRAP_KIT_ID);
    if consumable_count(inventory, TRAP_KIT_ID) == 0 {
        log.send(LogMessage("함정 키트가 없다.".into()));
        return;
    }

    let (px, py) = world_to_tile_coords(player_world);
    let (tx, ty) = (
        (px as i32 + facing.x).max(0) as usize,
        (py as i32 + facing.y).max(0) as usize,
    );

    // 점유 판정은 몬스터·주민 타일을 합쳐 본다(플레이어 자신은 정면이라 제외됨).
    let mut blocked: HashSet<(usize, usize)> = occupied.0.clone();
    blocked.extend(monster_tiles.0.iter().copied());

    if !can_place_trap(map_res.map(), &blocked, tx, ty) {
        log.send(LogMessage("그 자리에는 함정을 설치할 수 없다.".into()));
        return;
    }

    // 키트 소모 + 함정 스폰(노출 상태로 설치 — 플레이어가 위치를 안다).
    inventory.use_consumable(kit);
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");
    let e = spawn_trap_entity(commands, &font, PLAYER_TRAP_KIND, tx, ty, false);
    commands.entity(e).insert(PlayerTrap);

    acted.send(PlayerActedEvent);
    log.send(LogMessage(format!(
        "{}을(를) ({}, {})에 설치했다.", PLAYER_TRAP_KIND.name_ko(), tx, ty
    )));
}

// ── 해제 ─────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn handle_disarm_trap(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    eq: Res<EquipmentPanelOpen>,
    shop: Res<ShopPanelOpen>,
    help: Res<HelpPanelOpen>,
    guide: Res<GuidePanelOpen>,
    mut inventory: ResMut<PlayerInventory>,
    player_q: Query<&Transform, (With<Player>, Without<Defeated>)>,
    trap_q: Query<(Entity, &Trap)>,
    mut acted: EventWriter<PlayerActedEvent>,
    mut log: EventWriter<LogMessage>,
) {
    if any_panel_open(&eq, &shop, &help, &guide) { return; }
    if !keyboard.just_pressed(KEY_DISARM_TRAP) { return; }
    let Ok(transform) = player_q.get_single() else { return };
    let (px, py) = world_to_tile_coords(transform.translation);

    // 인접/현재의 노출된 함정 중 가장 가까운 것을 고른다.
    let Some((target, kind)) = nearest_disarmable_trap(&trap_q, px, py) else {
        log.send(LogMessage("해제할 노출된 함정이 주변에 없다.".into()));
        return;
    };

    attempt_disarm(
        &mut commands, &mut inventory, target, kind,
        rand::random::<f32>(), &mut acted, &mut log,
    );
}

/// 인접(체비쇼프 `DISARM_REACH`)한 노출 함정 중 가장 가까운 것을 찾는다.
fn nearest_disarmable_trap(
    trap_q: &Query<(Entity, &Trap)>,
    px: usize, py: usize,
) -> Option<(Entity, TrapKind)> {
    trap_q.iter()
        .filter(|(_, t)| !t.hidden)
        .filter_map(|(e, t)| {
            let dx = (t.tile_x as i32 - px as i32).abs();
            let dy = (t.tile_y as i32 - py as i32).abs();
            let cheb = dx.max(dy);
            (cheb <= DISARM_REACH).then_some((cheb, e, t.kind))
        })
        .min_by_key(|(cheb, _, _)| *cheb)
        .map(|(_, e, kind)| (e, kind))
}

/// 함정 해제를 시도하는 핵심 로직(시스템에서 분리해 단독 호출 가능).
///
/// 해제 도구 보유 시 확정 성공, 없으면 `roll` 로 확률 판정(`disarm_succeeds`).
/// 성공 시 함정 despawn + 회수형 키트 회수(키트 +1) + 턴 소비.
/// 실패 시에도 턴은 소비(시도 행동)하지만 함정은 남는다.
pub fn attempt_disarm(
    commands: &mut Commands,
    inventory: &mut PlayerInventory,
    trap_entity: Entity,
    kind: TrapKind,
    roll: f32,
    acted: &mut EventWriter<PlayerActedEvent>,
    log: &mut EventWriter<LogMessage>,
) {
    let has_tool = consumable_count(inventory, DISARM_TOOL_ID) > 0;
    let success = disarm_succeeds(has_tool, roll);

    acted.send(PlayerActedEvent);

    if success {
        commands.entity(trap_entity).despawn();
        // 회수형 키트 회수.
        inventory.add_consumable(ConsumableKind(TRAP_KIT_ID));
        log.send(LogMessage(format!(
            "{} 해제 성공! 함정 키트를 회수했다.", kind.name_ko()
        )));
    } else {
        log.send(LogMessage(format!("{} 해제 실패...", kind.name_ko())));
    }
}

#[cfg(test)]
mod tests;
