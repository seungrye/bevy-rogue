use bevy::prelude::*;
use rand::Rng;
use std::collections::{HashMap, HashSet};
use serde::Deserialize;
use crate::modules::{
    item::{PlayerInventory, ItemKind, QuestItemKind, InventoryItem, Item, ItemSystemSet},
    map::{MapResource, TILE_SIZE, tile_to_world_coords, UsedSpawnTiles, random_floor_tile_anywhere},
    ui::minimap::{DiscoveredMarkers, MarkerKind},
    zone::{ZoneId, SpawnQuestPortalEvent, CloseQuestPortalEvent},
};

// ── RON 데이터 구조 (assets/quests/*.ron) ────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub struct QuestDef {
    pub id: String,
    pub title: String,
    pub giver_npc: String,
    pub initial_phase: String,
    pub phases: HashMap<String, QuestPhaseDef>,
    /// 순서 있는 상태 전환 규칙 목록. 각 trigger 유형별로 첫 번째 매칭 규칙만 실행.
    #[serde(default)]
    pub transitions: Vec<QuestTransition>,
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
    pub objective: Option<String>,
}

/// NPC 상호작용 또는 자동 조건 체크로 발동하는 상태 전환 규칙.
/// `transitions` 목록 순서대로 평가하며 첫 번째 매칭 규칙만 실행한다.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename = "Transition")]
pub struct QuestTransition {
    pub from: String,
    pub trigger: TriggerKind,
    /// 없으면 항상 매칭 (unconditional)
    #[serde(default)]
    pub when: Option<QuestCondition>,
    /// 전환 시 실행할 사이드이펙트 액션 목록. Auto trigger 는 DespawnWorldItem/RemoveItem/SetFlag 만 허용.
    #[serde(default)]
    pub actions: Vec<QuestAction>,
    pub to: String,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub enum TriggerKind {
    /// NPC 마지막 대사 이후 플레이어 interact 시 평가
    Interact,
    /// 매 프레임 조건 자동 평가
    Auto,
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

/// 포털을 어디에 스폰할지 결정한다.
/// 기본값은 `InsideRoom` — 기존 동작 유지로 호환성 보장.
#[derive(Debug, Deserialize, Clone, Default)]
pub enum PortalPlacement {
    /// 랜덤 방의 floor (기존 StairDown 위치 결정 동작).
    #[default]
    InsideRoom,
    /// 맵 외곽선에서 가장 가까운 floor — 마을·야외 맵 입구로 자연스럽다.
    Border,
    /// 맵 전체 floor 중 임의 위치.
    Random,
    /// 퀘스트 giver NPC 의 반경 `radius` 타일 안 floor.
    /// giver 위치를 못 찾으면 `InsideRoom` 으로 fallback 한다.
    NearGiver { radius: usize },
}

#[derive(Debug, Deserialize, Clone)]
pub enum QuestAction {
    GiveItem(String),
    RemoveItem(String),
    Log(String),
    SetFlag { flag: String, value: String },
    ClearFlag(String),
    KillNpc(String),
    /// 현재 존에 Named 존으로 이어지는 포탈을 즉시 스폰한다.
    /// `placement` 미지정 시 `InsideRoom` (기본).
    OpenPortal {
        zone: String,
        generator: String,
        #[serde(default)]
        placement: PortalPlacement,
    },
    /// Named 존으로 가는 포탈과 그 zone 등록을 모두 닫는다 (퀘스트 종료 시 정리)
    ClosePortal(String),
    /// 아이템을 수량 지정하여 지급
    GiveItems { item: String, count: u32 },
    /// 월드에 놓인 아이템 엔티티를 즉시 제거한다 (인벤토리는 건들지 않음)
    DespawnWorldItem(String),
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

    /// `giver_npc == villager_id` 인 퀘스트 (active 무관) 의 (id, def) 반환.
    /// villager 가 어떤 quest 의 giver 인지 확인할 때 사용 — VillagerDef.quest_id
    /// 필드를 대체. unique 가정 (한 NPC 가 여러 quest 의 giver 면 첫 매치).
    pub fn quest_for_giver<'a>(&'a self, villager_id: &str) -> Option<(&'a str, &'a QuestDef)> {
        self.quests.iter()
            .find(|(_, q)| q.giver_npc == villager_id)
            .map(|(id, def)| (id.as_str(), def))
    }

    /// `giver_npc == villager_id` 이면서 active 인 퀘스트의 id. 활성 퀘스트
    /// dialog/글리프 분기에 사용.
    pub fn active_quest_for_giver<'a>(&'a self, villager_id: &str) -> Option<&'a str> {
        self.quests.iter()
            .find(|(qid, q)| q.giver_npc == villager_id && self.is_quest_active(qid))
            .map(|(id, _)| id.as_str())
    }
}

