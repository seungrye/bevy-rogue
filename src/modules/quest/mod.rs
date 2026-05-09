use bevy::prelude::*;
use rand::Rng;
use std::collections::{HashMap, HashSet};
use serde::Deserialize;
use crate::modules::{
    item::{PlayerInventory, ItemKind, QuestItemKind, InventoryItem, Item, ItemSystemSet},
    map::{MapResource, TILE_SIZE, tile_to_world_coords, UsedSpawnTiles, random_floor_tile_anywhere},
    ui::minimap::{DiscoveredMarkers, MarkerKind},
    zone::{ZoneId, SpawnQuestPortalEvent},
};

// ── RON 데이터 구조 (assets/quests/*.ron) ────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub struct QuestDef {
    pub id: String,
    pub title: String,
    pub giver_npc: String,
    pub initial_phase: String,
    pub phases: HashMap<String, QuestPhaseDef>,
    #[serde(default)]
    pub spawns: Vec<QuestSpawn>,
    /// 게임 시작 시 이 퀘스트가 활성화될 확률 (0.0 ~ 1.0). 기본값 1.0 (항상 등장).
    #[serde(default = "default_spawn_chance")]
    pub spawn_chance: f32,
}

fn default_spawn_chance() -> f32 { 1.0 }

#[derive(Debug, Deserialize, Clone)]
pub struct QuestPhaseDef {
    pub dialog: Vec<String>,
    #[serde(default)]
    pub on_interact: Vec<QuestAction>,
    #[serde(default)]
    pub auto_advance: Vec<AutoAdvance>,
    #[serde(default)]
    pub objective: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AutoAdvance {
    pub condition: QuestCondition,
    pub next_phase: String,
    /// 조건 발동 시 즉시 실행되는 액션 (RemoveItem, DespawnWorldItem, SetFlag 지원)
    #[serde(default)]
    pub actions: Vec<QuestAction>,
}

#[derive(Debug, Deserialize, Clone)]
pub enum QuestCondition {
    HasItem(String),
    InZone(ZoneId),
    PhaseIs { quest: String, phase: String },
    FlagIs { flag: String, value: String },
    HasFlag(String),
    And(Vec<QuestCondition>),
    Or(Vec<QuestCondition>),
    Not(Box<QuestCondition>),
}

#[derive(Debug, Deserialize, Clone)]
pub enum QuestAction {
    AdvancePhase(String),
    GiveItem(String),
    RemoveItem(String),
    Log(String),
    SetFlag { flag: String, value: String },
    ClearFlag(String),
    KillNpc(String),
    /// 현재 존에 Named 존으로 이어지는 포탈을 즉시 스폰한다
    OpenPortal { zone: String, generator: String },
    /// 아이템을 수량 지정하여 지급
    GiveItems { item: String, count: u32 },
    /// 월드에 놓인 아이템 엔티티를 즉시 제거한다 (인벤토리는 건들지 않음)
    DespawnWorldItem(String),
    Branch {
        condition: Box<QuestCondition>,
        if_true: Vec<QuestAction>,
        if_false: Vec<QuestAction>,
    },
}

#[derive(Debug, Deserialize, Clone)]
pub struct QuestSpawn {
    pub phase: String,
    pub item: String,
    pub zone: ZoneId,
    /// 스폰할 아이템 수 (기본 1)
    #[serde(default = "default_spawn_count")]
    pub count: u32,
    /// 추가 스폰 조건 — 설정 시 phase 일치 + 이 조건 충족일 때만 스폰
    #[serde(default)]
    pub condition: Option<QuestCondition>,
}

fn default_spawn_count() -> u32 { 1 }

/// 퀘스트 시스템의 시작(Startup) 단계 실행 순서를 정의한다
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum QuestSystemSet {
    Load,
}

// ── 이벤트 ───────────────────────────────────────────────────────────────────

#[derive(Event)]
pub struct KillNpcEvent(pub String);

/// 월드에 놓인 특정 아이템 엔티티를 제거하도록 요청하는 이벤트
#[derive(Event)]
pub struct DespawnWorldItemEvent(pub String);

// ── 런타임 상태 ───────────────────────────────────────────────────────────────

#[derive(Resource, Default)]
pub struct QuestRegistry {
    pub quests: HashMap<String, QuestDef>,
    /// 이번 런에 활성화된 퀘스트 ID 집합 (spawn_chance 확률로 결정됨)
    pub active: HashSet<String>,
}

impl QuestRegistry {
    pub fn get(&self, id: &str) -> Option<&QuestDef> {
        self.quests.get(id)
    }

    pub fn is_quest_active(&self, quest_id: &str) -> bool {
        self.active.contains(quest_id)
    }

    #[allow(dead_code)]
    pub fn phase<'a>(&'a self, quest_id: &str, phase_id: &str) -> Option<&'a QuestPhaseDef> {
        self.quests.get(quest_id)?.phases.get(phase_id)
    }
}

#[derive(Resource, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct QuestState {
    pub phases: HashMap<String, String>,            // quest_id → current_phase_id
    pub spawned: std::collections::HashSet<String>, // "quest_id:item_id" 이미 스폰됨
    pub flags: HashMap<String, String>,             // 자유 플래그 (관계·세계 상태 추적)
}

impl QuestState {
    #[allow(dead_code)]
    pub fn current_phase<'a>(&'a self, quest_id: &str) -> Option<&'a str> {
        self.phases.get(quest_id).map(|s| s.as_str())
    }

    pub fn set_phase(&mut self, quest_id: &str, phase: &str) {
        self.phases.insert(quest_id.to_string(), phase.to_string());
    }

    pub fn is_spawn_done(&self, quest_id: &str, item_id: &str) -> bool {
        self.spawned.contains(&format!("{}:{}", quest_id, item_id))
    }

    pub fn mark_spawned(&mut self, quest_id: &str, item_id: &str) {
        self.spawned.insert(format!("{}:{}", quest_id, item_id));
    }

    pub fn set_flag(&mut self, flag: &str, value: &str) {
        self.flags.insert(flag.to_string(), value.to_string());
    }

    pub fn clear_flag(&mut self, flag: &str) {
        self.flags.remove(flag);
    }

    pub fn get_flag(&self, flag: &str) -> Option<&str> {
        self.flags.get(flag).map(|s| s.as_str())
    }

    pub fn has_flag(&self, flag: &str) -> bool {
        self.flags.contains_key(flag)
    }

    pub fn flag_is(&self, flag: &str, value: &str) -> bool {
        self.flags.get(flag).map(|v| v == value).unwrap_or(false)
    }
}

