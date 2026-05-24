use bevy::prelude::*;
use rand::prelude::*;
use serde::Deserialize;
use std::collections::HashSet;
use crate::modules::{
    map::{
        draw_map, Map, TileKind, MapType, OccupiedTiles, Rect,
        tile_to_world_coords, world_to_tile_coords,
        MAP_HEIGHT, MAP_WIDTH, TILE_SIZE,
        MapSystemSet, VillagerRespawnEvent, PlayerActedEvent, BumpTileEvent,
    },
    player::{Player, MovingTo, MoveQueue, PlayerSystemSet, LERP_SPEED},
    ui::{LogMessage, minimap::{DiscoveredMarkers, MarkerKind}},
    quest::{QuestRegistry, QuestState, QuestSystemSet, KillNpcEvent, DespawnWorldItemEvent, execute_actions, QuestDef, eval_condition},
    item::{PlayerInventory},
    zone::{WorldState, SpawnQuestPortalEvent},
    combat::Speed,
};

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
}

/// 게임 시작 시 RON 에서 불러온 villager 정의 모음
#[derive(Resource, Default)]
pub struct VillagerRegistry {
    pub villagers: Vec<VillagerDef>,
}

/// villager 시스템의 Startup 단계 실행 순서
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum VillagerSystemSet {
    Load,
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
}

// 이번 턴에 주민이 이동해야 하는지 판단하고 플래그를 초기화한다
pub fn take_turn(villager: &mut Villager) -> bool {
    if villager.just_bumped {
        villager.just_bumped = false;
        return false;
    }
    true
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
    let path = "assets/villagers/villagers.ron";
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            error!("[치명적] villager 파일 {} 을 읽을 수 없습니다: {}", path, e);
            std::process::exit(1);
        }
    };

    let villagers: Vec<VillagerDef> = match ron::de::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            error!("[치명적] villager RON 파싱 실패: {}", e);
            std::process::exit(1);
        }
    };

    info!("villager 로드: {} 명", villagers.len());
    registry.villagers = villagers;
}

/// 모든 퀘스트의 giver_npc 가 villager registry 에 존재하는지 검증한다
fn validate_quest_villager_refs(
    quest_registry: Res<QuestRegistry>,
    villager_registry: Res<VillagerRegistry>,
) {
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
    if !errors.is_empty() {
        for msg in &errors {
            error!("[치명적] {}", msg);
        }
        std::process::exit(1);
    }
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
        do_spawn(&mut commands, &map.rooms.clone(), &asset_server, &quest_registry, &villager_registry);
    }
}

fn respawn_on_regen(
    mut commands: Commands,
    mut events: EventReader<VillagerRespawnEvent>,
    villager_query: Query<Entity, With<Villager>>,
    asset_server: Res<AssetServer>,
    quest_registry: Res<QuestRegistry>,
    villager_registry: Res<VillagerRegistry>,
) {
    for event in events.read() {
        for entity in villager_query.iter() {
            commands.entity(entity).despawn();
        }
        if event.map_type == MapType::Village {
            do_spawn(&mut commands, &event.rooms, &asset_server, &quest_registry, &villager_registry);
        }
    }
}

fn do_spawn(
    commands: &mut Commands,
    rooms: &[Rect],
    asset_server: &AssetServer,
    quest_registry: &QuestRegistry,
    villager_registry: &VillagerRegistry,
) {
    if rooms.is_empty() { return; }
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");

    // 퀘스트 NPC와 일반 NPC 를 분리: 퀘스트 NPC 는 활성 퀘스트인 것만 스폰.
    // VillagerDef.quest_id 를 두지 않고 quest_registry 에서 giver_npc 매칭으로 판정.
    let quest_npcs: Vec<&VillagerDef> = villager_registry.villagers.iter()
        .filter(|d| quest_registry.active_quest_for_giver(&d.id).is_some())
        .collect();
    let regular_npcs: Vec<&VillagerDef> = villager_registry.villagers.iter()
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
        let (cx, cy) = room.center();
        let coord = tile_to_world_coords(cx, cy);

        let dialogues: Vec<String> = data.dialogs.iter()
            .map(|s| format!("{}: {}", data.name, s))
            .collect();

        let base_color = Color::rgb(data.color[0], data.color[1], data.color[2]);
        let is_quest_npc = quest_registry.quest_for_giver(&data.id).is_some();
        // 퀘스트 NPC 는 항상 노란 '?' 로 시작 — 수락·진행 시 update_villager_glyph 가 갱신
        let (glyph, display_color) = if is_quest_npc {
            ("?", Color::rgb(1.0, 0.9, 0.1))
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
                home_room: if is_quest_npc { Some(*room) } else { None },
            },
            Speed::new(data.speed),
            MoveQueue::default(),
        ));
    }
}