#[derive(Resource, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct QuestState {
    pub phases: HashMap<String, String>,            // quest_id → current_phase_id
    pub spawned: std::collections::HashSet<String>, // "quest_id:item_id" — per-quest idempotency
    /// zone-단위 dedup: "{zone:?}:{item_id}". 여러 퀘스트가 같은 (zone, item) 에 spawn
    /// 시도 시 첫 한 퀘스트만 실제 spawn, 나머지는 skip. 직교성 보장.
    /// `#[serde(default)]` — legacy 세이브 호환.
    #[serde(default)]
    pub zone_spawned: std::collections::HashSet<String>,
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

    /// zone-단위 spawn 키 — `format!("{:?}:{}", zone, item_id)`.
    fn zone_spawn_key(zone: &crate::modules::zone::ZoneId, item_id: &str) -> String {
        format!("{:?}:{}", zone, item_id)
    }

    pub fn is_zone_spawn_done(&self, zone: &crate::modules::zone::ZoneId, item_id: &str) -> bool {
        self.zone_spawned.contains(&Self::zone_spawn_key(zone, item_id))
    }

    pub fn mark_zone_spawned(&mut self, zone: &crate::modules::zone::ZoneId, item_id: &str) {
        self.zone_spawned.insert(Self::zone_spawn_key(zone, item_id));
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
        // 모든 transition 의 actions 탐색
        for transition in &qdef.transitions {
            for action in &transition.actions {
                check_action_item_ids(action, qid, &transition.from, &quest_items, &mut errors);
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

    // transitions 검증
    for (i, t) in def.transitions.iter().enumerate() {
        if !def.phases.contains_key(&t.from) {
            errors.push(format!("transitions[{}]: from '{}' 이 phases 에 없습니다", i, t.from));
        }
        if !def.phases.contains_key(&t.to) {
            errors.push(format!("transitions[{}]: to '{}' 이 phases 에 없습니다", i, t.to));
        }
        // Auto trigger 는 일부 액션만 허용
        if t.trigger == TriggerKind::Auto {
            for action in &t.actions {
                if !is_auto_action_supported(action) {
                    errors.push(format!(
                        "transitions[{}] (Auto, from '{}'): 지원하지 않는 액션 {:?}",
                        i, t.from, action
                    ));
                }
            }
        }
        // 액션 내 아이템 ID 검증
        for action in &t.actions {
            collect_action_errors(action, &t.from, quest_items, &mut errors);
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
    phase_id: &str,
    quest_items: &crate::modules::item::QuestItemRegistry,
    errors: &mut Vec<String>,
) {
    match action {
        QuestAction::GiveItem(id) | QuestAction::GiveItems { item: id, .. }
        | QuestAction::RemoveItem(id) | QuestAction::DespawnWorldItem(id) => {
            if item_id_to_kind(id, quest_items).is_none() {
                errors.push(format!(
                    "phase '{}': item_id '{}' 를 인식할 수 없습니다", phase_id, id
                ));
            }
        }
        _ => {}
    }
}

fn is_auto_action_supported(action: &QuestAction) -> bool {
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
        // 현재 phase 에서 시작하는 Auto 트리거 transition 순서대로 평가, 첫 매칭만 실행
        for t in &quest_def.transitions {
            if t.from != current || t.trigger != TriggerKind::Auto { continue; }
            let condition_met = t.when.as_ref()
                .map(|c| eval_condition(c, &inventory, &world, &state, &quest_items))
                .unwrap_or(true);
            if condition_met {
                advances.push((quest_id.clone(), t.to.clone(), t.actions.clone()));
                break;
            }
        }
    }

    for (quest_id, next_phase, actions) in advances {
        info!("퀘스트 [{}] 자동 전진: {}", quest_id, next_phase);
        state.set_phase(&quest_id, &next_phase);
        // Auto trigger 허용 액션만 실행 (DespawnWorldItem, RemoveItem, SetFlag)
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
                _ => {} // Interact 전용 액션(OpenPortal, KillNpc 등)은 Auto trigger 에서 미지원
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
    kill_npc: &mut EventWriter<KillNpcEvent>,
    open_portal: &mut EventWriter<SpawnQuestPortalEvent>,
    close_portal: &mut EventWriter<CloseQuestPortalEvent>,
    despawn_item: &mut EventWriter<DespawnWorldItemEvent>,
    quest_items: &crate::modules::item::QuestItemRegistry,
) {
    for action in actions {
        match action {
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
            QuestAction::OpenPortal { zone, generator, placement } => {
                open_portal.send(SpawnQuestPortalEvent {
                    zone: zone.clone(),
                    generator: generator.clone(),
                    placement: placement.clone(),
                    quest_id: quest_id.to_string(),
                });
                log.send(crate::modules::ui::LogMessage(
                    format!("포탈이 열렸다 — {}.", zone)
                ));
                info!("퀘스트 포탈 열기: {} (생성기: {})", zone, generator);
            }
            QuestAction::ClosePortal(zone) => {
                close_portal.send(CloseQuestPortalEvent { zone: zone.clone() });
                log.send(crate::modules::ui::LogMessage(
                    format!("포탈이 닫혔다 — {}.", zone)
                ));
                info!("퀘스트 포탈 닫기: {}", zone);
            }
            QuestAction::DespawnWorldItem(item_id) => {
                despawn_item.send(DespawnWorldItemEvent(item_id.clone()));
                info!("월드 아이템 제거: {}", item_id);
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
            // zone-단위 dedup: 다른 퀘스트가 같은 (zone, item) 을 이미 spawn 했으면 skip.
            // 같은 인스턴스가 두 퀘스트의 HasItem 을 모두 충족.
            if state.is_zone_spawn_done(&world.current, &spawn.item) {
                state.mark_spawned(quest_id, &spawn.item);
                continue;
            }

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
            state.mark_zone_spawned(&world.current, &spawn.item);
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
                    objective: None,
                });
                m.insert("active".into(), QuestPhaseDef {
                    dialog: vec!["아직".into()],
                    objective: Some("영원의 보석을 찾아라".into()),
                });
                m.insert("ready".into(), QuestPhaseDef {
                    dialog: vec!["보석을 가져왔군!".into()],
                    objective: None,
                });
                m
            },
            transitions: vec![
                QuestTransition {
                    from: "not_started".into(),
                    trigger: TriggerKind::Interact,
                    when: None,
                    actions: vec![],
                    to: "active".into(),
                },
                QuestTransition {
                    from: "active".into(),
                    trigger: TriggerKind::Auto,
                    when: Some(QuestCondition::HasItem("eternal_gem".into())),
                    actions: vec![],
                    to: "ready".into(),
                },
            ],
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

    #[test]
    fn zone_spawn_tracking_dedups_across_quests() {
        use crate::modules::zone::ZoneId;
        let mut state = QuestState::default();
        let dungeon2 = ZoneId::Dungeon(2);
        assert!(!state.is_zone_spawn_done(&dungeon2, "eternal_gem"));
        // gem_quest 가 먼저 spawn — zone 마크.
        state.mark_zone_spawned(&dungeon2, "eternal_gem");
        // world_fracture 가 같은 zone+item 시도 → 이미 spawn 됨으로 인식.
        assert!(state.is_zone_spawn_done(&dungeon2, "eternal_gem"));
    }

    #[test]
    fn zone_spawn_separate_for_different_zones() {
        use crate::modules::zone::ZoneId;
        let mut state = QuestState::default();
        state.mark_zone_spawned(&ZoneId::Dungeon(1), "ancient_scroll");
        // 같은 item 이라도 다른 zone 은 별도.
        assert!(state.is_zone_spawn_done(&ZoneId::Dungeon(1), "ancient_scroll"));
        assert!(!state.is_zone_spawn_done(&ZoneId::Forest, "ancient_scroll"));
    }

    #[test]
    fn zone_spawn_separate_for_different_items() {
        use crate::modules::zone::ZoneId;
        let mut state = QuestState::default();
        state.mark_zone_spawned(&ZoneId::Dungeon(2), "eternal_gem");
        // 같은 zone 이라도 다른 item 은 별도.
        assert!(!state.is_zone_spawn_done(&ZoneId::Dungeon(2), "dragon_scale"));
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
        let state = QuestState::default();
        let transitions = vec![
            QuestTransition {
                from: "gathering".into(),
                trigger: TriggerKind::Auto,
                when: Some(QuestCondition::And(vec![
                    QuestCondition::HasItem("dragon_scale".into()),
                    QuestCondition::HasItem("ancient_scroll".into()),
                ])),
                actions: vec![],
                to: "both_ready".into(),
            },
            QuestTransition {
                from: "gathering".into(),
                trigger: TriggerKind::Auto,
                when: Some(QuestCondition::HasItem("dragon_scale".into())),
                actions: vec![],
                to: "has_scale_hint".into(),
            },
        ];
        let world = make_world();
        let inv_both = make_inventory_with(&["dragon_scale", "ancient_scroll"]);
        let matched = transitions.iter()
            .filter(|t| t.from == "gathering" && t.trigger == TriggerKind::Auto)
            .find(|t| t.when.as_ref().map(|c| eval_condition(c, &inv_both, &world, &state, qi())).unwrap_or(true))
            .map(|t| t.to.clone());
        assert_eq!(matched.as_deref(), Some("both_ready"), "둘 다 있으면 1순위가 선택돼야 한다");

        let inv_scale = make_inventory_with(&["dragon_scale"]);
        let matched2 = transitions.iter()
            .filter(|t| t.from == "gathering" && t.trigger == TriggerKind::Auto)
            .find(|t| t.when.as_ref().map(|c| eval_condition(c, &inv_scale, &world, &state, qi())).unwrap_or(true))
            .map(|t| t.to.clone());
        assert_eq!(matched2.as_deref(), Some("has_scale_hint"), "용비늘만 있으면 2순위가 선택돼야 한다");
    }

    #[test]
    fn ordered_interact_transitions_first_match_wins() {
        // both_ready 단계: 두 재료 있으면 1순위 normal_done, 없으면 fallback gathering
        let state = QuestState::default();
        let inv = make_inventory_with(&["dragon_scale", "ancient_scroll"]);
        let world = make_world();
        let transitions = vec![
            QuestTransition {
                from: "both_ready".into(),
                trigger: TriggerKind::Interact,
                when: Some(QuestCondition::And(vec![
                    QuestCondition::HasItem("dragon_scale".into()),
                    QuestCondition::HasItem("ancient_scroll".into()),
                ])),
                actions: vec![
                    QuestAction::RemoveItem("dragon_scale".into()),
                    QuestAction::RemoveItem("ancient_scroll".into()),
                    QuestAction::Log("정통 결말".into()),
                ],
                to: "normal_done".into(),
            },
            QuestTransition {
                from: "both_ready".into(),
                trigger: TriggerKind::Interact,
                when: None,
                actions: vec![QuestAction::Log("재료 부족".into())],
                to: "gathering".into(),
            },
        ];
        let matched = transitions.iter()
            .filter(|t| t.from == "both_ready" && t.trigger == TriggerKind::Interact)
            .find(|t| t.when.as_ref().map(|c| eval_condition(c, &inv, &world, &state, qi())).unwrap_or(true));
        assert!(matched.is_some());
        assert_eq!(matched.unwrap().to, "normal_done", "두 재료 있으면 1순위가 선택돼야 한다");

        let empty_inv = PlayerInventory::default();
        let matched2 = transitions.iter()
            .filter(|t| t.from == "both_ready" && t.trigger == TriggerKind::Interact)
            .find(|t| t.when.as_ref().map(|c| eval_condition(c, &empty_inv, &world, &state, qi())).unwrap_or(true));
        assert!(matched2.is_some());
        assert_eq!(matched2.unwrap().to, "gathering", "재료 없으면 unconditional fallback이 선택돼야 한다");
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
    fn transition_actions_field_defaults_to_empty() {
        let def: QuestDef = ron::de::from_str(r#"
            #![enable(implicit_some)]
            QuestDef(
                id: "test",
                title: "test",
                giver_npc: "npc",
                initial_phase: "start",
                phases: {
                    "start": QuestPhaseDef(dialog: []),
                    "done": QuestPhaseDef(dialog: []),
                },
                transitions: [
                    Transition(from: "start", trigger: Auto, when: HasItem("eternal_gem"), to: "done"),
                ],
            )
        "#).expect("RON 파싱 성공해야 한다");
        assert!(def.transitions[0].actions.is_empty(), "actions 미지정 시 빈 vec이어야 한다");
    }

    #[test]
    fn close_portal_action_parses_from_ron() {
        let def: QuestDef = ron::de::from_str(r#"
            QuestDef(
                id: "test", title: "test", giver_npc: "npc", initial_phase: "p1",
                phases: {
                    "p1": QuestPhaseDef(dialog: []),
                    "p2": QuestPhaseDef(dialog: []),
                },
                transitions: [
                    Transition(from: "p1", trigger: Interact, actions: [ClosePortal("d_rank_dungeon")], to: "p2"),
                ],
            )
        "#).expect("RON 파싱 성공");
        let actions = &def.transitions[0].actions;
        assert!(matches!(&actions[0], QuestAction::ClosePortal(z) if z == "d_rank_dungeon"));
    }

    #[test]
    fn auto_transition_actions_parsed_from_ron() {
        let def: QuestDef = ron::de::from_str(r#"
            #![enable(implicit_some)]
            QuestDef(
                id: "test",
                title: "test",
                giver_npc: "npc",
                initial_phase: "start",
                phases: {
                    "start": QuestPhaseDef(dialog: []),
                    "done": QuestPhaseDef(dialog: []),
                },
                transitions: [
                    Transition(
                        from: "start",
                        trigger: Auto,
                        when: HasItem("prologue_greatsword"),
                        actions: [
                            DespawnWorldItem("prologue_daggers"),
                            DespawnWorldItem("prologue_bowtorch"),
                        ],
                        to: "done",
                    ),
                ],
            )
        "#).expect("RON 파싱 성공해야 한다");
        let actions = &def.transitions[0].actions;
        assert_eq!(actions.len(), 2);
        assert!(matches!(&actions[0], QuestAction::DespawnWorldItem(id) if id == "prologue_daggers"));
        assert!(matches!(&actions[1], QuestAction::DespawnWorldItem(id) if id == "prologue_bowtorch"));
    }

    #[test]
    fn validate_rejects_unsupported_auto_transition_action() {
        let def: QuestDef = ron::de::from_str(r#"
            #![enable(implicit_some)]
            QuestDef(
                id: "test",
                title: "test",
                giver_npc: "npc",
                initial_phase: "start",
                phases: {
                    "start": QuestPhaseDef(dialog: []),
                    "done": QuestPhaseDef(dialog: []),
                },
                transitions: [
                    Transition(
                        from: "start",
                        trigger: Auto,
                        when: HasFlag("ready"),
                        actions: [OpenPortal(zone: "rift", generator: "bsp")],
                        to: "done",
                    ),
                ],
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
    fn validate_rejects_unknown_item_in_transition_action() {
        let def: QuestDef = ron::de::from_str(r#"
            QuestDef(
                id: "test", title: "test", giver_npc: "npc", initial_phase: "start",
                phases: {
                    "start": QuestPhaseDef(dialog: []),
                    "done": QuestPhaseDef(dialog: []),
                },
                transitions: [
                    Transition(from: "start", trigger: Interact, actions: [GiveItem("invalid_item_id")], to: "done"),
                ],
            )
        "#).expect("RON 파싱 성공해야 한다");
        let errors = validate_quest_def(&def, qi());
        assert!(errors.iter().any(|e| e.contains("invalid_item_id")));
    }
}