// ── Plugin ───────────────────────────────────────────────────────────────────

pub struct QuestPlugin;

impl Plugin for QuestPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuestRegistry>()
            .init_resource::<QuestState>()
            .add_event::<KillNpcEvent>()
            .add_event::<DespawnWorldItemEvent>()
            .add_systems(Startup, (
                load_quests.in_set(QuestSystemSet::Load).after(ItemSystemSet::Load),
                validate_quest_item_refs
                    .after(QuestSystemSet::Load)
                    .after(ItemSystemSet::Load),
            ))
            .add_systems(Update, (
                check_auto_advance,
                // apply_map 이 새 MapResource 를 교체한 뒤에 실행되어야 한다.
                // 같은 frame 에 ordering 없이 실행되면 옛 map 의 rooms/tiles 를 보고
                // 좌표를 골라 새 map 에서 wall 위에 spawn 되는 race condition 발생.
                spawn_quest_items.after(crate::modules::map::MapSystemSet::ExecuteRegen),
            ));
    }
}

/// 모든 퀘스트의 GiveItem/RemoveItem/spawn 등에서 참조하는 quest item ID 가
/// quest_items registry 에 존재하는지 명시적으로 검증한다.
fn validate_quest_item_refs(
    quest_registry: Res<QuestRegistry>,
    quest_items: Res<crate::modules::item::QuestItemRegistry>,
) {
    let mut errors: Vec<String> = Vec::new();
    for (qid, qdef) in &quest_registry.quests {
        // spawns
        for spawn in &qdef.spawns {
            if item_id_to_kind(&spawn.item, &quest_items).is_none() {
                errors.push(format!(
                    "퀘스트 '{}' 의 spawns item_id '{}' 가 인식되지 않습니다",
                    qid, spawn.item
                ));
            }
        }
        // 모든 phase 의 action 들을 탐색
        for (phase_id, phase) in &qdef.phases {
            for action in phase.on_interact.iter() {
                check_action_item_ids(action, qid, phase_id, &quest_items, &mut errors);
            }
            for auto in &phase.auto_advance {
                for action in &auto.actions {
                    check_action_item_ids(action, qid, phase_id, &quest_items, &mut errors);
                }
            }
        }
    }
    if !errors.is_empty() {
        for msg in &errors {
            error!("[치명적] {}", msg);
        }
        std::process::exit(1);
    }
}

fn check_action_item_ids(
    action: &QuestAction,
    qid: &str,
    phase_id: &str,
    quest_items: &crate::modules::item::QuestItemRegistry,
    errors: &mut Vec<String>,
) {
    match action {
        QuestAction::GiveItem(id)
        | QuestAction::GiveItems { item: id, .. }
        | QuestAction::RemoveItem(id)
        | QuestAction::DespawnWorldItem(id) => {
            if item_id_to_kind(id, quest_items).is_none() {
                errors.push(format!(
                    "퀘스트 '{}' phase '{}': item_id '{}' 가 인식되지 않습니다",
                    qid, phase_id, id
                ));
            }
        }
        QuestAction::Branch { if_true, if_false, .. } => {
            for a in if_true.iter().chain(if_false.iter()) {
                check_action_item_ids(a, qid, phase_id, quest_items, errors);
            }
        }
        _ => {}
    }
}

// ── Systems ──────────────────────────────────────────────────────────────────

fn load_quests(mut registry: ResMut<QuestRegistry>, quest_items: Res<crate::modules::item::QuestItemRegistry>) {
    let Ok(dir) = std::fs::read_dir("assets/quests") else {
        error!("[치명적] assets/quests 디렉터리를 찾을 수 없습니다. 게임을 시작할 수 없습니다.");
        std::process::exit(1);
    };

    let mut has_error = false;

    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ron") { continue; }

        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                error!("[퀘스트 오류] {:?} 읽기 실패: {}", path, e);
                has_error = true;
                continue;
            }
        };

        let def = match ron::de::from_str::<QuestDef>(&text) {
            Ok(d) => d,
            Err(e) => {
                error!("[퀘스트 오류] {:?} RON 파싱 실패:\n  {}", path, e);
                has_error = true;
                continue;
            }
        };

        // 시맨틱 검증
        let errors = validate_quest_def(&def, &quest_items);
        if !errors.is_empty() {
            for msg in &errors {
                error!("[퀘스트 오류] {:?} — {}", path, msg);
            }
            has_error = true;
            continue;
        }

        info!("퀘스트 로드: {} ({})", def.title, def.id);
        registry.quests.insert(def.id.clone(), def);
    }

    if has_error {
        error!("[치명적] 퀘스트 파일에 오류가 있습니다. 위 오류를 수정한 후 다시 실행하세요.");
        std::process::exit(1);
    }

    // spawn_chance 확률로 이번 런에 활성화할 퀘스트 결정
    let mut rng = rand::thread_rng();
    let active: HashSet<String> = registry.quests.iter()
        .filter(|(_, def)| rng.gen::<f32>() < def.spawn_chance)
        .map(|(id, _)| id.clone())
        .collect();
    for id in &active {
        info!("퀘스트 활성화: {}", id);
    }
    registry.active = active;
}

