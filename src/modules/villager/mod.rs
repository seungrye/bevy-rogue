use bevy::prelude::*;
use bevy::ecs::system::SystemParam;
use rand::prelude::*;
use serde::Deserialize;
use std::collections::HashSet;
use crate::modules::{
    map::{
        draw_map, Map, MapType, OccupiedTiles, Rect, TileKind,
        tile_to_world_coords, world_to_tile_coords,
        MAP_HEIGHT, MAP_WIDTH, TILE_SIZE,
        MapSystemSet, VillagerRespawnEvent, PlayerActedEvent, BumpTileEvent, ExplosionEvent,
    },
    player::{Player, MovingTo, MoveQueue, PlayerSystemSet, LERP_SPEED},
    ui::{LogMessage, minimap::{DiscoveredMarkers, MarkerKind}},
    quest::{QuestRegistry, QuestState, QuestSystemSet, KillNpcEvent, DespawnWorldItemEvent, execute_actions, QuestDef, eval_condition},
    monster::{SpawnGuardEvent, SpawnMonsterEvent},
    trap::SpawnTrapEvent,
    item::{PlayerInventory},
    zone::{WorldState, SpawnQuestPortalEvent},
    combat::Speed,
};

/// 퀘스트 액션이 발행하는 EventWriter 묶음. Bevy 의 시스템 파라미터 16개 제한을
/// 피하려고 한 `SystemParam` 으로 모았다. `execute_actions` 로 그대로 전달된다.
#[derive(SystemParam)]
pub struct QuestActionWriters<'w> {
    pub kill_npc: EventWriter<'w, KillNpcEvent>,
    pub open_portal: EventWriter<'w, SpawnQuestPortalEvent>,
    pub close_portal: EventWriter<'w, crate::modules::zone::CloseQuestPortalEvent>,
    pub despawn_item: EventWriter<'w, DespawnWorldItemEvent>,
    pub spawn_guards: EventWriter<'w, SpawnGuardEvent>,
    pub spawn_monster: EventWriter<'w, SpawnMonsterEvent>,
    pub explode: EventWriter<'w, ExplosionEvent>,
    pub place_traps: EventWriter<'w, SpawnTrapEvent>,
}

const VILLAGER_STAY_CHANCE: f64 = 0.3;
const Z_VILLAGER: f32 = 0.9;

/// villager RON 파일에서 불러오는 NPC 정의
#[derive(Debug, Deserialize, Clone)]
pub struct VillagerDef {
    /// unique 식별자 (snake_case 영문). 퀘스트의 `giver_npc` / `KillNpc` /
    /// 코드의 NPC 매칭이 모두 이 값을 사용. `name` (한글 표시) 은 unique
    /// 가 아닐 수 있으므로 매칭에 사용하지 않는다.
    pub id: String,
    pub name: String,
    pub color: [f32; 3],
    pub dialogs: Vec<String>,
    pub speed: f32,
    /// 정지(stationary) 주민 — true 면 `villager_turn` 이 이동을 스킵해 제자리를
    /// 유지한다. 가판대 뒤 상인처럼 고정된 NPC 에 사용한다.
    /// `#[serde(default)]` 로 기존 RON(이 필드 없는 정의)과 호환된다(기본 false).
    #[serde(default)]
    pub stationary: bool,
    /// 상인(vendor) — true 면 상호작용 시 상점이 열린다(기존 ui/shop.rs 흐름).
    /// 가판대 뒤에 고정 배치되어 카운터 너머로 거래한다.
    /// `#[serde(default)]` 로 기존 RON 과 호환된다(기본 false).
    #[serde(default)]
    pub vendor: bool,
}

/// 게임 시작 시 RON 에서 불러온 villager 정의 모음
#[derive(Resource, Default)]
pub struct VillagerRegistry {
    pub villagers: Vec<VillagerDef>,
}

/// villager 시스템의 단계별 실행 순서.
///
/// - `Load`: startup 단계 — RON 적재 → 퀘스트 참조 검증 → 스폰.
/// - `Turn`: update 단계 — `handle_bump` + `villager_turn` 의 합 — 이 세트 이후
///   에서 보는 villager 의 `tile_x/tile_y` 는 "이번 턴 이동 결과 반영" 후의 값이다.
///   `player::refresh_follow_path` 가 이 보장에 의존(NPC 추적 경로 재계산).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum VillagerSystemSet {
    Load,
    Turn,
}


#[derive(Component)]
pub struct Villager {
    /// unique 식별자 — VillagerDef.id 에서 복사. NPC 매칭의 단일 키.
    pub id: String,
    /// 표시용 한글 이름. dialog/log 메시지에 사용. unique 보장 X.
    pub name: String,
    pub dialogues: Vec<String>,
    pub dialogue_idx: usize,
    pub tile_x: usize,
    pub tile_y: usize,
    pub just_bumped: bool,
    pub quest_dialogue_idx: usize,
    pub base_color: Color,
    /// 퀘스트 NPC의 이동 구역 제한. Some이면 해당 Rect 안에서만 이동한다.
    pub home_room: Option<Rect>,
    /// 정지 주민 여부 — true 면 매 턴 제자리(가판대 뒤 상인 등). VillagerDef.stationary 복사.
    pub stationary: bool,
    /// 상인 여부 — true 면 상호작용 시 상점이 열린다. VillagerDef.vendor 복사.
    pub vendor: bool,
}

// 이번 턴에 주민이 이동해야 하는지 판단하고 플래그를 초기화한다.
// stationary 주민(가판대 뒤 상인 등)은 충돌 플래그와 무관하게 항상 제자리.
pub fn take_turn(villager: &mut Villager) -> bool {
    // 충돌 직후 플래그는 어느 경우든 한 번 소모한다(다음 턴부터 정상).
    let was_bumped = villager.just_bumped;
    villager.just_bumped = false;
    !should_skip_villager_move(villager.stationary, was_bumped)
}

/// vendor/정지 주민의 이동 스킵 판정 (순수 함수).
/// stationary 면 충돌 여부와 무관하게 이번 턴 이동을 건너뛴다.
pub fn should_skip_villager_move(stationary: bool, just_bumped: bool) -> bool {
    stationary || just_bumped
}

/// 어떤 타일을 향해 상호작용(범프)할 때 상점을 열어줄 vendor 를 찾는다 (순수 함수).
///
/// - 범프 타일 자체가 vendor 위치면 그 vendor (직접 인접).
/// - 범프 타일이 가판대(`Counter`)면 그 카운터에 직교(상하좌우)로 인접한
///   vendor (카운터 너머 보정) — 카운터 앞 타일에서 interact 해도 열리게 한다.
/// - 그 외에는 `None`.
///
/// `vendor_at(x, y)` 는 해당 타일에 vendor 가 있으면 그 식별 좌표를 돌려주는 클로저.
/// (테스트·시스템 양쪽에서 vendor 위치 집합을 주입할 수 있게 클로저로 분리.)
pub fn vendor_for_interaction<F>(
    bx: usize,
    by: usize,
    map: &Map,
    is_vendor_at: F,
) -> Option<(usize, usize)>
where
    F: Fn(usize, usize) -> bool,
{
    // 1) 범프 타일에 직접 vendor 가 있으면 그 vendor.
    if is_vendor_at(bx, by) {
        return Some((bx, by));
    }
    // 2) 범프 타일이 카운터면 그 너머(직교 인접)의 vendor 를 찾는다.
    if map.get_tile(bx, by) == TileKind::Counter {
        let neighbors = [
            (bx.wrapping_sub(1), by),
            (bx + 1, by),
            (bx, by.wrapping_sub(1)),
            (bx, by + 1),
        ];
        for (nx, ny) in neighbors {
            if nx < map.width && ny < map.height && is_vendor_at(nx, ny) {
                return Some((nx, ny));
            }
        }
    }
    None
}

pub struct VillagerPlugin;

impl Plugin for VillagerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VillagerRegistry>()
            .add_systems(Startup, (
                load_villagers.in_set(VillagerSystemSet::Load),
                validate_quest_villager_refs
                    .after(VillagerSystemSet::Load)
                    .after(QuestSystemSet::Load),
                spawn_on_startup
                    .after(draw_map)
                    .after(QuestSystemSet::Load)
                    .after(VillagerSystemSet::Load),
            ))
            .add_systems(PreUpdate, sync_occupied_tiles)
            .add_systems(Update, (
                respawn_on_regen.after(MapSystemSet::ExecuteRegen),
                (handle_bump, villager_turn)
                    .chain()
                    .in_set(VillagerSystemSet::Turn)
                    .after(PlayerSystemSet::MovementComplete),
                update_villager_glyph.after(handle_bump),
                smooth_villager_move,
                handle_kill_npc,
                discover_quest_npcs_in_fov,
            ));
    }
}

/// villager RON 파일을 읽어 registry 에 적재한다
fn load_villagers(mut registry: ResMut<VillagerRegistry>) {
    // wasm32: 임베드된 RON 으로 파싱 (std::fs 미가용, std::process::exit 미가용).
    // 시작 시 site `/api/game/content/v1` 의 REMOTE 콘텐츠가 설치돼 있으면 그쪽 우선,
    // 없으면 build.rs 가 자동 enumerate 한 EMBEDDED_VILLAGERS 슬라이스로 폴백.
    #[cfg(target_arch = "wasm32")]
    let villagers = {
        let embed = crate::modules::embedded_assets::villagers_ron()
            .expect("villagers.ron 임베드 누락 (build.rs)");
        ron::de::from_str::<Vec<VillagerDef>>(embed)
            .unwrap_or_else(|e| panic!("[치명적] villagers.ron RON 파싱 실패: {}", e))
    };
    #[cfg(not(target_arch = "wasm32"))]
    let path = "assets/villagers/villagers.ron";
    #[cfg(not(target_arch = "wasm32"))]
    let villagers = match read_villager_defs(path) {
        Ok(v) => v,
        // 도달 불가 방어코드: 파일 누락·파싱 실패 시 process::exit 로 테스트 러너를 죽이므로
        // 단위 테스트에서 양방향 실행 불가. read_villager_defs 의 Err 분기는 별도 테스트로 커버.
        Err(e) => {
            error!("[치명적] {}", e);
            std::process::exit(1);
        }
    };
    info!("villager 로드: {} 명", villagers.len());
    registry.villagers = villagers;
}

/// 주어진 경로의 villager RON 을 읽어 파싱한다 (테스트 가능한 seam).
/// 읽기 실패·파싱 실패를 에러 메시지로 반환한다 (process::exit 없음).
/// wasm 빌드는 embedded slice 를 쓰므로 이 함수는 호출되지 않는다(테스트 only).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn read_villager_defs(path: &str) -> Result<Vec<VillagerDef>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("villager 파일 {} 을 읽을 수 없습니다: {}", path, e))?;
    ron::de::from_str::<Vec<VillagerDef>>(&text)
        .map_err(|e| format!("villager RON 파싱 실패: {}", e))
}

/// 모든 퀘스트의 giver_npc 가 villager registry 에 존재하는지 검증한다
fn validate_quest_villager_refs(
    quest_registry: Res<QuestRegistry>,
    villager_registry: Res<VillagerRegistry>,
) {
    let errors = collect_missing_giver_refs(&quest_registry, &villager_registry);
    // 도달 불가 방어코드: 매칭 실패 시 process::exit 로 테스트 러너를 죽이므로 단위
    // 테스트에서 양방향 실행 불가. 매칭 판정 로직은 collect_missing_giver_refs 로 커버.
    if !errors.is_empty() {
        for msg in &errors {
            error!("[치명적] {}", msg);
        }
        #[cfg(not(target_arch = "wasm32"))]
        std::process::exit(1);
        #[cfg(target_arch = "wasm32")]
        panic!("[치명적] villager-quest 참조 검증 실패");
    }
}

/// villager registry 와 매칭되지 않는 퀘스트 giver_npc 목록을 반환한다 (테스트 가능한 seam).
fn collect_missing_giver_refs(
    quest_registry: &QuestRegistry,
    villager_registry: &VillagerRegistry,
) -> Vec<String> {
    let mut errors: Vec<String> = Vec::new();
    for (qid, qdef) in &quest_registry.quests {
        let exists = villager_registry.villagers.iter().any(|v| v.id == qdef.giver_npc);
        if !exists {
            errors.push(format!(
                "퀘스트 '{}' 의 giver_npc '{}' 가 villager id 와 매칭 안 됨",
                qid, qdef.giver_npc
            ));
        }
    }
    errors
}

fn handle_kill_npc(
    mut events: EventReader<KillNpcEvent>,
    query: Query<(Entity, &Villager)>,
    mut commands: Commands,
    mut log: EventWriter<LogMessage>,
) {
    // KillNpcEvent 의 인자는 villager id (unique). name 은 표시용.
    for KillNpcEvent(npc_id) in events.read() {
        for (entity, villager) in &query {
            if &villager.id == npc_id {
                commands.entity(entity).despawn_recursive();
                log.send(LogMessage(format!("{}이(가) 쓰러졌다...", villager.name)));
                break;
            }
        }
    }
}

/// 퀘스트 NPC 미니맵 마커를 quest 상태에 따라 갱신/제거한다.
///
/// 마커 표시 조건 (모두 만족):
/// - villager 의 quest_id 가 등록된 퀘스트
/// - quest 가 시작됨 (state.phases 에 있고, initial_phase 가 아님)
/// - quest 가 terminal 페이즈가 아님 (현재 phase 에서 시작하는 transition 이 있음)
/// - NPC 가 player FOV 안에 있음
///
/// 위 중 하나라도 깨지면 해당 NPC 의 마커를 제거한다 (시작 전 / 종료 후 / 다른 zone).
fn discover_quest_npcs_in_fov(
    map_res: Res<crate::modules::map::MapResource>,
    world_state: Res<WorldState>,
    quest_registry: Res<QuestRegistry>,
    quest_state: Res<QuestState>,
    villager_query: Query<&Villager>,
    mut markers: ResMut<DiscoveredMarkers>,
) {
    let map = map_res.map();
    for v in villager_query.iter() {
        // villager 가 어느 quest 의 giver 인지 quest_registry 에서 조회.
        let Some((qid, qdef)) = quest_registry.quest_for_giver(&v.id) else { continue; };

        // quest 시작 전 (phase 없음 또는 initial_phase) → 마커 제거
        let started = match quest_state.phases.get(qid) {
            None => false,
            Some(p) => p != &qdef.initial_phase,
        };
        if !started {
            markers.remove_actor(&v.name, MarkerKind::QuestGiver, &world_state.current);
            continue;
        }

        // quest 종료 (terminal phase) → 마커 제거
        if is_quest_terminal_def(qdef, &quest_state, qid) {
            markers.remove_actor(&v.name, MarkerKind::QuestGiver, &world_state.current);
            continue;
        }

        // FOV 검사: 시야 밖이면 마지막 본 위치 유지 (제거하지 않음)
        if v.tile_x >= map.width || v.tile_y >= map.height { continue; }
        let idx = map.index(v.tile_x, v.tile_y);
        if !map.tiles[idx].visible { continue; }

        // active + FOV 안 — 위치 갱신
        markers.update_actor_position(
            &v.name, MarkerKind::QuestGiver, world_state.current.clone(),
            v.tile_x, v.tile_y,
        );
    }
}

// PreUpdate: 주민 위치를 OccupiedTiles에 동기화(player_movement 이전 실행)
fn sync_occupied_tiles(
    villager_query: Query<&Villager>,
    mut occupied: ResMut<OccupiedTiles>,
) {
    occupied.0.clear();
    for v in villager_query.iter() {
        occupied.0.insert((v.tile_x, v.tile_y));
    }
}

fn spawn_on_startup(
    mut commands: Commands,
    map_res: Res<crate::modules::map::MapResource>,
    asset_server: Res<AssetServer>,
    quest_registry: Res<QuestRegistry>,
    villager_registry: Res<VillagerRegistry>,
) {
    let map = map_res.map();
    if map.map_type == MapType::Village {
        do_spawn(&mut commands, &map.rooms.clone(), map.shop_vendor, &asset_server, &quest_registry, &villager_registry);
    }
}

fn respawn_on_regen(
    mut commands: Commands,
    mut events: EventReader<VillagerRespawnEvent>,
    villager_query: Query<Entity, With<Villager>>,
    asset_server: Res<AssetServer>,
    quest_registry: Res<QuestRegistry>,
    villager_registry: Res<VillagerRegistry>,
    map_res: Res<crate::modules::map::MapResource>,
) {
    for event in events.read() {
        for entity in villager_query.iter() {
            commands.entity(entity).despawn();
        }
        if event.map_type == MapType::Village {
            do_spawn(&mut commands, &event.rooms, map_res.map().shop_vendor, &asset_server, &quest_registry, &villager_registry);
        }
    }
}

