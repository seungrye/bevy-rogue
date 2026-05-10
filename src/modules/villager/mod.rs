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
    quest::{QuestRegistry, QuestState, QuestSystemSet, KillNpcEvent, DespawnWorldItemEvent, execute_actions, QuestDef, QuestAction, eval_condition},
    item::{PlayerInventory},
    zone::{WorldState, SpawnQuestPortalEvent},
    combat::Speed,
};

const VILLAGER_STAY_CHANCE: f64 = 0.3;
const Z_VILLAGER: f32 = 0.9;

/// villager RON 파일에서 불러오는 NPC 정의
#[derive(Debug, Deserialize, Clone)]
pub struct VillagerDef {
    pub name: String,
    pub color: [f32; 3],
    pub dialogs: Vec<String>,
    pub quest_id: Option<String>,
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
    pub name: String,
    pub dialogues: Vec<String>,
    pub dialogue_idx: usize,
    pub tile_x: usize,
    pub tile_y: usize,
    pub just_bumped: bool,
    pub quest_id: Option<String>,
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
        let exists = villager_registry.villagers.iter().any(|v| v.name == qdef.giver_npc);
        if !exists {
            errors.push(format!(
                "퀘스트 '{}' 의 giver_npc '{}' 가 villager registry 에 없습니다",
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
    for KillNpcEvent(name) in events.read() {
        for (entity, villager) in &query {
            if &villager.name == name {
                commands.entity(entity).despawn_recursive();
                log.send(LogMessage(format!("{}이(가) 쓰러졌다...", name)));
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
/// - quest 가 terminal 페이즈가 아님 (on_interact / auto_advance 가 비어있지 않음)
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
        let Some(qid) = &v.quest_id else { continue; };
        let Some(qdef) = quest_registry.get(qid) else { continue; };

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

    // 퀘스트 NPC와 일반 NPC 를 분리: 퀘스트 NPC 는 활성 퀘스트인 것만 스폰
    let quest_npcs: Vec<&VillagerDef> = villager_registry.villagers.iter()
        .filter(|d| d.quest_id.as_deref().map_or(false, |qid| quest_registry.is_quest_active(qid)))
        .collect();
    let regular_npcs: Vec<&VillagerDef> = villager_registry.villagers.iter()
        .filter(|d| d.quest_id.is_none())
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
        // 퀘스트 NPC 는 항상 노란 '?' 로 시작 — 수락·진행 시 update_villager_glyph 가 갱신
        let (glyph, display_color) = if data.quest_id.is_some() {
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
                name: data.name.clone(),
                dialogues,
                dialogue_idx: 0,
                tile_x: cx,
                tile_y: cy,
                just_bumped: false,
                quest_id: data.quest_id.clone(),
                quest_dialogue_idx: 0,
                base_color,
                home_room: if data.quest_id.is_some() { Some(*room) } else { None },
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

            if villager.name == "상인" {
                shop_open.send(crate::modules::ui::shop::ShopOpenEvent);
            } else if let Some(quest_id) = villager.quest_id.clone() {
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
    // + Interact 시점에 한꺼번에 set_phase + on_interact 실행.
    let phase_id = state.phases.get(quest_id)
        .cloned()
        .unwrap_or_else(|| quest_def.initial_phase.clone());

    let Some(phase) = quest_def.phases.get(&phase_id) else { return };

    let dialog = phase.dialog.clone();
    let actions = phase.on_interact.clone();
    let npc_name = quest_def.giver_npc.clone();

    let idx = villager.quest_dialogue_idx.min(dialog.len().saturating_sub(1));
    if let Some(line) = dialog.get(idx) {
        log.send(LogMessage(format!("{}: {}", npc_name, line)));
    }

    // 마지막 줄에서 액션 실행
    if !dialog.is_empty() && idx + 1 >= dialog.len() {
        villager.quest_dialogue_idx = 0;
        // 첫 phase 등록 (아직 안 됐으면) — 마지막 대화 후 Interact 시점.
        if !state.phases.contains_key(quest_id) {
            state.set_phase(quest_id, &quest_def.initial_phase.clone());
        }
        execute_actions(&actions, quest_id, state, inventory, log, world, kill_npc, open_portal, close_portal, despawn_item, quest_items);
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
        let Some(ref qid) = villager.quest_id else { continue };
        let Some(def) = registry.get(qid) else { continue };
        let (glyph, color) = quest_npc_glyph(qid, def, &quest_state, &inventory, &world, villager.base_color, &quest_items);
        text.sections[0].value = glyph.to_string();
        text.sections[0].style.color = color;
    }
}

/// on_interact 액션 트리 안에 AdvancePhase 가 있는지 재귀 탐색한다.
/// Branch 의 if_true/if_false 를 포함해 모든 경로를 탐색한다.
/// Log·SetFlag 등 순수 힌트 액션만 있는 페이즈에서는 false 를 반환한다.
pub fn on_interact_can_advance(actions: &[QuestAction]) -> bool {
    actions.iter().any(|a| match a {
        QuestAction::AdvancePhase(_) => true,
        QuestAction::Branch { if_true, if_false, .. } =>
            on_interact_can_advance(if_true) || on_interact_can_advance(if_false),
        _ => false,
    })
}

/// 퀘스트 NPC 의 현재 퀘스트 상태에 따라 표시 글리프와 색상을 결정한다
///
/// - `?` (노란색) : 퀘스트 있음 — initial_phase 이거나 아직 수락 전
/// - `?` (초록색) : 퀘스트 수락, 다음 페이즈로 넘어갈 수 없는 상태 (아이템 수집·이동 중)
/// - `!` (초록색) : 다음 페이즈로 넘어갈 수 있는 상태 (on_interact 가 AdvancePhase 포함 또는 auto_advance 조건 충족)
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
    let Some(phase) = def.phases.get(pid) else {
        return ("v", base_color);
    };

    // on_interact 에 AdvancePhase 가 있으면 NPC 에게 말을 걸어 다음 페이즈로 넘어갈 수 있다.
    // Log·Branch(Log) 등 순수 힌트 액션만 있는 경우는 진행 불가로 간주한다.
    if on_interact_can_advance(&phase.on_interact) {
        return ("!", green);
    }

    // auto_advance 조건이 현재 충족됐으면 다음 페이즈로 넘어갈 수 있다
    let can_advance = phase.auto_advance.iter()
        .any(|aa| eval_condition(&aa.condition, inventory, world, state, quest_items));
    if can_advance {
        return ("!", green);
    }

    // 조건 미충족 — 아직 진행 중
    ("?", green)
}

fn is_quest_terminal_def(def: &QuestDef, state: &QuestState, quest_id: &str) -> bool {
    let Some(phase_id) = state.phases.get(quest_id) else { return false };
    let Some(phase) = def.phases.get(phase_id) else { return false };
    phase.on_interact.is_empty() && phase.auto_advance.is_empty()
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
            name: "테스트".to_string(),
            dialogues: vec![],
            dialogue_idx: 0,
            tile_x: 0,
            tile_y: 0,
            just_bumped,
            quest_id: None,
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
            name: "장로".to_string(),
            dialogues: vec![],
            dialogue_idx: 0,
            tile_x: 0,
            tile_y: 0,
            just_bumped: false,
            quest_id: Some("gem_quest".to_string()),
            quest_dialogue_idx: 0,
            base_color: Color::WHITE,
            home_room: None,
        };
        assert_eq!(v.quest_id.as_deref(), Some("gem_quest"));
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

    #[test]
    fn noin_npc_has_world_fracture_quest() {
        let defs = load_villager_defs();
        let noin = defs.iter().find(|d| d.name == "노인").expect("노인 NPC가 존재해야 한다");
        assert_eq!(noin.quest_id.as_deref(), Some("world_fracture"), "노인은 world_fracture 퀘스트를 가져야 한다");
    }

    #[test]
    fn gretchen_npc_has_parry_quest() {
        let defs = load_villager_defs();
        let gretchen = defs.iter().find(|d| d.name == "그레체").expect("그레체 NPC가 존재해야 한다");
        assert_eq!(gretchen.quest_id.as_deref(), Some("parry_quest"), "그레체는 parry_quest를 가져야 한다");
    }

    #[test]
    fn bastian_npc_has_demonsword_quest() {
        let defs = load_villager_defs();
        let bastian = defs.iter().find(|d| d.name == "바스티안").expect("바스티안 NPC가 존재해야 한다");
        assert_eq!(bastian.quest_id.as_deref(), Some("demonsword_quest"), "바스티안은 demonsword_quest를 가져야 한다");
    }

    #[test]
    fn discover_quest_npcs_marker_skips_non_quest_villager() {
        // 퀘스트 NPC만 마커, 일반 NPC 는 마커 없음.
        // (시스템 내부 로직만 검증 — Bevy App 통합 없이 데이터 검증)
        let regular = Villager {
            name: "농부".into(),
            dialogues: vec![],
            dialogue_idx: 0,
            tile_x: 5, tile_y: 5,
            just_bumped: false,
            quest_id: None,  // 일반 NPC
            quest_dialogue_idx: 0,
            base_color: Color::WHITE,
            home_room: None,
        };
        let quest_npc = Villager {
            name: "엘렌".into(),
            dialogues: vec![],
            dialogue_idx: 0,
            tile_x: 7, tile_y: 7,
            just_bumped: false,
            quest_id: Some("herb_quest".into()),
            quest_dialogue_idx: 0,
            base_color: Color::WHITE,
            home_room: None,
        };
        // 시스템 로직 시뮬레이션 — quest_id 있는 NPC 만 마커 추가됨
        let mut markers = DiscoveredMarkers::default();
        for v in [&regular, &quest_npc] {
            if v.quest_id.is_some() {
                markers.add(v.tile_x, v.tile_y, MarkerKind::QuestGiver, crate::modules::zone::ZoneId::Town);
            }
        }
        assert_eq!(markers.0.len(), 1, "quest_id 있는 NPC 만 마커가 추가돼야 한다");
        assert_eq!(markers.0[0].tile_x, 7);
        assert_eq!(markers.0[0].tile_y, 7);
    }

    #[test]
    fn villagers_ron_parses() {
        let defs = load_villager_defs();
        assert!(defs.len() >= 11, "11명 이상의 NPC 가 정의되어야 한다");
    }

    #[test]
    fn all_quest_giver_npcs_exist_in_villager_registry() {
        let villager_defs = load_villager_defs();
        let villager_names: HashSet<String> = villager_defs.iter().map(|d| d.name.clone()).collect();

        let dir = std::fs::read_dir("assets/quests").expect("assets/quests 가 존재해야 한다");
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("ron") { continue; }
            let text = std::fs::read_to_string(&path).unwrap();
            let qdef: QuestDef = ron::de::from_str(&text)
                .unwrap_or_else(|e| panic!("{:?} 파싱 실패: {}", path, e));
            assert!(
                villager_names.contains(&qdef.giver_npc),
                "퀘스트 {:?} 의 giver_npc '{}' 가 villager registry 에 없습니다",
                path, qdef.giver_npc
            );
        }
    }

    use crate::modules::quest::{QuestDef, QuestPhaseDef, QuestState, QuestAction, AutoAdvance, QuestCondition};
    use crate::modules::item::{PlayerInventory, InventoryItem, ItemKind, QuestItemKind};
    use crate::modules::zone::WorldState;
    use std::collections::HashMap as HM;

    fn make_test_quest_def() -> QuestDef {
        // not_started → active → ready → done
        let mut phases = HM::new();
        phases.insert("not_started".to_string(), QuestPhaseDef {
            dialog: vec![],
            on_interact: vec![QuestAction::AdvancePhase("active".to_string())],
            auto_advance: vec![],
            objective: None,
        });
        phases.insert("active".to_string(), QuestPhaseDef {
            dialog: vec![],
            on_interact: vec![],
            auto_advance: vec![AutoAdvance {
                condition: QuestCondition::HasItem("eternal_gem".to_string()),
                next_phase: "ready".to_string(),
                actions: vec![],
            }],
            objective: None,
        });
        phases.insert("ready".to_string(), QuestPhaseDef {
            dialog: vec![],
            on_interact: vec![QuestAction::AdvancePhase("done".to_string())],
            auto_advance: vec![],
            objective: None,
        });
        phases.insert("done".to_string(), QuestPhaseDef {
            dialog: vec![],
            on_interact: vec![],
            auto_advance: vec![],
            objective: None,
        });
        QuestDef {
            id: "test_quest".to_string(),
            title: "테스트".to_string(),
            giver_npc: "장로".to_string(),
            initial_phase: "not_started".to_string(),
            phases,
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
    fn glyph_green_exclamation_when_has_on_interact() {
        let def = make_test_quest_def();
        let state = make_state_at("ready");
        let inv = empty_inventory();
        let world = default_world();
        let (glyph, color) = quest_npc_glyph("test_quest", &def, &state, &inv, &world, Color::WHITE, qi());
        assert_eq!(glyph, "!");
        assert_eq!(color, Color::rgb(0.3, 1.0, 0.6), "on_interact 있음 = 초록 !");
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

    // on_interact_can_advance 단위 테스트
    #[test]
    fn can_advance_true_for_direct_advance_phase() {
        let actions = vec![QuestAction::AdvancePhase("next".to_string())];
        assert!(on_interact_can_advance(&actions));
    }

    #[test]
    fn can_advance_false_for_log_only() {
        let actions = vec![QuestAction::Log("힌트".to_string())];
        assert!(!on_interact_can_advance(&actions));
    }

    #[test]
    fn can_advance_true_for_branch_containing_advance() {
        let actions = vec![QuestAction::Branch {
            condition: Box::new(QuestCondition::HasItem("x".to_string())),
            if_true: vec![QuestAction::AdvancePhase("next".to_string())],
            if_false: vec![QuestAction::Log("없음".to_string())],
        }];
        assert!(on_interact_can_advance(&actions));
    }

    #[test]
    fn can_advance_false_for_branch_with_log_only() {
        let actions = vec![QuestAction::Branch {
            condition: Box::new(QuestCondition::HasItem("x".to_string())),
            if_true: vec![QuestAction::Log("a".to_string())],
            if_false: vec![QuestAction::Log("b".to_string())],
        }];
        assert!(!on_interact_can_advance(&actions));
    }

    #[test]
    fn glyph_green_question_when_on_interact_is_hint_only() {
        // gathering 페이즈처럼 on_interact 에 Log/Branch(Log) 만 있는 경우 초록 '?'
        // initial_phase 는 "not_started", 현재 페이즈는 "gathering" — initial 분기에 걸리지 않음
        let mut phases = HM::new();
        phases.insert("not_started".to_string(), QuestPhaseDef {
            dialog: vec![],
            on_interact: vec![QuestAction::AdvancePhase("gathering".to_string())],
            auto_advance: vec![],
            objective: None,
        });
        phases.insert("gathering".to_string(), QuestPhaseDef {
            dialog: vec!["아직 재료가 부족하네.".to_string()],
            on_interact: vec![QuestAction::Branch {
                condition: Box::new(QuestCondition::HasItem("dragon_scale".to_string())),
                if_true: vec![QuestAction::Log("있군".to_string())],
                if_false: vec![QuestAction::Log("없군".to_string())],
            }],
            auto_advance: vec![],
            objective: None,
        });
        let def = QuestDef {
            id: "alchemist_quest".to_string(),
            title: "연금술사".to_string(),
            giver_npc: "연금술사".to_string(),
            initial_phase: "not_started".to_string(),
            phases,
            spawns: vec![],
            spawn_chance: 1.0,
        };
        let mut state = QuestState::default();
        state.phases.insert("alchemist_quest".to_string(), "gathering".to_string());
        let inv = empty_inventory();
        let world = default_world();
        let (glyph, color) = quest_npc_glyph("alchemist_quest", &def, &state, &inv, &world, Color::WHITE, qi());
        assert_eq!(glyph, "?", "힌트만 있는 on_interact 는 '?' 여야 한다");
        assert_eq!(color, Color::rgb(0.3, 1.0, 0.6));
    }
}