/// QuestDef 의 내부 일관성을 검증하고 오류 메시지 목록을 반환한다
pub fn validate_quest_def(def: &QuestDef, quest_items: &crate::modules::item::QuestItemRegistry) -> Vec<String> {
    let mut errors = Vec::new();

    // initial_phase 존재 확인
    if !def.phases.contains_key(&def.initial_phase) {
        errors.push(format!(
            "initial_phase '{}' 이 phases 에 없습니다", def.initial_phase
        ));
    }

    for (phase_id, phase) in &def.phases {
        // on_interact AdvancePhase 참조 확인
        for action in &phase.on_interact {
            collect_action_errors(action, &def.phases, phase_id, quest_items, &mut errors);
        }
        // auto_advance next_phase 및 런타임에서 실제 실행되는 액션만 허용
        for auto in &phase.auto_advance {
            if !def.phases.contains_key(&auto.next_phase) {
                errors.push(format!(
                    "페이즈 '{}': auto_advance next_phase '{}' 이 없습니다",
                    phase_id, auto.next_phase
                ));
            }
            for action in &auto.actions {
                if !is_auto_advance_action_supported(action) {
                    errors.push(format!(
                        "페이즈 '{}': auto_advance actions 에서 지원하지 않는 액션 {:?}",
                        phase_id, action
                    ));
                }
                collect_action_errors(action, &def.phases, phase_id, quest_items, &mut errors);
            }
        }
    }

    // spawns 아이템 ID 확인
    for spawn in &def.spawns {
        if item_id_to_kind(&spawn.item, quest_items).is_none() {
            errors.push(format!(
                "spawns: item_id '{}' 를 인식할 수 없습니다", spawn.item
            ));
        }
        if !def.phases.contains_key(&spawn.phase) {
            errors.push(format!(
                "spawns: phase '{}' 이 phases 에 없습니다", spawn.phase
            ));
        }
        if let Some(condition) = &spawn.condition {
            if condition_uses_inventory(condition) {
                errors.push(format!(
                    "spawns: item '{}' condition 에 HasItem 을 사용할 수 없습니다 (스폰 조건은 플래그/존/페이즈만 지원)",
                    spawn.item
                ));
            }
        }
    }

    errors
}

fn collect_action_errors(
    action: &QuestAction,
    phases: &HashMap<String, QuestPhaseDef>,
    phase_id: &str,
    quest_items: &crate::modules::item::QuestItemRegistry,
    errors: &mut Vec<String>,
) {
    match action {
        QuestAction::AdvancePhase(next) => {
            if !phases.contains_key(next) {
                errors.push(format!(
                    "페이즈 '{}': AdvancePhase('{}') 이 없습니다", phase_id, next
                ));
            }
        }
        QuestAction::GiveItem(id) | QuestAction::GiveItems { item: id, .. } | QuestAction::RemoveItem(id) | QuestAction::DespawnWorldItem(id) => {
            if item_id_to_kind(id, quest_items).is_none() {
                errors.push(format!(
                    "페이즈 '{}': item_id '{}' 를 인식할 수 없습니다", phase_id, id
                ));
            }
        }
        QuestAction::Branch { if_true, if_false, .. } => {
            for a in if_true.iter().chain(if_false.iter()) {
                collect_action_errors(a, phases, phase_id, quest_items, errors);
            }
        }
        _ => {}
    }
}

fn is_auto_advance_action_supported(action: &QuestAction) -> bool {
    matches!(
        action,
        QuestAction::DespawnWorldItem(_)
            | QuestAction::RemoveItem(_)
            | QuestAction::SetFlag { .. }
    )
}

fn condition_uses_inventory(condition: &QuestCondition) -> bool {
    match condition {
        QuestCondition::HasItem(_) => true,
        QuestCondition::And(conditions) | QuestCondition::Or(conditions) => {
            conditions.iter().any(condition_uses_inventory)
        }
        QuestCondition::Not(inner) => condition_uses_inventory(inner),
        _ => false,
    }
}

/// auto_advance 조건을 매 프레임 평가하여 단계를 자동 전진시킨다
/// Vec 순서로 평가하며 첫 번째 충족 조건만 적용한다
fn check_auto_advance(
    registry: Res<QuestRegistry>,
    mut state: ResMut<QuestState>,
    mut inventory: ResMut<PlayerInventory>,
    world: Res<crate::modules::zone::WorldState>,
    mut despawn_item: EventWriter<DespawnWorldItemEvent>,
    quest_items: Res<crate::modules::item::QuestItemRegistry>,
) {
    let mut advances: Vec<(String, String, Vec<QuestAction>)> = Vec::new();

    for (quest_id, quest_def) in &registry.quests {
        if !registry.is_quest_active(quest_id) { continue; }
        let current = match state.phases.get(quest_id) {
            Some(p) => p.clone(),
            None => continue,
        };
        let phase_def = match quest_def.phases.get(&current) {
            Some(p) => p,
            None => continue,
        };
        for auto in &phase_def.auto_advance {
            if eval_condition(&auto.condition, &inventory, &world, &state, &quest_items) {
                advances.push((quest_id.clone(), auto.next_phase.clone(), auto.actions.clone()));
                break; // 첫 번째 충족 조건만 사용
            }
        }
    }

    for (quest_id, next_phase, actions) in advances {
        info!("퀘스트 [{}] 자동 전진: {}", quest_id, next_phase);
        state.set_phase(&quest_id, &next_phase);
        // auto_advance 전용 인라인 실행 (DespawnWorldItem, RemoveItem, SetFlag 지원)
        for action in &actions {
            match action {
                QuestAction::DespawnWorldItem(item_id) => {
                    despawn_item.send(DespawnWorldItemEvent(item_id.clone()));
                }
                QuestAction::RemoveItem(item_id) => {
                    if let Some(kind) = item_id_to_kind(item_id, &quest_items) {
                        inventory.items.retain(|i| i.kind != kind);
                    }
                }
                QuestAction::SetFlag { flag, value } => {
                    state.set_flag(flag, value);
                }
                _ => {} // on_interact 전용 액션(OpenPortal, KillNpc 등)은 auto_advance에서 미지원
            }
        }
    }
}