// 플레이어가 주민 타일을 밀어 넣었을 때 대사를 출력한다
fn handle_bump(
    mut events: EventReader<BumpTileEvent>,
    mut villager_query: Query<&mut Villager>,
    mut log_writer: EventWriter<LogMessage>,
    registry: Res<QuestRegistry>,
    mut quest_state: ResMut<QuestState>,
    mut inventory: ResMut<PlayerInventory>,
    world_state: Res<WorldState>,
    mut kill_npc: EventWriter<KillNpcEvent>,
    mut open_portal: EventWriter<SpawnQuestPortalEvent>,
    mut close_portal: EventWriter<crate::modules::zone::CloseQuestPortalEvent>,
    mut despawn_item: EventWriter<DespawnWorldItemEvent>,
    mut shop_open: EventWriter<crate::modules::ui::shop::ShopOpenEvent>,
    quest_items: Res<crate::modules::item::QuestItemRegistry>,
) {
    for BumpTileEvent(bx, by) in events.read() {
        for mut villager in villager_query.iter_mut() {
            if villager.tile_x != *bx || villager.tile_y != *by { continue; }

            if villager.id == "merchant" {
                shop_open.send(crate::modules::ui::shop::ShopOpenEvent);
            } else if let Some(quest_id) = registry.quest_for_giver(&villager.id).map(|(qid, _)| qid.to_string()) {
                show_quest_dialog(&mut villager, &quest_id, &registry, &mut quest_state, &mut inventory, &mut log_writer, &world_state, &mut kill_npc, &mut open_portal, &mut close_portal, &mut despawn_item, &quest_items);
                // QuestGiver 마커는 discover_quest_npcs_in_fov 가 quest 상태에 따라 자동 갱신
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
    kill_npc: &mut EventWriter<KillNpcEvent>,
    open_portal: &mut EventWriter<SpawnQuestPortalEvent>,
    close_portal: &mut EventWriter<crate::modules::zone::CloseQuestPortalEvent>,
    despawn_item: &mut EventWriter<DespawnWorldItemEvent>,
    quest_items: &crate::modules::item::QuestItemRegistry,
) {
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
            execute_actions(&t.actions, quest_id, state, inventory, log, kill_npc, open_portal, close_portal, despawn_item, quest_items);
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
                && map.get_tile(nx, ny) == TileKind::Floor
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
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use std::sync::OnceLock;

    static TEST_QI: OnceLock<crate::modules::item::QuestItemRegistry> = OnceLock::new();
    fn qi() -> &'static crate::modules::item::QuestItemRegistry {
        TEST_QI.get_or_init(|| crate::modules::item::build_test_registry())
    }

    #[test]
    fn next_dialogue_advances() {
        assert_eq!(next_dialogue_idx(0, 3), 1);
        assert_eq!(next_dialogue_idx(1, 3), 2);
    }

    #[test]
    fn next_dialogue_wraps_at_end() {
        assert_eq!(next_dialogue_idx(2, 3), 0);
    }

    #[test]
    fn next_dialogue_single_stays_zero() {
        assert_eq!(next_dialogue_idx(0, 1), 0);
    }

    #[test]
    fn next_dialogue_zero_total_returns_zero() {
        assert_eq!(next_dialogue_idx(0, 0), 0);
    }

    #[test]
    fn pick_next_tile_surrounded_by_walls_stays_put() {
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
    fn pick_next_tile_returns_floor_neighbor() {
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
    fn pick_next_tile_never_moves_to_wall() {
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
    fn pick_next_tile_skips_occupied_neighbor() {
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
        }
    }

    #[test]
    fn take_turn_returns_false_and_resets_flag_when_bumped() {
        let mut v = make_villager(true);
        assert!(!take_turn(&mut v), "충돌 직후에는 이동하지 않아야 한다");
        assert!(!v.just_bumped, "플래그는 한 번만 소모된다");
    }

    #[test]
    fn take_turn_returns_true_when_not_bumped() {
        let mut v = make_villager(false);
        assert!(take_turn(&mut v), "충돌 없는 주민은 정상 이동해야 한다");
    }

    #[test]
    fn quest_villager_fields_default_correctly() {
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
        };
        assert_eq!(v.id, "elder");
        assert_eq!(v.quest_dialogue_idx, 0);
    }

    // villager_turn 에서 플레이어 타일(Transform 또는 MovingTo 목적지)을
    // occupied 에 추가해 overlap 을 방지하는 메커니즘 검증
    #[test]
    fn pick_next_tile_blocked_by_player_tile() {
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
    fn pick_next_tile_respects_home_rect() {
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
    fn world_fracture_giver_resolves_to_villager() {
        assert_giver_resolves("world_fracture", "old_man");
    }

    #[test]
    fn parry_quest_giver_resolves_to_villager() {
        assert_giver_resolves("parry_quest", "grace");
    }

    #[test]
    fn demonsword_quest_giver_resolves_to_villager() {
        assert_giver_resolves("demonsword_quest", "bastian");
    }

    #[test]
    fn discover_quest_npcs_marker_uses_quest_for_giver_lookup() {
        // 퀘스트 NPC만 마커. 새 모델: quest_registry 에서 giver_npc 매칭.
        // (test 환경에선 quest_registry 가 없으므로 villager id 기반으로 시뮬.)
        let regular_id = "farmer";  // villagers.ron 에서 어느 quest 의 giver 도 아님
        let quest_id = "ellen";     // herb_quest.ron 의 giver_npc
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
    fn villagers_ron_parses() {
        let defs = load_villager_defs();
        assert!(defs.len() >= 11, "11명 이상의 NPC 가 정의되어야 한다");
    }

    #[test]
    fn all_quest_giver_npcs_exist_in_villager_registry() {
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
    fn glyph_yellow_when_quest_not_in_state() {
        let def = make_test_quest_def();
        let state = QuestState::default();
        let inv = empty_inventory();
        let world = default_world();
        let (glyph, color) = quest_npc_glyph("test_quest", &def, &state, &inv, &world, Color::WHITE, qi());
        assert_eq!(glyph, "?");
        assert_eq!(color, Color::rgb(1.0, 0.9, 0.1), "퀘스트 있음 = 노란색");
    }

    #[test]
    fn glyph_yellow_when_at_initial_phase() {
        let def = make_test_quest_def();
        let state = make_state_at("not_started");
        let inv = empty_inventory();
        let world = default_world();
        let (glyph, color) = quest_npc_glyph("test_quest", &def, &state, &inv, &world, Color::WHITE, qi());
        assert_eq!(glyph, "?");
        assert_eq!(color, Color::rgb(1.0, 0.9, 0.1), "initial_phase = 노란 ?");
    }

    #[test]
    fn glyph_green_question_when_in_progress_no_item() {
        let def = make_test_quest_def();
        let state = make_state_at("active");
        let inv = empty_inventory(); // 아이템 없음 → auto_advance 조건 미충족
        let world = default_world();
        let (glyph, color) = quest_npc_glyph("test_quest", &def, &state, &inv, &world, Color::WHITE, qi());
        assert_eq!(glyph, "?");
        assert_eq!(color, Color::rgb(0.3, 1.0, 0.6), "조건 미충족 = 초록 ?");
    }

    #[test]
    fn glyph_green_exclamation_when_auto_advance_condition_met() {
        let def = make_test_quest_def();
        let state = make_state_at("active");
        // eternal_gem 보유 → auto_advance 조건 충족 → 다음 페이즈로 넘어갈 수 있다
        let mut inv = empty_inventory();
        inv.items.push(InventoryItem {
            kind: ItemKind::QuestItem(QuestItemKind("eternal_gem")),
        });
        let world = default_world();
        let (glyph, color) = quest_npc_glyph("test_quest", &def, &state, &inv, &world, Color::WHITE, qi());
        assert_eq!(glyph, "!");
        assert_eq!(color, Color::rgb(0.3, 1.0, 0.6), "auto_advance 조건 충족 = 초록 !");
    }

    #[test]
    fn glyph_green_exclamation_when_has_interact_transition() {
        let def = make_test_quest_def();
        let state = make_state_at("ready");
        let inv = empty_inventory();
        let world = default_world();
        let (glyph, color) = quest_npc_glyph("test_quest", &def, &state, &inv, &world, Color::WHITE, qi());
        assert_eq!(glyph, "!");
        assert_eq!(color, Color::rgb(0.3, 1.0, 0.6), "Interact transition 있음 = 초록 !");
    }

    #[test]
    fn glyph_v_when_terminal() {
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
    fn interact_can_advance_true_for_interact_transition() {
        let def = make_test_quest_def();
        // ready phase 에는 Interact transition (ready → done) 이 있다
        assert!(interact_can_advance(&def, "ready"));
    }

    #[test]
    fn interact_can_advance_false_for_auto_only_phase() {
        let def = make_test_quest_def();
        // active phase 에는 Auto transition 만 있고 Interact transition 은 없다
        assert!(!interact_can_advance(&def, "active"));
    }

    #[test]
    fn interact_can_advance_false_for_self_loop_only() {
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
    fn glyph_green_question_when_interact_is_self_loop_hint_only() {
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
}