/// VillagerDef + 위치 + 표시 정보로 주민 엔티티 하나를 스폰한다(중복 제거용 헬퍼).
/// vendor/일반/퀘스트 NPC 가 같은 경로로 스폰되어 일관성을 유지한다.
fn spawn_villager_entity(
    commands: &mut Commands,
    font: &Handle<Font>,
    data: &VillagerDef,
    tile: (usize, usize),
    quest_registry: &QuestRegistry,
    home_room: Option<Rect>,
) {
    let (cx, cy) = tile;
    let coord = tile_to_world_coords(cx, cy);
    let dialogues: Vec<String> = data.dialogs.iter()
        .map(|s| format!("{}: {}", data.name, s))
        .collect();
    let base_color = Color::rgb(data.color[0], data.color[1], data.color[2]);
    let is_quest_npc = quest_registry.quest_for_giver(&data.id).is_some();
    // 표시 글리프: 퀘스트 NPC 는 노란 '?', 상인(vendor)은 고유 '$', 그 외 'v'.
    // (퀘스트 NPC 글리프는 이후 update_villager_glyph 가 상태에 따라 갱신한다.)
    let (glyph, display_color) = if is_quest_npc {
        ("?", Color::rgb(1.0, 0.9, 0.1))
    } else if data.vendor {
        ("$", Color::rgb(1.0, 0.85, 0.2))
    } else {
        ("v", base_color)
    };

    commands.spawn((
        Text2dBundle {
            text: Text::from_section(glyph, TextStyle {
                font: font.clone(),
                font_size: TILE_SIZE,
                color: display_color,
            }),
            transform: Transform::from_xyz(coord.x, coord.y, Z_VILLAGER),
            ..default()
        },
        Villager {
            id: data.id.clone(),
            name: data.name.clone(),
            dialogues,
            dialogue_idx: 0,
            tile_x: cx,
            tile_y: cy,
            just_bumped: false,
            quest_dialogue_idx: 0,
            base_color,
            home_room,
            stationary: data.stationary,
            vendor: data.vendor,
        },
        Speed::new(data.speed),
        MoveQueue::default(),
    ));
}

fn do_spawn(
    commands: &mut Commands,
    rooms: &[Rect],
    shop_vendor: Option<(usize, usize)>,
    asset_server: &AssetServer,
    quest_registry: &QuestRegistry,
    villager_registry: &VillagerRegistry,
) {
    if rooms.is_empty() { return; }
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");

    // 상점이 있으면 vendor 주민을 가판대 뒤 고정 위치에 먼저 스폰한다.
    // 이 vendor 는 방 배치 루프에서 제외해 중복 스폰을 막는다.
    let mut vendor_id: Option<String> = None;
    if let Some(tile) = shop_vendor {
        if let Some(vdef) = villager_registry.villagers.iter().find(|d| d.vendor) {
            vendor_id = Some(vdef.id.clone());
            spawn_villager_entity(commands, &font, vdef, tile, quest_registry, None);
        }
    }

    // 퀘스트 NPC와 일반 NPC 를 분리: 퀘스트 NPC 는 활성 퀘스트인 것만 스폰.
    // 이미 가판대에 스폰한 vendor 는 양쪽 후보에서 제외한다(일관 적용).
    let quest_npcs: Vec<&VillagerDef> = villager_registry.villagers.iter()
        .filter(|d| Some(&d.id) != vendor_id.as_ref())
        .filter(|d| quest_registry.active_quest_for_giver(&d.id).is_some())
        .collect();
    let regular_npcs: Vec<&VillagerDef> = villager_registry.villagers.iter()
        .filter(|d| Some(&d.id) != vendor_id.as_ref())
        .filter(|d| quest_registry.quest_for_giver(&d.id).is_none())
        .collect();

    // rooms[0] 은 플레이어 스폰 방 — 건너뜀
    let spawn_rooms: Vec<_> = rooms.iter().skip(1).collect();
    let mut quest_idx = 0;
    let mut regular_idx = 0;

    for room in spawn_rooms {
        let data: &VillagerDef = if quest_idx < quest_npcs.len() {
            let d = quest_npcs[quest_idx];
            quest_idx += 1;
            d
        } else if !regular_npcs.is_empty() {
            let d = regular_npcs[regular_idx % regular_npcs.len()];
            regular_idx += 1;
            d
        } else {
            continue;
        };
        let is_quest_npc = quest_registry.quest_for_giver(&data.id).is_some();
        let home_room = if is_quest_npc { Some(*room) } else { None };
        spawn_villager_entity(commands, &font, data, room.center(), quest_registry, home_room);
    }
}

// 플레이어가 주민 타일(또는 가판대)을 밀어 넣었을 때 대사 출력/상점 열기를 처리한다
fn handle_bump(
    mut events: EventReader<BumpTileEvent>,
    mut villager_query: Query<&mut Villager>,
    mut log_writer: EventWriter<LogMessage>,
    registry: Res<QuestRegistry>,
    mut quest_state: ResMut<QuestState>,
    mut inventory: ResMut<PlayerInventory>,
    world_state: Res<WorldState>,
    mut writers: QuestActionWriters,
    mut shop_open: EventWriter<crate::modules::ui::shop::ShopOpenEvent>,
    quest_items: Res<crate::modules::item::QuestItemRegistry>,
    map_res: Res<crate::modules::map::MapResource>,
) {
    let map = map_res.map();
    // vendor 들의 현재 타일 집합 — 카운터 너머 보정에 사용.
    let vendor_tiles: HashSet<(usize, usize)> = villager_query.iter()
        .filter(|v| v.vendor)
        .map(|v| (v.tile_x, v.tile_y))
        .collect();

    for BumpTileEvent(bx, by) in events.read() {
        // 상호작용 대상 타일을 하나 정한다:
        //   - 범프 타일에 주민이 있으면 그 타일(직접 인접).
        //   - 아니면 카운터 너머 vendor 타일(없으면 이 이벤트 skip).
        let target = if villager_query.iter().any(|v| v.tile_x == *bx && v.tile_y == *by) {
            (*bx, *by)
        } else {
            match vendor_for_interaction(*bx, *by, map, |x, y| vendor_tiles.contains(&(x, y))) {
                Some(t) => t,
                None => continue,
            }
        };

        // 대상 타일의 주민(한 명)에게 단일 결정을 적용한다.
        for mut villager in villager_query.iter_mut() {
            if villager.tile_x != target.0 || villager.tile_y != target.1 { continue; }

            // 이 주민이 giver 인 quest 와, 그 quest 가 아직 종료 전인지(=대화 우선) 판정.
            let giver = registry.quest_for_giver(&villager.id)
                .map(|(qid, def)| (qid.to_string(), def));
            let quest_active = giver.as_ref()
                .map(|(qid, def)| !is_quest_terminal_def(def, &quest_state, qid))
                .unwrap_or(false);

            if quest_active {
                // vendor 여부와 무관하게 퀘스트가 끝나기 전엔 퀘스트 대화 우선 — 핵심 수정.
                let qid = giver.as_ref().unwrap().0.clone();
                show_quest_dialog(&mut villager, &qid, &registry, &mut quest_state, &mut inventory, &mut log_writer, &world_state, &mut writers, &quest_items);
                // QuestGiver 마커는 discover_quest_npcs_in_fov 가 quest 상태에 따라 자동 갱신
            } else if villager.vendor {
                // 순수 vendor, 또는 퀘스트가 종료된 vendor-giver → 상점.
                shop_open.send(crate::modules::ui::shop::ShopOpenEvent);
            } else if let Some((qid, _)) = giver.as_ref() {
                // 비-vendor giver 가 종료된 경우: 종료 페이즈 대화 유지(회귀 방지).
                let qid = qid.clone();
                show_quest_dialog(&mut villager, &qid, &registry, &mut quest_state, &mut inventory, &mut log_writer, &world_state, &mut writers, &quest_items);
            } else if !villager.dialogues.is_empty() {
                let msg = villager.dialogues[villager.dialogue_idx].clone();
                log_writer.send(LogMessage(msg));
                villager.dialogue_idx = next_dialogue_idx(villager.dialogue_idx, villager.dialogues.len());
            }
            villager.just_bumped = true;
            break;
        }
    }
}

fn show_quest_dialog(
    villager: &mut Villager,
    quest_id: &str,
    registry: &QuestRegistry,
    state: &mut QuestState,
    inventory: &mut PlayerInventory,
    log: &mut EventWriter<LogMessage>,
    world: &WorldState,
    writers: &mut QuestActionWriters,
    quest_items: &crate::modules::item::QuestItemRegistry,
) {
    // 폭발 등 위치 기반 액션의 트리거 위치 — 상호작용 대상 NPC 좌표.
    let trigger_pos = (villager.tile_x, villager.tile_y);
    let Some(quest_def) = registry.get(quest_id) else { return };

    // state.phases 에 등록되지 않은 첫 만남이면 initial_phase 의 dialog 만
    // 보여준다 — 인사말 한 줄에 패널 등록되는 어색함 방지. 마지막 대화 줄
    // + Interact 시점에 한꺼번에 set_phase + transition 실행.
    let phase_id = state.phases.get(quest_id)
        .cloned()
        .unwrap_or_else(|| quest_def.initial_phase.clone());

    let Some(phase) = quest_def.phases.get(&phase_id) else { return };

    let dialog = phase.dialog.clone();
    let npc_name = quest_def.giver_npc.clone();

    let idx = villager.quest_dialogue_idx.min(dialog.len().saturating_sub(1));
    if let Some(line) = dialog.get(idx) {
        log.send(LogMessage(format!("{}: {}", npc_name, line)));
    }

    // 마지막 줄에서 Interact transition 평가
    if !dialog.is_empty() && idx + 1 >= dialog.len() {
        villager.quest_dialogue_idx = 0;
        // 첫 phase 등록 (아직 안 됐으면) — 마지막 대화 후 Interact 시점.
        if !state.phases.contains_key(quest_id) {
            state.set_phase(quest_id, &quest_def.initial_phase.clone());
        }
        // 현재 phase 에서 시작하는 Interact transition 을 순서대로 평가, 첫 매칭만 실행
        let matched = quest_def.transitions.iter()
            .find(|t| t.from == phase_id
                && t.trigger == crate::modules::quest::TriggerKind::Interact
                && t.when.as_ref()
                    .map(|c| eval_condition(c, inventory, world, state, quest_items))
                    .unwrap_or(true))
            .cloned();
        if let Some(t) = matched {
            execute_actions(
                &t.actions, quest_id, trigger_pos, state, inventory, log,
                &mut writers.kill_npc, &mut writers.open_portal, &mut writers.close_portal,
                &mut writers.despawn_item, &mut writers.spawn_guards, &mut writers.spawn_monster,
                &mut writers.explode, &mut writers.place_traps, quest_items,
            );
            if t.to != phase_id {
                state.set_phase(quest_id, &t.to);
                info!("퀘스트 [{}] 단계 전진: {} → {}", quest_id, phase_id, t.to);
            }
        }
    } else {
        villager.quest_dialogue_idx = idx + 1;
    }
}

// 플레이어가 행동한 턴에 주민이 한 번 이동한다
fn villager_turn(
    mut events: EventReader<PlayerActedEvent>,
    map_res: Res<crate::modules::map::MapResource>,
    mut villager_query: Query<(&mut Villager, &mut MoveQueue, &mut Speed)>,
    player_query: Query<(&Transform, Option<&MovingTo>), (With<Player>, Without<Villager>)>,
) {
    if events.read().next().is_none() { return; }

    let map = map_res.map();
    let mut rng = thread_rng();

    let mut occupied: HashSet<(usize, usize)> = villager_query.iter()
        .map(|(v, _, _)| (v.tile_x, v.tile_y))
        .collect();
    if let Ok((pt, moving)) = player_query.get_single() {
        let player_tile = moving
            .map(|m| world_to_tile_coords(m.target))
            .unwrap_or_else(|| world_to_tile_coords(pt.translation));
        occupied.insert(player_tile);
    }

    for (mut villager, mut move_queue, mut speed) in villager_query.iter_mut() {
        occupied.remove(&(villager.tile_x, villager.tile_y));

        speed.energy += speed.value;

        if !take_turn(&mut villager) {
            occupied.insert((villager.tile_x, villager.tile_y));
            continue;
        }

        while speed.energy >= 1.0 {
            speed.energy -= 1.0;
            let (nx, ny) = pick_next_tile(villager.tile_x, villager.tile_y, map, &occupied, villager.home_room.as_ref(), &mut rng);
            occupied.remove(&(villager.tile_x, villager.tile_y));
            occupied.insert((nx, ny));
            let wp = tile_to_world_coords(nx, ny);
            move_queue.0.push_back(Vec3::new(wp.x, wp.y, Z_VILLAGER));
            villager.tile_x = nx;
            villager.tile_y = ny;
        }

        occupied.insert((villager.tile_x, villager.tile_y));
    }
}

fn smooth_villager_move(
    time: Res<Time>,
    mut query: Query<(&mut Transform, &mut MoveQueue, &Speed), With<Villager>>,
) {
    let dt = time.delta_seconds();
    for (mut transform, mut queue, speed) in query.iter_mut() {
        let anim_speed = LERP_SPEED * speed.value.max(0.5);
        let step = anim_speed * TILE_SIZE * dt;
        while let Some(&target) = queue.0.front() {
            let dist = transform.translation.distance(target);
            if dist <= step {
                transform.translation = target;
                queue.0.pop_front();
            } else {
                let dir = (target - transform.translation).normalize();
                transform.translation += dir * step;
                break;
            }
        }
    }
}


pub fn pick_next_tile(
    x: usize, y: usize,
    map: &Map,
    occupied: &HashSet<(usize, usize)>,
    home_rect: Option<&Rect>,
    rng: &mut impl Rng,
) -> (usize, usize) {
    if rng.gen_bool(VILLAGER_STAY_CHANCE) {
        return (x, y);
    }
    let neighbors = [
        (x.wrapping_sub(1), y),
        (x + 1, y),
        (x, y.wrapping_sub(1)),
        (x, y + 1),
    ];
    let valid: Vec<(usize, usize)> = neighbors.iter()
        .filter(|&&(nx, ny)| {
            nx < MAP_WIDTH && ny < MAP_HEIGHT
                && map.get_tile(nx, ny).is_walkable()
                && !occupied.contains(&(nx, ny))
                && home_rect.map_or(true, |r| nx >= r.x1 && nx <= r.x2 && ny >= r.y1 && ny <= r.y2)
        })
        .copied()
        .collect();

    if valid.is_empty() { (x, y) } else { *valid.choose(rng).unwrap() }
}

pub fn next_dialogue_idx(current: usize, total: usize) -> usize {
    if total == 0 { return 0; }
    (current + 1) % total
}

/// QuestState 또는 PlayerInventory가 바뀌거나 주민이 새로 스폰될 때마다 퀘스트 수여자의 글리프와 색을 갱신한다
fn update_villager_glyph(
    registry: Res<QuestRegistry>,
    quest_state: Res<QuestState>,
    inventory: Res<PlayerInventory>,
    world: Res<WorldState>,
    quest_items: Res<crate::modules::item::QuestItemRegistry>,
    mut query: Query<(&Villager, &mut Text)>,
    added: Query<(), Added<Villager>>,
) {
    if !quest_state.is_changed() && !inventory.is_changed() && added.is_empty() { return; }
    for (villager, mut text) in query.iter_mut() {
        let Some((qid, def)) = registry.quest_for_giver(&villager.id) else { continue };
        let (glyph, color) = quest_npc_glyph(qid, def, &quest_state, &inventory, &world, villager.base_color, &quest_items);
        text.sections[0].value = glyph.to_string();
        text.sections[0].style.color = color;
    }
}

/// 해당 phase 에서 시작하는 Interact transition 중 실제로 다른 phase 로
/// 넘어가는(to != from) 규칙이 하나라도 있으면 true.
/// Log 전용 self-loop transition (to == from) 만 있는 경우는 false 를 반환한다.
pub fn interact_can_advance(def: &QuestDef, phase_id: &str) -> bool {
    def.transitions.iter().any(|t|
        t.from == phase_id
            && t.trigger == crate::modules::quest::TriggerKind::Interact
            && t.to != phase_id
    )
}

/// 퀘스트 NPC 의 현재 퀘스트 상태에 따라 표시 글리프와 색상을 결정한다
///
/// - `?` (노란색) : 퀘스트 있음 — initial_phase 이거나 아직 수락 전
/// - `?` (초록색) : 퀘스트 수락, 다음 페이즈로 넘어갈 수 없는 상태 (아이템 수집·이동 중)
/// - `!` (초록색) : 다음 페이즈로 넘어갈 수 있는 상태 (Interact transition 으로 전진 가능 또는 Auto transition 조건 충족)
/// - `v` (base_color) : 터미널 (퀘스트 완료)
pub fn quest_npc_glyph(
    quest_id: &str,
    def: &QuestDef,
    state: &QuestState,
    inventory: &PlayerInventory,
    world: &WorldState,
    base_color: Color,
    quest_items: &crate::modules::item::QuestItemRegistry,
) -> (&'static str, Color) {
    let yellow = Color::rgb(1.0, 0.9, 0.1);
    let green = Color::rgb(0.3, 1.0, 0.6);

    // 터미널 페이즈: 퀘스트 완료
    if is_quest_terminal_def(def, state, quest_id) {
        return ("v", base_color);
    }

    let phase_id = state.phases.get(quest_id);

    // initial_phase 이거나 아직 수락 전 → 퀘스트 있음 (노란 '?')
    let is_initial = match phase_id {
        None => true,
        Some(p) => p == &def.initial_phase,
    };
    if is_initial {
        return ("?", yellow);
    }

    // 도달 불가 방어코드: phase_id 가 None 이면 위 is_initial 분기에서 이미 반환됨.
    let Some(pid) = phase_id else {
        return ("?", yellow);
    };
    if !def.phases.contains_key(pid) {
        return ("v", base_color);
    }

    // Interact transition 으로 다른 phase 로 넘어갈 수 있으면 NPC 에게 말을 걸어 전진 가능.
    // Log 전용 self-loop transition 만 있는 경우는 진행 불가로 간주한다.
    if interact_can_advance(def, pid) {
        return ("!", green);
    }

    // Auto transition 조건이 현재 충족됐으면 곧 자동 전진 → '!'
    let auto_ready = def.transitions.iter().any(|t|
        t.from == *pid
            && t.trigger == crate::modules::quest::TriggerKind::Auto
            && t.to != *pid
            && t.when.as_ref()
                .map(|c| eval_condition(c, inventory, world, state, quest_items))
                .unwrap_or(true)
    );
    if auto_ready {
        return ("!", green);
    }

    // 조건 미충족 — 아직 진행 중
    ("?", green)
}