// ── 퀘스트 액션 실행 (빌리저 시스템에서 호출) ────────────────────────────────

pub fn execute_actions(
    actions: &[QuestAction],
    quest_id: &str,
    state: &mut QuestState,
    inventory: &mut PlayerInventory,
    log: &mut EventWriter<crate::modules::ui::LogMessage>,
    world: &crate::modules::zone::WorldState,
    kill_npc: &mut EventWriter<KillNpcEvent>,
    open_portal: &mut EventWriter<SpawnQuestPortalEvent>,
    despawn_item: &mut EventWriter<DespawnWorldItemEvent>,
    quest_items: &crate::modules::item::QuestItemRegistry,
) {
    for action in actions {
        match action {
            QuestAction::AdvancePhase(phase) => {
                state.set_phase(quest_id, phase);
                info!("퀘스트 [{}] 단계 전진: {}", quest_id, phase);
            }
            QuestAction::GiveItem(item_id) => {
                if let Some(kind) = item_id_to_kind(item_id, quest_items) {
                    inventory.items.push(InventoryItem { kind });
                    log.send(crate::modules::ui::LogMessage(
                        format!("{} 획득!", kind.display_name(quest_items))
                    ));
                }
            }
            QuestAction::GiveItems { item: item_id, count } => {
                if let Some(kind) = item_id_to_kind(item_id, quest_items) {
                    for _ in 0..*count {
                        match kind {
                            ItemKind::Consumable(ck) => inventory.add_consumable(ck),
                            _ => inventory.items.push(InventoryItem { kind }),
                        }
                    }
                    log.send(crate::modules::ui::LogMessage(
                        format!("{} x{} 획득!", kind.display_name(quest_items), count)
                    ));
                }
            }
            QuestAction::RemoveItem(item_id) => {
                if let Some(kind) = item_id_to_kind(item_id, quest_items) {
                    inventory.items.retain(|i| i.kind != kind);
                    log.send(crate::modules::ui::LogMessage(
                        format!("{} 반납.", kind.display_name(quest_items))
                    ));
                }
            }
            QuestAction::Log(msg) => {
                log.send(crate::modules::ui::LogMessage(msg.clone()));
            }
            QuestAction::SetFlag { flag, value } => {
                state.set_flag(flag, value);
                info!("퀘스트 플래그 설정: {} = {}", flag, value);
            }
            QuestAction::ClearFlag(flag) => {
                state.clear_flag(flag);
                info!("퀘스트 플래그 해제: {}", flag);
            }
            QuestAction::KillNpc(name) => {
                kill_npc.send(KillNpcEvent(name.clone()));
                info!("NPC 사망 이벤트: {}", name);
            }
            QuestAction::OpenPortal { zone, generator } => {
                open_portal.send(SpawnQuestPortalEvent {
                    zone: zone.clone(),
                    generator: generator.clone(),
                });
                log.send(crate::modules::ui::LogMessage(
                    format!("포탈이 열렸다 — {}.", zone)
                ));
                info!("퀘스트 포탈 열기: {} (생성기: {})", zone, generator);
            }
            QuestAction::DespawnWorldItem(item_id) => {
                despawn_item.send(DespawnWorldItemEvent(item_id.clone()));
                info!("월드 아이템 제거: {}", item_id);
            }
            QuestAction::Branch { condition, if_true, if_false } => {
                let branch = if eval_condition(condition, inventory, world, state, quest_items) {
                    if_true.as_slice()
                } else {
                    if_false.as_slice()
                };
                execute_actions(branch, quest_id, state, inventory, log, world, kill_npc, open_portal, despawn_item, quest_items);
            }
        }
    }
}

// ── 조건 평가 ────────────────────────────────────────────────────────────────

pub fn eval_condition(
    cond: &QuestCondition,
    inventory: &PlayerInventory,
    world: &crate::modules::zone::WorldState,
    quest_state: &QuestState,
    quest_items: &crate::modules::item::QuestItemRegistry,
) -> bool {
    match cond {
        QuestCondition::HasItem(item_id) => {
            let Some(kind) = item_id_to_kind(item_id, quest_items) else { return false };
            inventory.items.iter().any(|i| i.kind == kind)
        }
        QuestCondition::InZone(zone) => &world.current == zone,
        QuestCondition::PhaseIs { quest, phase } => {
            quest_state.phases.get(quest).map(|p| p == phase).unwrap_or(false)
        }
        QuestCondition::FlagIs { flag, value } => quest_state.flag_is(flag, value),
        QuestCondition::HasFlag(flag) => quest_state.has_flag(flag),
        QuestCondition::And(conds) => {
            conds.iter().all(|c| eval_condition(c, inventory, world, quest_state, quest_items))
        }
        QuestCondition::Or(conds) => {
            conds.iter().any(|c| eval_condition(c, inventory, world, quest_state, quest_items))
        }
        QuestCondition::Not(inner) => {
            !eval_condition(inner, inventory, world, quest_state, quest_items)
        }
    }
}