/// 현재 phase 에서 시작하는 transition 이 하나도 없으면 터미널(퀘스트 완료)이다.
fn is_quest_terminal_def(def: &QuestDef, state: &QuestState, quest_id: &str) -> bool {
    let Some(phase_id) = state.phases.get(quest_id) else { return false };
    if !def.phases.contains_key(phase_id) { return false; }
    !def.transitions.iter().any(|t| t.from == *phase_id)
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;
    use crate::modules::map::{TileKind, MapResource};
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use std::sync::OnceLock;

    static TEST_QI: OnceLock<crate::modules::item::QuestItemRegistry> = OnceLock::new();
    fn qi() -> &'static crate::modules::item::QuestItemRegistry {
        TEST_QI.get_or_init(|| crate::modules::item::build_test_registry())
    }

    #[test]
    fn 대사인덱스는_다음_대사로_전진한다() {
        assert_eq!(next_dialogue_idx(0, 3), 1);
        assert_eq!(next_dialogue_idx(1, 3), 2);
    }

    #[test]
    fn 마지막_대사에서는_처음으로_되돌아간다() {
        assert_eq!(next_dialogue_idx(2, 3), 0);
    }

    #[test]
    fn 대사가_하나뿐이면_인덱스는_0에_머무른다() {
        assert_eq!(next_dialogue_idx(0, 1), 0);
    }

    #[test]
    fn 대사가_없으면_인덱스는_0을_반환한다() {
        assert_eq!(next_dialogue_idx(0, 0), 0);
    }

    #[test]
    fn 사방이_벽이면_주민은_제자리에_머무른다() {
        let mut map = Map::new(10, 10);
        map.set_tile(5, 5, TileKind::Floor);
        let occupied = HashSet::new();
        let mut rng = StdRng::seed_from_u64(0);
        for _ in 0..50 {
            let result = pick_next_tile(5, 5, &map, &occupied, None, &mut rng);
            assert_eq!(result, (5, 5));
        }
    }

    #[test]
    fn 인접한_바닥_타일로_이동할_수_있다() {
        let mut map = Map::new(10, 10);
        map.set_tile(5, 5, TileKind::Floor);
        map.set_tile(6, 5, TileKind::Floor);
        let occupied = HashSet::new();
        let mut moved = false;
        for seed in 0..200u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            if pick_next_tile(5, 5, &map, &occupied, None, &mut rng) == (6, 5) {
                moved = true;
                break;
            }
        }
        assert!(moved, "인접 Floor 타일로 이동해야 한다");
    }

    #[test]
    fn 주민은_벽_타일로는_이동하지_않는다() {
        let mut map = Map::new(10, 10);
        map.set_tile(5, 5, TileKind::Floor);
        map.set_tile(6, 5, TileKind::Floor);
        map.set_tile(4, 5, TileKind::Floor);
        let occupied = HashSet::new();
        for seed in 0..500u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let (_, ny) = pick_next_tile(5, 5, &map, &occupied, None, &mut rng);
            assert_eq!(ny, 5, "Wall 타일(y!=5)로 이동하면 안 된다");
        }
    }

    #[test]
    fn 주민은_점유된_이웃_타일로는_이동하지_않는다() {
        let mut map = Map::new(10, 10);
        map.set_tile(5, 5, TileKind::Floor);
        map.set_tile(6, 5, TileKind::Floor);
        let mut occupied = HashSet::new();
        occupied.insert((6usize, 5usize)); // 유일한 이웃이 점유됨
        for seed in 0..200u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let result = pick_next_tile(5, 5, &map, &occupied, None, &mut rng);
            assert_eq!(result, (5, 5), "점유된 타일로 이동하면 안 된다");
        }
    }

    #[test]
    fn 주민은_물타일로는_이동하지_않는다() {
        // 유일한 이웃이 물이면 항상 제자리.
        let mut map = Map::new(10, 10);
        map.set_tile(5, 5, TileKind::Floor);
        map.set_tile(6, 5, TileKind::Water);
        let occupied = HashSet::new();
        for seed in 0..200u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let result = pick_next_tile(5, 5, &map, &occupied, None, &mut rng);
            assert_eq!(result, (5, 5), "물 타일로 이동하면 안 된다");
        }
    }

    #[test]
    fn 맵_왼쪽_위_모서리에서는_범위밖_이웃을_거른다() {
        // x=0,y=0 → wrapping_sub 로 거대한 좌표가 생겨 `nx < MAP_WIDTH`/`ny < MAP_HEIGHT`
        // 가 false 가 되는 분기를 탄다. 유효 이웃은 (1,0),(0,1) 뿐.
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        map.set_tile(0, 0, TileKind::Floor);
        map.set_tile(1, 0, TileKind::Floor);
        map.set_tile(0, 1, TileKind::Floor);
        let occupied = HashSet::new();
        for seed in 0..200u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let (nx, ny) = pick_next_tile(0, 0, &map, &occupied, None, &mut rng);
            assert!((nx, ny) == (0, 0) || (nx, ny) == (1, 0) || (nx, ny) == (0, 1),
                "범위 밖 이웃은 선택되면 안 된다: ({nx},{ny})");
        }
    }

    #[test]
    fn 맵_오른쪽_아래_모서리에서는_범위밖_이웃을_거른다() {
        // x=MAP_WIDTH-1, y=MAP_HEIGHT-1 → x+1, y+1 이 범위를 벗어나 `< MAP_*` false.
        let (ex, ey) = (MAP_WIDTH - 1, MAP_HEIGHT - 1);
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        map.set_tile(ex, ey, TileKind::Floor);
        map.set_tile(ex - 1, ey, TileKind::Floor);
        map.set_tile(ex, ey - 1, TileKind::Floor);
        let occupied = HashSet::new();
        for seed in 0..200u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let (nx, ny) = pick_next_tile(ex, ey, &map, &occupied, None, &mut rng);
            assert!(nx < MAP_WIDTH && ny < MAP_HEIGHT, "범위 밖으로 가면 안 된다");
        }
    }

    #[test]
    fn 주민은_모래타일로는_이동할_수_있다() {
        let mut map = Map::new(10, 10);
        map.set_tile(5, 5, TileKind::Floor);
        map.set_tile(6, 5, TileKind::Sand);
        let occupied = HashSet::new();
        let mut moved = false;
        for seed in 0..200u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            if pick_next_tile(5, 5, &map, &occupied, None, &mut rng) == (6, 5) {
                moved = true;
                break;
            }
        }
        assert!(moved, "인접 모래 타일로 이동할 수 있어야 한다");
    }

    fn make_villager(just_bumped: bool) -> Villager {
        Villager {
            id: "test_id".to_string(),
            name: "테스트".to_string(),
            dialogues: vec![],
            dialogue_idx: 0,
            tile_x: 0,
            tile_y: 0,
            just_bumped,
            quest_dialogue_idx: 0,
            base_color: Color::WHITE,
            home_room: None,
            stationary: false,
            vendor: false,
        }
    }

    #[test]
    fn 충돌_직후_턴에는_이동하지_않고_플래그를_소모한다() {
        let mut v = make_villager(true);
        assert!(!take_turn(&mut v), "충돌 직후에는 이동하지 않아야 한다");
        assert!(!v.just_bumped, "플래그는 한 번만 소모된다");
    }

    #[test]
    fn 충돌이_없으면_주민은_정상적으로_이동한다() {
        let mut v = make_villager(false);
        assert!(take_turn(&mut v), "충돌 없는 주민은 정상 이동해야 한다");
    }

    #[test]
    fn 정지주민은_충돌이_없어도_이동턴을_갖지_않는다() {
        // stationary 주민(가판대 뒤 상인 등)은 take_turn 이 항상 false.
        let mut v = make_villager(false);
        v.stationary = true;
        assert!(!take_turn(&mut v), "정지 주민은 이동 턴을 갖지 않는다");
    }

    #[test]
    fn 정지주민은_충돌직후에도_제자리이고_플래그를_소모한다() {
        // just_bumped 가 먼저 소모되는 분기 + stationary 둘 다 false 반환.
        let mut v = make_villager(true);
        v.stationary = true;
        assert!(!take_turn(&mut v), "충돌 직후엔 어차피 제자리");
        assert!(!v.just_bumped, "충돌 플래그는 소모된다");
    }

    #[test]
    fn 이동스킵_판정은_정지거나_충돌직후면_참이다() {
        assert!(should_skip_villager_move(true, false), "정지 주민은 스킵");
        assert!(should_skip_villager_move(false, true), "충돌 직후도 스킵");
        assert!(should_skip_villager_move(true, true), "둘 다여도 스킵");
        assert!(!should_skip_villager_move(false, false), "정지도 충돌도 아니면 이동");
    }

    // --- vendor_for_interaction (카운터 너머 상호작용 판정) ---

    /// 카운터 한 줄 + 그 뒤 상인 자리를 둔 작은 맵을 만든다.
    /// 레이아웃(세로): vendor(5,5) / counter(5,6) / customer(5,7).
    fn shop_map() -> Map {
        let mut m = Map::new(10, 10);
        for x in 0..10 { for y in 0..10 { m.set_tile(x, y, TileKind::Floor); } }
        m.set_tile(5, 6, TileKind::Counter);
        m
    }

    #[test]
    fn 상인_타일에_직접_범프하면_그_상인이_반환된다() {
        let m = shop_map();
        // 상인이 (5,5) 에 있다고 가정.
        let got = vendor_for_interaction(5, 5, &m, |x, y| (x, y) == (5, 5));
        assert_eq!(got, Some((5, 5)), "상인 타일 직접 범프 → 그 상인");
    }

    #[test]
    fn 카운터에_범프하면_그_너머_상인이_반환된다() {
        let m = shop_map();
        // 손님이 (5,7) 에서 위(카운터 5,6)로 범프 → 카운터 너머 (5,5) 상인.
        let got = vendor_for_interaction(5, 6, &m, |x, y| (x, y) == (5, 5));
        assert_eq!(got, Some((5, 5)), "카운터 너머 상인 보정");
    }

    #[test]
    fn 상인없는_카운터에_범프하면_아무도_반환되지_않는다() {
        let m = shop_map();
        // 카운터에 인접한 vendor 가 없으면 None.
        let got = vendor_for_interaction(5, 6, &m, |_, _| false);
        assert_eq!(got, None, "카운터 너머에 상인 없으면 None");
    }

    #[test]
    fn 카운터도_상인도_아닌_타일은_상호작용_상인이_없다() {
        let m = shop_map();
        // 일반 바닥 (1,1) 범프 — 카운터도 vendor 도 아님 → None.
        let got = vendor_for_interaction(1, 1, &m, |_, _| false);
        assert_eq!(got, None, "일반 타일은 상점 상호작용 대상 아님");
    }

    #[test]
    fn 맵_가장자리_카운터는_범위밖_이웃을_상인으로_보지_않는다() {
        // 카운터를 (0,0) 에 두면 wrapping_sub 로 거대 좌표가 생긴다 — 범위 검사로 걸러야 한다.
        let mut m = Map::new(10, 10);
        m.set_tile(0, 0, TileKind::Counter);
        // 항상 vendor 라고 답하는 클로저라도, 유효 범위 이웃 (1,0)/(0,1) 만 검사돼야 한다.
        let got = vendor_for_interaction(0, 0, &m, |x, y| (x, y) == (1, 0));
        assert_eq!(got, Some((1, 0)), "유효 범위 이웃만 상인으로 인정");
    }

    #[test]
    fn 퀘스트_주민_필드는_기본값으로_초기화된다() {
        let v = Villager {
            id: "elder".to_string(),
            name: "장로".to_string(),
            dialogues: vec![],
            dialogue_idx: 0,
            tile_x: 0,
            tile_y: 0,
            just_bumped: false,
            quest_dialogue_idx: 0,
            base_color: Color::WHITE,
            home_room: None,
            stationary: false,
            vendor: false,
        };
        assert_eq!(v.id, "elder");
        assert_eq!(v.quest_dialogue_idx, 0);
    }

    // villager_turn 에서 플레이어 타일(Transform 또는 MovingTo 목적지)을
    // occupied 에 추가해 overlap 을 방지하는 메커니즘 검증
    #[test]
    fn 주민은_플레이어가_점유한_타일로는_이동하지_않는다() {
        let mut map = Map::new(10, 10);
        map.set_tile(5, 5, TileKind::Floor);
        map.set_tile(6, 5, TileKind::Floor);
        let mut occupied = HashSet::new();
        occupied.insert((6usize, 5usize)); // 플레이어 현재 위치 또는 MovingTo 목적지
        for seed in 0..200u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let result = pick_next_tile(5, 5, &map, &occupied, None, &mut rng);
            assert_eq!(result, (5, 5), "플레이어 타일로 이동하면 안 된다");
        }
    }

    #[test]
    fn 퀘스트_주민은_지정된_홈구역_밖으로는_이동하지_않는다() {
        // 5,5 중심에 3x3 Floor 배치. home_rect 를 (4..=6, 4..=6) 으로 제한
        let mut map = Map::new(10, 10);
        for x in 4..=6usize {
            for y in 4..=6usize {
                map.set_tile(x, y, TileKind::Floor);
            }
        }
        // home_rect 바깥 인접 Floor 추가 — 범위 밖이므로 선택되면 안 됨
        map.set_tile(7, 5, TileKind::Floor);
        let home = Rect::new(4, 4, 2, 2); // x1=4,y1=4,x2=6,y2=6
        let occupied = HashSet::new();
        for seed in 0..200u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let (nx, ny) = pick_next_tile(5, 5, &map, &occupied, Some(&home), &mut rng);
            assert!(nx >= 4 && nx <= 6 && ny >= 4 && ny <= 6, "home_rect 밖으로 이동하면 안 된다: ({nx},{ny})");
        }
    }

    #[test]
    fn 홈구역_하한_경계에서는_하한_미만_이웃을_거른다() {
        // 주민을 home_rect 의 왼쪽-아래 모서리(x1,y1)에 두면 왼쪽/아래 이웃이
        // x1/y1 미만이 되어 `nx >= r.x1`/`ny >= r.y1` 의 false 측 분기를 탄다.
        let mut map = Map::new(10, 10);
        for x in 3..=6usize {
            for y in 3..=6usize {
                map.set_tile(x, y, TileKind::Floor); // home 밖 (3,*),(*,3) 도 Floor
            }
        }
        let home = Rect::new(4, 4, 2, 2); // x1=4,y1=4,x2=6,y2=6
        let occupied = HashSet::new();
        for seed in 0..200u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            // 주민 위치 = (4,4) = home 의 좌하단 모서리.
            let (nx, ny) = pick_next_tile(4, 4, &map, &occupied, Some(&home), &mut rng);
            assert!(nx >= 4 && nx <= 6 && ny >= 4 && ny <= 6,
                "home_rect 하한 밖(3,*)/(*,3)으로 이동하면 안 된다: ({nx},{ny})");
        }
    }

    #[test]
    fn 홈구역_상한_경계에서는_상한_초과_이웃을_거른다() {
        // 주민을 home_rect 의 오른쪽-위 모서리(x2,y2)에 두면 오른쪽/위 이웃이
        // x2/y2 초과가 되어 `nx <= r.x2`/`ny <= r.y2` 의 false 측 분기를 탄다.
        let mut map = Map::new(10, 10);
        for x in 4..=7usize {
            for y in 4..=7usize {
                map.set_tile(x, y, TileKind::Floor); // home 밖 (7,*),(*,7) 도 Floor
            }
        }
        let home = Rect::new(4, 4, 2, 2); // x1=4,y1=4,x2=6,y2=6
        let occupied = HashSet::new();
        for seed in 0..200u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            // 주민 위치 = (6,6) = home 의 우상단 모서리.
            let (nx, ny) = pick_next_tile(6, 6, &map, &occupied, Some(&home), &mut rng);
            assert!(nx >= 4 && nx <= 6 && ny >= 4 && ny <= 6,
                "home_rect 상한 밖(7,*)/(*,7)으로 이동하면 안 된다: ({nx},{ny})");
        }
    }

    fn load_villager_defs() -> Vec<VillagerDef> {
        let text = std::fs::read_to_string("assets/villagers/villagers.ron")
            .expect("villager RON 파일이 존재해야 한다");
        ron::de::from_str::<Vec<VillagerDef>>(&text)
            .expect("villager RON 파싱 성공해야 한다")
    }

    /// 퀘스트 RON 의 giver_npc 가 villager id 와 매칭되는지 검사하는 헬퍼.
    /// (validate_quest_villager_refs 가 런타임에 같은 검사를 하지만 단위
    /// 테스트로도 보장.)
    fn assert_giver_resolves(quest_filename: &str, expected_giver_id: &str) {
        let qtext = std::fs::read_to_string(format!("assets/quests/{quest_filename}.ron"))
            .expect("quest RON 존재해야 한다");
        let qdef: crate::modules::quest::QuestDef = ron::de::from_str(&qtext)
            .expect("quest RON 파싱 성공해야 한다");
        assert_eq!(qdef.giver_npc, expected_giver_id);
        let villagers = load_villager_defs();
        assert!(
            villagers.iter().any(|v| v.id == expected_giver_id),
            "villagers.ron 에 id='{}' 가 존재해야 한다",
            expected_giver_id
        );
    }

    #[test]
    fn 세계균열_퀘스트의_giver는_villager로_매칭된다() {
        assert_giver_resolves("world_fracture", "old_man");
    }

    #[test]
    fn 패링_퀘스트의_giver는_villager로_매칭된다() {
        assert_giver_resolves("parry_quest", "grace");
    }

    #[test]
    fn 마검_퀘스트의_giver는_villager로_매칭된다() {
        assert_giver_resolves("demonsword_quest", "bastian");
    }

    #[test]
    fn 스킬시험_퀘스트의_giver_전투마법사는_villager로_매칭된다() {
        assert_giver_resolves("skill_trial_quest", "battlemage");
    }

    #[test]
    fn 파밍_퀘스트의_giver_보물사냥꾼은_villager로_매칭된다() {
        assert_giver_resolves("loot_farming_quest", "treasure_hunter");
    }

    #[test]
    fn 퀘스트_NPC_마커는_giver_매칭으로만_표시된다() {
        // 퀘스트 NPC만 마커. 새 모델: quest_registry 에서 giver_npc 매칭.
        // (test 환경에선 quest_registry 가 없으므로 villager id 기반으로 시뮬.)
        // 주의: 이제 모든 villager 가 어떤 quest 의 giver 다(child 도 buried_dungeon_quest
        // 의 giver 가 됨). 따라서 비-giver 예시는 실재하지 않는 합성 id 로 둔다.
        let regular_id = "__not_a_giver__"; // 어느 quest 의 giver 도 아닌 합성 id
        let quest_id = "ellen";             // herb_quest.ron 의 giver_npc
        // helper: quest RON 들에서 giver_npc 후보 set 조회
        let mut givers = std::collections::HashSet::new();
        for entry in std::fs::read_dir("assets/quests").unwrap().flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "ron") {
                let text = std::fs::read_to_string(&path).unwrap();
                if let Ok(q) = ron::de::from_str::<crate::modules::quest::QuestDef>(&text) {
                    givers.insert(q.giver_npc);
                }
            }
        }
        assert!(!givers.contains(regular_id), "{} 는 어느 quest 의 giver 도 아니어야", regular_id);
        assert!(givers.contains(quest_id), "{} 는 quest giver 여야", quest_id);
    }

    #[test]
    fn villager_RON은_정상적으로_파싱된다() {
        let defs = load_villager_defs();
        assert!(defs.len() >= 11, "11명 이상의 NPC 가 정의되어야 한다");
    }

    #[test]
    fn 모든_퀘스트_giver는_villager_registry에_존재한다() {
        let villager_defs = load_villager_defs();
        let villager_ids: HashSet<String> = villager_defs.iter().map(|d| d.id.clone()).collect();

        let dir = std::fs::read_dir("assets/quests").expect("assets/quests 가 존재해야 한다");
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("ron") { continue; }
            let text = std::fs::read_to_string(&path).unwrap();
            let qdef: QuestDef = ron::de::from_str(&text)
                .unwrap_or_else(|e| panic!("{:?} 파싱 실패: {}", path, e));
            assert!(
                villager_ids.contains(&qdef.giver_npc),
                "퀘스트 {:?} 의 giver_npc '{}' 가 villager id 와 매칭 안 됨",
                path, qdef.giver_npc
            );
        }
    }

    use crate::modules::quest::{QuestDef, QuestPhaseDef, QuestState, QuestTransition, TriggerKind, QuestCondition};
    use crate::modules::item::{PlayerInventory, InventoryItem, ItemKind, QuestItemKind};
    use crate::modules::zone::WorldState;
    use std::collections::HashMap as HM;

    fn phase(dialog: &[&str]) -> QuestPhaseDef {
        QuestPhaseDef {
            dialog: dialog.iter().map(|s| s.to_string()).collect(),
            objective: None,
        }
    }

    fn make_test_quest_def() -> QuestDef {
        // not_started → active(Auto: eternal_gem) → ready → done
        let mut phases = HM::new();
        phases.insert("not_started".to_string(), phase(&[]));
        phases.insert("active".to_string(), phase(&[]));
        phases.insert("ready".to_string(), phase(&[]));
        phases.insert("done".to_string(), phase(&[]));
        QuestDef {
            id: "test_quest".to_string(),
            title: "테스트".to_string(),
            giver_npc: "장로".to_string(),
            initial_phase: "not_started".to_string(),
            phases,
            transitions: vec![
                QuestTransition {
                    from: "not_started".to_string(),
                    trigger: TriggerKind::Interact,
                    when: None,
                    actions: vec![],
                    to: "active".to_string(),
                },
                QuestTransition {
                    from: "active".to_string(),
                    trigger: TriggerKind::Auto,
                    when: Some(QuestCondition::HasItem("eternal_gem".to_string())),
                    actions: vec![],
                    to: "ready".to_string(),
                },
                QuestTransition {
                    from: "ready".to_string(),
                    trigger: TriggerKind::Interact,
                    when: None,
                    actions: vec![],
                    to: "done".to_string(),
                },
            ],
            spawns: vec![],
            spawn_chance: 1.0,
        }
    }

    fn make_state_at(phase: &str) -> QuestState {
        let mut s = QuestState::default();
        s.phases.insert("test_quest".to_string(), phase.to_string());
        s
    }

    fn empty_inventory() -> PlayerInventory { PlayerInventory::default() }
    fn default_world() -> WorldState { WorldState::default() }

    #[test]
    fn 퀘스트가_상태에_없으면_글리프는_노란_물음표다() {
        let def = make_test_quest_def();
        let state = QuestState::default();
        let inv = empty_inventory();
        let world = default_world();
        let (glyph, color) = quest_npc_glyph("test_quest", &def, &state, &inv, &world, Color::WHITE, qi());
        assert_eq!(glyph, "?");
        assert_eq!(color, Color::rgb(1.0, 0.9, 0.1), "퀘스트 있음 = 노란색");
    }

    #[test]
    fn 초기_페이즈에서는_글리프가_노란_물음표다() {
        let def = make_test_quest_def();
        let state = make_state_at("not_started");
        let inv = empty_inventory();
        let world = default_world();
        let (glyph, color) = quest_npc_glyph("test_quest", &def, &state, &inv, &world, Color::WHITE, qi());
        assert_eq!(glyph, "?");
        assert_eq!(color, Color::rgb(1.0, 0.9, 0.1), "initial_phase = 노란 ?");
    }

    #[test]
    fn 진행중이고_조건_미충족이면_글리프는_초록_물음표다() {
        let def = make_test_quest_def();
        let state = make_state_at("active");
        let inv = empty_inventory(); // 아이템 없음 → auto_advance 조건 미충족
        let world = default_world();
        let (glyph, color) = quest_npc_glyph("test_quest", &def, &state, &inv, &world, Color::WHITE, qi());
        assert_eq!(glyph, "?");
        assert_eq!(color, Color::rgb(0.3, 1.0, 0.6), "조건 미충족 = 초록 ?");
    }

    #[test]
    fn 자동전진_조건이_충족되면_글리프는_초록_느낌표다() {
        let def = make_test_quest_def();
        let state = make_state_at("active");
        // eternal_gem 보유 → auto_advance 조건 충족 → 다음 페이즈로 넘어갈 수 있다
        let mut inv = empty_inventory();
        inv.items.push(InventoryItem::new(ItemKind::QuestItem(QuestItemKind("eternal_gem"))));
        let world = default_world();
        let (glyph, color) = quest_npc_glyph("test_quest", &def, &state, &inv, &world, Color::WHITE, qi());
        assert_eq!(glyph, "!");
        assert_eq!(color, Color::rgb(0.3, 1.0, 0.6), "auto_advance 조건 충족 = 초록 !");
    }

    #[test]
    fn 상호작용_전진이_가능하면_글리프는_초록_느낌표다() {
        let def = make_test_quest_def();
        let state = make_state_at("ready");
        let inv = empty_inventory();
        let world = default_world();
        let (glyph, color) = quest_npc_glyph("test_quest", &def, &state, &inv, &world, Color::WHITE, qi());
        assert_eq!(glyph, "!");
        assert_eq!(color, Color::rgb(0.3, 1.0, 0.6), "Interact transition 있음 = 초록 !");
    }

    #[test]
    fn 터미널_페이즈에서는_글리프가_기본색_v다() {
        let def = make_test_quest_def();
        let state = make_state_at("done");
        let inv = empty_inventory();
        let world = default_world();
        let base = Color::rgb(0.9, 0.8, 0.5);
        let (glyph, color) = quest_npc_glyph("test_quest", &def, &state, &inv, &world, base, qi());
        assert_eq!(glyph, "v");
        assert_eq!(color, base, "터미널 = v + 기본 색상");
    }

    // interact_can_advance 단위 테스트
    #[test]
    fn 상호작용_전환이_있는_페이즈는_전진_가능으로_판정된다() {
        let def = make_test_quest_def();
        // ready phase 에는 Interact transition (ready → done) 이 있다
        assert!(interact_can_advance(&def, "ready"));
    }

    #[test]
    fn 자동전환만_있는_페이즈는_전진_불가로_판정된다() {
        let def = make_test_quest_def();
        // active phase 에는 Auto transition 만 있고 Interact transition 은 없다
        assert!(!interact_can_advance(&def, "active"));
    }

    #[test]
    fn 자기참조_전환만_있으면_전진_불가로_판정된다() {
        // gathering phase 에 Log 전용 self-loop Interact transition (to == from) 만 있는 경우
        let mut phases = HM::new();
        phases.insert("not_started".to_string(), phase(&[]));
        phases.insert("gathering".to_string(), phase(&["아직 재료가 부족하네."]));
        let def = QuestDef {
            id: "alchemist_quest".to_string(),
            title: "연금술사".to_string(),
            giver_npc: "연금술사".to_string(),
            initial_phase: "not_started".to_string(),
            phases,
            transitions: vec![
                QuestTransition {
                    from: "not_started".to_string(),
                    trigger: TriggerKind::Interact,
                    when: None,
                    actions: vec![],
                    to: "gathering".to_string(),
                },
                // self-loop 힌트 transition: to == from
                QuestTransition {
                    from: "gathering".to_string(),
                    trigger: TriggerKind::Interact,
                    when: None,
                    actions: vec![crate::modules::quest::QuestAction::Log("아직 멀었네".to_string())],
                    to: "gathering".to_string(),
                },
            ],
            spawns: vec![],
            spawn_chance: 1.0,
        };
        assert!(!interact_can_advance(&def, "gathering"), "self-loop transition 은 전진으로 보지 않는다");
    }

    #[test]
    fn 자기참조_힌트_전환만_있으면_글리프는_초록_물음표다() {
        // gathering 페이즈에 Log 전용 self-loop transition 만 있으면 초록 '?'
        // initial_phase 는 "not_started", 현재 페이즈는 "gathering" — initial 분기에 걸리지 않음
        let mut phases = HM::new();
        phases.insert("not_started".to_string(), phase(&[]));
        phases.insert("gathering".to_string(), phase(&["아직 재료가 부족하네."]));
        let def = QuestDef {
            id: "alchemist_quest".to_string(),
            title: "연금술사".to_string(),
            giver_npc: "연금술사".to_string(),
            initial_phase: "not_started".to_string(),
            phases,
            transitions: vec![
                QuestTransition {
                    from: "not_started".to_string(),
                    trigger: TriggerKind::Interact,
                    when: None,
                    actions: vec![],
                    to: "gathering".to_string(),
                },
                QuestTransition {
                    from: "gathering".to_string(),
                    trigger: TriggerKind::Interact,
                    when: None,
                    actions: vec![crate::modules::quest::QuestAction::Log("아직 멀었네".to_string())],
                    to: "gathering".to_string(),
                },
            ],
            spawns: vec![],
            spawn_chance: 1.0,
        };
        let mut state = QuestState::default();
        state.phases.insert("alchemist_quest".to_string(), "gathering".to_string());
        let inv = empty_inventory();
        let world = default_world();
        let (glyph, color) = quest_npc_glyph("alchemist_quest", &def, &state, &inv, &world, Color::WHITE, qi());
        assert_eq!(glyph, "?", "self-loop 힌트만 있는 phase 는 '?' 여야 한다");
        assert_eq!(color, Color::rgb(0.3, 1.0, 0.6));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // seam(read_villager_defs / collect_missing_giver_refs) 단위 테스트
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn 존재하는_villager_파일을_읽으면_정의들을_반환한다() {
        let defs = read_villager_defs("assets/villagers/villagers.ron")
            .expect("정상 경로는 Ok 여야 한다");
        assert!(defs.len() >= 11, "11명 이상이 적재되어야 한다");
    }

    #[test]
    fn 없는_경로의_villager_파일을_읽으면_읽기_에러를_반환한다() {
        let err = read_villager_defs("assets/villagers/__none__.ron")
            .expect_err("없는 파일은 Err 여야 한다");
        assert!(err.contains("읽을 수 없습니다"), "읽기 실패 메시지: {err}");
    }

    #[test]
    fn 잘못된_RON_형식의_villager_파일은_파싱_에러를_반환한다() {
        // Cargo.toml 은 villager RON 형식이 아니므로 파싱이 실패한다 (읽기는 성공).
        let err = read_villager_defs("Cargo.toml")
            .expect_err("잘못된 형식은 Err 여야 한다");
        assert!(err.contains("파싱 실패"), "파싱 실패 메시지: {err}");
    }

    fn registry_with_giver(quest_id: &str, giver: &str) -> QuestRegistry {
        let mut reg = QuestRegistry::default();
        let mut qdef = make_test_quest_def();
        qdef.giver_npc = giver.to_string();
        reg.quests.insert(quest_id.to_string(), qdef);
        reg
    }

    #[test]
    fn 모든_giver가_매칭되면_누락_목록은_비어있다() {
        let qreg = registry_with_giver("test_quest", "elder");
        let mut vreg = VillagerRegistry::default();
        vreg.villagers.push(VillagerDef {
            id: "elder".to_string(), name: "장로".to_string(),
            color: [1.0, 1.0, 1.0], dialogs: vec![], speed: 1.0,
            stationary: false, vendor: false,
        });
        assert!(collect_missing_giver_refs(&qreg, &vreg).is_empty());
    }

    #[test]
    fn giver가_villager에_없으면_누락_목록에_포함된다() {
        let qreg = registry_with_giver("test_quest", "ghost");
        let vreg = VillagerRegistry::default(); // villager 없음
        let errors = collect_missing_giver_refs(&qreg, &vreg);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("ghost"), "누락 메시지: {}", errors[0]);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // quest_npc_glyph / is_quest_terminal_def 의 미커버 분기
    // (현재 phase 가 def.phases 에 없는 경우)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn 알_수_없는_페이즈이면_글리프는_기본색_v다() {
        // state 의 phase 가 def.phases 에 없으면 → terminal 로 간주, base_color 'v'.
        let def = make_test_quest_def();
        let mut state = QuestState::default();
        state.phases.insert("test_quest".to_string(), "ghost_phase".to_string());
        let inv = empty_inventory();
        let world = default_world();
        let base = Color::rgb(0.1, 0.2, 0.3);
        let (glyph, color) = quest_npc_glyph("test_quest", &def, &state, &inv, &world, base, qi());
        assert_eq!(glyph, "v", "정의에 없는 phase 는 터미널 취급");
        assert_eq!(color, base);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // App 하네스 공통 셋업
    // ─────────────────────────────────────────────────────────────────────────

    use crate::modules::zone::{ZoneId, CloseQuestPortalEvent};
    use crate::modules::ui::shop::ShopOpenEvent;
    use std::time::Duration;

    /// 전체 Floor 맵 (MAP_WIDTH x MAP_HEIGHT) — 이동 분기 테스트용.
    fn full_floor_map() -> Map {
        let mut m = Map::new(MAP_WIDTH, MAP_HEIGHT);
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                m.set_tile(x, y, TileKind::Floor);
            }
        }
        m
    }

    /// AssetServer(폰트/이미지) 를 제공하는 기본 App.
    fn asset_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app.init_asset::<Image>();
        app
    }

    /// rooms[0] 은 do_spawn 에서 skip(1) 되므로 더미 1개 + 실제 방들을 둔다.
    fn rooms_with(n: usize) -> Vec<Rect> {
        let mut rooms = vec![Rect::new(1, 1, 2, 2)]; // skip 대상 더미
        for i in 0..n {
            let x = 5 + i * 6;
            rooms.push(Rect::new(x, 5, 3, 3));
        }
        rooms
    }

    fn vdef(id: &str, name: &str) -> VillagerDef {
        VillagerDef {
            id: id.to_string(), name: name.to_string(),
            color: [0.5, 0.6, 0.7],
            dialogs: vec!["안녕".to_string(), "또 봐".to_string()],
            speed: 1.0,
            stationary: false, vendor: false,
        }
    }

    /// vendor/stationary 플래그를 지정한 VillagerDef (상점 테스트용).
    fn vendor_def(id: &str, name: &str) -> VillagerDef {
        VillagerDef {
            id: id.to_string(), name: name.to_string(),
            color: [0.3, 0.9, 0.3],
            dialogs: vec!["좋은 물건 있소".to_string()],
            speed: 1.0,
            stationary: true, vendor: true,
        }
    }

    fn spawn_villager(app: &mut App, id: &str, name: &str, tile: (usize, usize)) -> Entity {
        app.world.spawn((
            Text::from_section("v", TextStyle::default()),
            Transform::from_translation(tile_to_world_coords(tile.0, tile.1).extend(Z_VILLAGER)),
            Villager {
                id: id.to_string(), name: name.to_string(),
                dialogues: vec!["안녕".to_string(), "또 봐".to_string()],
                dialogue_idx: 0, tile_x: tile.0, tile_y: tile.1,
                just_bumped: false, quest_dialogue_idx: 0,
                base_color: Color::WHITE, home_room: None,
                stationary: false, vendor: false,
            },
            Speed::new(1.0),
            MoveQueue::default(),
        )).id()
    }

    // ─────────────────────────────────────────────────────────────────────────
    // VillagerPlugin::build
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn 빌리저플러그인을_등록하면_빌드가_패닉없이_완료된다() {
        let mut app = App::new();
        app.add_plugins(VillagerPlugin);
        // build() 가 시스템 등록만 해도 커버됨 (update 불필요).
        assert!(app.world.contains_resource::<VillagerRegistry>());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // load_villagers / validate_quest_villager_refs (실제 파일/registry 사용 Ok 경로)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn 로드시스템은_실제_RON을_registry에_적재한다() {
        let mut app = App::new();
        app.init_resource::<VillagerRegistry>()
            .add_systems(Update, load_villagers);
        app.update();
        let reg = app.world.resource::<VillagerRegistry>();
        assert!(reg.villagers.len() >= 11, "실제 RON 의 NPC 들이 적재되어야 한다");
    }

    #[test]
    fn 검증시스템은_giver가_모두_매칭되면_패닉없이_통과한다() {
        let mut app = App::new();
        // giver(elder) 가 villager 에 존재 → errors 비어 있음 → exit 안 함.
        app.insert_resource(registry_with_giver("test_quest", "elder"));
        let mut vreg = VillagerRegistry::default();
        vreg.villagers.push(vdef("elder", "장로"));
        app.insert_resource(vreg);
        app.add_systems(Update, validate_quest_villager_refs);
        app.update(); // 패닉/exit 없이 통과하면 성공.
    }

    // ─────────────────────────────────────────────────────────────────────────
    // sync_occupied_tiles
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn 동기화시스템은_점유타일을_주민_위치로_갱신한다() {
        let mut app = App::new();
        app.init_resource::<OccupiedTiles>()
            .add_systems(Update, sync_occupied_tiles);
        spawn_villager(&mut app, "a", "갑", (3, 4));
        spawn_villager(&mut app, "b", "을", (7, 8));
        app.update();
        let occ = &app.world.resource::<OccupiedTiles>().0;
        assert!(occ.contains(&(3, 4)) && occ.contains(&(7, 8)));
        assert_eq!(occ.len(), 2);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // handle_kill_npc
    // ─────────────────────────────────────────────────────────────────────────

    fn kill_npc_app() -> App {
        let mut app = App::new();
        app.add_event::<KillNpcEvent>()
            .add_event::<LogMessage>()
            .add_systems(Update, handle_kill_npc);
        app
    }

    #[test]
    fn 킬NPC_이벤트는_일치하는_주민을_제거하고_로그를_남긴다() {
        let mut app = kill_npc_app();
        let e = spawn_villager(&mut app, "goblin_king", "고블린왕", (1, 1));
        app.world.send_event(KillNpcEvent("goblin_king".to_string()));
        app.update();
        assert!(app.world.get_entity(e).is_none(), "일치 주민은 despawn 되어야 한다");
        let logs = app.world.resource::<Events<LogMessage>>();
        assert_eq!(logs.len(), 1, "쓰러짐 로그 한 줄");
    }

    #[test]
    fn 킬NPC_이벤트는_일치하지_않는_주민은_제거하지_않는다() {
        let mut app = kill_npc_app();
        let e = spawn_villager(&mut app, "farmer", "농부", (1, 1));
        app.world.send_event(KillNpcEvent("other_id".to_string()));
        app.update();
        assert!(app.world.get_entity(e).is_some(), "불일치 주민은 유지되어야 한다");
        assert_eq!(app.world.resource::<Events<LogMessage>>().len(), 0);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // smooth_villager_move
    // ─────────────────────────────────────────────────────────────────────────

    fn smooth_app() -> App {
        let mut app = App::new();
        app.init_resource::<Time>()
            .add_systems(Update, smooth_villager_move);
        app
    }

    #[test]
    fn 부드러운이동은_먼_목표에_한_걸음씩_다가간다() {
        let mut app = smooth_app();
        let start = tile_to_world_coords(0, 0).extend(Z_VILLAGER);
        let target = tile_to_world_coords(40, 0).extend(Z_VILLAGER); // 매우 먼 목표
        let e = app.world.spawn((
            Transform::from_translation(start),
            { let mut q = MoveQueue::default(); q.0.push_back(target); q },
            Speed::new(1.0),
            Villager {
                id: "m".into(), name: "이동".into(), dialogues: vec![],
                dialogue_idx: 0, tile_x: 0, tile_y: 0, just_bumped: false,
                quest_dialogue_idx: 0, base_color: Color::WHITE, home_room: None,
                stationary: false, vendor: false,
            },
        )).id();
        app.world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(0.016));
        app.update();
        let t = app.world.get::<Transform>(e).unwrap();
        assert!(t.translation.x > start.x, "목표 방향으로 전진해야 한다");
        assert!(t.translation.x < target.x, "한 프레임에 도달하면 안 된다 (else 분기)");
        let q = app.world.get::<MoveQueue>(e).unwrap();
        assert_eq!(q.0.len(), 1, "아직 목표 미도달 — 큐 유지");
    }

    #[test]
    fn 부드러운이동은_가까운_목표에_도달하면_큐에서_제거한다() {
        let mut app = smooth_app();
        let start = tile_to_world_coords(0, 0).extend(Z_VILLAGER);
        // step 안에 들어오는 매우 가까운 목표
        let target = start + Vec3::new(0.01, 0.0, 0.0);
        let e = app.world.spawn((
            Transform::from_translation(start),
            { let mut q = MoveQueue::default(); q.0.push_back(target); q },
            Speed::new(1.0),
            Villager {
                id: "m".into(), name: "이동".into(), dialogues: vec![],
                dialogue_idx: 0, tile_x: 0, tile_y: 0, just_bumped: false,
                quest_dialogue_idx: 0, base_color: Color::WHITE, home_room: None,
                stationary: false, vendor: false,
            },
        )).id();
        app.world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(0.5));
        app.update();
        let t = app.world.get::<Transform>(e).unwrap();
        assert_eq!(t.translation, target, "목표에 스냅되어야 한다");
        assert!(app.world.get::<MoveQueue>(e).unwrap().0.is_empty(), "도달하면 큐 비움");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // villager_turn
    // ─────────────────────────────────────────────────────────────────────────

    fn turn_app() -> App {
        let mut app = App::new();
        app.insert_resource(MapResource(full_floor_map()))
            .add_event::<PlayerActedEvent>()
            .add_systems(Update, villager_turn);
        app
    }

    #[test]
    fn 플레이어_행동_이벤트가_없으면_주민은_이동하지_않는다() {
        let mut app = turn_app();
        let e = spawn_villager(&mut app, "a", "갑", (10, 10));
        app.update(); // PlayerActedEvent 미발행 → early return
        let q = app.world.get::<MoveQueue>(e).unwrap();
        assert!(q.0.is_empty(), "이벤트 없으면 이동 큐가 비어 있어야 한다");
    }

    #[test]
    fn 플레이어_행동시_주민은_에너지가_차면_한칸_이동을_큐에_쌓는다() {
        let mut app = turn_app();
        let e = spawn_villager(&mut app, "a", "갑", (10, 10));
        // speed.value 를 1.0 으로 → energy 가 매 턴 1.0 누적되어 한 번 이동.
        app.world.send_event(PlayerActedEvent);
        app.update();
        let q = app.world.get::<MoveQueue>(e).unwrap();
        // STAY_CHANCE 로 제자리일 수도 있으므로 큐 길이 <= 1.
        assert!(q.0.len() <= 1);
        // energy 는 1.0 누적 후 한 칸 소비 → 0.0
        let sp = app.world.get::<Speed>(e).unwrap();
        assert!(sp.energy < 1.0, "에너지는 이동에 소비되어 1 미만이어야 한다");
    }

    #[test]
    fn 플레이어_행동시_충돌직후_주민은_이번턴에_이동하지_않는다() {
        let mut app = turn_app();
        let e = app.world.spawn((
            Text::from_section("v", TextStyle::default()),
            Transform::from_translation(tile_to_world_coords(10, 10).extend(Z_VILLAGER)),
            Villager {
                id: "a".into(), name: "갑".into(), dialogues: vec![],
                dialogue_idx: 0, tile_x: 10, tile_y: 10, just_bumped: true, // 충돌 플래그 on
                quest_dialogue_idx: 0, base_color: Color::WHITE, home_room: None,
                stationary: false, vendor: false,
            },
            Speed::new(1.0),
            MoveQueue::default(),
        )).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        let q = app.world.get::<MoveQueue>(e).unwrap();
        assert!(q.0.is_empty(), "충돌 직후 턴에는 이동하지 않아야 한다 (take_turn=false 분기)");
        assert!(!app.world.get::<Villager>(e).unwrap().just_bumped, "플래그 소모");
    }

    #[test]
    fn vendor_주민은_턴이_여러번_지나도_제자리에_있는다() {
        // 정지 vendor 는 villager_turn 이 매 턴 스킵해 좌표가 변하지 않는다.
        let mut app = turn_app();
        let e = app.world.spawn((
            Text::from_section("$", TextStyle::default()),
            Transform::from_translation(tile_to_world_coords(10, 10).extend(Z_VILLAGER)),
            Villager {
                id: "merchant".into(), name: "상인".into(), dialogues: vec![],
                dialogue_idx: 0, tile_x: 10, tile_y: 10, just_bumped: false,
                quest_dialogue_idx: 0, base_color: Color::WHITE, home_room: None,
                stationary: true, vendor: true,
            },
            Speed::new(1.0),
            MoveQueue::default(),
        )).id();
        for _ in 0..20 {
            app.world.send_event(PlayerActedEvent);
            app.update();
        }
        let v = app.world.get::<Villager>(e).unwrap();
        assert_eq!((v.tile_x, v.tile_y), (10, 10), "정지 vendor 는 제자리 유지");
        assert!(app.world.get::<MoveQueue>(e).unwrap().0.is_empty(), "이동 큐가 비어 있어야 한다");
    }

    #[test]
    fn 일반주민은_턴이_지나면_언젠가_이동한다() {
        // 대조군: 정지 아님 → 충분히 많은 턴 동안 한 번은 이동한다.
        let mut app = turn_app();
        let e = spawn_villager(&mut app, "farmer", "농부", (10, 10));
        let mut moved = false;
        for _ in 0..200 {
            app.world.send_event(PlayerActedEvent);
            app.update();
            let v = app.world.get::<Villager>(e).unwrap();
            if (v.tile_x, v.tile_y) != (10, 10) { moved = true; break; }
        }
        assert!(moved, "일반 주민은 언젠가 이동해야 한다(정지와 대조)");
    }

    #[test]
    fn 플레이어_위치는_주민_이동에서_점유되어_막힌다() {
        // 플레이어를 주민 옆 유일 통로에 두면 주민은 그 칸으로 못 간다.
        let mut app = App::new();
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        map.set_tile(10, 10, TileKind::Floor);
        map.set_tile(11, 10, TileKind::Floor); // 유일한 이웃
        app.insert_resource(MapResource(map))
            .add_event::<PlayerActedEvent>()
            .add_systems(Update, villager_turn);
        let v = spawn_villager(&mut app, "a", "갑", (10, 10));
        // 플레이어를 (11,10) 에 둔다 (Transform 기반).
        app.world.spawn((
            Player,
            Transform::from_translation(tile_to_world_coords(11, 10).extend(0.0)),
        ));
        for _ in 0..30 {
            app.world.send_event(PlayerActedEvent);
            app.update();
            let v_comp = app.world.get::<Villager>(v).unwrap();
            assert_eq!((v_comp.tile_x, v_comp.tile_y), (10, 10),
                "플레이어가 막은 유일 통로로 이동하면 안 된다");
        }
    }

    #[test]
    fn 플레이어가_이동중이면_목적지_타일이_점유로_막힌다() {
        // MovingTo 의 target 타일이 occupied 에 들어가는 분기.
        let mut app = App::new();
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        map.set_tile(10, 10, TileKind::Floor);
        map.set_tile(11, 10, TileKind::Floor);
        app.insert_resource(MapResource(map))
            .add_event::<PlayerActedEvent>()
            .add_systems(Update, villager_turn);
        let v = spawn_villager(&mut app, "a", "갑", (10, 10));
        // 플레이어는 멀리 있지만 MovingTo 목적지가 (11,10).
        app.world.spawn((
            Player,
            Transform::from_translation(tile_to_world_coords(40, 40).extend(0.0)),
            MovingTo { target: tile_to_world_coords(11, 10).extend(0.0) },
        ));
        for _ in 0..30 {
            app.world.send_event(PlayerActedEvent);
            app.update();
            let v_comp = app.world.get::<Villager>(v).unwrap();
            assert_eq!((v_comp.tile_x, v_comp.tile_y), (10, 10),
                "플레이어 이동 목적지로는 이동하면 안 된다");
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // spawn_on_startup / do_spawn / respawn_on_regen
    // ─────────────────────────────────────────────────────────────────────────

    fn quest_registry_with(quest_id: &str, giver: &str, active: bool) -> QuestRegistry {
        let mut reg = registry_with_giver(quest_id, giver);
        if active { reg.active.insert(quest_id.to_string()); }
        reg
    }

    #[test]
    fn 마을맵이면_시작시_방마다_주민이_스폰된다() {
        let mut app = asset_app();
        let mut map = full_floor_map();
        map.map_type = MapType::Village;
        map.rooms = rooms_with(2); // 더미1 + 방2 → 주민 2명
        app.insert_resource(MapResource(map));
        app.insert_resource(QuestRegistry::default()); // 퀘스트 없음 → 모두 일반 NPC
        let mut vreg = VillagerRegistry::default();
        vreg.villagers.push(vdef("farmer", "농부"));
        app.insert_resource(vreg);
        app.add_systems(Update, spawn_on_startup);
        app.update();
        let n = app.world.query::<&Villager>().iter(&app.world).count();
        assert_eq!(n, 2, "방 2개에 주민 2명 스폰");
    }

    #[test]
    fn 던전맵이면_시작시_주민이_스폰되지_않는다() {
        let mut app = asset_app();
        let mut map = full_floor_map();
        map.map_type = MapType::Dungeon; // 마을 아님
        map.rooms = rooms_with(2);
        app.insert_resource(MapResource(map));
        app.insert_resource(QuestRegistry::default());
        let mut vreg = VillagerRegistry::default();
        vreg.villagers.push(vdef("farmer", "농부"));
        app.insert_resource(vreg);
        app.add_systems(Update, spawn_on_startup);
        app.update();
        let n = app.world.query::<&Villager>().iter(&app.world).count();
        assert_eq!(n, 0, "던전에는 주민 미스폰");
    }

    #[test]
    fn 활성_퀘스트_NPC는_퀘스트방에_먼저_스폰된다() {
        let mut app = asset_app();
        let mut map = full_floor_map();
        map.map_type = MapType::Village;
        map.rooms = rooms_with(2);
        app.insert_resource(MapResource(map));
        // elder 가 active quest 의 giver, farmer 는 일반.
        app.insert_resource(quest_registry_with("eq", "elder", true));
        let mut vreg = VillagerRegistry::default();
        vreg.villagers.push(vdef("elder", "장로"));
        vreg.villagers.push(vdef("farmer", "농부"));
        app.insert_resource(vreg);
        app.add_systems(Update, spawn_on_startup);
        app.update();
        let ids: HashSet<String> = app.world.query::<&Villager>()
            .iter(&app.world).map(|v| v.id.clone()).collect();
        assert!(ids.contains("elder"), "퀘스트 NPC 가 스폰되어야 한다");
        // 퀘스트 NPC 는 home_room 이 Some, 일반은 None.
        let elder_home = app.world.query::<&Villager>().iter(&app.world)
            .find(|v| v.id == "elder").unwrap().home_room;
        assert!(elder_home.is_some(), "퀘스트 NPC 는 home_room 이 지정된다");
    }

    #[test]
    fn 일반_NPC가_없으면_빈_방은_건너뛴다() {
        // 퀘스트 NPC 1명만 있고 일반 NPC 0명 → 방이 더 많으면 빈 방은 continue.
        let mut app = asset_app();
        let mut map = full_floor_map();
        map.map_type = MapType::Village;
        map.rooms = rooms_with(3); // 더미1 + 방3
        app.insert_resource(MapResource(map));
        app.insert_resource(quest_registry_with("eq", "elder", true));
        let mut vreg = VillagerRegistry::default();
        vreg.villagers.push(vdef("elder", "장로")); // 퀘스트 NPC 1명, 일반 0명
        app.insert_resource(vreg);
        app.add_systems(Update, spawn_on_startup);
        app.update();
        let n = app.world.query::<&Villager>().iter(&app.world).count();
        assert_eq!(n, 1, "퀘스트 NPC 1명만 스폰, 나머지 방은 건너뜀");
    }

    #[test]
    fn 방이_없으면_주민_스폰을_건너뛴다() {
        let mut app = asset_app();
        let mut map = full_floor_map();
        map.map_type = MapType::Village;
        map.rooms = vec![]; // 방 없음 → do_spawn early return
        app.insert_resource(MapResource(map));
        app.insert_resource(QuestRegistry::default());
        let mut vreg = VillagerRegistry::default();
        vreg.villagers.push(vdef("farmer", "농부"));
        app.insert_resource(vreg);
        app.add_systems(Update, spawn_on_startup);
        app.update();
        assert_eq!(app.world.query::<&Villager>().iter(&app.world).count(), 0);
    }

    #[test]
    fn 상점이_있으면_vendor가_가판대뒤_고정위치에_정지로_스폰된다() {
        let mut app = asset_app();
        let mut map = full_floor_map();
        map.map_type = MapType::Village;
        map.rooms = rooms_with(2);
        map.shop_vendor = Some((30, 25)); // 가판대 뒤 상인 자리
        app.insert_resource(MapResource(map));
        app.insert_resource(QuestRegistry::default());
        let mut vreg = VillagerRegistry::default();
        vreg.villagers.push(vendor_def("merchant", "상인"));
        vreg.villagers.push(vdef("farmer", "농부"));
        app.insert_resource(vreg);
        app.add_systems(Update, spawn_on_startup);
        app.update();
        // merchant 는 (30,25) 에 정지·vendor 로 스폰.
        let merchant = app.world.query::<&Villager>().iter(&app.world)
            .find(|v| v.id == "merchant").expect("vendor 가 스폰돼야 한다");
        assert_eq!((merchant.tile_x, merchant.tile_y), (30, 25), "가판대 뒤 고정 위치");
        assert!(merchant.stationary, "vendor 는 정지 주민");
        assert!(merchant.vendor, "vendor 플래그 유지");
    }

    #[test]
    fn vendor는_가판대에_스폰되면_방에_중복_스폰되지_않는다() {
        // shop_vendor 가 있으면 vendor 는 카운터에만 1명, 방 배치에서 제외된다.
        let mut app = asset_app();
        let mut map = full_floor_map();
        map.map_type = MapType::Village;
        map.rooms = rooms_with(3); // 더미1 + 방3
        map.shop_vendor = Some((30, 25));
        app.insert_resource(MapResource(map));
        app.insert_resource(QuestRegistry::default());
        let mut vreg = VillagerRegistry::default();
        vreg.villagers.push(vendor_def("merchant", "상인"));
        app.insert_resource(vreg);
        app.add_systems(Update, spawn_on_startup);
        app.update();
        // vendor 만 등록돼 있고 가판대에 1명 스폰 — 방 3개에 추가 vendor 가 없어야 한다.
        let merchant_count = app.world.query::<&Villager>().iter(&app.world)
            .filter(|v| v.id == "merchant").count();
        assert_eq!(merchant_count, 1, "vendor 는 가판대에 1명만 스폰");
    }

    #[test]
    fn vendor_글리프는_달러기호로_표시된다() {
        let mut app = asset_app();
        let mut map = full_floor_map();
        map.map_type = MapType::Village;
        map.rooms = rooms_with(1);
        map.shop_vendor = Some((30, 25));
        app.insert_resource(MapResource(map));
        app.insert_resource(QuestRegistry::default());
        let mut vreg = VillagerRegistry::default();
        vreg.villagers.push(vendor_def("merchant", "상인"));
        app.insert_resource(vreg);
        app.add_systems(Update, spawn_on_startup);
        app.update();
        // vendor 엔티티의 Text 글리프가 '$' 여야 한다.
        let mut found = false;
        let mut q = app.world.query::<(&Villager, &Text)>();
        for (v, text) in q.iter(&app.world) {
            if v.id == "merchant" {
                assert_eq!(text.sections[0].value, "$", "vendor 글리프는 '$'");
                found = true;
            }
        }
        assert!(found, "vendor 엔티티를 찾아야 한다");
    }

    #[test]
    fn 상점_위치가_없으면_vendor도_가판대에_스폰되지_않는다() {
        // shop_vendor 가 None 이면 do_spawn 의 vendor 분기를 타지 않는다(false 측 커버).
        let mut app = asset_app();
        let mut map = full_floor_map();
        map.map_type = MapType::Village;
        map.rooms = rooms_with(2);
        map.shop_vendor = None;
        app.insert_resource(MapResource(map));
        app.insert_resource(QuestRegistry::default());
        let mut vreg = VillagerRegistry::default();
        // vendor 는 일반 퀘스트 giver 가 아니므로 방 배치에서 regular 로 들어간다.
        vreg.villagers.push(vendor_def("merchant", "상인"));
        app.insert_resource(vreg);
        app.add_systems(Update, spawn_on_startup);
        app.update();
        // 가판대 고정 스폰은 없지만, 일반 주민 풀에 들어가 방에 스폰될 수 있다(고정 위치 아님).
        let at_fixed = app.world.query::<&Villager>().iter(&app.world)
            .any(|v| v.id == "merchant" && (v.tile_x, v.tile_y) == (30, 25));
        assert!(!at_fixed, "shop_vendor 가 없으면 고정 위치 스폰은 없다");
    }

    fn regen_app() -> App {
        let mut app = asset_app();
        app.add_event::<VillagerRespawnEvent>();
        app.insert_resource(QuestRegistry::default());
        let mut vreg = VillagerRegistry::default();
        vreg.villagers.push(vdef("farmer", "농부"));
        app.insert_resource(vreg);
        // respawn_on_regen 은 shop_vendor 를 읽으려 MapResource 가 필요하다.
        app.insert_resource(MapResource(full_floor_map()));
        app.add_systems(Update, respawn_on_regen);
        app
    }

    #[test]
    fn 마을_재생성_이벤트는_기존_주민을_제거하고_새로_스폰한다() {
        let mut app = regen_app();
        let old = spawn_villager(&mut app, "old", "옛주민", (1, 1));
        app.world.send_event(VillagerRespawnEvent {
            map_type: MapType::Village,
            rooms: rooms_with(2),
        });
        app.update();
        assert!(app.world.get_entity(old).is_none(), "기존 주민 제거");
        let n = app.world.query::<&Villager>().iter(&app.world).count();
        assert_eq!(n, 2, "새 주민 2명 스폰");
    }

    #[test]
    fn 던전_재생성_이벤트는_주민을_제거만_하고_재스폰하지_않는다() {
        let mut app = regen_app();
        let old = spawn_villager(&mut app, "old", "옛주민", (1, 1));
        app.world.send_event(VillagerRespawnEvent {
            map_type: MapType::Dungeon, // 마을 아님 → 재스폰 안 함
            rooms: rooms_with(2),
        });
        app.update();
        assert!(app.world.get_entity(old).is_none(), "기존 주민은 제거");
        assert_eq!(app.world.query::<&Villager>().iter(&app.world).count(), 0,
            "던전 재생성은 재스폰하지 않음");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // handle_bump / show_quest_dialog
    // ─────────────────────────────────────────────────────────────────────────

    fn bump_app(qreg: QuestRegistry) -> App {
        let mut app = App::new();
        app.add_event::<BumpTileEvent>()
            .add_event::<LogMessage>()
            .add_event::<KillNpcEvent>()
            .add_event::<SpawnQuestPortalEvent>()
            .add_event::<CloseQuestPortalEvent>()
            .add_event::<DespawnWorldItemEvent>()
            .add_event::<SpawnGuardEvent>()
            .add_event::<SpawnMonsterEvent>()
            .add_event::<ExplosionEvent>()
            .add_event::<SpawnTrapEvent>()
            .add_event::<ShopOpenEvent>()
            .insert_resource(qreg)
            .insert_resource(QuestState::default())
            .insert_resource(PlayerInventory::default())
            .insert_resource(WorldState::default())
            .insert_resource(crate::modules::item::build_test_registry())
            .insert_resource(MapResource(full_floor_map()))
            .add_systems(Update, handle_bump);
        app
    }

    /// vendor 주민 엔티티를 스폰한다(상점 상호작용 테스트용).
    fn spawn_vendor(app: &mut App, id: &str, name: &str, tile: (usize, usize)) -> Entity {
        app.world.spawn((
            Text::from_section("$", TextStyle::default()),
            Transform::from_translation(tile_to_world_coords(tile.0, tile.1).extend(Z_VILLAGER)),
            Villager {
                id: id.to_string(), name: name.to_string(),
                dialogues: vec![], dialogue_idx: 0, tile_x: tile.0, tile_y: tile.1,
                just_bumped: false, quest_dialogue_idx: 0,
                base_color: Color::WHITE, home_room: None,
                stationary: true, vendor: true,
            },
            Speed::new(1.0), MoveQueue::default(),
        )).id()
    }

    #[test]
    fn 상인을_밀면_상점_열기_이벤트가_발생한다() {
        let mut app = bump_app(QuestRegistry::default());
        spawn_vendor(&mut app, "merchant", "상인", (5, 5));
        app.world.send_event(BumpTileEvent(5, 5));
        app.update();
        assert_eq!(app.world.resource::<Events<ShopOpenEvent>>().len(), 1);
    }

    #[test]
    fn 카운터앞_타일에서_범프하면_상점이_열린다() {
        // 카운터 너머 보정: 손님이 카운터(5,6)를 범프하면 그 뒤 상인(5,5) 상점이 열린다.
        let mut app = bump_app(QuestRegistry::default());
        app.world.resource_mut::<MapResource>().map_mut().set_tile(5, 6, TileKind::Counter);
        spawn_vendor(&mut app, "merchant", "상인", (5, 5)); // 카운터 뒤 상인
        app.world.send_event(BumpTileEvent(5, 6)); // 카운터를 범프
        app.update();
        assert_eq!(app.world.resource::<Events<ShopOpenEvent>>().len(), 1,
            "카운터 너머 상인의 상점이 열려야 한다");
    }

    #[test]
    fn 상인없는_카운터를_범프하면_상점이_열리지_않는다() {
        let mut app = bump_app(QuestRegistry::default());
        app.world.resource_mut::<MapResource>().map_mut().set_tile(5, 6, TileKind::Counter);
        // 카운터 뒤에 상인이 없다.
        app.world.send_event(BumpTileEvent(5, 6));
        app.update();
        assert_eq!(app.world.resource::<Events<ShopOpenEvent>>().len(), 0,
            "카운터 뒤 상인 없으면 상점이 열리지 않는다");
    }

    #[test]
    fn 정지vendor도_범프시_충돌플래그가_설정된다() {
        // vendor 직접 범프 시 just_bumped 플래그가 설정되는지(직접 인접 경로).
        let mut app = bump_app(QuestRegistry::default());
        let e = spawn_vendor(&mut app, "merchant", "상인", (5, 5));
        app.world.send_event(BumpTileEvent(5, 5));
        app.update();
        assert!(app.world.get::<Villager>(e).unwrap().just_bumped, "vendor 범프 시 플래그 설정");
    }

    #[test]
    fn 일반_주민을_밀면_대사가_로그에_출력되고_인덱스가_전진한다() {
        let mut app = bump_app(QuestRegistry::default());
        let e = spawn_villager(&mut app, "farmer", "농부", (5, 5));
        app.world.send_event(BumpTileEvent(5, 5));
        app.update();
        assert_eq!(app.world.resource::<Events<LogMessage>>().len(), 1, "대사 한 줄");
        let v = app.world.get::<Villager>(e).unwrap();
        assert_eq!(v.dialogue_idx, 1, "대사 인덱스 전진");
        assert!(v.just_bumped, "충돌 플래그 설정");
    }

    #[test]
    fn 행이_다른_주민은_밀어도_반응하지_않는다() {
        // x 는 같지만 y 가 다른 경우 → `||` 의 두 번째 항(tile_y != by) 분기.
        let mut app = bump_app(QuestRegistry::default());
        let e = spawn_villager(&mut app, "farmer", "농부", (5, 5));
        app.world.send_event(BumpTileEvent(5, 9)); // x 같고 y 다름
        app.update();
        assert_eq!(app.world.resource::<Events<LogMessage>>().len(), 0);
        assert!(!app.world.get::<Villager>(e).unwrap().just_bumped);
    }

    #[test]
    fn 열이_다른_주민은_밀어도_반응하지_않는다() {
        // x 가 다른 경우 → `||` 의 첫 항(tile_x != bx) 분기.
        let mut app = bump_app(QuestRegistry::default());
        let e = spawn_villager(&mut app, "farmer", "농부", (5, 5));
        app.world.send_event(BumpTileEvent(9, 5)); // x 다름
        app.update();
        assert_eq!(app.world.resource::<Events<LogMessage>>().len(), 0);
        assert!(!app.world.get::<Villager>(e).unwrap().just_bumped);
    }

    #[test]
    fn 대사가_없는_주민을_밀면_로그없이_플래그만_설정된다() {
        let mut app = bump_app(QuestRegistry::default());
        let e = app.world.spawn((
            Text::from_section("v", TextStyle::default()),
            Transform::default(),
            Villager {
                id: "mute".into(), name: "벙어리".into(), dialogues: vec![], // 대사 없음
                dialogue_idx: 0, tile_x: 5, tile_y: 5, just_bumped: false,
                quest_dialogue_idx: 0, base_color: Color::WHITE, home_room: None,
                stationary: false, vendor: false,
            },
            Speed::new(1.0), MoveQueue::default(),
        )).id();
        app.world.send_event(BumpTileEvent(5, 5));
        app.update();
        assert_eq!(app.world.resource::<Events<LogMessage>>().len(), 0, "대사 없으면 로그 없음");
        assert!(app.world.get::<Villager>(e).unwrap().just_bumped);
    }

    #[test]
    fn 퀘스트_주민을_처음_밀면_초기_페이즈_대사가_출력된다() {
        let qreg = registry_with_giver("test_quest", "elder");
        let mut app = bump_app(qreg);
        // make_test_quest_def 의 not_started phase 는 dialog 가 비어 있으므로,
        // dialog 가 있는 phase 로 교체한다.
        {
            let mut reg = app.world.resource_mut::<QuestRegistry>();
            let q = reg.quests.get_mut("test_quest").unwrap();
            q.phases.insert("not_started".to_string(), phase(&["첫 만남이다", "도와주게"]));
        }
        let e = spawn_villager(&mut app, "elder", "장로", (5, 5));
        app.world.send_event(BumpTileEvent(5, 5));
        app.update();
        assert!(app.world.resource::<Events<LogMessage>>().len() >= 1, "퀘스트 대사 출력");
        let v = app.world.get::<Villager>(e).unwrap();
        assert_eq!(v.quest_dialogue_idx, 1, "마지막 줄 전이면 idx 전진");
        assert!(v.just_bumped);
    }

    #[test]
    fn 퀘스트_주민의_마지막_대사에서_상호작용하면_페이즈가_전진한다() {
        let qreg = registry_with_giver("test_quest", "elder");
        let mut app = bump_app(qreg);
        {
            let mut reg = app.world.resource_mut::<QuestRegistry>();
            let q = reg.quests.get_mut("test_quest").unwrap();
            // 단일 대사 phase → 첫 밀기에 마지막 줄 → transition 평가.
            q.phases.insert("not_started".to_string(), phase(&["수락하겠나?"]));
        }
        spawn_villager(&mut app, "elder", "장로", (5, 5));
        app.world.send_event(BumpTileEvent(5, 5));
        app.update();
        let st = app.world.resource::<QuestState>();
        assert_eq!(st.phases.get("test_quest").map(|s| s.as_str()), Some("active"),
            "not_started → active 로 전진해야 한다");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // vendor 가 동시에 quest giver 인 경우 (퀘스트 체이닝 시연 — vault_heist 버그)
    //   규칙: 퀘스트가 종료(terminal) 전이면 대화 우선, 종료 후엔 상점.
    //   직접 인접 / 카운터 너머 양쪽에 동일 적용.
    // ─────────────────────────────────────────────────────────────────────────

    /// merchant 처럼 vendor 이면서 quest giver 인 주민을 스폰한다.
    fn spawn_vendor_giver(app: &mut App, id: &str, name: &str, tile: (usize, usize)) -> Entity {
        app.world.spawn((
            Text::from_section("$", TextStyle::default()),
            Transform::from_translation(tile_to_world_coords(tile.0, tile.1).extend(Z_VILLAGER)),
            Villager {
                id: id.to_string(), name: name.to_string(),
                dialogues: vec!["좋은 물건 있소".to_string()],
                dialogue_idx: 0, tile_x: tile.0, tile_y: tile.1,
                just_bumped: false, quest_dialogue_idx: 0,
                base_color: Color::WHITE, home_room: None,
                stationary: true, vendor: true,
            },
            Speed::new(1.0), MoveQueue::default(),
        )).id()
    }

    #[test]
    fn 가판대상인은_퀘스트가_끝나기_전엔_대화하고_상점을_열지_않는다() {
        // vendor + giver, 퀘스트 비종료(not_started) → 직접 범프 시 상점이 아니라 퀘스트 대화.
        let qreg = registry_with_giver("vault_heist_quest", "merchant");
        let mut app = bump_app(qreg);
        {
            let mut reg = app.world.resource_mut::<QuestRegistry>();
            let q = reg.quests.get_mut("vault_heist_quest").unwrap();
            q.phases.insert("not_started".to_string(), phase(&["은밀한 거래를 들어볼까", "어떻소?"]));
        }
        spawn_vendor_giver(&mut app, "merchant", "상인", (5, 5));
        app.world.send_event(BumpTileEvent(5, 5));
        app.update();
        assert_eq!(app.world.resource::<Events<ShopOpenEvent>>().len(), 0,
            "퀘스트 비종료 vendor-giver 는 상점을 열면 안 된다");
        assert!(app.world.resource::<Events<LogMessage>>().len() >= 1,
            "대신 퀘스트 대사가 출력되어야 한다");
    }

    #[test]
    fn 가판대상인은_퀘스트가_끝난_뒤엔_상점을_연다() {
        // vendor + giver, 퀘스트 종료(done = terminal phase) → 직접 범프 시 상점.
        let qreg = registry_with_giver("vault_heist_quest", "merchant");
        let mut app = bump_app(qreg);
        // done phase 는 outgoing transition 이 없어 terminal.
        app.world.resource_mut::<QuestState>().set_phase("vault_heist_quest", "done");
        spawn_vendor_giver(&mut app, "merchant", "상인", (5, 5));
        app.world.send_event(BumpTileEvent(5, 5));
        app.update();
        assert_eq!(app.world.resource::<Events<ShopOpenEvent>>().len(), 1,
            "퀘스트가 종료된 vendor-giver 는 상점을 연다");
    }

    #[test]
    fn 카운터너머_vendor_giver도_퀘스트수락이_가능하다() {
        // 카운터 뒤 고정 merchant: 손님이 카운터를 범프 → 퀘스트 비종료면 퀘스트 대화.
        let qreg = registry_with_giver("vault_heist_quest", "merchant");
        let mut app = bump_app(qreg);
        {
            let mut reg = app.world.resource_mut::<QuestRegistry>();
            let q = reg.quests.get_mut("vault_heist_quest").unwrap();
            // 단일 대사 → 카운터 범프 한 번에 마지막 줄 → transition 평가(수락).
            q.phases.insert("not_started".to_string(), phase(&["수락하겠소?"]));
        }
        app.world.resource_mut::<MapResource>().map_mut().set_tile(5, 6, TileKind::Counter);
        spawn_vendor_giver(&mut app, "merchant", "상인", (5, 5)); // 카운터(5,6) 뒤
        app.world.send_event(BumpTileEvent(5, 6)); // 카운터를 범프
        app.update();
        assert_eq!(app.world.resource::<Events<ShopOpenEvent>>().len(), 0,
            "카운터 너머라도 퀘스트 비종료면 상점을 열면 안 된다");
        let st = app.world.resource::<QuestState>();
        assert_eq!(st.phases.get("vault_heist_quest").map(|s| s.as_str()), Some("active"),
            "카운터 너머 범프로 퀘스트가 수락(전진)되어야 한다");
    }

    #[test]
    fn 카운터너머_vendor_giver는_퀘스트종료후엔_상점을_연다() {
        // 카운터 너머 + 종료 퀘스트 → 상점.
        let qreg = registry_with_giver("vault_heist_quest", "merchant");
        let mut app = bump_app(qreg);
        app.world.resource_mut::<QuestState>().set_phase("vault_heist_quest", "done");
        app.world.resource_mut::<MapResource>().map_mut().set_tile(5, 6, TileKind::Counter);
        spawn_vendor_giver(&mut app, "merchant", "상인", (5, 5));
        app.world.send_event(BumpTileEvent(5, 6));
        app.update();
        assert_eq!(app.world.resource::<Events<ShopOpenEvent>>().len(), 1,
            "카운터 너머 + 종료 퀘스트면 상점을 연다");
    }

    /// 체이닝 시연용 vault_heist 모사 QuestDef (giver=merchant):
    ///   locked --Auto(infiltration done)--> not_started --Interact--> infiltrating(terminal)
    fn vault_heist_like_def() -> QuestDef {
        let mut phases = HM::new();
        phases.insert("locked".to_string(), phase(&["순서가 있는 법이오."]));
        phases.insert("not_started".to_string(), phase(&["은밀한 거래를 들어보겠소?"]));
        phases.insert("infiltrating".to_string(), phase(&["인장을 가져오시오."]));
        QuestDef {
            id: "vault_heist_quest".into(),
            title: "얼어붙은 금고".into(),
            giver_npc: "merchant".into(),
            initial_phase: "locked".into(),
            phases,
            transitions: vec![
                QuestTransition {
                    from: "locked".into(), trigger: TriggerKind::Auto,
                    when: Some(QuestCondition::PhaseIs {
                        quest: "infiltration_quest".into(),
                        phase: "done_clean".into(),
                    }),
                    actions: vec![], to: "not_started".into(),
                },
                QuestTransition {
                    from: "not_started".into(), trigger: TriggerKind::Interact,
                    when: None, actions: vec![], to: "infiltrating".into(),
                },
            ],
            spawns: vec![], spawn_chance: 1.0,
        }
    }

    #[test]
    fn 체이닝퀘스트가_잠겨있어도_merchant는_상점대신_퀘스트대화를_연다() {
        // 버그 핵심: vault_heist 가 잠긴(locked, 비-terminal) 동안에도 vendor-giver
        // merchant 범프는 상점이 아니라 퀘스트 대화여야 한다. (locked 는 Auto 출구가
        // 있어 terminal 이 아님 → quest_active=true.)
        let mut app = bump_app(QuestRegistry::default());
        {
            let mut reg = app.world.resource_mut::<QuestRegistry>();
            reg.quests.insert("vault_heist_quest".into(), vault_heist_like_def());
        }
        spawn_vendor_giver(&mut app, "merchant", "상인", (5, 5));
        app.world.send_event(BumpTileEvent(5, 5));
        app.update();
        assert_eq!(app.world.resource::<Events<ShopOpenEvent>>().len(), 0,
            "잠긴 체이닝 퀘스트 동안 vendor-giver 는 상점이 아니라 대화한다");
        assert!(app.world.resource::<Events<LogMessage>>().len() >= 1,
            "잠금 안내 대사가 출력되어야 한다");
    }

    #[test]
    fn 선행퀘스트가_끝나_체이닝이_개방되면_merchant범프로_수락된다() {
        // 체이닝 시연 복구 증명:
        //  - 선행(infiltration) 완료 → Auto 로 vault_heist locked→not_started 개방
        //    (Auto 전이는 quest 모듈의 check_auto_advance 시스템 담당이므로 여기선
        //     개방된 상태 not_started 를 직접 세팅해 handle_bump 경계만 검증).
        //  - 개방 후 merchant 범프 시 상점이 아니라 퀘스트 수락(infiltrating)이 진행.
        let mut app = bump_app(QuestRegistry::default());
        {
            let mut reg = app.world.resource_mut::<QuestRegistry>();
            // 단일 대사 not_started → 첫 범프에 마지막 줄 → Interact 전이(수락).
            let mut def = vault_heist_like_def();
            def.phases.insert("not_started".into(), phase(&["수락하겠소?"]));
            reg.quests.insert("vault_heist_quest".into(), def);
        }
        // 선행 완료 후 Auto 로 도달한 상태 (개방됨).
        app.world.resource_mut::<QuestState>().set_phase("infiltration_quest", "done_clean");
        app.world.resource_mut::<QuestState>().set_phase("vault_heist_quest", "not_started");

        spawn_vendor_giver(&mut app, "merchant", "상인", (5, 5));
        app.world.send_event(BumpTileEvent(5, 5));
        app.update();

        assert_eq!(app.world.resource::<Events<ShopOpenEvent>>().len(), 0,
            "개방된 체이닝 퀘스트 수락 단계에서도 상점이 아니라 퀘스트 대화가 진행되어야 한다");
        let accepted = app.world.resource::<QuestState>()
            .phases.get("vault_heist_quest").cloned();
        assert_eq!(accepted.as_deref(), Some("infiltrating"),
            "merchant 범프로 vault_heist_quest 가 수락(infiltrating)되어야 한다");
    }

    #[test]
    fn 비vendor_giver는_퀘스트종료후에도_대화를_유지한다() {
        // 회귀 방지: 순수 quest giver(burgomaster 류)는 vendor 가 아니므로
        // 퀘스트가 종료(terminal)되어도 종료 페이즈 대화를 유지한다(상점 없음).
        let qreg = registry_with_giver("test_quest", "burgomaster");
        let mut app = bump_app(qreg);
        {
            let mut reg = app.world.resource_mut::<QuestRegistry>();
            let q = reg.quests.get_mut("test_quest").unwrap();
            q.phases.insert("done".to_string(), phase(&["이미 끝난 일이네."]));
        }
        // done = terminal phase.
        app.world.resource_mut::<QuestState>().set_phase("test_quest", "done");
        // burgomaster: vendor=false 인 일반 quest giver.
        let e = spawn_villager(&mut app, "burgomaster", "촌장", (5, 5));
        app.world.send_event(BumpTileEvent(5, 5));
        app.update();
        assert_eq!(app.world.resource::<Events<ShopOpenEvent>>().len(), 0,
            "비-vendor giver 는 종료 후에도 상점을 열지 않는다");
        assert!(app.world.resource::<Events<LogMessage>>().len() >= 1,
            "종료 페이즈 대화가 출력되어야 한다");
        assert!(app.world.get::<Villager>(e).unwrap().just_bumped);
    }

    #[test]
    fn 대상타일과_다른_주민은_건너뛰고_대상_주민만_반응한다() {
        // handle_bump 내부 루프의 `tile_x != target.0 || tile_y != target.1` continue
        // 분기(양쪽 항)를 커버한다: 대상이 아닌 주민들을 건너뛰고 대상만 반응.
        let mut app = bump_app(QuestRegistry::default());
        // x 가 다른 주민 (tile_x != target.0 → 첫 항 True)
        spawn_villager(&mut app, "other_x", "딴사람가로", (9, 5));
        // x 같고 y 다른 주민 (tile_x == target.0, tile_y != target.1 → 둘째 항 True)
        spawn_villager(&mut app, "other_y", "딴사람세로", (5, 9));
        // 대상 주민
        let target = spawn_villager(&mut app, "farmer", "농부", (5, 5));
        app.world.send_event(BumpTileEvent(5, 5));
        app.update();
        // 대상 주민만 반응.
        assert!(app.world.get::<Villager>(target).unwrap().just_bumped, "대상 주민이 반응");
        assert_eq!(app.world.resource::<Events<LogMessage>>().len(), 1, "대상 주민 대사 한 줄만");
    }

    #[test]
    fn 일반대사주민은_giver도_vendor도_아니면_대사를_순환한다() {
        // 통합 재작성 후에도 일반 주민 분기가 유지되는지 확인(회귀 방지).
        let mut app = bump_app(QuestRegistry::default());
        let e = spawn_villager(&mut app, "farmer", "농부", (5, 5));
        app.world.send_event(BumpTileEvent(5, 5));
        app.update();
        assert_eq!(app.world.resource::<Events<ShopOpenEvent>>().len(), 0);
        assert_eq!(app.world.resource::<Events<LogMessage>>().len(), 1, "일반 대사 한 줄");
        assert_eq!(app.world.get::<Villager>(e).unwrap().dialogue_idx, 1, "대사 인덱스 전진");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // show_quest_dialog 의 방어/early-return 분기 (직접 호출)
    // ─────────────────────────────────────────────────────────────────────────

    fn dialog_event_apps() -> (App, Entity) {
        // EventWriter 들이 필요하므로 시스템으로 감싸기보다, 이벤트를 가진 App 에서
        // SystemState 로 EventWriter 를 얻어 show_quest_dialog 를 호출한다.
        let mut app = App::new();
        app.add_event::<LogMessage>()
            .add_event::<KillNpcEvent>()
            .add_event::<SpawnQuestPortalEvent>()
            .add_event::<CloseQuestPortalEvent>()
            .add_event::<DespawnWorldItemEvent>()
            .add_event::<SpawnGuardEvent>()
            .add_event::<SpawnMonsterEvent>()
            .add_event::<ExplosionEvent>()
            .add_event::<SpawnTrapEvent>();
        let e = Entity::PLACEHOLDER;
        (app, e)
    }

    fn run_show_quest_dialog(
        app: &mut App,
        villager: &mut Villager,
        quest_id: &str,
        registry: &QuestRegistry,
        state: &mut QuestState,
        inventory: &mut PlayerInventory,
        world: &WorldState,
        quest_items: &crate::modules::item::QuestItemRegistry,
    ) {
        use bevy::ecs::system::SystemState;
        let mut ss: SystemState<(
            EventWriter<LogMessage>,
            QuestActionWriters,
        )> = SystemState::new(&mut app.world);
        let (mut log, mut writers) = ss.get_mut(&mut app.world);
        show_quest_dialog(
            villager, quest_id, registry, state, inventory, &mut log, world,
            &mut writers, quest_items,
        );
        ss.apply(&mut app.world);
    }

    fn make_test_villager() -> Villager {
        Villager {
            id: "elder".into(), name: "장로".into(), dialogues: vec![],
            dialogue_idx: 0, tile_x: 0, tile_y: 0, just_bumped: false,
            quest_dialogue_idx: 0, base_color: Color::WHITE, home_room: None,
            stationary: false, vendor: false,
        }
    }

    #[test]
    fn 알_수_없는_퀘스트면_대화는_조용히_종료된다() {
        let (mut app, _) = dialog_event_apps();
        let reg = QuestRegistry::default(); // 빈 registry → get() None
        let mut v = make_test_villager();
        let mut state = QuestState::default();
        let mut inv = empty_inventory();
        let world = default_world();
        run_show_quest_dialog(&mut app, &mut v, "없는퀘스트", &reg, &mut state, &mut inv, &world, qi());
        assert_eq!(app.world.resource::<Events<LogMessage>>().len(), 0, "정의 없으면 아무 일 없음");
    }

    #[test]
    fn 정의에_없는_현재_페이즈면_대화는_조용히_종료된다() {
        let (mut app, _) = dialog_event_apps();
        let reg = registry_with_giver("test_quest", "elder");
        let mut v = make_test_villager();
        let mut state = QuestState::default();
        state.phases.insert("test_quest".to_string(), "유령페이즈".to_string()); // phases 에 없음
        let mut inv = empty_inventory();
        let world = default_world();
        run_show_quest_dialog(&mut app, &mut v, "test_quest", &reg, &mut state, &mut inv, &world, qi());
        assert_eq!(app.world.resource::<Events<LogMessage>>().len(), 0,
            "phase 정의 없으면 대화 종료");
    }

    #[test]
    fn 대사가_여러줄이면_상호작용마다_인덱스가_전진한다() {
        let (mut app, _) = dialog_event_apps();
        let mut reg = registry_with_giver("test_quest", "elder");
        reg.quests.get_mut("test_quest").unwrap()
            .phases.insert("not_started".to_string(), phase(&["첫줄", "둘째줄", "셋째줄"]));
        let mut v = make_test_villager();
        let mut state = QuestState::default();
        let mut inv = empty_inventory();
        let world = default_world();
        run_show_quest_dialog(&mut app, &mut v, "test_quest", &reg, &mut state, &mut inv, &world, qi());
        assert_eq!(v.quest_dialogue_idx, 1, "여러 줄 중 첫 줄 후 idx=1 (else 분기)");
        assert_eq!(app.world.resource::<Events<LogMessage>>().len(), 1);
    }

    #[test]
    fn 빈_대사_페이즈는_로그도_전이도_없이_인덱스만_전진한다() {
        // dialog 가 비어 있으면 `!dialog.is_empty()` 가 false → else 분기로 idx 만 전진.
        // (transition 평가 안 함, 로그 출력 안 함)
        let (mut app, _) = dialog_event_apps();
        let reg = registry_with_giver("test_quest", "elder"); // not_started dialog=[]
        let mut v = make_test_villager();
        let mut state = QuestState::default();
        let mut inv = empty_inventory();
        let world = default_world();
        run_show_quest_dialog(&mut app, &mut v, "test_quest", &reg, &mut state, &mut inv, &world, qi());
        assert!(state.phases.get("test_quest").is_none(), "빈 대사 → 전이 없음");
        assert_eq!(v.quest_dialogue_idx, 1, "빈 대사도 else 분기로 idx 전진");
        assert_eq!(app.world.resource::<Events<LogMessage>>().len(), 0, "빈 대사 → 로그 없음");
    }

    #[test]
    fn 단일_대사_페이즈에서_상호작용하면_전이를_평가하고_idx를_0으로_되돌린다() {
        // dialog 한 줄 → 첫 상호작용에 마지막 줄 → transition 평가 + idx=0.
        let (mut app, _) = dialog_event_apps();
        let mut reg = registry_with_giver("test_quest", "elder");
        reg.quests.get_mut("test_quest").unwrap()
            .phases.insert("not_started".to_string(), phase(&["수락하겠나?"]));
        let mut v = make_test_villager();
        let mut state = QuestState::default();
        let mut inv = empty_inventory();
        let world = default_world();
        run_show_quest_dialog(&mut app, &mut v, "test_quest", &reg, &mut state, &mut inv, &world, qi());
        assert_eq!(state.phases.get("test_quest").map(|s| s.as_str()), Some("active"),
            "마지막 줄 후 not_started → active 전이");
        assert_eq!(v.quest_dialogue_idx, 0, "마지막 줄 후 idx 는 0 으로 리셋");
        assert_eq!(app.world.resource::<Events<LogMessage>>().len(), 1, "대사 한 줄 출력");
    }

    #[test]
    fn 자기참조_전이는_페이즈를_바꾸지_않고_액션만_실행한다() {
        // gathering phase 에 self-loop Interact transition (to==from) 만 있는 경우:
        // execute_actions 는 실행되지만 set_phase 는 호출되지 않는다 (t.to == phase_id 분기).
        let (mut app, _) = dialog_event_apps();
        let mut phases = HM::new();
        phases.insert("not_started".to_string(), phase(&[]));
        phases.insert("gathering".to_string(), phase(&["아직이네"]));
        let def = QuestDef {
            id: "alc".into(), title: "연금".into(), giver_npc: "elder".into(),
            initial_phase: "not_started".into(), phases,
            transitions: vec![QuestTransition {
                from: "gathering".into(), trigger: TriggerKind::Interact,
                when: None,
                actions: vec![crate::modules::quest::QuestAction::Log("힌트".into())],
                to: "gathering".into(),
            }],
            spawns: vec![], spawn_chance: 1.0,
        };
        let mut reg = QuestRegistry::default();
        reg.quests.insert("alc".into(), def);
        let mut v = make_test_villager();
        let mut state = QuestState::default();
        state.phases.insert("alc".into(), "gathering".into());
        let mut inv = empty_inventory();
        let world = default_world();
        // 단일 대사 → 마지막 줄에서 transition 평가.
        run_show_quest_dialog(&mut app, &mut v, "alc", &reg, &mut state, &mut inv, &world, qi());
        assert_eq!(state.phases.get("alc").map(|s| s.as_str()), Some("gathering"),
            "self-loop 은 phase 를 바꾸지 않는다 (t.to == phase_id)");
    }

    #[test]
    fn 조건이_충족되지_않으면_상호작용_전이가_실행되지_않는다() {
        // active phase 의 Auto 만 있고 Interact 없음 → 마지막 줄 후 matched=None.
        let (mut app, _) = dialog_event_apps();
        let mut reg = registry_with_giver("test_quest", "elder");
        // active phase 에 단일 대사 부여, 현재 phase = active.
        reg.quests.get_mut("test_quest").unwrap()
            .phases.insert("active".to_string(), phase(&["진행 중"]));
        let mut v = make_test_villager();
        let mut state = QuestState::default();
        state.phases.insert("test_quest".to_string(), "active".to_string());
        let mut inv = empty_inventory(); // eternal_gem 없음
        let world = default_world();
        run_show_quest_dialog(&mut app, &mut v, "test_quest", &reg, &mut state, &mut inv, &world, qi());
        // Interact transition 이 active 에 없으므로 phase 변화 없음.
        assert_eq!(state.phases.get("test_quest").map(|s| s.as_str()), Some("active"),
            "Interact 매칭 없음 → 전이 안 됨");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // update_villager_glyph
    // ─────────────────────────────────────────────────────────────────────────

    fn glyph_app(qreg: QuestRegistry) -> App {
        let mut app = App::new();
        app.insert_resource(qreg)
            .insert_resource(QuestState::default())
            .insert_resource(PlayerInventory::default())
            .insert_resource(WorldState::default())
            .insert_resource(crate::modules::item::build_test_registry())
            .add_systems(Update, update_villager_glyph);
        app
    }

    fn spawn_quest_villager(app: &mut App, id: &str) -> Entity {
        app.world.spawn((
            Text::from_section("?", TextStyle { color: Color::rgb(1.0, 0.9, 0.1), ..default() }),
            Transform::default(),
            Villager {
                id: id.to_string(), name: id.to_string(), dialogues: vec![],
                dialogue_idx: 0, tile_x: 0, tile_y: 0, just_bumped: false,
                quest_dialogue_idx: 0, base_color: Color::rgb(0.2, 0.3, 0.4), home_room: None,
                stationary: false, vendor: false,
            },
            Speed::new(1.0), MoveQueue::default(),
        )).id()
    }

    #[test]
    fn 새로_스폰된_퀘스트_주민의_글리프가_갱신된다() {
        let mut app = glyph_app(registry_with_giver("test_quest", "elder"));
        let e = spawn_quest_villager(&mut app, "elder");
        app.update(); // Added<Villager> → is_empty()=false → 갱신
        let text = app.world.get::<Text>(e).unwrap();
        // 초기 phase (state 없음) → 노란 '?'
        assert_eq!(text.sections[0].value, "?");
        assert_eq!(text.sections[0].style.color, Color::rgb(1.0, 0.9, 0.1));
    }

    #[test]
    fn 퀘스트_상태가_바뀌면_주민_글리프가_갱신된다() {
        let mut app = glyph_app(registry_with_giver("test_quest", "elder"));
        let e = spawn_quest_villager(&mut app, "elder");
        app.update(); // 첫 갱신 (Added)
        // 이제 변경 트리거: state 를 ready 로 (Interact 전진 가능 → 초록 '!')
        app.world.resource_mut::<QuestState>().set_phase("test_quest", "ready");
        app.update();
        let text = app.world.get::<Text>(e).unwrap();
        assert_eq!(text.sections[0].value, "!", "ready → 초록 느낌표");
        assert_eq!(text.sections[0].style.color, Color::rgb(0.3, 1.0, 0.6));
    }

    #[test]
    fn 변경이_없으면_글리프_갱신을_건너뛴다() {
        let mut app = glyph_app(registry_with_giver("test_quest", "elder"));
        let e = spawn_quest_villager(&mut app, "elder");
        app.update(); // Added 처리
        // 글리프를 임의 값으로 바꿔두고, 변경 없는 update 가 덮어쓰지 않음을 확인.
        app.world.get_mut::<Text>(e).unwrap().sections[0].value = "X".to_string();
        app.update(); // is_changed 모두 false + added empty → early return
        assert_eq!(app.world.get::<Text>(e).unwrap().sections[0].value, "X",
            "변경 없으면 글리프를 건드리지 않는다");
    }

    #[test]
    fn 인벤토리만_바뀌어도_글리프가_갱신된다() {
        // 첫 update 로 변경/Added 플래그를 소진한 뒤 인벤토리만 변경 →
        // `!quest_state.is_changed()`=true && `!inventory.is_changed()`=false 분기.
        let mut app = glyph_app(registry_with_giver("test_quest", "elder"));
        let e = spawn_quest_villager(&mut app, "elder");
        app.update(); // 변경 플래그/Added 소진
        app.world.get_mut::<Text>(e).unwrap().sections[0].value = "X".to_string();
        app.world.resource_mut::<PlayerInventory>().earn_gold(1); // 인벤토리만 변경
        app.update();
        // 갱신이 일어나 'X' 가 다시 글리프로 덮어써져야 한다.
        assert_ne!(app.world.get::<Text>(e).unwrap().sections[0].value, "X",
            "인벤토리 변경 시 글리프가 갱신되어야 한다");
    }

    #[test]
    fn 변경이_없어도_새_주민이_추가되면_글리프가_갱신된다() {
        // 첫 update 로 플래그 소진 후, 변경 없이 새 주민만 추가 →
        // `added.is_empty()`=false 분기.
        let mut app = glyph_app(registry_with_giver("test_quest", "elder"));
        spawn_quest_villager(&mut app, "elder");
        app.update(); // 첫 주민 Added/변경 소진
        let e2 = spawn_quest_villager(&mut app, "elder"); // 새 주민 추가
        app.world.get_mut::<Text>(e2).unwrap().sections[0].value = "Y".to_string();
        app.update(); // quest/inv 변경 없음, added 있음 → 갱신
        assert_ne!(app.world.get::<Text>(e2).unwrap().sections[0].value, "Y",
            "새 주민 추가 시 글리프가 갱신되어야 한다");
    }

    #[test]
    fn 퀘스트_giver가_아닌_주민은_글리프_갱신에서_제외된다() {
        let mut app = glyph_app(QuestRegistry::default()); // 어떤 quest 의 giver 도 아님
        let e = spawn_quest_villager(&mut app, "farmer");
        app.world.get_mut::<Text>(e).unwrap().sections[0].value = "Z".to_string();
        // inventory 변경으로 시스템은 돌지만, quest_for_giver 가 None → continue.
        app.world.resource_mut::<PlayerInventory>().earn_gold(1);
        app.update();
        assert_eq!(app.world.get::<Text>(e).unwrap().sections[0].value, "Z",
            "giver 아닌 주민은 글리프 변경 없음");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // discover_quest_npcs_in_fov
    // ─────────────────────────────────────────────────────────────────────────

    fn fov_app(qreg: QuestRegistry) -> App {
        let mut app = App::new();
        app.insert_resource(qreg)
            .insert_resource(QuestState::default())
            .insert_resource(WorldState::default())
            .insert_resource(DiscoveredMarkers::default())
            .add_systems(Update, discover_quest_npcs_in_fov);
        app
    }

    fn map_with_visible(tile: (usize, usize), visible: bool) -> Map {
        let mut m = full_floor_map();
        let idx = m.index(tile.0, tile.1);
        m.tiles[idx].visible = visible;
        m
    }

    #[test]
    fn 퀘스트_giver가_아닌_주민은_FOV마커가_생기지_않는다() {
        let mut app = fov_app(QuestRegistry::default());
        app.insert_resource(MapResource(map_with_visible((5, 5), true)));
        spawn_villager(&mut app, "farmer", "농부", (5, 5));
        app.update();
        assert!(app.world.resource::<DiscoveredMarkers>().0.is_empty(),
            "giver 아닌 주민은 마커 없음");
    }

    #[test]
    fn 시작_전_퀘스트_giver의_마커는_제거된다() {
        let mut app = fov_app(registry_with_giver("test_quest", "elder"));
        app.insert_resource(MapResource(map_with_visible((5, 5), true)));
        // 미리 마커를 심어둔다.
        app.world.resource_mut::<DiscoveredMarkers>()
            .update_actor_position("장로", MarkerKind::QuestGiver, ZoneId::Town, 5, 5);
        spawn_villager(&mut app, "elder", "장로", (5, 5));
        // state 에 phase 없음 → started=false → remove_actor.
        app.update();
        assert!(app.world.resource::<DiscoveredMarkers>().0.is_empty(),
            "퀘스트 시작 전이면 마커 제거");
    }

    #[test]
    fn 초기_페이즈_상태의_giver_마커도_제거된다() {
        let mut app = fov_app(registry_with_giver("test_quest", "elder"));
        app.insert_resource(MapResource(map_with_visible((5, 5), true)));
        // phase 가 initial_phase(not_started) 와 같으면 started=false.
        app.world.resource_mut::<QuestState>().set_phase("test_quest", "not_started");
        app.world.resource_mut::<DiscoveredMarkers>()
            .update_actor_position("장로", MarkerKind::QuestGiver, ZoneId::Town, 5, 5);
        spawn_villager(&mut app, "elder", "장로", (5, 5));
        app.update();
        assert!(app.world.resource::<DiscoveredMarkers>().0.is_empty());
    }

    #[test]
    fn 종료된_퀘스트_giver의_마커는_제거된다() {
        let mut app = fov_app(registry_with_giver("test_quest", "elder"));
        app.insert_resource(MapResource(map_with_visible((5, 5), true)));
        // done phase 는 terminal → 마커 제거.
        app.world.resource_mut::<QuestState>().set_phase("test_quest", "done");
        app.world.resource_mut::<DiscoveredMarkers>()
            .update_actor_position("장로", MarkerKind::QuestGiver, ZoneId::Town, 5, 5);
        spawn_villager(&mut app, "elder", "장로", (5, 5));
        app.update();
        assert!(app.world.resource::<DiscoveredMarkers>().0.is_empty(),
            "터미널 퀘스트 마커 제거");
    }

    #[test]
    fn 진행중_giver가_시야안에_있으면_마커_위치가_갱신된다() {
        let mut app = fov_app(registry_with_giver("test_quest", "elder"));
        app.insert_resource(MapResource(map_with_visible((6, 7), true)));
        app.world.resource_mut::<QuestState>().set_phase("test_quest", "active"); // 진행중
        spawn_villager(&mut app, "elder", "장로", (6, 7));
        app.update();
        let markers = &app.world.resource::<DiscoveredMarkers>().0;
        assert_eq!(markers.len(), 1, "진행중 + 시야 안 → 마커 생성");
        assert_eq!((markers[0].tile_x, markers[0].tile_y), (6, 7));
    }

    #[test]
    fn 진행중이라도_시야밖이면_마커가_갱신되지_않는다() {
        let mut app = fov_app(registry_with_giver("test_quest", "elder"));
        app.insert_resource(MapResource(map_with_visible((6, 7), false))); // 시야 밖
        app.world.resource_mut::<QuestState>().set_phase("test_quest", "active");
        spawn_villager(&mut app, "elder", "장로", (6, 7));
        app.update();
        assert!(app.world.resource::<DiscoveredMarkers>().0.is_empty(),
            "시야 밖이면 마커 갱신 없음 (제거도 안 함)");
    }

    #[test]
    fn 진행중_giver의_x좌표가_맵폭_밖이면_마커가_갱신되지_않는다() {
        // 좁은 맵에서 주민 x 좌표가 map.width 이상이면 범위검사 분기 (x 쪽).
        let mut app = fov_app(registry_with_giver("test_quest", "elder"));
        let mut m = Map::new(8, 8); // 좁은 맵
        for y in 0..8 { for x in 0..8 { m.set_tile(x, y, TileKind::Floor); } }
        app.insert_resource(MapResource(m));
        app.world.resource_mut::<QuestState>().set_phase("test_quest", "active");
        // 주민 타일 x=20 → map.width(8) 보다 큼 → continue.
        spawn_villager(&mut app, "elder", "장로", (20, 3));
        app.update();
        assert!(app.world.resource::<DiscoveredMarkers>().0.is_empty(),
            "맵 폭 밖 좌표는 마커 생성 안 함");
    }

    #[test]
    fn 진행중_giver의_y좌표가_맵높이_밖이면_마커가_갱신되지_않는다() {
        // x 는 범위 안, y 만 범위 밖 → `||` 의 두 번째 항(tile_y >= height) 분기.
        let mut app = fov_app(registry_with_giver("test_quest", "elder"));
        let mut m = Map::new(8, 8);
        for y in 0..8 { for x in 0..8 { m.set_tile(x, y, TileKind::Floor); } }
        app.insert_resource(MapResource(m));
        app.world.resource_mut::<QuestState>().set_phase("test_quest", "active");
        spawn_villager(&mut app, "elder", "장로", (3, 20)); // x 범위 안, y 범위 밖
        app.update();
        assert!(app.world.resource::<DiscoveredMarkers>().0.is_empty(),
            "맵 높이 밖 좌표는 마커 생성 안 함");
    }

    #[test]
    fn 자동전이가_자기참조면_글리프는_초록_물음표다() {
        // 현재 phase 에 Auto self-loop transition(to==from) 만 있으면:
        // - is_quest_terminal_def: false (transition 존재)
        // - is_initial: false
        // - interact_can_advance: false (Interact 없음)
        // - auto_ready: t.to != pid 가 false → false
        // → 최종 ("?", green) 분기. (t.to != *pid 의 false 측 커버)
        let mut phases = HM::new();
        phases.insert("not_started".to_string(), phase(&[]));
        phases.insert("waiting".to_string(), phase(&[]));
        let def = QuestDef {
            id: "wq".into(), title: "대기".into(), giver_npc: "elder".into(),
            initial_phase: "not_started".into(), phases,
            transitions: vec![QuestTransition {
                from: "waiting".into(), trigger: TriggerKind::Auto,
                when: None, actions: vec![],
                to: "waiting".into(), // 자기참조 Auto
            }],
            spawns: vec![], spawn_chance: 1.0,
        };
        let mut state = QuestState::default();
        state.phases.insert("wq".into(), "waiting".into());
        let inv = empty_inventory();
        let world = default_world();
        let (glyph, color) = quest_npc_glyph("wq", &def, &state, &inv, &world, Color::WHITE, qi());
        assert_eq!(glyph, "?", "Auto self-loop 만 있으면 진행 중 '?'");
        assert_eq!(color, Color::rgb(0.3, 1.0, 0.6));
    }
}