/// 퀘스트 스폰 조건이 충족되면 존에 아이템을 스폰한다 (맵 변경 시에만 실행)
fn spawn_quest_items(
    registry: Res<QuestRegistry>,
    mut state: ResMut<QuestState>,
    world: Res<crate::modules::zone::WorldState>,
    map_res: Res<MapResource>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut used_spawn: ResMut<UsedSpawnTiles>,
    mut markers: ResMut<DiscoveredMarkers>,
    quest_items: Res<crate::modules::item::QuestItemRegistry>,
) {
    if !map_res.is_changed() { return; }

    let mut rng = rand::thread_rng();

    for (quest_id, quest_def) in &registry.quests {
        if !registry.is_quest_active(quest_id) { continue; }
        let current_phase = match state.phases.get(quest_id) {
            Some(p) => p.clone(),
            None => continue,
        };

        for spawn in &quest_def.spawns {
            if spawn.phase != current_phase { continue; }
            if spawn.zone != world.current { continue; }
            if state.is_spawn_done(quest_id, &spawn.item) { continue; }

            // 추가 조건이 있으면 평가
            if let Some(ref cond) = spawn.condition {
                // spawn_quest_items 에서는 inventory 접근 불가 — 플래그/존/페이즈 조건만 평가
                let dummy_inv = crate::modules::item::PlayerInventory::default();
                if !eval_condition(cond, &dummy_inv, &world, &state, &quest_items) { continue; }
            }

            let Some(kind) = item_id_to_kind(&spawn.item, &quest_items) else { continue };
            let map = &map_res.0;

            // rooms[0] 은 보통 player 시작 방 — 가능하면 다른 room 우선.
            // 1 개뿐이면 그 방에라도 스폰.
            let rooms_slice: &[crate::modules::map::Rect] = if map.rooms.len() > 1 {
                &map.rooms[1..]
            } else {
                &map.rooms[..]
            };
            let font = asset_server.load("fonts/FiraMono-Medium.ttf");

            for _ in 0..spawn.count {
                let Some((tx, ty)) = random_floor_tile_anywhere(rooms_slice, map, &mut used_spawn.0, &mut rng)
                else {
                    info!("퀘스트 아이템 스폰 실패 — Floor 타일 없음: {}", spawn.item);
                    continue;
                };

                // 안전망: random_floor_tile_anywhere 가 Floor 만 반환해야 하지만
                // race condition / map 캐시 불일치 등으로 wall 좌표가 나올 가능성을 가드한다.
                if map.get_tile(tx, ty) != crate::modules::map::TileKind::Floor {
                    error!("퀘스트 아이템 spawn 좌표 ({}, {}) 가 Floor 가 아님 — 스킵: {}", tx, ty, spawn.item);
                    continue;
                }

                let pos = tile_to_world_coords(tx, ty);
                commands.spawn((
                    Text2dBundle {
                        text: Text::from_section(kind.glyph(&quest_items), TextStyle {
                            font: font.clone(),
                            font_size: TILE_SIZE,
                            color: kind.color(),
                        }),
                        transform: Transform::from_xyz(pos.x, pos.y, 0.3),
                        ..default()
                    },
                    Item { kind, tile_x: tx, tile_y: ty },
                ));
                // 퀘스트 아이템은 목표물 자체이므로 스폰되는 순간 미니맵 목표 마커로 등록한다.
                markers.add(tx, ty, MarkerKind::QuestTarget, world.current.clone());
                info!("퀘스트 아이템 스폰: {} at ({}, {})", spawn.item, tx, ty);
            }

            state.mark_spawned(quest_id, &spawn.item);
        }
    }
}

// ── item_id 매핑 ─────────────────────────────────────────────────────────────

pub fn item_id_to_kind(id: &str, quest_items: &crate::modules::item::QuestItemRegistry) -> Option<ItemKind> {
    match id {
        "sword"               => Some(ItemKind::Weapon(crate::modules::item::WeaponKind::SWORD)),
        "spear"               => Some(ItemKind::Weapon(crate::modules::item::WeaponKind::SPEAR)),
        "bow"                 => Some(ItemKind::Weapon(crate::modules::item::WeaponKind::BOW)),
        "leather_armor"       => Some(ItemKind::Armor(crate::modules::item::ArmorKind::LEATHER_ARMOR)),
        "health_potion"       => Some(ItemKind::Consumable(crate::modules::item::ConsumableKind::HEALTH_POTION)),
        // 그 외는 quest item registry 에서 조회 — 알려진 quest item ID 면 QuestItemKind 반환
        other => quest_items.intern_quest_item(other).map(|s| ItemKind::QuestItem(QuestItemKind(s))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    static TEST_QI: OnceLock<crate::modules::item::QuestItemRegistry> = OnceLock::new();
    fn qi() -> &'static crate::modules::item::QuestItemRegistry {
        TEST_QI.get_or_init(|| crate::modules::item::build_test_registry())
    }

    fn make_registry_with_gem_quest() -> QuestRegistry {
        let mut r = QuestRegistry::default();
        let def = QuestDef {
            id: "gem_quest".into(),
            title: "잃어버린 보석".into(),
            giver_npc: "장로".into(),
            initial_phase: "not_started".into(),
            phases: {
                let mut m = HashMap::new();
                m.insert("not_started".into(), QuestPhaseDef {
                    dialog: vec!["대화".into()],
                    on_interact: vec![QuestAction::AdvancePhase("active".into())],
                    auto_advance: vec![],
                    objective: None,
                });
                m.insert("active".into(), QuestPhaseDef {
                    dialog: vec!["아직".into()],
                    on_interact: vec![],
                    auto_advance: vec![AutoAdvance {
                        condition: QuestCondition::HasItem("eternal_gem".into()),
                        next_phase: "ready".into(),
                        actions: vec![],
                    }],
                    objective: Some("영원의 보석을 찾아라".into()),
                });
                m
            },
            spawns: vec![],
            spawn_chance: 1.0,
        };
        r.quests.insert(def.id.clone(), def);
        r
    }

    #[test]
    fn quest_state_phase_tracking() {
        let mut state = QuestState::default();
        assert!(state.current_phase("gem_quest").is_none());
        state.set_phase("gem_quest", "active");
        assert_eq!(state.current_phase("gem_quest"), Some("active"));
    }

    #[test]
    fn item_id_to_kind_maps_correctly() {
        let _ = qi();
        assert_eq!(item_id_to_kind("eternal_gem", qi()),        Some(ItemKind::QuestItem(QuestItemKind("eternal_gem"))));
        assert_eq!(item_id_to_kind("philosophers_stone", qi()), Some(ItemKind::QuestItem(QuestItemKind("philosophers_stone"))));
        assert!(item_id_to_kind("unknown", qi()).is_none());
    }

    #[test]
    fn registry_phase_lookup() {
        let reg = make_registry_with_gem_quest();
        assert!(reg.phase("gem_quest", "not_started").is_some());
        assert!(reg.phase("gem_quest", "missing").is_none());
        assert!(reg.phase("no_quest", "x").is_none());
    }

    #[test]
    fn spawn_tracking() {
        let mut state = QuestState::default();
        assert!(!state.is_spawn_done("gem_quest", "eternal_gem"));
        state.mark_spawned("gem_quest", "eternal_gem");
        assert!(state.is_spawn_done("gem_quest", "eternal_gem"));
    }

    fn make_world() -> crate::modules::zone::WorldState {
        crate::modules::zone::WorldState::default()
    }

    fn make_inventory_with(item_ids: &[&str]) -> PlayerInventory {
        let mut inv = PlayerInventory::default();
        for id in item_ids {
            if let Some(kind) = item_id_to_kind(id, qi()) {
                inv.items.push(InventoryItem { kind });
            }
        }
        inv
    }

    #[test]
    fn eval_and_requires_all() {
        let inv = make_inventory_with(&["dragon_scale"]);
        let state = QuestState::default();
        let world = make_world();
        let cond = QuestCondition::And(vec![
            QuestCondition::HasItem("dragon_scale".into()),
            QuestCondition::HasItem("ancient_scroll".into()),
        ]);
        assert!(!eval_condition(&cond, &inv, &world, &state, qi()));

        let inv2 = make_inventory_with(&["dragon_scale", "ancient_scroll"]);
        assert!(eval_condition(&cond, &inv2, &world, &state, qi()));
    }

    #[test]
    fn eval_or_requires_any() {
        let inv = make_inventory_with(&["dragon_scale"]);
        let state = QuestState::default();
        let world = make_world();
        let cond = QuestCondition::Or(vec![
            QuestCondition::HasItem("dragon_scale".into()),
            QuestCondition::HasItem("ancient_scroll".into()),
        ]);
        assert!(eval_condition(&cond, &inv, &world, &state, qi()));

        let empty = PlayerInventory::default();
        assert!(!eval_condition(&cond, &empty, &world, &state, qi()));
    }

    #[test]
    fn eval_not_inverts() {
        let inv = make_inventory_with(&["dragon_scale"]);
        let state = QuestState::default();
        let world = make_world();
        let cond = QuestCondition::Not(Box::new(QuestCondition::HasItem("ancient_scroll".into())));
        assert!(eval_condition(&cond, &inv, &world, &state, qi()));
        let cond2 = QuestCondition::Not(Box::new(QuestCondition::HasItem("dragon_scale".into())));
        assert!(!eval_condition(&cond2, &inv, &world, &state, qi()));
    }

    #[test]
    fn eval_phase_is_checks_quest_state() {
        let inv = PlayerInventory::default();
        let mut state = QuestState::default();
        let world = make_world();
        let cond = QuestCondition::PhaseIs { quest: "gem_quest".into(), phase: "done".into() };
        assert!(!eval_condition(&cond, &inv, &world, &state, qi()));
        state.set_phase("gem_quest", "done");
        assert!(eval_condition(&cond, &inv, &world, &state, qi()));
    }

    #[test]
    fn auto_advance_priority_first_match_wins() {
        // gathering 단계 재현: dragon_scale + ancient_scroll 있으면 1순위 both_ready
        let mut state = QuestState::default();
        state.set_phase("test_q", "gathering");
        let phase = QuestPhaseDef {
            dialog: vec![],
            on_interact: vec![],
            auto_advance: vec![
                AutoAdvance {
                    condition: QuestCondition::And(vec![
                        QuestCondition::HasItem("dragon_scale".into()),
                        QuestCondition::HasItem("ancient_scroll".into()),
                    ]),
                    next_phase: "both_ready".into(),
                    actions: vec![],
                },
                AutoAdvance {
                    condition: QuestCondition::HasItem("dragon_scale".into()),
                    next_phase: "has_scale_hint".into(),
                    actions: vec![],
                },
            ],
            objective: None,
        };
        let world = make_world();
        let inv_both = make_inventory_with(&["dragon_scale", "ancient_scroll"]);
        let matched: Option<String> = phase.auto_advance.iter()
            .find(|a| eval_condition(&a.condition, &inv_both, &world, &state, qi()))
            .map(|a| a.next_phase.clone());
        assert_eq!(matched.as_deref(), Some("both_ready"), "둘 다 있으면 1순위가 선택돼야 한다");

        let inv_scale = make_inventory_with(&["dragon_scale"]);
        let matched2: Option<String> = phase.auto_advance.iter()
            .find(|a| eval_condition(&a.condition, &inv_scale, &world, &state, qi()))
            .map(|a| a.next_phase.clone());
        assert_eq!(matched2.as_deref(), Some("has_scale_hint"), "용비늘만 있으면 2순위가 선택돼야 한다");
    }

    #[test]
    fn branch_action_selects_correct_path() {
        let mut state = QuestState::default();
        state.set_phase("test_q", "ready");
        let mut inv = make_inventory_with(&["dragon_scale", "ancient_scroll"]);
        let world = make_world();
        let mut log_msgs: Vec<String> = Vec::new();

        // Branch: dragon_scale + ancient_scroll 있으면 if_true
        let action = QuestAction::Branch {
            condition: Box::new(QuestCondition::And(vec![
                QuestCondition::HasItem("dragon_scale".into()),
                QuestCondition::HasItem("ancient_scroll".into()),
            ])),
            if_true: vec![
                QuestAction::RemoveItem("dragon_scale".into()),
                QuestAction::RemoveItem("ancient_scroll".into()),
                QuestAction::Log("정통 결말".into()),
                QuestAction::AdvancePhase("normal_done".into()),
            ],
            if_false: vec![
                QuestAction::Log("재료 부족".into()),
            ],
        };

        // EventWriter 없이 내부 로직만 재현
        if let QuestAction::Branch { condition, if_true, if_false } = &action {
            let branch = if eval_condition(condition, &inv, &world, &state, qi()) { if_true } else { if_false };
            for a in branch {
                match a {
                    QuestAction::RemoveItem(id) => {
                        if let Some(kind) = item_id_to_kind(id, qi()) {
                            inv.items.retain(|i| i.kind != kind);
                        }
                    }
                    QuestAction::Log(msg) => log_msgs.push(msg.clone()),
                    QuestAction::AdvancePhase(p) => state.set_phase("test_q", p),
                    _ => {}
                }
            }
        }

        assert_eq!(state.current_phase("test_q"), Some("normal_done"));
        assert!(log_msgs.contains(&"정통 결말".to_string()));
        assert!(!inv.items.iter().any(|i| matches!(i.kind, ItemKind::QuestItem(qk) if qk.0 == "dragon_scale")));
    }

    #[test]
    fn new_item_ids_mapped_correctly() {
        let _ = qi();
        assert_eq!(item_id_to_kind("dragon_scale", qi()),   Some(ItemKind::QuestItem(QuestItemKind("dragon_scale"))));
        assert_eq!(item_id_to_kind("ancient_scroll", qi()), Some(ItemKind::QuestItem(QuestItemKind("ancient_scroll"))));
    }

    #[test]
    fn flag_set_get_clear() {
        let mut state = QuestState::default();
        assert!(!state.has_flag("trust_elara"));
        assert_eq!(state.get_flag("trust_elara"), None);

        state.set_flag("trust_elara", "high");
        assert!(state.has_flag("trust_elara"));
        assert_eq!(state.get_flag("trust_elara"), Some("high"));
        assert!(state.flag_is("trust_elara", "high"));
        assert!(!state.flag_is("trust_elara", "low"));

        state.clear_flag("trust_elara");
        assert!(!state.has_flag("trust_elara"));
    }

    #[test]
    fn eval_flag_is_condition() {
        let inv = PlayerInventory::default();
        let world = make_world();
        let mut state = QuestState::default();
        let cond = QuestCondition::FlagIs { flag: "npc_alive".to_string(), value: "true".to_string() };
        assert!(!eval_condition(&cond, &inv, &world, &state, qi()));
        state.set_flag("npc_alive", "true");
        assert!(eval_condition(&cond, &inv, &world, &state, qi()));
    }

    #[test]
    fn eval_has_flag_condition() {
        let inv = PlayerInventory::default();
        let world = make_world();
        let mut state = QuestState::default();
        let cond = QuestCondition::HasFlag("village_burned".to_string());
        assert!(!eval_condition(&cond, &inv, &world, &state, qi()));
        state.set_flag("village_burned", "true");
        assert!(eval_condition(&cond, &inv, &world, &state, qi()));
        state.clear_flag("village_burned");
        assert!(!eval_condition(&cond, &inv, &world, &state, qi()));
    }

    #[test]
    fn auto_advance_actions_field_defaults_to_empty() {
        let def: QuestDef = ron::de::from_str(r#"
            QuestDef(
                id: "test",
                title: "test",
                giver_npc: "npc",
                initial_phase: "start",
                phases: {
                    "start": QuestPhaseDef(
                        dialog: [],
                        auto_advance: [
                            AutoAdvance(
                                condition: HasItem("eternal_gem"),
                                next_phase: "done",
                            ),
                        ],
                    ),
                    "done": QuestPhaseDef(dialog: []),
                },
            )
        "#).expect("RON 파싱 성공해야 한다");
        let phase = def.phases.get("start").unwrap();
        assert!(phase.auto_advance[0].actions.is_empty(), "actions 미지정 시 빈 vec이어야 한다");
    }

    #[test]
    fn auto_advance_actions_parsed_from_ron() {
        let def: QuestDef = ron::de::from_str(r#"
            QuestDef(
                id: "test",
                title: "test",
                giver_npc: "npc",
                initial_phase: "start",
                phases: {
                    "start": QuestPhaseDef(
                        dialog: [],
                        auto_advance: [
                            AutoAdvance(
                                condition: HasItem("prologue_greatsword"),
                                next_phase: "done",
                                actions: [
                                    DespawnWorldItem("prologue_daggers"),
                                    DespawnWorldItem("prologue_bowtorch"),
                                ],
                            ),
                        ],
                    ),
                    "done": QuestPhaseDef(dialog: []),
                },
            )
        "#).expect("RON 파싱 성공해야 한다");
        let phase = def.phases.get("start").unwrap();
        let actions = &phase.auto_advance[0].actions;
        assert_eq!(actions.len(), 2);
        assert!(matches!(&actions[0], QuestAction::DespawnWorldItem(id) if id == "prologue_daggers"));
        assert!(matches!(&actions[1], QuestAction::DespawnWorldItem(id) if id == "prologue_bowtorch"));
    }

    #[test]
    fn validate_rejects_unsupported_auto_advance_action() {
        let def: QuestDef = ron::de::from_str(r#"
            QuestDef(
                id: "test",
                title: "test",
                giver_npc: "npc",
                initial_phase: "start",
                phases: {
                    "start": QuestPhaseDef(
                        dialog: [],
                        auto_advance: [
                            AutoAdvance(
                                condition: HasFlag("ready"),
                                next_phase: "done",
                                actions: [OpenPortal(zone: "rift", generator: "bsp")],
                            ),
                        ],
                    ),
                    "done": QuestPhaseDef(dialog: []),
                },
            )
        "#).expect("RON 파싱 성공해야 한다");
        let errors = validate_quest_def(&def, qi());
        assert!(
            errors.iter().any(|e| e.contains("지원하지 않는 액션")),
            "unsupported auto action 오류가 있어야 한다: {:?}",
            errors
        );
    }

    #[test]
    fn validate_rejects_has_item_in_spawn_condition() {
        let def: QuestDef = ron::de::from_str(r#"
            QuestDef(
                id: "test",
                title: "test",
                giver_npc: "npc",
                initial_phase: "start",
                phases: {
                    "start": QuestPhaseDef(dialog: []),
                },
                spawns: [
                    QuestSpawn(
                        phase: "start",
                        item: "eternal_gem",
                        zone: Town,
                        condition: Some(HasItem("prologue_greatsword")),
                    ),
                ],
            )
        "#).expect("RON 파싱 성공해야 한다");
        let errors = validate_quest_def(&def, qi());
        assert!(
            errors.iter().any(|e| e.contains("HasItem")),
            "spawn HasItem 조건 오류가 있어야 한다: {:?}",
            errors
        );
    }

    // ── assets/quests/*.ron 파일 통합 검증 ──────────────────────────────────

    fn load_all_quest_defs() -> Vec<(String, QuestDef)> {
        let dir = std::fs::read_dir("assets/quests")
            .expect("assets/quests 디렉터리가 존재해야 한다");
        let mut defs = Vec::new();
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("ron") { continue; }
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{:?} 읽기 실패: {}", path, e));
            let def = ron::de::from_str::<QuestDef>(&text)
                .unwrap_or_else(|e| panic!("{:?} 파싱 실패: {}", path, e));
            defs.push((path.to_string_lossy().into_owned(), def));
        }
        assert!(!defs.is_empty(), "assets/quests 에 .ron 파일이 하나 이상 있어야 한다");
        defs
    }

    #[test]
    fn all_quest_files_parse_without_error() {
        // 파일 존재 + RON 파싱 성공 여부만 검증
        let defs = load_all_quest_defs();
        assert!(defs.len() >= 4, "prologue + 3 route 퀘스트 최소 4개여야 한다");
    }

    #[test]
    fn all_quest_files_pass_semantic_validation() {
        let _ = qi();
        for (path, def) in load_all_quest_defs() {
            let errors = validate_quest_def(&def, qi());
            assert!(
                errors.is_empty(),
                "{} 시맨틱 검증 실패:\n{}",
                path,
                errors.join("\n")
            );
        }
    }

    #[test]
    fn all_quest_item_ids_are_recognized() {
        let _ = qi();
        for (path, def) in load_all_quest_defs() {
            for spawn in &def.spawns {
                assert!(
                    item_id_to_kind(&spawn.item, qi()).is_some(),
                    "{}: spawns 의 item_id '{}' 가 item_id_to_kind 에 없다",
                    path, spawn.item
                );
            }
        }
    }

    #[test]
    fn quest_registry_is_quest_active() {
        let mut reg = QuestRegistry::default();
        reg.active.insert("gem_quest".to_string());
        assert!(reg.is_quest_active("gem_quest"));
        assert!(!reg.is_quest_active("herb_quest"));
    }

    #[test]
    fn spawn_chance_defaults_to_1_when_omitted_in_ron() {
        let def: QuestDef = ron::de::from_str(r#"
            QuestDef(
                id: "test",
                title: "test",
                giver_npc: "npc",
                initial_phase: "start",
                phases: { "start": QuestPhaseDef(dialog: []) },
            )
        "#).expect("RON 파싱 성공");
        assert_eq!(def.spawn_chance, 1.0, "spawn_chance 미지정 시 1.0이어야 한다");
    }

    #[test]
    fn spawn_chance_parsed_from_ron() {
        let def: QuestDef = ron::de::from_str(r#"
            QuestDef(
                id: "test",
                title: "test",
                giver_npc: "npc",
                initial_phase: "start",
                spawn_chance: 0.5,
                phases: { "start": QuestPhaseDef(dialog: []) },
            )
        "#).expect("RON 파싱 성공");
        assert_eq!(def.spawn_chance, 0.5);
    }

    #[test]
    fn all_quest_ron_files_have_spawn_chance_in_valid_range() {
        for (path, def) in load_all_quest_defs() {
            assert!(
                (0.0..=1.0).contains(&def.spawn_chance),
                "{}: spawn_chance {} 가 0.0~1.0 범위를 벗어났다",
                path, def.spawn_chance
            );
        }
    }

    #[test]
    fn parry_quest_item_ids_mapped_correctly() {
        let _ = qi();
        assert_eq!(item_id_to_kind("prototype_hammer", qi()), Some(ItemKind::QuestItem(QuestItemKind("prototype_hammer"))));
        assert_eq!(item_id_to_kind("steel_core", qi()),       Some(ItemKind::QuestItem(QuestItemKind("steel_core"))));
        assert_eq!(item_id_to_kind("pilot_badge", qi()),      Some(ItemKind::QuestItem(QuestItemKind("pilot_badge"))));
    }

    #[test]
    fn demonsword_item_ids_mapped_correctly() {
        let _ = qi();
        assert_eq!(item_id_to_kind("demon_sword", qi()),         Some(ItemKind::QuestItem(QuestItemKind("demon_sword"))));
        assert_eq!(item_id_to_kind("elenas_memo", qi()),         Some(ItemKind::QuestItem(QuestItemKind("elenas_memo"))));
        assert_eq!(item_id_to_kind("ancient_ritual_book", qi()), Some(ItemKind::QuestItem(QuestItemKind("ancient_ritual_book"))));
    }

    #[test]
    fn check_action_item_ids_detects_unknown_id() {
        let _ = qi();
        let bad_action = QuestAction::GiveItem("nonexistent_item".to_string());
        let mut errors = Vec::new();
        check_action_item_ids(&bad_action, "test_quest", "phase1", qi(), &mut errors);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("nonexistent_item"));
    }

    #[test]
    fn check_action_item_ids_passes_known_ids() {
        let _ = qi();
        let good_action = QuestAction::GiveItem("eternal_gem".to_string());
        let mut errors = Vec::new();
        check_action_item_ids(&good_action, "test_quest", "phase1", qi(), &mut errors);
        assert!(errors.is_empty());
    }

    #[test]
    fn check_action_item_ids_recurses_into_branches() {
        let _ = qi();
        let action = QuestAction::Branch {
            condition: Box::new(QuestCondition::HasFlag("test".into())),
            if_true: vec![QuestAction::GiveItem("invalid_item_id".into())],
            if_false: vec![],
        };
        let mut errors = Vec::new();
        check_action_item_ids(&action, "q", "p", qi(), &mut errors);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("invalid_item_id"));
    }
}
