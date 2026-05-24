use bevy::prelude::*;
use rand::Rng;
use std::collections::{HashMap, HashSet};
use serde::Deserialize;
use crate::modules::{
    item::{PlayerInventory, ItemKind, QuestItemKind, InventoryItem, Item, ItemSystemSet},
    map::{MapResource, TILE_SIZE, tile_to_world_coords, UsedSpawnTiles, random_floor_tile_anywhere, ExplosionEvent},
    ui::minimap::{DiscoveredMarkers, MarkerKind},
    zone::{ZoneId, SpawnQuestPortalEvent, CloseQuestPortalEvent},
    monster::{PlayerDetectedEvent, SpawnGuardEvent, SpawnMonsterEvent},
    trap::SpawnTrapEvent,
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
    /// 현재 맵에 가드를 `count` 마리 스폰한다 (잠입 구역 경비).
    /// monster 모듈의 `SpawnGuardEvent` 를 발행해 실제 스폰을 위임한다.
    SpawnGuards { count: u32 },
    /// 현재 맵에 특정 MonsterDef(`id`) 를 `count` 마리 스폰한다 (보스/퀘스트 전용).
    /// monster 모듈의 `SpawnMonsterEvent` 를 발행해 실제 스폰을 위임한다.
    SpawnMonster { id: String, count: u32 },
    /// 트리거 위치(플레이어/NPC 좌표) 기준으로 폭발을 일으킨다.
    /// map 모듈의 `ExplosionEvent` 를 발행해 지형 파괴·엔티티 피해를 위임한다.
    Explode { radius: i32, terrain: bool, entity_damage: i32 },
    /// 현재 맵에 함정을 `count` 개 배치한다 (잠입 구역의 경보 함정 등).
    /// trap 모듈의 `SpawnTrapEvent` 를 발행해 실제 배치를 위임한다.
    /// `hidden` 미지정 시 숨김(true) 함정으로 둔다.
    PlaceTraps {
        kind: crate::modules::trap::TrapKind,
        count: u32,
        #[serde(default = "default_trap_hidden")]
        hidden: bool,
    },
}

fn default_trap_hidden() -> bool { true }

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

    #[allow(dead_code)] // 테스트에서만 참조되는 공개 접근자 (프로덕션 미사용)
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
            // monster 모듈이 발행하는 이벤트 — MonsterPlugin 도 등록하지만
            // QuestPlugin 단독(테스트 등)에서도 동작하도록 여기서도 보장한다.
            .add_event::<PlayerDetectedEvent>()
            .add_event::<SpawnGuardEvent>()
            .add_event::<SpawnMonsterEvent>()
            .add_event::<SpawnTrapEvent>()
            .add_systems(Startup, (
                load_quests.in_set(QuestSystemSet::Load).after(ItemSystemSet::Load),
                validate_quest_item_refs
                    .after(QuestSystemSet::Load)
                    .after(ItemSystemSet::Load),
            ))
            .add_systems(Update, (
                check_auto_advance,
                handle_player_detected,
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
    let errors = collect_quest_item_ref_errors(&quest_registry, &quest_items);
    if !errors.is_empty() {
        // 도달 불가 방어코드 — 오류 수집 로직은 collect_quest_item_ref_errors 로 테스트.
        // 실제 에셋이 유효하므로 이 process::exit 분기는 테스트에서 도달 불가.
        for msg in &errors {
            error!("[치명적] {}", msg);
        }
        std::process::exit(1);
    }
}

/// 레지스트리의 모든 퀘스트 spawns/transition actions 가 참조하는 item ID 가
/// quest_items registry 에 존재하는지 검사하고 오류 메시지를 모아 반환한다.
/// `exit` 결정을 호출자에게 남기는 seam — 양쪽 분기를 테스트할 수 있다.
fn collect_quest_item_ref_errors(
    quest_registry: &QuestRegistry,
    quest_items: &crate::modules::item::QuestItemRegistry,
) -> Vec<String> {
    let mut errors: Vec<String> = Vec::new();
    for (qid, qdef) in &quest_registry.quests {
        // spawns
        for spawn in &qdef.spawns {
            if item_id_to_kind(&spawn.item, quest_items).is_none() {
                errors.push(format!(
                    "퀘스트 '{}' 의 spawns item_id '{}' 가 인식되지 않습니다",
                    qid, spawn.item
                ));
            }
        }
        // 모든 transition 의 actions 탐색
        for transition in &qdef.transitions {
            for action in &transition.actions {
                check_action_item_ids(action, qid, &transition.from, quest_items, &mut errors);
            }
        }
    }
    errors
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
    let quests = match read_quest_dir("assets/quests", &quest_items) {
        Ok(q) => q,
        // 도달 불가 방어코드 — read_quest_dir 의 Err 분기는 read_quest_dir 테스트로
        // 검증. 실제 에셋이 유효하므로 이 process::exit 분기는 테스트에서 도달 불가.
        Err(errors) => {
            for msg in &errors {
                error!("{}", msg);
            }
            error!("[치명적] 퀘스트 파일에 오류가 있습니다. 위 오류를 수정한 후 다시 실행하세요.");
            std::process::exit(1);
        }
    };

    for (_, def) in &quests {
        info!("퀘스트 로드: {} ({})", def.title, def.id);
    }

    // spawn_chance 확률로 이번 런에 활성화할 퀘스트 결정
    let mut rng = rand::thread_rng();
    let active = select_active_quests(&quests, &mut rng);
    for id in &active {
        info!("퀘스트 활성화: {}", id);
    }
    registry.quests = quests;
    registry.active = active;
}

/// `dir_path` 안의 모든 `.ron` 퀘스트를 읽어 파싱·시맨틱 검증한다.
/// 디렉터리 열기/파일 읽기/파싱/검증 중 하나라도 실패하면 모든 오류 메시지를
/// `Err` 로 모아 반환한다 (호출자가 `exit` 여부를 결정 — seam).
/// 고정 경로 대신 인자로 받아 임시 디렉터리로 양쪽 분기를 테스트할 수 있다.
fn read_quest_dir(
    dir_path: &str,
    quest_items: &crate::modules::item::QuestItemRegistry,
) -> Result<HashMap<String, QuestDef>, Vec<String>> {
    let Ok(dir) = std::fs::read_dir(dir_path) else {
        return Err(vec![format!(
            "[치명적] {} 디렉터리를 찾을 수 없습니다. 게임을 시작할 수 없습니다.",
            dir_path
        )]);
    };

    let mut quests: HashMap<String, QuestDef> = HashMap::new();
    let mut errors: Vec<String> = Vec::new();

    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ron") { continue; }

        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                errors.push(format!("[퀘스트 오류] {:?} 읽기 실패: {}", path, e));
                continue;
            }
        };

        let def = match ron::de::from_str::<QuestDef>(&text) {
            Ok(d) => d,
            Err(e) => {
                errors.push(format!("[퀘스트 오류] {:?} RON 파싱 실패:\n  {}", path, e));
                continue;
            }
        };

        // 시맨틱 검증
        let semantic_errors = validate_quest_def(&def, quest_items);
        if !semantic_errors.is_empty() {
            for msg in &semantic_errors {
                errors.push(format!("[퀘스트 오류] {:?} — {}", path, msg));
            }
            continue;
        }

        quests.insert(def.id.clone(), def);
    }

    if errors.is_empty() { Ok(quests) } else { Err(errors) }
}

/// 각 퀘스트를 `spawn_chance` 확률로 활성화해 ID 집합을 반환한다.
/// rand 의존부를 분리해 결정적 rng 로 양쪽 경계를 테스트할 수 있게 한다.
fn select_active_quests(
    quests: &HashMap<String, QuestDef>,
    rng: &mut impl Rng,
) -> HashSet<String> {
    quests.iter()
        .filter(|(_, def)| rng.gen::<f32>() < def.spawn_chance)
        .map(|(id, _)| id.clone())
        .collect()
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
    trigger_pos: (usize, usize),
    state: &mut QuestState,
    inventory: &mut PlayerInventory,
    log: &mut EventWriter<crate::modules::ui::LogMessage>,
    kill_npc: &mut EventWriter<KillNpcEvent>,
    open_portal: &mut EventWriter<SpawnQuestPortalEvent>,
    close_portal: &mut EventWriter<CloseQuestPortalEvent>,
    despawn_item: &mut EventWriter<DespawnWorldItemEvent>,
    spawn_guards: &mut EventWriter<SpawnGuardEvent>,
    spawn_monster: &mut EventWriter<SpawnMonsterEvent>,
    explode: &mut EventWriter<ExplosionEvent>,
    place_traps: &mut EventWriter<SpawnTrapEvent>,
    quest_items: &crate::modules::item::QuestItemRegistry,
) {
    for action in actions {
        match action {
            QuestAction::GiveItem(item_id) => {
                if let Some(kind) = item_id_to_kind(item_id, quest_items) {
                    inventory.items.push(InventoryItem::new(kind));
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
                            _ => inventory.items.push(InventoryItem::new(kind)),
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
            QuestAction::SpawnGuards { count } => {
                spawn_guards.send(SpawnGuardEvent { count: *count });
                info!("가드 스폰 요청: {}마리", count);
            }
            QuestAction::SpawnMonster { id, count } => {
                spawn_monster.send(SpawnMonsterEvent { id: id.clone(), count: *count });
                info!("몬스터 스폰 요청: {} {}마리", id, count);
            }
            QuestAction::Explode { radius, terrain, entity_damage } => {
                explode.send(ExplosionEvent {
                    center: trigger_pos,
                    radius: *radius,
                    terrain: *terrain,
                    entity_damage: *entity_damage,
                });
                info!(
                    "폭발 발생: 중심 {:?} 반경 {} (지형 {}, 피해 {})",
                    trigger_pos, radius, terrain, entity_damage
                );
            }
            QuestAction::PlaceTraps { kind, count, hidden } => {
                place_traps.send(SpawnTrapEvent { kind: *kind, count: *count, hidden: *hidden });
                info!("함정 배치 요청: {:?} {}개 (숨김 {})", kind, count, hidden);
            }
        }
    }
}

/// 가드가 플레이어를 탐지하면(`PlayerDetectedEvent`) 잠입 실패 플래그
/// `stealth_blown` 을 세운다. 한 프레임에 여러 탐지 이벤트가 와도 한 번만
/// 세우면 충분하므로 이벤트가 하나라도 있으면 플래그를 설정한다.
/// 기존 alert/추적/전투 흐름은 monster 모듈이 그대로 처리한다(B안).
fn handle_player_detected(
    mut events: EventReader<PlayerDetectedEvent>,
    mut state: ResMut<QuestState>,
) {
    if events.read().next().is_some() {
        state.set_flag("stealth_blown", "true");
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

                // 안전망: random_floor_tile_anywhere 가 통과타일만 반환해야 하지만
                // race condition / map 캐시 불일치 등으로 벽 좌표가 나올 가능성을 가드한다.
                // 도달 불가 방어코드 — random_floor_tile_anywhere 는 항상 통과타일만 반환한다.
                if !map.get_tile(tx, ty).is_walkable() {
                    error!("퀘스트 아이템 spawn 좌표 ({}, {}) 가 통과타일이 아님 — 스킵: {}", tx, ty, spawn.item);
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
                    Item { kind, tile_x: tx, tile_y: ty, rolled_attack: None, rolled_defense: None },
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
        // 그 외는 레지스트리에서 조회한다. quest item → consumable → weapon → armor
        // 순으로 시도해 첫 매칭 kind 를 반환한다. 이렇게 해야 퀘스트가 trap_kit/
        // disarm_tool 같은 일반 소모품·장비도 GiveItem/RemoveItem 으로 다룰 수 있다.
        other => quest_items.intern_quest_item(other).map(|s| ItemKind::QuestItem(QuestItemKind(s)))
            .or_else(|| quest_items.intern_consumable(other)
                .map(|s| ItemKind::Consumable(crate::modules::item::ConsumableKind(s))))
            .or_else(|| quest_items.intern_weapon(other)
                .map(|s| ItemKind::Weapon(crate::modules::item::WeaponKind(s))))
            .or_else(|| quest_items.intern_armor(other)
                .map(|s| ItemKind::Armor(crate::modules::item::ArmorKind(s)))),
    }
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
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
    fn 페이즈를_설정하면_현재페이즈로_조회된다() {
        let mut state = QuestState::default();
        assert!(state.current_phase("gem_quest").is_none());
        state.set_phase("gem_quest", "active");
        assert_eq!(state.current_phase("gem_quest"), Some("active"));
    }

    #[test]
    fn 내장_아이템ID와_퀘스트레지스트리_아이템ID가_올바른_kind로_매핑된다() {
        let _ = qi();
        assert_eq!(item_id_to_kind("eternal_gem", qi()),        Some(ItemKind::QuestItem(QuestItemKind("eternal_gem"))));
        assert_eq!(item_id_to_kind("philosophers_stone", qi()), Some(ItemKind::QuestItem(QuestItemKind("philosophers_stone"))));
        assert!(item_id_to_kind("unknown", qi()).is_none());
    }

    #[test]
    fn 퀘스트가_참조하는_일반소모품과_장비ID도_레지스트리에서_kind로_매핑된다() {
        // 퀘스트가 trap_kit/disarm_tool 같은 일반 소모품이나 일반 무기·방어구를
        // GiveItem/RemoveItem 으로 지급할 수 있도록, item_id_to_kind 가 quest item
        // 이외의 소모품/무기/방어구 레지스트리도 순서대로 조회해 매핑한다.
        use crate::modules::item::{ConsumableKind, WeaponKind, ArmorKind};
        let _ = qi();
        // 소모품 분기 (health_potion 이외)
        assert_eq!(item_id_to_kind("trap_kit", qi()),    Some(ItemKind::Consumable(ConsumableKind("trap_kit"))));
        assert_eq!(item_id_to_kind("disarm_tool", qi()), Some(ItemKind::Consumable(ConsumableKind("disarm_tool"))));
        // 무기 분기 (sword/spear/bow 이외)
        assert_eq!(item_id_to_kind("dagger", qi()),      Some(ItemKind::Weapon(WeaponKind("dagger"))));
        // 방어구 분기 (leather_armor 이외)
        assert_eq!(item_id_to_kind("plate_armor", qi()), Some(ItemKind::Armor(ArmorKind("plate_armor"))));
    }

    #[test]
    fn 레지스트리는_존재하는_페이즈는_찾고_없는_페이즈는_None을_반환한다() {
        let reg = make_registry_with_gem_quest();
        assert!(reg.phase("gem_quest", "not_started").is_some());
        assert!(reg.phase("gem_quest", "missing").is_none());
        assert!(reg.phase("no_quest", "x").is_none());
    }

    #[test]
    fn 스폰을_마크하면_해당_퀘스트아이템은_스폰완료로_기록된다() {
        let mut state = QuestState::default();
        assert!(!state.is_spawn_done("gem_quest", "eternal_gem"));
        state.mark_spawned("gem_quest", "eternal_gem");
        assert!(state.is_spawn_done("gem_quest", "eternal_gem"));
    }

    #[test]
    fn 존스폰을_마크하면_다른_퀘스트의_같은_존아이템도_중복스폰되지_않는다() {
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
    fn 같은_아이템이라도_존이_다르면_존스폰은_별개로_추적된다() {
        use crate::modules::zone::ZoneId;
        let mut state = QuestState::default();
        state.mark_zone_spawned(&ZoneId::Dungeon(1), "ancient_scroll");
        // 같은 item 이라도 다른 zone 은 별도.
        assert!(state.is_zone_spawn_done(&ZoneId::Dungeon(1), "ancient_scroll"));
        assert!(!state.is_zone_spawn_done(&ZoneId::Forest, "ancient_scroll"));
    }

    #[test]
    fn 같은_존이라도_아이템이_다르면_존스폰은_별개로_추적된다() {
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
                inv.items.push(InventoryItem::new(kind));
            }
        }
        inv
    }

    #[test]
    fn And조건은_모든_하위조건이_충족돼야_참이_된다() {
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
    fn Or조건은_하위조건_하나라도_충족되면_참이_된다() {
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
    fn Not조건은_하위조건의_참거짓을_뒤집는다() {
        let inv = make_inventory_with(&["dragon_scale"]);
        let state = QuestState::default();
        let world = make_world();
        let cond = QuestCondition::Not(Box::new(QuestCondition::HasItem("ancient_scroll".into())));
        assert!(eval_condition(&cond, &inv, &world, &state, qi()));
        let cond2 = QuestCondition::Not(Box::new(QuestCondition::HasItem("dragon_scale".into())));
        assert!(!eval_condition(&cond2, &inv, &world, &state, qi()));
    }

    #[test]
    fn PhaseIs조건은_퀘스트상태의_현재페이즈와_일치할때만_참이_된다() {
        let inv = PlayerInventory::default();
        let mut state = QuestState::default();
        let world = make_world();
        let cond = QuestCondition::PhaseIs { quest: "gem_quest".into(), phase: "done".into() };
        assert!(!eval_condition(&cond, &inv, &world, &state, qi()));
        state.set_phase("gem_quest", "done");
        assert!(eval_condition(&cond, &inv, &world, &state, qi()));
    }

    #[test]
    fn Auto전이는_우선순위가_높은_첫_매칭_규칙이_선택된다() {
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
    fn 순서있는_Interact전이는_첫_매칭이_없으면_무조건_fallback이_선택된다() {
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
    fn 추가된_퀘스트아이템ID들이_올바른_kind로_매핑된다() {
        let _ = qi();
        assert_eq!(item_id_to_kind("dragon_scale", qi()),   Some(ItemKind::QuestItem(QuestItemKind("dragon_scale"))));
        assert_eq!(item_id_to_kind("ancient_scroll", qi()), Some(ItemKind::QuestItem(QuestItemKind("ancient_scroll"))));
    }

    #[test]
    fn 플래그는_설정_조회_해제가_일관되게_동작한다() {
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
    fn FlagIs조건은_플래그값이_일치할때만_참이_된다() {
        let inv = PlayerInventory::default();
        let world = make_world();
        let mut state = QuestState::default();
        let cond = QuestCondition::FlagIs { flag: "npc_alive".to_string(), value: "true".to_string() };
        assert!(!eval_condition(&cond, &inv, &world, &state, qi()));
        state.set_flag("npc_alive", "true");
        assert!(eval_condition(&cond, &inv, &world, &state, qi()));
    }

    #[test]
    fn HasFlag조건은_플래그_존재여부로_참거짓이_갈린다() {
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
    fn 전이의_actions를_생략하면_빈_목록으로_파싱된다() {
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
    fn ClosePortal액션이_RON에서_올바르게_파싱된다() {
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
    fn Auto전이의_actions가_RON에서_순서대로_파싱된다() {
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
    fn 검증은_Auto전이에서_지원하지않는_액션을_거부한다() {
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
    fn 검증은_스폰조건에_HasItem을_사용하면_거부한다() {
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
    fn 모든_퀘스트파일이_오류없이_파싱된다() {
        // 파일 존재 + RON 파싱 성공 여부만 검증
        let defs = load_all_quest_defs();
        assert!(defs.len() >= 4, "prologue + 3 route 퀘스트 최소 4개여야 한다");
    }

    #[test]
    fn 모든_퀘스트파일이_시맨틱검증을_통과한다() {
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
    fn 모든_퀘스트파일의_스폰_아이템ID가_인식된다() {
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

    /// 지정 퀘스트 RON 의 not_started→infiltrating 수락 전이가 정찰 도구를 지급하는지 검사.
    fn accept_transition_gives_scout_lens(quest_file: &str) -> bool {
        let text = std::fs::read_to_string(quest_file)
            .unwrap_or_else(|e| panic!("{} 읽기 실패: {}", quest_file, e));
        let def: QuestDef = ron::de::from_str(&text)
            .unwrap_or_else(|e| panic!("{} 파싱 실패: {}", quest_file, e));
        def.transitions.iter()
            .filter(|t| t.from == "not_started" && t.to == "infiltrating")
            .flat_map(|t| t.actions.iter())
            .any(|a| matches!(a, QuestAction::GiveItem(id) if id == "scout_lens"))
    }

    #[test]
    fn 잠입_퀘스트_수락은_정찰도구_올빼미안경을_지급한다() {
        assert!(
            accept_transition_gives_scout_lens("assets/quests/infiltration_quest.ron"),
            "infiltration_quest 수락 전이는 GiveItem(scout_lens) 를 포함해야 한다"
        );
        assert!(
            accept_transition_gives_scout_lens("assets/quests/vault_heist_quest.ron"),
            "vault_heist_quest 수락 전이는 GiveItem(scout_lens) 를 포함해야 한다"
        );
    }

    #[test]
    fn 레지스트리는_active집합에_있는_퀘스트만_활성으로_판정한다() {
        let mut reg = QuestRegistry::default();
        reg.active.insert("gem_quest".to_string());
        assert!(reg.is_quest_active("gem_quest"));
        assert!(!reg.is_quest_active("herb_quest"));
    }

    #[test]
    fn RON에서_spawn_chance를_생략하면_기본값_1로_파싱된다() {
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
    fn RON에_명시한_spawn_chance값이_그대로_파싱된다() {
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
    fn 모든_퀘스트파일의_spawn_chance가_유효범위_안에_있다() {
        for (path, def) in load_all_quest_defs() {
            assert!(
                (0.0..=1.0).contains(&def.spawn_chance),
                "{}: spawn_chance {} 가 0.0~1.0 범위를 벗어났다",
                path, def.spawn_chance
            );
        }
    }

    #[test]
    fn 패리퀘스트_아이템ID들이_올바른_kind로_매핑된다() {
        let _ = qi();
        assert_eq!(item_id_to_kind("prototype_hammer", qi()), Some(ItemKind::QuestItem(QuestItemKind("prototype_hammer"))));
        assert_eq!(item_id_to_kind("steel_core", qi()),       Some(ItemKind::QuestItem(QuestItemKind("steel_core"))));
        assert_eq!(item_id_to_kind("pilot_badge", qi()),      Some(ItemKind::QuestItem(QuestItemKind("pilot_badge"))));
    }

    #[test]
    fn 마검퀘스트_아이템ID들이_올바른_kind로_매핑된다() {
        let _ = qi();
        assert_eq!(item_id_to_kind("demon_sword", qi()),         Some(ItemKind::QuestItem(QuestItemKind("demon_sword"))));
        assert_eq!(item_id_to_kind("elenas_memo", qi()),         Some(ItemKind::QuestItem(QuestItemKind("elenas_memo"))));
        assert_eq!(item_id_to_kind("ancient_ritual_book", qi()), Some(ItemKind::QuestItem(QuestItemKind("ancient_ritual_book"))));
    }

    #[test]
    fn 액션아이템ID검사는_미등록ID를_오류로_감지한다() {
        let _ = qi();
        let bad_action = QuestAction::GiveItem("nonexistent_item".to_string());
        let mut errors = Vec::new();
        check_action_item_ids(&bad_action, "test_quest", "phase1", qi(), &mut errors);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("nonexistent_item"));
    }

    #[test]
    fn 액션아이템ID검사는_등록된ID는_통과시킨다() {
        let _ = qi();
        let good_action = QuestAction::GiveItem("eternal_gem".to_string());
        let mut errors = Vec::new();
        check_action_item_ids(&good_action, "test_quest", "phase1", qi(), &mut errors);
        assert!(errors.is_empty());
    }

    #[test]
    fn 검증은_전이액션에_미등록_아이템ID가_있으면_거부한다() {
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

    // ── eval_condition: InZone ──────────────────────────────────────────────

    #[test]
    fn InZone조건은_플레이어가_해당존에_있을때만_참이_된다() {
        use crate::modules::zone::ZoneId;
        let inv = PlayerInventory::default();
        let state = QuestState::default();
        let mut world = make_world(); // 기본 current = Town
        let cond = QuestCondition::InZone(ZoneId::Dungeon(2));
        assert!(!eval_condition(&cond, &inv, &world, &state, qi()), "다른 존이면 거짓");
        world.current = ZoneId::Dungeon(2);
        assert!(eval_condition(&cond, &inv, &world, &state, qi()), "같은 존이면 참");
    }

    #[test]
    fn HasItem조건은_등록되지않은_아이템ID면_거짓을_반환한다() {
        let inv = make_inventory_with(&["dragon_scale"]);
        let state = QuestState::default();
        let world = make_world();
        // 미등록 ID → item_id_to_kind None → early return false
        let cond = QuestCondition::HasItem("totally_unknown_item".into());
        assert!(!eval_condition(&cond, &inv, &world, &state, qi()));
    }

    // ── condition_uses_inventory: And/Or/Not 재귀 분기 ────────────────────────

    #[test]
    fn condition_uses_inventory는_중첩된_HasItem을_모든_조합자에서_탐지한다() {
        use crate::modules::zone::ZoneId;
        // 단일 HasItem
        assert!(condition_uses_inventory(&QuestCondition::HasItem("x".into())));
        // And 안에 HasItem
        assert!(condition_uses_inventory(&QuestCondition::And(vec![
            QuestCondition::HasFlag("f".into()),
            QuestCondition::HasItem("x".into()),
        ])));
        // Or 안에 HasItem
        assert!(condition_uses_inventory(&QuestCondition::Or(vec![
            QuestCondition::HasItem("x".into()),
        ])));
        // Not 안에 HasItem
        assert!(condition_uses_inventory(&QuestCondition::Not(Box::new(
            QuestCondition::HasItem("x".into())
        ))));
        // HasItem 이 전혀 없으면 false (And/Or/Not 의 false 분기)
        assert!(!condition_uses_inventory(&QuestCondition::And(vec![
            QuestCondition::HasFlag("f".into()),
            QuestCondition::InZone(ZoneId::Town),
        ])));
        assert!(!condition_uses_inventory(&QuestCondition::Not(Box::new(
            QuestCondition::HasFlag("f".into())
        ))));
        // 인벤토리와 무관한 단일 조건 (_=> false arm)
        assert!(!condition_uses_inventory(&QuestCondition::HasFlag("f".into())));
    }

    // ── check_action_item_ids: 나머지 액션 변형 + _ arm ──────────────────────

    #[test]
    fn 액션아이템ID검사는_GiveItems_RemoveItem_DespawnWorldItem도_검사한다() {
        let _ = qi();
        for action in [
            QuestAction::GiveItems { item: "bad_id".into(), count: 2 },
            QuestAction::RemoveItem("bad_id".into()),
            QuestAction::DespawnWorldItem("bad_id".into()),
        ] {
            let mut errors = Vec::new();
            check_action_item_ids(&action, "q", "p", qi(), &mut errors);
            assert_eq!(errors.len(), 1, "{:?} 미등록 ID 오류 1개", action);
            assert!(errors[0].contains("bad_id"));
        }
    }

    #[test]
    fn 액션아이템ID검사는_아이템과_무관한_액션은_무시한다() {
        let _ = qi();
        // _ => {} arm: Log 는 아이템 ID 가 없어 검사 대상 아님
        let mut errors = Vec::new();
        check_action_item_ids(&QuestAction::Log("hi".into()), "q", "p", qi(), &mut errors);
        assert!(errors.is_empty());
    }

    // ── validate_quest_def: 미커버 오류 분기 ─────────────────────────────────

    #[test]
    fn 검증은_initial_phase가_phases에_없으면_거부한다() {
        let def: QuestDef = ron::de::from_str(r#"
            QuestDef(
                id: "t", title: "t", giver_npc: "n", initial_phase: "missing",
                phases: { "start": QuestPhaseDef(dialog: []) },
            )
        "#).expect("RON 파싱 성공");
        let errors = validate_quest_def(&def, qi());
        assert!(errors.iter().any(|e| e.contains("initial_phase")), "{:?}", errors);
    }

    #[test]
    fn 검증은_전이의_from과_to가_phases에_없으면_각각_거부한다() {
        let def: QuestDef = ron::de::from_str(r#"
            QuestDef(
                id: "t", title: "t", giver_npc: "n", initial_phase: "start",
                phases: { "start": QuestPhaseDef(dialog: []) },
                transitions: [
                    Transition(from: "nowhere", trigger: Interact, to: "alsonowhere"),
                ],
            )
        "#).expect("RON 파싱 성공");
        let errors = validate_quest_def(&def, qi());
        assert!(errors.iter().any(|e| e.contains("from 'nowhere'")), "{:?}", errors);
        assert!(errors.iter().any(|e| e.contains("to 'alsonowhere'")), "{:?}", errors);
    }

    #[test]
    fn 검증은_스폰의_아이템ID와_페이즈가_없으면_각각_거부한다() {
        let def: QuestDef = ron::de::from_str(r#"
            QuestDef(
                id: "t", title: "t", giver_npc: "n", initial_phase: "start",
                phases: { "start": QuestPhaseDef(dialog: []) },
                spawns: [
                    QuestSpawn(phase: "no_such_phase", item: "unknown_item_id", zone: Town),
                ],
            )
        "#).expect("RON 파싱 성공");
        let errors = validate_quest_def(&def, qi());
        assert!(errors.iter().any(|e| e.contains("unknown_item_id")), "{:?}", errors);
        assert!(errors.iter().any(|e| e.contains("no_such_phase")), "{:?}", errors);
    }

    #[test]
    fn 검증은_정상적인_퀘스트정의는_오류없이_통과시킨다() {
        // 모든 검증 분기의 "통과" 쪽을 한 번에 커버
        let def: QuestDef = ron::de::from_str(r#"
            #![enable(implicit_some)]
            QuestDef(
                id: "t", title: "t", giver_npc: "n", initial_phase: "start",
                phases: {
                    "start": QuestPhaseDef(dialog: []),
                    "done": QuestPhaseDef(dialog: []),
                },
                transitions: [
                    Transition(from: "start", trigger: Auto, when: HasFlag("ready"),
                        actions: [SetFlag(flag: "x", value: "1")], to: "done"),
                ],
                spawns: [
                    QuestSpawn(phase: "start", item: "eternal_gem", zone: Town,
                        condition: InZone(Town)),
                ],
            )
        "#).expect("RON 파싱 성공");
        let errors = validate_quest_def(&def, qi());
        assert!(errors.is_empty(), "{:?}", errors);
    }

    // ── read_quest_dir / select_active_quests (load_quests seam) ──────────────

    /// 임시 디렉터리를 만들고 콜백 안에서만 사용한 뒤 정리한다.
    /// 실제 assets/quests 는 절대 건드리지 않는다.
    fn with_temp_quest_dir(files: &[(&str, &str)], f: impl FnOnce(&str)) {
        let base = std::env::temp_dir().join(format!(
            "bevyrogue_quest_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&base).expect("임시 디렉터리 생성");
        for (name, content) in files {
            std::fs::write(base.join(name), content).expect("임시 파일 쓰기");
        }
        let path = base.to_string_lossy().into_owned();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&path)));
        let _ = std::fs::remove_dir_all(&base);
        if let Err(e) = result { std::panic::resume_unwind(e); }
    }

    #[test]
    fn 디렉터리에서_정상_퀘스트RON을_읽으면_레지스트리에_적재된다() {
        let _ = qi();
        with_temp_quest_dir(&[
            ("a.ron", r#"
                QuestDef(id: "a", title: "A", giver_npc: "n", initial_phase: "s",
                    phases: { "s": QuestPhaseDef(dialog: []) })
            "#),
            // .ron 이 아닌 파일은 무시되는 분기도 커버
            ("readme.txt", "ignore me"),
        ], |dir| {
            let quests = read_quest_dir(dir, qi()).expect("정상 로드 성공");
            assert_eq!(quests.len(), 1);
            assert!(quests.contains_key("a"));
        });
    }

    #[test]
    fn 존재하지않는_디렉터리를_읽으면_치명적_오류를_반환한다() {
        let _ = qi();
        let missing = std::env::temp_dir()
            .join("bevyrogue_quest_does_not_exist_xyz_12345");
        let _ = std::fs::remove_dir_all(&missing);
        let result = read_quest_dir(&missing.to_string_lossy(), qi());
        let errors = result.expect_err("없는 디렉터리는 Err");
        assert!(errors[0].contains("디렉터리를 찾을 수 없습니다"));
    }

    #[test]
    fn RON_파싱에_실패한_파일이_있으면_오류를_반환한다() {
        let _ = qi();
        with_temp_quest_dir(&[
            ("broken.ron", "this is not valid ron )))("),
        ], |dir| {
            let errors = read_quest_dir(dir, qi()).expect_err("파싱 실패는 Err");
            assert!(errors.iter().any(|e| e.contains("RON 파싱 실패")), "{:?}", errors);
        });
    }

    #[test]
    fn 시맨틱검증에_실패한_파일이_있으면_오류를_반환한다() {
        let _ = qi();
        with_temp_quest_dir(&[
            // initial_phase 가 phases 에 없음 → 시맨틱 오류
            ("bad.ron", r#"
                QuestDef(id: "b", title: "B", giver_npc: "n", initial_phase: "ghost",
                    phases: { "s": QuestPhaseDef(dialog: []) })
            "#),
        ], |dir| {
            let errors = read_quest_dir(dir, qi()).expect_err("시맨틱 실패는 Err");
            assert!(errors.iter().any(|e| e.contains("initial_phase")), "{:?}", errors);
        });
    }

    #[test]
    fn ron확장자_경로가_읽기에_실패하면_읽기실패_오류를_반환한다() {
        let _ = qi();
        // ".ron" 으로 끝나는 *디렉터리* 를 만들면 read_to_string 이 실패한다 (IsADirectory).
        let base = std::env::temp_dir().join(format!(
            "bevyrogue_readfail_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(base.join("trap.ron")).expect("디렉터리형 .ron 생성");
        let result = read_quest_dir(&base.to_string_lossy(), qi());
        let _ = std::fs::remove_dir_all(&base);
        let errors = result.expect_err("읽기 실패는 Err");
        assert!(errors.iter().any(|e| e.contains("읽기 실패")), "{:?}", errors);
    }

    #[test]
    fn 아이템참조검사는_미등록ID는_오류로_수집하고_정상이면_빈목록을_반환한다() {
        let _ = qi();
        // 정상: 등록된 spawn item + 정상 action
        let mut ok_reg = make_registry_with_gem_quest();
        ok_reg.quests.get_mut("gem_quest").unwrap().spawns.push(QuestSpawn {
            phase: "active".into(), item: "eternal_gem".into(),
            zone: crate::modules::zone::ZoneId::Dungeon(2), count: 1, condition: None,
        });
        assert!(collect_quest_item_ref_errors(&ok_reg, qi()).is_empty(), "정상이면 빈 목록");

        // 비정상: 미등록 spawn item + 미등록 transition action item
        let mut bad_reg = make_registry_with_gem_quest();
        {
            let q = bad_reg.quests.get_mut("gem_quest").unwrap();
            q.spawns.push(QuestSpawn {
                phase: "active".into(), item: "no_such_spawn".into(),
                zone: crate::modules::zone::ZoneId::Town, count: 1, condition: None,
            });
            q.transitions.push(QuestTransition {
                from: "active".into(), trigger: TriggerKind::Interact, when: None,
                actions: vec![QuestAction::GiveItem("no_such_action_item".into())],
                to: "ready".into(),
            });
        }
        let errors = collect_quest_item_ref_errors(&bad_reg, qi());
        assert!(errors.iter().any(|e| e.contains("no_such_spawn")), "{:?}", errors);
        assert!(errors.iter().any(|e| e.contains("no_such_action_item")), "{:?}", errors);
    }

    #[test]
    fn load_quests시스템은_실제_에셋을_읽어_레지스트리를_채운다() {
        // 실제 assets/quests 는 모두 유효하므로 exit 분기 없이 성공 경로만 실행된다.
        let mut app = App::new();
        app.insert_resource(crate::modules::item::build_test_registry())
            .insert_resource(QuestRegistry::default())
            .add_systems(Startup, load_quests);
        app.update(); // Startup 1회 실행
        let reg = app.world.resource::<QuestRegistry>();
        assert!(reg.quests.len() >= 4, "최소 4개 퀘스트 로드");
        assert!(reg.quests.contains_key("gem_quest"));
    }

    #[test]
    fn validate_quest_item_refs시스템은_유효한_레지스트리에서_종료하지_않는다() {
        // 실제 에셋을 load_quests 로 채운 뒤 검증 시스템을 돌려 정상 경로(에러 없음)를 탄다.
        let mut app = App::new();
        app.insert_resource(crate::modules::item::build_test_registry())
            .insert_resource(QuestRegistry::default())
            .add_systems(Startup, (load_quests, validate_quest_item_refs).chain());
        app.update();
        // exit 되지 않고 도달하면 통과
        assert!(app.world.resource::<QuestRegistry>().quests.contains_key("gem_quest"));
    }

    #[test]
    fn select_active_quests는_spawn_chance가_1이면_항상_0이면_절대_활성화한다() {
        let mut quests: HashMap<String, QuestDef> = HashMap::new();
        let mk = |id: &str, chance: f32| QuestDef {
            id: id.into(), title: id.into(), giver_npc: "n".into(),
            initial_phase: "s".into(),
            phases: { let mut m = HashMap::new();
                m.insert("s".into(), QuestPhaseDef { dialog: vec![], objective: None }); m },
            transitions: vec![], spawns: vec![], spawn_chance: chance,
        };
        quests.insert("always".into(), mk("always", 1.0));  // gen() < 1.0 항상 참
        quests.insert("never".into(), mk("never", 0.0));    // gen() < 0.0 항상 거짓
        let mut rng = rand::thread_rng();
        let active = select_active_quests(&quests, &mut rng);
        assert!(active.contains("always"), "spawn_chance 1.0 은 항상 활성");
        assert!(!active.contains("never"), "spawn_chance 0.0 은 절대 비활성");
    }

    // ── App 하네스 ──────────────────────────────────────────────────────────

    fn asset_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app.init_asset::<Image>();
        app
    }

    // ── QuestPlugin::build ───────────────────────────────────────────────────

    #[test]
    fn 플러그인을_등록하면_퀘스트_리소스와_이벤트가_초기화된다() {
        let mut app = App::new();
        app.add_plugins(QuestPlugin);
        // build() 안의 init_resource 들이 실행됐는지 확인 (update 불필요)
        assert!(app.world.get_resource::<QuestRegistry>().is_some());
        assert!(app.world.get_resource::<QuestState>().is_some());
        assert!(app.world.get_resource::<Events<KillNpcEvent>>().is_some());
        assert!(app.world.get_resource::<Events<DespawnWorldItemEvent>>().is_some());
    }

    #[test]
    fn 플러그인_Startup스케줄을_실행하면_퀘스트가_스케줄러를_통해_로드된다() {
        // QuestSystemSet 디스패치를 스케줄러 경유로 실행한다.
        // Update 스케줄(spawn_quest_items 등)은 추가 리소스가 필요하므로
        // Startup 스케줄만 직접 돌린다.
        let mut app = App::new();
        app.insert_resource(crate::modules::item::build_test_registry());
        app.add_plugins(QuestPlugin);
        app.world.run_schedule(bevy::app::Startup);
        assert!(
            app.world.resource::<QuestRegistry>().quests.contains_key("gem_quest"),
            "스케줄러 경유로 퀘스트가 로드돼야 한다"
        );
    }

    // ── execute_actions: 모든 액션 변형 ───────────────────────────────────────

    /// execute_actions 가 요구하는 6 개 EventWriter 를 모아 한 번에 실행하는
    /// 하네스 시스템. 입력은 리소스로 주입한다.
    #[derive(Resource, Clone)]
    struct ActionInput {
        actions: Vec<QuestAction>,
        quest_id: String,
        trigger_pos: (usize, usize),
    }

    fn run_execute_actions_system(
        input: Res<ActionInput>,
        mut state: ResMut<QuestState>,
        mut inventory: ResMut<PlayerInventory>,
        mut log: EventWriter<crate::modules::ui::LogMessage>,
        mut kill_npc: EventWriter<KillNpcEvent>,
        mut open_portal: EventWriter<SpawnQuestPortalEvent>,
        mut close_portal: EventWriter<CloseQuestPortalEvent>,
        mut despawn_item: EventWriter<DespawnWorldItemEvent>,
        mut spawn_guards: EventWriter<SpawnGuardEvent>,
        mut spawn_monster: EventWriter<SpawnMonsterEvent>,
        mut explode: EventWriter<ExplosionEvent>,
        mut place_traps: EventWriter<SpawnTrapEvent>,
        quest_items: Res<crate::modules::item::QuestItemRegistry>,
    ) {
        execute_actions(
            &input.actions, &input.quest_id, input.trigger_pos, &mut state, &mut inventory,
            &mut log, &mut kill_npc, &mut open_portal, &mut close_portal,
            &mut despawn_item, &mut spawn_guards, &mut spawn_monster, &mut explode,
            &mut place_traps, &quest_items,
        );
    }

    fn execute_actions_app(actions: Vec<QuestAction>) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(crate::modules::item::build_test_registry())
            .insert_resource(QuestState::default())
            .insert_resource(PlayerInventory::default())
            .insert_resource(ActionInput { actions, quest_id: "q".into(), trigger_pos: (5, 5) })
            .add_event::<crate::modules::ui::LogMessage>()
            .add_event::<KillNpcEvent>()
            .add_event::<SpawnQuestPortalEvent>()
            .add_event::<CloseQuestPortalEvent>()
            .add_event::<DespawnWorldItemEvent>()
            .add_event::<SpawnGuardEvent>()
            .add_event::<SpawnMonsterEvent>()
            .add_event::<ExplosionEvent>()
            .add_event::<SpawnTrapEvent>()
            .add_systems(Update, run_execute_actions_system);
        app
    }

    fn count_log_messages(app: &mut App) -> usize {
        app.world.resource::<Events<crate::modules::ui::LogMessage>>().len()
    }

    #[test]
    fn GiveItem액션은_퀘스트아이템을_인벤토리에_추가하고_로그를_남긴다() {
        let mut app = execute_actions_app(vec![QuestAction::GiveItem("eternal_gem".into())]);
        app.update();
        let inv = app.world.resource::<PlayerInventory>();
        assert_eq!(inv.items.len(), 1);
        assert_eq!(inv.items[0].kind, ItemKind::QuestItem(QuestItemKind("eternal_gem")));
        assert_eq!(count_log_messages(&mut app), 1);
    }

    #[test]
    fn GiveItem액션은_미등록_아이템ID면_아무것도_하지_않는다() {
        let mut app = execute_actions_app(vec![QuestAction::GiveItem("unknown".into())]);
        app.update();
        assert!(app.world.resource::<PlayerInventory>().items.is_empty());
        assert_eq!(count_log_messages(&mut app), 0);
    }

    #[test]
    fn GiveItems액션은_소모품은_스택하고_일반아이템은_개수만큼_추가한다() {
        // 소모품 분기: health_potion 3개 → consumables 에 (kind, 3)
        let mut app = execute_actions_app(vec![
            QuestAction::GiveItems { item: "health_potion".into(), count: 3 },
        ]);
        app.update();
        let inv = app.world.resource::<PlayerInventory>();
        assert_eq!(inv.consumables.len(), 1);
        assert_eq!(inv.consumables[0].1, 3, "소모품은 스택");
        assert!(inv.items.is_empty());
        assert_eq!(count_log_messages(&mut app), 1);

        // 일반 아이템 분기: 퀘스트아이템 2개 → items 에 2개 push
        let mut app2 = execute_actions_app(vec![
            QuestAction::GiveItems { item: "dragon_scale".into(), count: 2 },
        ]);
        app2.update();
        assert_eq!(app2.world.resource::<PlayerInventory>().items.len(), 2);
    }

    #[test]
    fn GiveItems액션은_미등록_아이템ID면_아무것도_하지_않는다() {
        let mut app = execute_actions_app(vec![
            QuestAction::GiveItems { item: "unknown".into(), count: 5 },
        ]);
        app.update();
        let inv = app.world.resource::<PlayerInventory>();
        assert!(inv.items.is_empty() && inv.consumables.is_empty());
        assert_eq!(count_log_messages(&mut app), 0);
    }

    #[test]
    fn RemoveItem액션은_인벤토리에서_해당아이템을_제거하고_로그를_남긴다() {
        let mut app = execute_actions_app(vec![QuestAction::RemoveItem("eternal_gem".into())]);
        // 사전에 보유 상태로 세팅
        {
            let kind = item_id_to_kind("eternal_gem", qi()).unwrap();
            app.world.resource_mut::<PlayerInventory>().items.push(InventoryItem::new(kind));
        }
        app.update();
        assert!(app.world.resource::<PlayerInventory>().items.is_empty());
        assert_eq!(count_log_messages(&mut app), 1);
    }

    #[test]
    fn RemoveItem액션은_미등록_아이템ID면_아무것도_하지_않는다() {
        let mut app = execute_actions_app(vec![QuestAction::RemoveItem("unknown".into())]);
        app.update();
        assert_eq!(count_log_messages(&mut app), 0);
    }

    #[test]
    fn Log액션은_로그메시지_이벤트를_발생시킨다() {
        let mut app = execute_actions_app(vec![QuestAction::Log("안녕".into())]);
        app.update();
        assert_eq!(count_log_messages(&mut app), 1);
    }

    #[test]
    fn SetFlag와_ClearFlag액션은_퀘스트상태의_플래그를_쓰고_지운다() {
        let mut app = execute_actions_app(vec![
            QuestAction::SetFlag { flag: "f".into(), value: "v".into() },
        ]);
        app.update();
        assert_eq!(app.world.resource::<QuestState>().get_flag("f"), Some("v"));

        let mut app2 = execute_actions_app(vec![QuestAction::ClearFlag("f".into())]);
        app2.world.resource_mut::<QuestState>().set_flag("f", "v");
        app2.update();
        assert!(!app2.world.resource::<QuestState>().has_flag("f"));
    }

    #[test]
    fn KillNpc액션은_NPC사망_이벤트를_발생시킨다() {
        let mut app = execute_actions_app(vec![QuestAction::KillNpc("바스티안".into())]);
        app.update();
        let events = app.world.resource::<Events<KillNpcEvent>>();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn OpenPortal액션은_포탈스폰_이벤트와_로그를_발생시킨다() {
        let mut app = execute_actions_app(vec![QuestAction::OpenPortal {
            zone: "demon_cave".into(),
            generator: "cellular_automata".into(),
            placement: PortalPlacement::Border,
        }]);
        app.update();
        assert_eq!(app.world.resource::<Events<SpawnQuestPortalEvent>>().len(), 1);
        assert_eq!(count_log_messages(&mut app), 1);
    }

    #[test]
    fn ClosePortal액션은_포탈닫기_이벤트와_로그를_발생시킨다() {
        let mut app = execute_actions_app(vec![QuestAction::ClosePortal("demon_cave".into())]);
        app.update();
        assert_eq!(app.world.resource::<Events<CloseQuestPortalEvent>>().len(), 1);
        assert_eq!(count_log_messages(&mut app), 1);
    }

    #[test]
    fn DespawnWorldItem액션은_월드아이템_제거_이벤트를_발생시킨다() {
        let mut app = execute_actions_app(vec![
            QuestAction::DespawnWorldItem("prologue_daggers".into()),
        ]);
        app.update();
        assert_eq!(app.world.resource::<Events<DespawnWorldItemEvent>>().len(), 1);
    }

    #[test]
    fn SpawnGuards액션은_요청한_마릿수로_가드스폰_이벤트를_발행한다() {
        let mut app = execute_actions_app(vec![QuestAction::SpawnGuards { count: 3 }]);
        app.update();
        let events = app.world.resource::<Events<SpawnGuardEvent>>();
        let mut cursor = events.get_reader();
        let counts: Vec<u32> = cursor.read(events).map(|e| e.count).collect();
        assert_eq!(counts, vec![3], "한 번의 SpawnGuardEvent 에 count=3");
    }

    #[test]
    fn SpawnMonster액션은_지정id와_마릿수로_몬스터스폰_이벤트를_발행한다() {
        let mut app = execute_actions_app(vec![QuestAction::SpawnMonster {
            id: "dragon".into(), count: 2,
        }]);
        app.update();
        let events = app.world.resource::<Events<SpawnMonsterEvent>>();
        let mut cursor = events.get_reader();
        let payloads: Vec<(String, u32)> = cursor.read(events)
            .map(|e| (e.id.clone(), e.count)).collect();
        assert_eq!(payloads, vec![("dragon".to_string(), 2)], "id·count 그대로 전달");
    }

    #[test]
    fn PlaceTraps액션은_지정종류와_개수로_함정스폰_이벤트를_발행한다() {
        use crate::modules::trap::TrapKind;
        let mut app = execute_actions_app(vec![QuestAction::PlaceTraps {
            kind: TrapKind::Alarm, count: 4, hidden: true,
        }]);
        app.update();
        let events = app.world.resource::<Events<SpawnTrapEvent>>();
        let mut cursor = events.get_reader();
        let payloads: Vec<(TrapKind, u32, bool)> = cursor.read(events)
            .map(|e| (e.kind, e.count, e.hidden)).collect();
        assert_eq!(payloads, vec![(TrapKind::Alarm, 4, true)], "종류·개수·숨김 그대로 전달");
    }

    #[test]
    fn PlaceTraps는_hidden_생략시_기본값_숨김으로_역직렬화된다() {
        // RON 에 hidden 을 안 적으면 default_trap_hidden() == true.
        let ron = r#"PlaceTraps(kind: Spike, count: 2)"#;
        let action: QuestAction = ron::de::from_str(ron).expect("PlaceTraps 역직렬화");
        match action {
            QuestAction::PlaceTraps { kind, count, hidden } => {
                assert_eq!(kind, crate::modules::trap::TrapKind::Spike);
                assert_eq!(count, 2);
                assert!(hidden, "hidden 생략 시 기본 숨김(true)");
            }
            _ => panic!("PlaceTraps 로 파싱돼야 한다"),
        }
    }

    #[test]
    fn Explode액션은_트리거위치를_중심으로_폭발이벤트를_발행한다() {
        // execute_actions_app 의 trigger_pos 는 (5,5).
        let mut app = execute_actions_app(vec![QuestAction::Explode {
            radius: 3, terrain: true, entity_damage: 6,
        }]);
        app.update();
        let events = app.world.resource::<Events<ExplosionEvent>>();
        let mut cursor = events.get_reader();
        let evs: Vec<&ExplosionEvent> = cursor.read(events).collect();
        assert_eq!(evs.len(), 1, "폭발 이벤트 한 번");
        assert_eq!(evs[0].center, (5, 5), "트리거 위치가 폭발 중심");
        assert_eq!(evs[0].radius, 3);
        assert!(evs[0].terrain);
        assert_eq!(evs[0].entity_damage, 6);
    }

    // ── check_auto_advance (App 하네스) — 상태머신 주요 경로 ───────────────────

    fn auto_advance_app(registry: QuestRegistry, state: QuestState) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(crate::modules::item::build_test_registry())
            .insert_resource(registry)
            .insert_resource(state)
            .insert_resource(PlayerInventory::default())
            .insert_resource(crate::modules::zone::WorldState::default())
            .add_event::<DespawnWorldItemEvent>()
            .add_systems(Update, check_auto_advance);
        app
    }

    #[test]
    fn 자동전이는_조건충족시_페이즈를_전진시킨다() {
        // gem_quest: active 에서 eternal_gem 보유 시 Auto 로 ready 전진
        let mut reg = make_registry_with_gem_quest();
        reg.active.insert("gem_quest".into());
        let mut state = QuestState::default();
        state.set_phase("gem_quest", "active");

        let mut app = auto_advance_app(reg, state);
        // 보석 보유
        let kind = item_id_to_kind("eternal_gem", qi()).unwrap();
        app.world.resource_mut::<PlayerInventory>().items.push(InventoryItem::new(kind));
        app.update();
        assert_eq!(
            app.world.resource::<QuestState>().current_phase("gem_quest"),
            Some("ready"),
            "보석 보유 시 active → ready 자동 전진"
        );
    }

    #[test]
    fn 자동전이는_조건미충족이면_페이즈를_유지한다() {
        let mut reg = make_registry_with_gem_quest();
        reg.active.insert("gem_quest".into());
        let mut state = QuestState::default();
        state.set_phase("gem_quest", "active");
        let mut app = auto_advance_app(reg, state);
        app.update(); // 보석 없음
        assert_eq!(
            app.world.resource::<QuestState>().current_phase("gem_quest"),
            Some("active"),
            "보석 없으면 전진 안 함"
        );
    }

    #[test]
    fn 자동전이는_비활성_퀘스트는_평가하지_않는다() {
        // active 집합에 넣지 않음 → continue
        let reg = make_registry_with_gem_quest();
        let mut state = QuestState::default();
        state.set_phase("gem_quest", "active");
        let mut app = auto_advance_app(reg, state);
        let kind = item_id_to_kind("eternal_gem", qi()).unwrap();
        app.world.resource_mut::<PlayerInventory>().items.push(InventoryItem::new(kind));
        app.update();
        assert_eq!(
            app.world.resource::<QuestState>().current_phase("gem_quest"),
            Some("active"),
            "비활성 퀘스트는 자동 전진하지 않음"
        );
    }

    #[test]
    fn 자동전이는_페이즈가_등록되지않은_퀘스트는_건너뛴다() {
        // state.phases 에 gem_quest 없음 → None → continue
        let mut reg = make_registry_with_gem_quest();
        reg.active.insert("gem_quest".into());
        let mut app = auto_advance_app(reg, QuestState::default());
        app.update();
        assert!(
            app.world.resource::<QuestState>().current_phase("gem_quest").is_none(),
            "등록 안 된 퀘스트는 전진 평가 대상이 아님"
        );
    }

    /// Auto 전이 + 액션(DespawnWorldItem, RemoveItem, SetFlag) 동시 실행 검증용 레지스트리
    fn make_registry_with_auto_actions() -> QuestRegistry {
        let mut r = QuestRegistry::default();
        let def = QuestDef {
            id: "q".into(), title: "q".into(), giver_npc: "n".into(),
            initial_phase: "p1".into(),
            phases: {
                let mut m = HashMap::new();
                m.insert("p1".into(), QuestPhaseDef { dialog: vec![], objective: None });
                m.insert("p2".into(), QuestPhaseDef { dialog: vec![], objective: None });
                m
            },
            transitions: vec![QuestTransition {
                from: "p1".into(),
                trigger: TriggerKind::Auto,
                when: Some(QuestCondition::HasFlag("go".into())),
                actions: vec![
                    QuestAction::DespawnWorldItem("prologue_daggers".into()),
                    QuestAction::RemoveItem("eternal_gem".into()),
                    QuestAction::SetFlag { flag: "done".into(), value: "1".into() },
                    QuestAction::Log("이건 Auto 에서 무시됨".into()),
                ],
                to: "p2".into(),
            }],
            spawns: vec![], spawn_chance: 1.0,
        };
        r.quests.insert("q".into(), def);
        r.active.insert("q".into());
        r
    }

    #[test]
    fn 자동전이의_허용액션은_실행되고_미허용액션은_무시된다() {
        let reg = make_registry_with_auto_actions();
        let mut state = QuestState::default();
        state.set_phase("q", "p1");
        state.set_flag("go", "yes"); // 조건 충족
        let mut app = auto_advance_app(reg, state);
        // 인벤토리에 eternal_gem 보유 → RemoveItem 대상
        let kind = item_id_to_kind("eternal_gem", qi()).unwrap();
        app.world.resource_mut::<PlayerInventory>().items.push(InventoryItem::new(kind));
        app.update();

        let st = app.world.resource::<QuestState>();
        assert_eq!(st.current_phase("q"), Some("p2"), "전진");
        assert_eq!(st.get_flag("done"), Some("1"), "SetFlag 실행됨");
        assert!(
            app.world.resource::<PlayerInventory>().items.is_empty(),
            "RemoveItem 실행됨"
        );
        // DespawnWorldItem 이벤트 1개 (Log 는 Auto 에서 미지원 → 무시)
        assert_eq!(app.world.resource::<Events<DespawnWorldItemEvent>>().len(), 1);
    }

    // ── spawn_quest_items (App 하네스) — 스폰 주요 경로 ────────────────────────

    /// 모든 타일이 Floor 인 단순 맵 + 방 하나를 만든다.
    fn make_floor_map() -> crate::modules::map::Map {
        use crate::modules::map::{Map, TileKind, Rect};
        let mut map = Map::new(20, 20);
        for y in 0..20 { for x in 0..20 { map.set_tile(x, y, TileKind::Floor); } }
        map.rooms = vec![Rect::new(1, 1, 8, 8), Rect::new(10, 10, 6, 6)];
        map
    }

    fn spawn_app(registry: QuestRegistry, state: QuestState, world: crate::modules::zone::WorldState) -> App {
        let mut app = asset_app();
        app.insert_resource(crate::modules::item::build_test_registry())
            .insert_resource(registry)
            .insert_resource(state)
            .insert_resource(world)
            .insert_resource(MapResource(make_floor_map()))
            .insert_resource(UsedSpawnTiles::default())
            .insert_resource(DiscoveredMarkers::default())
            .add_systems(Update, spawn_quest_items);
        app
    }

    /// 단일 spawn 을 가진 퀘스트 레지스트리. 활성 + 지정 phase 등록까지 포함하지 않음.
    fn make_registry_with_spawn(
        item: &str, zone: crate::modules::zone::ZoneId, count: u32,
        condition: Option<QuestCondition>,
    ) -> QuestRegistry {
        let mut r = QuestRegistry::default();
        let def = QuestDef {
            id: "sq".into(), title: "sq".into(), giver_npc: "n".into(),
            initial_phase: "spawn_phase".into(),
            phases: {
                let mut m = HashMap::new();
                m.insert("spawn_phase".into(), QuestPhaseDef { dialog: vec![], objective: None });
                m
            },
            transitions: vec![],
            spawns: vec![QuestSpawn {
                phase: "spawn_phase".into(),
                item: item.into(),
                zone,
                count,
                condition,
            }],
            spawn_chance: 1.0,
        };
        r.quests.insert("sq".into(), def);
        r.active.insert("sq".into());
        r
    }

    fn count_items(app: &mut App) -> usize {
        app.world.query::<&Item>().iter(&app.world).count()
    }

    #[test]
    fn 맵이_바뀌지않으면_퀘스트아이템을_스폰하지_않는다() {
        use crate::modules::zone::ZoneId;
        let reg = make_registry_with_spawn("eternal_gem", ZoneId::Town, 1, None);
        let mut state = QuestState::default();
        state.set_phase("sq", "spawn_phase");
        let mut app = spawn_app(reg, state, crate::modules::zone::WorldState::default());
        // 첫 update 에서 MapResource 가 inserted 라 is_changed()==true 이므로
        // change tick 을 소비시킨 뒤 다시 update 하면 미변경.
        app.update(); // 1회차: 변경됨 → 스폰
        let after_first = count_items(&mut app);
        assert_eq!(after_first, 1);
        app.update(); // 2회차: 미변경 → 스폰 안 함 (early return)
        assert_eq!(count_items(&mut app), after_first, "맵 미변경 시 추가 스폰 없음");
    }

    #[test]
    fn 활성_퀘스트의_현재페이즈_현재존_조건이_맞으면_아이템과_마커가_생성된다() {
        use crate::modules::zone::ZoneId;
        let reg = make_registry_with_spawn("eternal_gem", ZoneId::Dungeon(2), 1, None);
        let mut state = QuestState::default();
        state.set_phase("sq", "spawn_phase");
        let mut world = crate::modules::zone::WorldState::default();
        world.current = ZoneId::Dungeon(2);
        let mut app = spawn_app(reg, state, world);
        app.update();
        assert_eq!(count_items(&mut app), 1, "조건 일치 시 1개 스폰");
        assert_eq!(app.world.resource::<DiscoveredMarkers>().0.len(), 1, "QuestTarget 마커 등록");
        // dedup 키 마킹 확인
        let st = app.world.resource::<QuestState>();
        assert!(st.is_spawn_done("sq", "eternal_gem"));
        assert!(st.is_zone_spawn_done(&ZoneId::Dungeon(2), "eternal_gem"));
    }

    #[test]
    fn 스폰은_count만큼_여러개를_생성한다() {
        use crate::modules::zone::ZoneId;
        let reg = make_registry_with_spawn("eternal_gem", ZoneId::Town, 3, None);
        let mut state = QuestState::default();
        state.set_phase("sq", "spawn_phase");
        let mut app = spawn_app(reg, state, crate::modules::zone::WorldState::default());
        app.update();
        assert_eq!(count_items(&mut app), 3);
    }

    #[test]
    fn 스폰은_페이즈가_다르거나_존이_다르면_건너뛴다() {
        use crate::modules::zone::ZoneId;
        // 존 불일치: spawn zone=Dungeon(2), world=Town
        let reg = make_registry_with_spawn("eternal_gem", ZoneId::Dungeon(2), 1, None);
        let mut state = QuestState::default();
        state.set_phase("sq", "spawn_phase");
        let mut app = spawn_app(reg, state, crate::modules::zone::WorldState::default());
        app.update();
        assert_eq!(count_items(&mut app), 0, "존 불일치 시 스폰 안 함");

        // 페이즈 불일치: state 페이즈를 다른 값으로
        let reg2 = make_registry_with_spawn("eternal_gem", ZoneId::Town, 1, None);
        let mut state2 = QuestState::default();
        state2.set_phase("sq", "other_phase");
        let mut app2 = spawn_app(reg2, state2, crate::modules::zone::WorldState::default());
        app2.update();
        assert_eq!(count_items(&mut app2), 0, "페이즈 불일치 시 스폰 안 함");
    }

    #[test]
    fn 스폰은_비활성_퀘스트나_미등록페이즈_퀘스트는_건너뛴다() {
        use crate::modules::zone::ZoneId;
        // 비활성: active 비움
        let mut reg = make_registry_with_spawn("eternal_gem", ZoneId::Town, 1, None);
        reg.active.clear();
        let mut state = QuestState::default();
        state.set_phase("sq", "spawn_phase");
        let mut app = spawn_app(reg, state, crate::modules::zone::WorldState::default());
        app.update();
        assert_eq!(count_items(&mut app), 0, "비활성 퀘스트 스폰 안 함");

        // 활성이지만 phases 미등록 (None → continue)
        let reg2 = make_registry_with_spawn("eternal_gem", ZoneId::Town, 1, None);
        let mut app2 = spawn_app(reg2, QuestState::default(), crate::modules::zone::WorldState::default());
        app2.update();
        assert_eq!(count_items(&mut app2), 0, "페이즈 미등록 퀘스트 스폰 안 함");
    }

    #[test]
    fn 이미_스폰완료한_퀘스트아이템은_재스폰하지_않는다() {
        use crate::modules::zone::ZoneId;
        let reg = make_registry_with_spawn("eternal_gem", ZoneId::Town, 1, None);
        let mut state = QuestState::default();
        state.set_phase("sq", "spawn_phase");
        state.mark_spawned("sq", "eternal_gem"); // 이미 스폰됨
        let mut app = spawn_app(reg, state, crate::modules::zone::WorldState::default());
        app.update();
        assert_eq!(count_items(&mut app), 0, "이미 스폰 완료면 재스폰 안 함");
    }

    #[test]
    fn 다른_퀘스트가_같은존아이템을_이미_스폰했으면_스킵하고_per_quest로만_마크한다() {
        use crate::modules::zone::ZoneId;
        let reg = make_registry_with_spawn("eternal_gem", ZoneId::Town, 1, None);
        let mut state = QuestState::default();
        state.set_phase("sq", "spawn_phase");
        // zone-단위로는 이미 spawn 됨 (다른 퀘스트가 했다고 가정)
        state.mark_zone_spawned(&ZoneId::Town, "eternal_gem");
        let mut app = spawn_app(reg, state, crate::modules::zone::WorldState::default());
        app.update();
        assert_eq!(count_items(&mut app), 0, "zone dedup 으로 스킵");
        // 그래도 per-quest 마크는 됨 (재평가 방지)
        assert!(app.world.resource::<QuestState>().is_spawn_done("sq", "eternal_gem"));
    }

    #[test]
    fn 스폰조건이_충족되지않으면_스폰하지_않고_충족되면_스폰한다() {
        use crate::modules::zone::ZoneId;
        // 조건: InZone(Dungeon(2)). world=Town → 미충족
        let reg = make_registry_with_spawn(
            "eternal_gem", ZoneId::Town, 1,
            Some(QuestCondition::InZone(ZoneId::Dungeon(2))),
        );
        let mut state = QuestState::default();
        state.set_phase("sq", "spawn_phase");
        let mut app = spawn_app(reg, state, crate::modules::zone::WorldState::default());
        app.update();
        assert_eq!(count_items(&mut app), 0, "조건 미충족 시 스폰 안 함");

        // 조건: InZone(Town). world=Town → 충족
        let reg2 = make_registry_with_spawn(
            "eternal_gem", ZoneId::Town, 1,
            Some(QuestCondition::InZone(ZoneId::Town)),
        );
        let mut state2 = QuestState::default();
        state2.set_phase("sq", "spawn_phase");
        let mut app2 = spawn_app(reg2, state2, crate::modules::zone::WorldState::default());
        app2.update();
        assert_eq!(count_items(&mut app2), 1, "조건 충족 시 스폰");
    }

    #[test]
    fn 스폰은_미등록_아이템ID면_건너뛴다() {
        use crate::modules::zone::ZoneId;
        // 검증을 통과한 정상 레지스트리를 만든 뒤, item_id 만 미등록으로 바꿔
        // item_id_to_kind None → continue 분기를 커버한다.
        let mut reg = make_registry_with_spawn("eternal_gem", ZoneId::Town, 1, None);
        reg.quests.get_mut("sq").unwrap().spawns[0].item = "unknown_item".into();
        let mut state = QuestState::default();
        state.set_phase("sq", "spawn_phase");
        let mut app = spawn_app(reg, state, crate::modules::zone::WorldState::default());
        app.update();
        assert_eq!(count_items(&mut app), 0);
    }

    #[test]
    fn 스폰은_방이_하나뿐이면_그_방에서라도_스폰한다() {
        use crate::modules::zone::ZoneId;
        use crate::modules::map::{Map, TileKind, Rect};
        let reg = make_registry_with_spawn("eternal_gem", ZoneId::Town, 1, None);
        let mut state = QuestState::default();
        state.set_phase("sq", "spawn_phase");
        // 방 1개짜리 맵 (rooms.len() == 1 → rooms[..] 분기)
        let mut map = Map::new(20, 20);
        for y in 0..20 { for x in 0..20 { map.set_tile(x, y, TileKind::Floor); } }
        map.rooms = vec![Rect::new(2, 2, 10, 10)];
        let mut app = spawn_app(reg, state, crate::modules::zone::WorldState::default());
        app.insert_resource(MapResource(map));
        app.update();
        assert_eq!(count_items(&mut app), 1, "방 하나뿐이어도 스폰");
    }

    #[test]
    fn 스폰은_Floor타일이_없으면_스폰에_실패하고_건너뛴다() {
        use crate::modules::zone::ZoneId;
        use crate::modules::map::{Map, Rect};
        let reg = make_registry_with_spawn("eternal_gem", ZoneId::Town, 1, None);
        let mut state = QuestState::default();
        state.set_phase("sq", "spawn_phase");
        // 전부 Wall 인 맵 → random_floor_tile_anywhere None → 실패 분기
        let mut map = Map::new(20, 20); // Map::new 는 전부 Wall
        map.rooms = vec![Rect::new(2, 2, 10, 10)];
        let mut app = spawn_app(reg, state, crate::modules::zone::WorldState::default());
        app.insert_resource(MapResource(map));
        app.update();
        assert_eq!(count_items(&mut app), 0, "Floor 타일 없으면 스폰 실패");
    }

    // ── handle_player_detected (탐지 → stealth_blown 플래그) ───────────────────

    fn detected_app() -> App {
        let mut app = App::new();
        app.add_event::<PlayerDetectedEvent>();
        app.init_resource::<QuestState>();
        app.add_systems(Update, handle_player_detected);
        app
    }

    #[test]
    fn 탐지이벤트가_오면_잠입실패_플래그가_설정된다() {
        let mut app = detected_app();
        app.world.send_event(PlayerDetectedEvent);
        app.update();
        assert!(app.world.resource::<QuestState>().flag_is("stealth_blown", "true"),
            "탐지 시 stealth_blown=true");
    }

    #[test]
    fn 탐지이벤트가_없으면_잠입실패_플래그는_설정되지_않는다() {
        let mut app = detected_app();
        app.update(); // 이벤트 없음
        assert!(!app.world.resource::<QuestState>().has_flag("stealth_blown"),
            "탐지 없으면 플래그 미설정");
    }

    #[test]
    fn 같은_프레임에_여러_탐지이벤트가_와도_플래그는_한번만_설정된다() {
        // events.read().next().is_some() 한 갈래만 타도 충분 — 다중 이벤트 입력.
        let mut app = detected_app();
        app.world.send_event(PlayerDetectedEvent);
        app.world.send_event(PlayerDetectedEvent);
        app.update();
        assert!(app.world.resource::<QuestState>().flag_is("stealth_blown", "true"));
    }

    // ── SpawnGuards 액션 직렬화/검증 ──────────────────────────────────────────

    #[test]
    fn SpawnGuards액션이_RON에서_올바르게_파싱된다() {
        let def: QuestDef = ron::de::from_str(r#"
            QuestDef(
                id: "test", title: "test", giver_npc: "npc", initial_phase: "p1",
                phases: {
                    "p1": QuestPhaseDef(dialog: []),
                    "p2": QuestPhaseDef(dialog: []),
                },
                transitions: [
                    Transition(from: "p1", trigger: Interact, actions: [SpawnGuards(count: 4)], to: "p2"),
                ],
            )
        "#).expect("RON 파싱 성공");
        let actions = &def.transitions[0].actions;
        assert!(matches!(&actions[0], QuestAction::SpawnGuards { count } if *count == 4));
    }

    #[test]
    fn 검증은_SpawnGuards를_포함한_퀘스트를_거부하지_않는다() {
        // SpawnGuards 는 item ID 를 참조하지 않으므로 validate 가 통과해야 한다.
        let def: QuestDef = ron::de::from_str(r#"
            QuestDef(
                id: "infil", title: "잠입", giver_npc: "장로", initial_phase: "start",
                phases: {
                    "start": QuestPhaseDef(dialog: []),
                    "guarded": QuestPhaseDef(dialog: []),
                },
                transitions: [
                    Transition(from: "start", trigger: Interact, actions: [SpawnGuards(count: 2)], to: "guarded"),
                ],
            )
        "#).expect("RON 파싱 성공");
        let errors = validate_quest_def(&def, qi());
        assert!(errors.is_empty(), "SpawnGuards 는 검증 오류를 내지 않아야 한다: {:?}", errors);
    }

    #[test]
    fn 시맨틱검증은_SpawnGuards를_거부하지_않는다() {
        // collect_quest_item_ref_errors → check_action_item_ids 의 SpawnGuards arm.
        let mut reg = QuestRegistry::default();
        let def: QuestDef = ron::de::from_str(r#"
            QuestDef(
                id: "infil", title: "잠입", giver_npc: "장로", initial_phase: "start",
                phases: {
                    "start": QuestPhaseDef(dialog: []),
                    "guarded": QuestPhaseDef(dialog: []),
                },
                transitions: [
                    Transition(from: "start", trigger: Interact, actions: [SpawnGuards(count: 2)], to: "guarded"),
                ],
            )
        "#).expect("RON 파싱 성공");
        reg.quests.insert(def.id.clone(), def);
        let errors = collect_quest_item_ref_errors(&reg, qi());
        assert!(errors.is_empty(), "SpawnGuards 는 item-ref 검증을 통과해야 한다");
    }

    #[test]
    fn 액션아이템ID검사는_Explode처럼_아이템을_안쓰는_액션을_통과시킨다() {
        // check_action_item_ids 의 `_ => {}` arm — Explode 는 item id 를 참조하지 않는다.
        let action = QuestAction::Explode { radius: 3, terrain: true, entity_damage: 6 };
        let mut errors = Vec::new();
        check_action_item_ids(&action, "test_quest", "phase1", qi(), &mut errors);
        assert!(errors.is_empty(), "Explode 는 item-ref 검증을 통과해야 한다");
    }

    #[test]
    fn 시맨틱검증은_Explode를_거부하지_않는다() {
        // validate_quest_def → collect_action_errors 의 Explode `_ => {}` arm.
        let def: QuestDef = ron::de::from_str(r#"
            QuestDef(
                id: "boom", title: "폭발", giver_npc: "장로", initial_phase: "start",
                phases: {
                    "start": QuestPhaseDef(dialog: []),
                    "wrecked": QuestPhaseDef(dialog: []),
                },
                transitions: [
                    Transition(from: "start", trigger: Interact, actions: [Explode(radius: 3, terrain: true, entity_damage: 0)], to: "wrecked"),
                ],
            )
        "#).expect("RON 파싱 성공");
        let errors = validate_quest_def(&def, qi());
        assert!(errors.is_empty(), "Explode 는 검증 오류를 내지 않아야 한다: {:?}", errors);
    }

    // ── infiltration_quest (잠입 퀘스트) 콘텐츠 검증 ──────────────────────────

    /// 실제 잠입 퀘스트 RON 파일을 로드한다.
    fn load_infiltration_quest() -> QuestDef {
        let text = std::fs::read_to_string("assets/quests/infiltration_quest.ron")
            .expect("infiltration_quest.ron 이 존재해야 한다");
        ron::de::from_str::<QuestDef>(&text)
            .expect("infiltration_quest.ron 이 파싱돼야 한다")
    }

    #[test]
    fn 잠입퀘스트는_파싱되고_시맨틱검증을_통과한다() {
        let def = load_infiltration_quest();
        assert_eq!(def.id, "infiltration_quest");
        assert_eq!(def.giver_npc, "burgomaster", "giver 는 villager 레지스트리에 존재하는 촌장이어야 한다");
        let errors = validate_quest_def(&def, qi());
        assert!(errors.is_empty(), "시맨틱 검증 통과해야 한다: {:?}", errors);
    }

    #[test]
    fn 잠입퀘스트의_기밀문서ID가_kind로_매핑된다() {
        let _ = qi();
        assert_eq!(
            item_id_to_kind("secret_document", qi()),
            Some(ItemKind::QuestItem(QuestItemKind("secret_document"))),
        );
    }

    #[test]
    fn 잠입퀘스트_수락전이는_플래그초기화와_포탈개방과_가드스폰을_실행한다() {
        let def = load_infiltration_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "not_started" && t.trigger == TriggerKind::Interact)
            .expect("수락 전이가 있어야 한다");
        assert!(t.actions.iter().any(|a| matches!(a, QuestAction::ClearFlag(f) if f == "stealth_blown")),
            "수락 시 stealth_blown 플래그를 초기화해야 한다");
        assert!(t.actions.iter().any(|a| matches!(a, QuestAction::OpenPortal { zone, .. } if zone == "infiltration")),
            "수락 시 잠입구역 포탈을 열어야 한다");
        assert!(t.actions.iter().any(|a| matches!(a, QuestAction::SpawnGuards { count } if (4..=6).contains(count))),
            "수락 시 가드 4~6마리를 스폰해야 한다");
    }

    #[test]
    fn 잠입퀘스트는_기밀문서를_잠입구역_진행페이즈에_스폰한다() {
        use crate::modules::zone::ZoneId;
        let def = load_infiltration_quest();
        let spawn = def.spawns.iter()
            .find(|s| s.item == "secret_document")
            .expect("기밀 문서 스폰이 있어야 한다");
        assert_eq!(spawn.zone, ZoneId::Named("infiltration".into()), "잠입구역에 스폰돼야 한다");
        assert_eq!(spawn.phase, "infiltrating", "잠입 진행 페이즈에 스폰돼야 한다");
    }

    /// extracted 페이즈에서 탐지 여부(stealth_blown 플래그)에 따라 어느 보상 전이가
    /// 선택되는지를 실제 RON 의 transition 순서/조건으로 재현한다.
    fn select_extracted_transition<'a>(def: &'a QuestDef, state: &QuestState) -> &'a QuestTransition {
        let inv = make_inventory_with(&["secret_document"]);
        let world = make_world();
        def.transitions.iter()
            .filter(|t| t.from == "extracted" && t.trigger == TriggerKind::Interact)
            .find(|t| t.when.as_ref()
                .map(|c| eval_condition(c, &inv, &world, state, qi()))
                .unwrap_or(true))
            .expect("extracted 에서 매칭되는 전이가 있어야 한다")
    }

    #[test]
    fn 잠입퀘스트_무탐지시_보너스분기가_선택된다() {
        let def = load_infiltration_quest();
        // stealth_blown 플래그 없음 → 무탐지
        let state = QuestState::default();
        let t = select_extracted_transition(&def, &state);
        assert_eq!(t.to, "done_clean", "무탐지면 보너스(done_clean) 분기로 가야 한다");
        // 보너스: 추가 아이템(현자의 돌) 지급이 포함돼야 한다
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::GiveItem(id) if id == "philosophers_stone")),
            "무탐지 보너스에는 추가 보상 아이템이 있어야 한다"
        );
    }

    #[test]
    fn 잠입퀘스트_탐지시_일반보상분기가_선택된다() {
        let def = load_infiltration_quest();
        // stealth_blown=true → 탐지됨 → 보너스 조건(Not HasFlag) 불충족 → fallback
        let mut state = QuestState::default();
        state.set_flag("stealth_blown", "true");
        let t = select_extracted_transition(&def, &state);
        assert_eq!(t.to, "done_blown", "탐지되면 일반 보상(done_blown) fallback 으로 가야 한다");
        // 일반 보상에는 보너스 전용 아이템이 없어야 한다
        assert!(
            !t.actions.iter().any(|a| matches!(a, QuestAction::GiveItem(id) if id == "philosophers_stone")),
            "탐지 시에는 보너스 아이템이 지급되지 않아야 한다"
        );
    }

    // ── vault_heist_quest (두 번째 잠입 퀘스트) 콘텐츠 검증 ────────────────────

    /// 실제 두 번째 잠입 퀘스트 RON 파일을 로드한다.
    fn load_vault_heist_quest() -> QuestDef {
        let text = std::fs::read_to_string("assets/quests/vault_heist_quest.ron")
            .expect("vault_heist_quest.ron 이 존재해야 한다");
        ron::de::from_str::<QuestDef>(&text)
            .expect("vault_heist_quest.ron 이 파싱돼야 한다")
    }

    #[test]
    fn 금고잠입퀘스트는_파싱되고_시맨틱검증을_통과한다() {
        let def = load_vault_heist_quest();
        assert_eq!(def.id, "vault_heist_quest");
        // 첫 잠입 퀘스트(burgomaster)와 다른 giver 여야 한다.
        assert_eq!(def.giver_npc, "merchant", "giver 는 첫 잠입 퀘스트와 다른 상인이어야 한다");
        assert_ne!(def.giver_npc, "burgomaster", "burgomaster 는 이미 첫 잠입 퀘스트의 giver 다");
        let errors = validate_quest_def(&def, qi());
        assert!(errors.is_empty(), "시맨틱 검증 통과해야 한다: {:?}", errors);
    }

    #[test]
    fn 금고잠입퀘스트의_스타크인장ID가_kind로_매핑된다() {
        let _ = qi();
        assert_eq!(
            item_id_to_kind("stark_signet", qi()),
            Some(ItemKind::QuestItem(QuestItemKind("stark_signet"))),
        );
    }

    #[test]
    fn 금고잠입퀘스트_수락전이는_플래그초기화와_포탈개방과_가드스폰을_실행한다() {
        let def = load_vault_heist_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "not_started" && t.trigger == TriggerKind::Interact)
            .expect("수락 전이가 있어야 한다");
        assert!(t.actions.iter().any(|a| matches!(a, QuestAction::ClearFlag(f) if f == "stealth_blown")),
            "수락 시 stealth_blown 플래그를 초기화해야 한다");
        // 첫 잠입 퀘스트(infiltration)와 다른 구역으로 포탈을 열어야 한다.
        assert!(t.actions.iter().any(|a| matches!(a, QuestAction::OpenPortal { zone, .. } if zone == "dreadfort_vault")),
            "수락 시 금고 구역 포탈을 열어야 한다");
        assert!(t.actions.iter().any(|a| matches!(a, QuestAction::SpawnGuards { count } if (4..=6).contains(count))),
            "수락 시 가드 4~6마리를 스폰해야 한다");
    }

    #[test]
    fn 금고잠입퀘스트는_등록된_생성기로_포탈을_연다() {
        // OpenPortal generator 가 map 모듈에 등록된 생성기 이름이어야 한다.
        // (첫 잠입 퀘스트는 walled_town, 이 퀘스트는 bsp_indoor 로 차별화.)
        let def = load_vault_heist_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "not_started" && t.trigger == TriggerKind::Interact)
            .expect("수락 전이가 있어야 한다");
        let gen = t.actions.iter().find_map(|a| match a {
            QuestAction::OpenPortal { generator, .. } => Some(generator.as_str()),
            _ => None,
        }).expect("OpenPortal 액션이 있어야 한다");
        assert_eq!(gen, "bsp_indoor", "첫 잠입 퀘스트(walled_town)와 다른 등록 생성기여야 한다");
    }

    #[test]
    fn 금고잠입퀘스트는_스타크인장을_금고구역_진행페이즈에_스폰한다() {
        use crate::modules::zone::ZoneId;
        let def = load_vault_heist_quest();
        let spawn = def.spawns.iter()
            .find(|s| s.item == "stark_signet")
            .expect("스타크 인장 스폰이 있어야 한다");
        assert_eq!(spawn.zone, ZoneId::Named("dreadfort_vault".into()), "금고 구역에 스폰돼야 한다");
        assert_eq!(spawn.phase, "infiltrating", "잠입 진행 페이즈에 스폰돼야 한다");
    }

    /// recovered 페이즈에서 탐지 여부(stealth_blown 플래그)에 따라 어느 보상 전이가
    /// 선택되는지를 실제 RON 의 transition 순서/조건으로 재현한다.
    fn select_vault_recovered_transition<'a>(def: &'a QuestDef, state: &QuestState) -> &'a QuestTransition {
        let inv = make_inventory_with(&["stark_signet"]);
        let world = make_world();
        def.transitions.iter()
            .filter(|t| t.from == "recovered" && t.trigger == TriggerKind::Interact)
            .find(|t| t.when.as_ref()
                .map(|c| eval_condition(c, &inv, &world, state, qi()))
                .unwrap_or(true))
            .expect("recovered 에서 매칭되는 전이가 있어야 한다")
    }

    #[test]
    fn 금고잠입퀘스트_무탐지시_보너스분기가_선택된다() {
        let def = load_vault_heist_quest();
        // stealth_blown 플래그 없음 → 무탐지
        let state = QuestState::default();
        let t = select_vault_recovered_transition(&def, &state);
        assert_eq!(t.to, "done_clean", "무탐지면 보너스(done_clean) 분기로 가야 한다");
        // 보너스: 추가 보상 아이템(영원의 보석) 지급이 포함돼야 한다
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::GiveItem(id) if id == "eternal_gem")),
            "무탐지 보너스에는 추가 보상 아이템이 있어야 한다"
        );
    }

    #[test]
    fn 금고잠입퀘스트_탐지시_일반보상분기가_선택된다() {
        let def = load_vault_heist_quest();
        // stealth_blown=true → 탐지됨 → 보너스 조건(Not HasFlag) 불충족 → fallback
        let mut state = QuestState::default();
        state.set_flag("stealth_blown", "true");
        let t = select_vault_recovered_transition(&def, &state);
        assert_eq!(t.to, "done_blown", "탐지되면 일반 보상(done_blown) fallback 으로 가야 한다");
        // 일반 보상에는 보너스 전용 아이템이 없어야 한다
        assert!(
            !t.actions.iter().any(|a| matches!(a, QuestAction::GiveItem(id) if id == "eternal_gem")),
            "탐지 시에는 보너스 아이템이 지급되지 않아야 한다"
        );
    }

    #[test]
    fn 금고잠입퀘스트는_첫_잠입퀘스트_완료_전에는_잠겨있고_완료후_열린다() {
        // 퀘스트 체이닝: vault_heist 는 locked 로 시작하고,
        // infiltration_quest 가 끝나야(어느 결말이든) not_started 로 열린다.
        let def = load_vault_heist_quest();
        assert_eq!(def.initial_phase, "locked", "선행 미완료 상태(locked)로 시작해야 한다");
        let gate = def.transitions.iter()
            .find(|t| t.from == "locked" && t.to == "not_started")
            .and_then(|t| t.when.clone())
            .expect("locked→not_started 게이트 전이에 선행 완료 조건이 있어야 한다");

        let inv = PlayerInventory::default();
        let world = make_world();

        // 첫 잠입 퀘스트 미완료 → 잠김
        let locked_state = QuestState::default();
        assert!(!eval_condition(&gate, &inv, &world, &locked_state, qi()),
            "첫 잠입 퀘스트 완료 전에는 열리지 않아야 한다");

        // 무탐지 완료 → 열림
        let mut clean_state = QuestState::default();
        clean_state.set_phase("infiltration_quest", "done_clean");
        assert!(eval_condition(&gate, &inv, &world, &clean_state, qi()),
            "첫 잠입을 무탐지로 끝내면 열려야 한다");

        // 탐지 완료여도 열림
        let mut blown_state = QuestState::default();
        blown_state.set_phase("infiltration_quest", "done_blown");
        assert!(eval_condition(&gate, &inv, &world, &blown_state, qi()),
            "첫 잠입을 탐지된 채 끝내도(완료이므로) 열려야 한다");
    }

    // ── trap_mine_quest (함정 공략 퀘스트) 콘텐츠 검증 ─────────────────────────

    /// 실제 함정 공략 퀘스트 RON 파일을 로드한다.
    fn load_trap_mine_quest() -> QuestDef {
        let text = std::fs::read_to_string("assets/quests/trap_mine_quest.ron")
            .expect("trap_mine_quest.ron 이 존재해야 한다");
        ron::de::from_str::<QuestDef>(&text)
            .expect("trap_mine_quest.ron 이 파싱돼야 한다")
    }

    #[test]
    fn 함정공략퀘스트는_파싱되고_시맨틱검증을_통과한다() {
        let def = load_trap_mine_quest();
        assert_eq!(def.id, "trap_mine_quest");
        // giver 가 기존 어떤 퀘스트와도 겹치지 않는 농부여야 한다.
        assert_eq!(def.giver_npc, "farmer", "giver 는 기존 퀘스트와 겹치지 않는 농부여야 한다");
        let errors = validate_quest_def(&def, qi());
        assert!(errors.is_empty(), "시맨틱 검증 통과해야 한다: {:?}", errors);
    }

    #[test]
    fn 함정공략퀘스트의_giver는_다른_퀘스트의_giver와_겹치지_않는다() {
        // quest_for_giver 가 1:1 을 가정하므로, 모든 퀘스트를 모아 farmer 를
        // giver 로 쓰는 퀘스트가 trap_mine_quest 하나뿐임을 확인한다.
        let mut reg = QuestRegistry::default();
        for (path, def) in load_all_quest_defs() {
            assert!(
                reg.quests.insert(def.id.clone(), def).is_none(),
                "{}: 퀘스트 id 가 중복된다", path
            );
        }
        let farmer_givers: Vec<&str> = reg.quests.values()
            .filter(|q| q.giver_npc == "farmer")
            .map(|q| q.id.as_str())
            .collect();
        assert_eq!(farmer_givers, vec!["trap_mine_quest"],
            "농부를 giver 로 쓰는 퀘스트는 trap_mine_quest 하나뿐이어야 한다");
    }

    #[test]
    fn 함정공략퀘스트의_광부로켓ID가_kind로_매핑된다() {
        let _ = qi();
        assert_eq!(
            item_id_to_kind("miners_locket", qi()),
            Some(ItemKind::QuestItem(QuestItemKind("miners_locket"))),
        );
    }

    #[test]
    fn 함정공략퀘스트_수락전이는_숨김함정을_배치하고_폐갱포탈을_등록생성기로_연다() {
        use crate::modules::trap::TrapKind;
        let def = load_trap_mine_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "not_started" && t.trigger == TriggerKind::Interact)
            .expect("수락 전이가 있어야 한다");

        // 숨김 가시 함정 다수 배치
        assert!(
            t.actions.iter().any(|a| matches!(a,
                QuestAction::PlaceTraps { kind: TrapKind::Spike, count, hidden: true } if *count >= 1)),
            "수락 시 숨김 가시 함정을 배치해야 한다",
        );
        // 숨김 독 함정 다수 배치
        assert!(
            t.actions.iter().any(|a| matches!(a,
                QuestAction::PlaceTraps { kind: TrapKind::Poison, count, hidden: true } if *count >= 1)),
            "수락 시 숨김 독 함정을 배치해야 한다",
        );
        // 폐갱 포탈 — 등록된 생성기(dla)로 연다.
        let gen = t.actions.iter().find_map(|a| match a {
            QuestAction::OpenPortal { zone, generator, .. } if zone == "trap_mine" => Some(generator.as_str()),
            _ => None,
        }).expect("trap_mine 포탈을 여는 OpenPortal 이 있어야 한다");
        assert_eq!(gen, "dla", "폐갱은 dla 생성기로 열어야 한다");
    }

    #[test]
    fn 함정공략퀘스트_수락전이는_해제도구와_안내도구를_지급한다() {
        // 핍진성: 도구를 건네며 T/Y 사용을 안내한다. 함정 해제/표시/설치 도구가
        // 모두 지급되어야 플레이어가 갑자기 메커니즘을 알게 되는 어색함이 없다.
        let def = load_trap_mine_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "not_started" && t.trigger == TriggerKind::Interact)
            .expect("수락 전이가 있어야 한다");
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::GiveItem(id) if id == "disarm_tool")),
            "수락 시 해제 도구(disarm_tool)를 지급해야 한다",
        );
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::GiveItem(id) if id == "scout_lens")),
            "수락 시 함정 표시용 올빼미 안경(scout_lens)을 지급해야 한다",
        );
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::GiveItems { item, count } if item == "trap_kit" && *count >= 1)),
            "수락 시 함정 키트(trap_kit)를 지급해야 한다",
        );
    }

    #[test]
    fn 함정공략퀘스트는_광부로켓을_폐갱_탐사페이즈에_스폰한다() {
        use crate::modules::zone::ZoneId;
        let def = load_trap_mine_quest();
        let spawn = def.spawns.iter()
            .find(|s| s.item == "miners_locket")
            .expect("광부 로켓 스폰이 있어야 한다");
        assert_eq!(spawn.zone, ZoneId::Named("trap_mine".into()), "폐갱 구역에 스폰돼야 한다");
        assert_eq!(spawn.phase, "delving", "탐사 진행 페이즈에 스폰돼야 한다");
    }

    #[test]
    fn 함정공략퀘스트는_로켓을_회수하면_자동으로_귀환단계로_전진한다() {
        let def = load_trap_mine_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "delving" && t.trigger == TriggerKind::Auto)
            .expect("탐사 중 자동 전이가 있어야 한다");
        assert_eq!(t.to, "recovered", "회수 시 귀환(recovered) 단계로 가야 한다");

        // 로켓을 가졌을 때만 조건이 충족되어야 한다.
        let cond = t.when.as_ref().expect("회수 자동 전이에 HasItem 조건이 있어야 한다");
        let world = make_world();
        let state = QuestState::default();
        let empty = PlayerInventory::default();
        assert!(!eval_condition(cond, &empty, &world, &state, qi()),
            "로켓이 없으면 전진하지 않아야 한다");
        let with_locket = make_inventory_with(&["miners_locket"]);
        assert!(eval_condition(cond, &with_locket, &world, &state, qi()),
            "로켓을 회수하면 전진 조건이 충족돼야 한다");
    }

    #[test]
    fn 함정공략퀘스트_완료전이는_로켓반납과_포탈정리와_보상지급으로_done에_도달한다() {
        let def = load_trap_mine_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "recovered" && t.trigger == TriggerKind::Interact)
            .expect("완료 전이가 있어야 한다");
        assert_eq!(t.to, "done", "전달하면 완료(done)에 도달해야 한다");
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::RemoveItem(id) if id == "miners_locket")),
            "완료 시 로켓을 반납해야 한다",
        );
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::ClosePortal(z) if z == "trap_mine")),
            "완료 시 폐갱 포탈을 닫아 정리해야 한다",
        );
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::GiveItem(id) if id == "eternal_gem"))
                || t.actions.iter().any(|a| matches!(a, QuestAction::GiveItems { .. })),
            "완료 시 보상을 지급해야 한다",
        );
    }

    #[test]
    fn 함정공략퀘스트_수락전이의_PlaceTraps가_실제_함정스폰_이벤트로_실행된다() {
        use crate::modules::trap::TrapKind;
        // RON 의 수락 전이 액션을 그대로 execute_actions 로 실행해, PlaceTraps 가
        // SpawnTrapEvent 로 발행되는지(가시/독 모두) end-to-end 로 확인한다.
        let def = load_trap_mine_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "not_started" && t.trigger == TriggerKind::Interact)
            .expect("수락 전이가 있어야 한다");
        let mut app = execute_actions_app(t.actions.clone());
        app.update();

        let events = app.world.resource::<Events<SpawnTrapEvent>>();
        let mut cursor = events.get_reader();
        let kinds: Vec<TrapKind> = cursor.read(events).map(|e| e.kind).collect();
        assert!(kinds.contains(&TrapKind::Spike), "가시 함정 스폰 이벤트가 발행돼야 한다");
        assert!(kinds.contains(&TrapKind::Poison), "독 함정 스폰 이벤트가 발행돼야 한다");
        // 함정이 모두 숨김으로 배치되는지(역직렬화·전달 일관성) 확인.
        let mut cursor2 = app.world.resource::<Events<SpawnTrapEvent>>().get_reader();
        assert!(
            cursor2.read(app.world.resource::<Events<SpawnTrapEvent>>()).all(|e| e.hidden),
            "퀘스트가 깐 함정은 모두 숨김이어야 한다",
        );
    }

    // ── dragon_hunt_quest (보스 토벌 퀘스트) 콘텐츠 검증 ──────────────────────

    /// 실제 보스 토벌 퀘스트 RON 파일을 로드한다.
    fn load_dragon_hunt_quest() -> QuestDef {
        let text = std::fs::read_to_string("assets/quests/dragon_hunt_quest.ron")
            .expect("dragon_hunt_quest.ron 이 존재해야 한다");
        ron::de::from_str::<QuestDef>(&text)
            .expect("dragon_hunt_quest.ron 이 파싱돼야 한다")
    }

    #[test]
    fn 보스토벌퀘스트는_파싱되고_시맨틱검증을_통과한다() {
        let def = load_dragon_hunt_quest();
        assert_eq!(def.id, "dragon_hunt_quest");
        // giver 가 기존 어떤 퀘스트와도 겹치지 않는 수렵단장이어야 한다.
        assert_eq!(def.giver_npc, "huntmaster", "giver 는 새 주민 수렵단장이어야 한다");
        let errors = validate_quest_def(&def, qi());
        assert!(errors.is_empty(), "시맨틱 검증 통과해야 한다: {:?}", errors);
    }

    #[test]
    fn 보스토벌퀘스트의_마룡심장ID가_kind로_매핑된다() {
        let _ = qi();
        assert_eq!(
            item_id_to_kind("wyrm_heart", qi()),
            Some(ItemKind::QuestItem(QuestItemKind("wyrm_heart"))),
        );
    }

    #[test]
    fn 보스토벌퀘스트_수락전이는_둥지포탈을_등록생성기로_열고_보스와_부하를_스폰한다() {
        let def = load_dragon_hunt_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "not_started" && t.trigger == TriggerKind::Interact)
            .expect("수락 전이가 있어야 한다");
        // 둥지 포탈 — 등록된 생성기(bsp)로 연다.
        let gen = t.actions.iter().find_map(|a| match a {
            QuestAction::OpenPortal { zone, generator, .. } if zone == "wyrm_lair" => Some(generator.as_str()),
            _ => None,
        }).expect("wyrm_lair 포탈을 여는 OpenPortal 이 있어야 한다");
        assert_eq!(gen, "bsp", "둥지는 bsp 생성기로 열어야 한다");
        // 보스(frost_wyrm)를 둥지에 스폰해야 한다.
        assert!(
            t.actions.iter().any(|a| matches!(a,
                QuestAction::SpawnMonster { id, count } if id == "frost_wyrm" && *count >= 1)),
            "수락 시 보스 서리 마룡을 스폰해야 한다",
        );
        // 부하도 함께 스폰한다("보스 + 부하" 패턴).
        assert!(
            t.actions.iter().any(|a| matches!(a,
                QuestAction::SpawnMonster { id, .. } if id == "troll")),
            "수락 시 부하 몬스터도 함께 스폰해야 한다",
        );
    }

    #[test]
    fn 보스토벌퀘스트가_스폰하는_보스id는_몬스터레지스트리에_실재한다() {
        // SpawnMonster 가 참조하는 모든 monster id 가 monsters.ron 에 존재해야
        // 런타임에 handle_spawn_monster 의 by_id 조회가 성공한다.
        let monster_reg = crate::modules::monster::build_test_registry();
        let def = load_dragon_hunt_quest();
        let spawned_ids: Vec<&str> = def.transitions.iter()
            .flat_map(|t| t.actions.iter())
            .filter_map(|a| match a {
                QuestAction::SpawnMonster { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert!(!spawned_ids.is_empty(), "SpawnMonster 액션이 있어야 한다");
        for id in spawned_ids {
            assert!(
                monster_reg.by_id(id).is_some(),
                "SpawnMonster 가 참조하는 monster id '{}' 가 monsters.ron 에 없다", id
            );
        }
    }

    #[test]
    fn 보스토벌퀘스트는_마룡심장을_둥지_토벌페이즈에_스폰한다() {
        use crate::modules::zone::ZoneId;
        let def = load_dragon_hunt_quest();
        let spawn = def.spawns.iter()
            .find(|s| s.item == "wyrm_heart")
            .expect("마룡 심장 스폰이 있어야 한다");
        assert_eq!(spawn.zone, ZoneId::Named("wyrm_lair".into()), "둥지 구역에 스폰돼야 한다");
        assert_eq!(spawn.phase, "hunting", "토벌 진행 페이즈에 스폰돼야 한다");
    }

    #[test]
    fn 보스토벌퀘스트는_심장을_회수하면_자동으로_귀환단계로_전진한다() {
        let def = load_dragon_hunt_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "hunting" && t.trigger == TriggerKind::Auto)
            .expect("토벌 중 자동 전이가 있어야 한다");
        assert_eq!(t.to, "slain", "회수 시 귀환(slain) 단계로 가야 한다");

        let cond = t.when.as_ref().expect("회수 자동 전이에 HasItem 조건이 있어야 한다");
        let world = make_world();
        let state = QuestState::default();
        let empty = PlayerInventory::default();
        assert!(!eval_condition(cond, &empty, &world, &state, qi()),
            "심장이 없으면 전진하지 않아야 한다");
        let with_heart = make_inventory_with(&["wyrm_heart"]);
        assert!(eval_condition(cond, &with_heart, &world, &state, qi()),
            "심장을 회수하면 전진 조건이 충족돼야 한다");
    }

    #[test]
    fn 보스토벌퀘스트_완료전이는_심장반납과_포탈정리와_보상지급으로_done에_도달한다() {
        let def = load_dragon_hunt_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "slain" && t.trigger == TriggerKind::Interact)
            .expect("완료 전이가 있어야 한다");
        assert_eq!(t.to, "done", "전달하면 완료(done)에 도달해야 한다");
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::RemoveItem(id) if id == "wyrm_heart")),
            "완료 시 심장을 반납해야 한다",
        );
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::ClosePortal(z) if z == "wyrm_lair")),
            "완료 시 둥지 포탈을 닫아 정리해야 한다",
        );
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::GiveItem(_) | QuestAction::GiveItems { .. })),
            "완료 시 보상을 지급해야 한다",
        );
    }

    #[test]
    fn 보스토벌퀘스트_수락전이의_SpawnMonster가_실제_몬스터스폰_이벤트로_실행된다() {
        // RON 의 수락 전이 액션을 그대로 execute_actions 로 실행해, SpawnMonster 가
        // SpawnMonsterEvent 로 발행되는지 end-to-end 로 확인한다(보스 + 부하).
        let def = load_dragon_hunt_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "not_started" && t.trigger == TriggerKind::Interact)
            .expect("수락 전이가 있어야 한다");
        let mut app = execute_actions_app(t.actions.clone());
        app.update();

        let events = app.world.resource::<Events<SpawnMonsterEvent>>();
        let mut cursor = events.get_reader();
        let payloads: Vec<(String, u32)> = cursor.read(events)
            .map(|e| (e.id.clone(), e.count)).collect();
        assert!(payloads.iter().any(|(id, c)| id == "frost_wyrm" && *c == 1),
            "보스 서리 마룡 1마리 스폰 이벤트가 발행돼야 한다: {:?}", payloads);
        assert!(payloads.iter().any(|(id, _)| id == "troll"),
            "부하 몬스터 스폰 이벤트도 발행돼야 한다: {:?}", payloads);
    }

    #[test]
    fn 보스토벌퀘스트의_giver_수렵단장은_다른_퀘스트의_giver와_겹치지_않는다() {
        let mut reg = QuestRegistry::default();
        for (path, def) in load_all_quest_defs() {
            assert!(
                reg.quests.insert(def.id.clone(), def).is_none(),
                "{}: 퀘스트 id 가 중복된다", path
            );
        }
        let givers: Vec<&str> = reg.quests.values()
            .filter(|q| q.giver_npc == "huntmaster")
            .map(|q| q.id.as_str())
            .collect();
        assert_eq!(givers, vec!["dragon_hunt_quest"],
            "수렵단장을 giver 로 쓰는 퀘스트는 dragon_hunt_quest 하나뿐이어야 한다");
    }

    // ── buried_dungeon_quest (지형폭발 던전 개방 퀘스트) 콘텐츠 검증 ───────────

    /// 실제 지형폭발 던전 개방 퀘스트 RON 파일을 로드한다.
    fn load_buried_dungeon_quest() -> QuestDef {
        let text = std::fs::read_to_string("assets/quests/buried_dungeon_quest.ron")
            .expect("buried_dungeon_quest.ron 이 존재해야 한다");
        ron::de::from_str::<QuestDef>(&text)
            .expect("buried_dungeon_quest.ron 이 파싱돼야 한다")
    }

    #[test]
    fn 폭발던전퀘스트는_파싱되고_시맨틱검증을_통과한다() {
        let def = load_buried_dungeon_quest();
        assert_eq!(def.id, "buried_dungeon_quest");
        // giver 는 유일한 비-giver 였던 아이(child)여야 한다.
        assert_eq!(def.giver_npc, "child", "giver 는 아이여야 한다");
        let errors = validate_quest_def(&def, qi());
        assert!(errors.is_empty(), "시맨틱 검증 통과해야 한다: {:?}", errors);
    }

    #[test]
    fn 폭발던전퀘스트의_봉인성물ID가_kind로_매핑된다() {
        let _ = qi();
        assert_eq!(
            item_id_to_kind("sealed_relic", qi()),
            Some(ItemKind::QuestItem(QuestItemKind("sealed_relic"))),
        );
    }

    #[test]
    fn 폭발던전퀘스트_수락전이는_지형폭발을_일으키고_숨겨진던전을_등록생성기로_연다() {
        let def = load_buried_dungeon_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "not_started" && t.trigger == TriggerKind::Interact)
            .expect("수락 전이가 있어야 한다");
        // 지형을 파괴하는(terrain:true) 폭발 + 엔티티 범위 피해.
        assert!(
            t.actions.iter().any(|a| matches!(a,
                QuestAction::Explode { radius, terrain: true, entity_damage }
                    if *radius >= 1 && *entity_damage >= 1)),
            "수락 시 지형을 파괴하고 범위 피해를 주는 폭발이 일어나야 한다",
        );
        // 드러난 숨겨진 던전 — 등록된 생성기(cellular_automata)로 연다.
        let gen = t.actions.iter().find_map(|a| match a {
            QuestAction::OpenPortal { zone, generator, .. } if zone == "buried_dungeon" => Some(generator.as_str()),
            _ => None,
        }).expect("buried_dungeon 포탈을 여는 OpenPortal 이 있어야 한다");
        assert_eq!(gen, "cellular_automata", "숨겨진 던전은 cellular_automata 생성기로 열어야 한다");
    }

    #[test]
    fn 폭발던전퀘스트의_폭발은_던전개방보다_먼저_실행된다() {
        // 서사 일관성: 폭발(지형 변화)이 먼저 일어나고 그 결과로 던전이 드러난다.
        let def = load_buried_dungeon_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "not_started" && t.trigger == TriggerKind::Interact)
            .expect("수락 전이가 있어야 한다");
        let explode_idx = t.actions.iter().position(|a| matches!(a, QuestAction::Explode { .. }))
            .expect("Explode 액션이 있어야 한다");
        let portal_idx = t.actions.iter().position(|a| matches!(a,
            QuestAction::OpenPortal { zone, .. } if zone == "buried_dungeon"))
            .expect("OpenPortal 액션이 있어야 한다");
        assert!(explode_idx < portal_idx, "폭발이 던전 개방보다 먼저 실행돼야 한다");
    }

    #[test]
    fn 폭발던전퀘스트는_봉인성물을_던전_탐사페이즈에_스폰한다() {
        use crate::modules::zone::ZoneId;
        let def = load_buried_dungeon_quest();
        let spawn = def.spawns.iter()
            .find(|s| s.item == "sealed_relic")
            .expect("봉인된 성물 스폰이 있어야 한다");
        assert_eq!(spawn.zone, ZoneId::Named("buried_dungeon".into()), "드러난 던전에 스폰돼야 한다");
        assert_eq!(spawn.phase, "exploring", "탐사 진행 페이즈에 스폰돼야 한다");
    }

    #[test]
    fn 폭발던전퀘스트는_성물을_회수하면_자동으로_귀환단계로_전진한다() {
        let def = load_buried_dungeon_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "exploring" && t.trigger == TriggerKind::Auto)
            .expect("탐사 중 자동 전이가 있어야 한다");
        assert_eq!(t.to, "recovered", "회수 시 귀환(recovered) 단계로 가야 한다");

        let cond = t.when.as_ref().expect("회수 자동 전이에 HasItem 조건이 있어야 한다");
        let world = make_world();
        let state = QuestState::default();
        let empty = PlayerInventory::default();
        assert!(!eval_condition(cond, &empty, &world, &state, qi()),
            "성물이 없으면 전진하지 않아야 한다");
        let with_relic = make_inventory_with(&["sealed_relic"]);
        assert!(eval_condition(cond, &with_relic, &world, &state, qi()),
            "성물을 회수하면 전진 조건이 충족돼야 한다");
    }

    #[test]
    fn 폭발던전퀘스트_완료전이는_성물반납과_포탈정리와_보상지급으로_done에_도달한다() {
        let def = load_buried_dungeon_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "recovered" && t.trigger == TriggerKind::Interact)
            .expect("완료 전이가 있어야 한다");
        assert_eq!(t.to, "done", "보여주면 완료(done)에 도달해야 한다");
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::RemoveItem(id) if id == "sealed_relic")),
            "완료 시 성물을 반납해야 한다",
        );
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::ClosePortal(z) if z == "buried_dungeon")),
            "완료 시 던전 포탈을 닫아 정리해야 한다",
        );
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::GiveItem(_) | QuestAction::GiveItems { .. })),
            "완료 시 보상을 지급해야 한다",
        );
    }

    #[test]
    fn 폭발던전퀘스트_수락전이의_Explode가_실제_폭발이벤트로_실행된다() {
        // RON 의 수락 전이 액션을 그대로 execute_actions 로 실행해, Explode 가
        // ExplosionEvent 로 발행되는지 end-to-end 로 확인한다.
        // execute_actions_app 의 trigger_pos 는 (5,5).
        let def = load_buried_dungeon_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "not_started" && t.trigger == TriggerKind::Interact)
            .expect("수락 전이가 있어야 한다");
        let mut app = execute_actions_app(t.actions.clone());
        app.update();

        let events = app.world.resource::<Events<ExplosionEvent>>();
        let mut cursor = events.get_reader();
        let evs: Vec<(usize, usize, i32, bool, i32)> = cursor.read(events)
            .map(|e| (e.center.0, e.center.1, e.radius, e.terrain, e.entity_damage)).collect();
        assert_eq!(evs.len(), 1, "폭발 이벤트가 한 번 발행돼야 한다");
        let (cx, cy, radius, terrain, dmg) = evs[0];
        assert_eq!((cx, cy), (5, 5), "트리거 위치가 폭발 중심이어야 한다");
        assert!(radius >= 1 && dmg >= 1, "폭발은 유효한 반경과 피해를 가져야 한다");
        assert!(terrain, "지형을 파괴하는 폭발이어야 한다");

        // 같은 수락 전이가 던전 포탈도 함께 열었는지 확인한다.
        assert_eq!(app.world.resource::<Events<SpawnQuestPortalEvent>>().len(), 1,
            "폭발 후 숨겨진 던전 포탈이 열려야 한다");
    }

    // ── skill_trial_quest (액티브 스킬 활용 퀘스트) 콘텐츠 검증 ────────────────

    /// 실제 액티브 스킬 활용 퀘스트 RON 파일을 로드한다.
    fn load_skill_trial_quest() -> QuestDef {
        let text = std::fs::read_to_string("assets/quests/skill_trial_quest.ron")
            .expect("skill_trial_quest.ron 이 존재해야 한다");
        ron::de::from_str::<QuestDef>(&text)
            .expect("skill_trial_quest.ron 이 파싱돼야 한다")
    }

    #[test]
    fn 스킬시험퀘스트는_파싱되고_시맨틱검증을_통과한다() {
        let def = load_skill_trial_quest();
        assert_eq!(def.id, "skill_trial_quest");
        // giver 는 새 주민 전투마법사여야 한다(기존 giver 와 겹치지 않음).
        assert_eq!(def.giver_npc, "battlemage", "giver 는 새 주민 전투마법사여야 한다");
        let errors = validate_quest_def(&def, qi());
        assert!(errors.is_empty(), "시맨틱 검증 통과해야 한다: {:?}", errors);
    }

    #[test]
    fn 스킬시험퀘스트의_비전초점ID가_kind로_매핑된다() {
        let _ = qi();
        assert_eq!(
            item_id_to_kind("arcane_focus", qi()),
            Some(ItemKind::QuestItem(QuestItemKind("arcane_focus"))),
        );
    }

    #[test]
    fn 스킬시험퀘스트는_다단계로_not_started부터_done까지_정의된다() {
        let def = load_skill_trial_quest();
        for phase in ["not_started", "trial", "passed", "done"] {
            assert!(def.phases.contains_key(phase), "phase '{}' 가 정의돼야 한다", phase);
        }
        assert_eq!(def.initial_phase, "not_started", "시작 페이즈는 not_started");
    }

    #[test]
    fn 스킬시험퀘스트_수락전이는_시험장포탈을_등록생성기로_열고_생존물약을_지급한다() {
        let def = load_skill_trial_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "not_started" && t.trigger == TriggerKind::Interact)
            .expect("수락 전이가 있어야 한다");
        // 시험장 포탈 — 등록된 생성기(coastal)로 연다(물길/파괴가능 지형이 함께 나옴).
        let gen = t.actions.iter().find_map(|a| match a {
            QuestAction::OpenPortal { zone, generator, .. } if zone == "skill_trial" => Some(generator.as_str()),
            _ => None,
        }).expect("skill_trial 포탈을 여는 OpenPortal 이 있어야 한다");
        assert_eq!(gen, "coastal", "시험장은 coastal 생성기로 열어야 한다");
        // 험지 생존용 물약을 지급해 스킬 운용(특히 치유 대체)을 돕는다.
        assert!(
            t.actions.iter().any(|a| matches!(a,
                QuestAction::GiveItems { item, count } if item == "health_potion" && *count >= 1)),
            "수락 시 험지 생존용 체력 물약을 지급해야 한다",
        );
    }

    #[test]
    fn 스킬시험퀘스트의_objective는_세_스킬_운용을_서사로_안내한다() {
        // 스킬 사용을 코드로 강제할 수 없으므로 objective/대사로 강하게 유도한다.
        let def = load_skill_trial_quest();
        let trial = def.phases.get("trial").expect("trial 페이즈");
        let obj = trial.objective.as_ref().expect("trial objective 가 있어야 한다");
        assert!(obj.contains("파이어볼"), "objective 가 파이어볼(벽 파괴)을 안내해야 한다");
        assert!(obj.contains("점멸"), "objective 가 점멸(물·틈 건너기)을 안내해야 한다");
        assert!(obj.contains("치유"), "objective 가 치유(험지 생존)를 안내해야 한다");
    }

    #[test]
    fn 스킬시험퀘스트는_비전초점을_시험장_돌파페이즈에_스폰한다() {
        use crate::modules::zone::ZoneId;
        let def = load_skill_trial_quest();
        let spawn = def.spawns.iter()
            .find(|s| s.item == "arcane_focus")
            .expect("비전의 초점 스폰이 있어야 한다");
        assert_eq!(spawn.zone, ZoneId::Named("skill_trial".into()), "시험장 구역에 스폰돼야 한다");
        assert_eq!(spawn.phase, "trial", "돌파 진행 페이즈에 스폰돼야 한다");
    }

    #[test]
    fn 스킬시험퀘스트는_초점을_회수하면_자동으로_귀환단계로_전진한다() {
        let def = load_skill_trial_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "trial" && t.trigger == TriggerKind::Auto)
            .expect("돌파 중 자동 전이가 있어야 한다");
        assert_eq!(t.to, "passed", "회수 시 귀환(passed) 단계로 가야 한다");

        let cond = t.when.as_ref().expect("회수 자동 전이에 HasItem 조건이 있어야 한다");
        let world = make_world();
        let state = QuestState::default();
        let empty = PlayerInventory::default();
        assert!(!eval_condition(cond, &empty, &world, &state, qi()),
            "초점이 없으면 전진하지 않아야 한다");
        let with_focus = make_inventory_with(&["arcane_focus"]);
        assert!(eval_condition(cond, &with_focus, &world, &state, qi()),
            "초점을 회수하면 전진 조건이 충족돼야 한다");
    }

    #[test]
    fn 스킬시험퀘스트_완료전이는_초점반납과_포탈정리와_보상지급으로_done에_도달한다() {
        let def = load_skill_trial_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "passed" && t.trigger == TriggerKind::Interact)
            .expect("완료 전이가 있어야 한다");
        assert_eq!(t.to, "done", "전달하면 완료(done)에 도달해야 한다");
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::RemoveItem(id) if id == "arcane_focus")),
            "완료 시 초점을 반납해야 한다",
        );
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::ClosePortal(z) if z == "skill_trial")),
            "완료 시 시험장 포탈을 닫아 정리해야 한다",
        );
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::GiveItem(_) | QuestAction::GiveItems { .. })),
            "완료 시 보상을 지급해야 한다",
        );
    }

    #[test]
    fn 스킬시험퀘스트의_giver_전투마법사는_다른_퀘스트의_giver와_겹치지_않는다() {
        let mut reg = QuestRegistry::default();
        for (path, def) in load_all_quest_defs() {
            assert!(
                reg.quests.insert(def.id.clone(), def).is_none(),
                "{}: 퀘스트 id 가 중복된다", path
            );
        }
        let givers: Vec<&str> = reg.quests.values()
            .filter(|q| q.giver_npc == "battlemage")
            .map(|q| q.id.as_str())
            .collect();
        assert_eq!(givers, vec!["skill_trial_quest"],
            "전투마법사를 giver 로 쓰는 퀘스트는 skill_trial_quest 하나뿐이어야 한다");
    }

    // ── loot_farming_quest (파밍·드롭 퀘스트) 콘텐츠 검증 ──────────────────────

    /// 실제 파밍·드롭 퀘스트 RON 파일을 로드한다.
    fn load_loot_farming_quest() -> QuestDef {
        let text = std::fs::read_to_string("assets/quests/loot_farming_quest.ron")
            .expect("loot_farming_quest.ron 이 존재해야 한다");
        ron::de::from_str::<QuestDef>(&text)
            .expect("loot_farming_quest.ron 이 파싱돼야 한다")
    }

    #[test]
    fn 파밍퀘스트는_파싱되고_시맨틱검증을_통과한다() {
        let def = load_loot_farming_quest();
        assert_eq!(def.id, "loot_farming_quest");
        // giver 는 새 주민 보물사냥꾼이어야 한다(기존 giver 와 겹치지 않음).
        assert_eq!(def.giver_npc, "treasure_hunter", "giver 는 새 주민 보물사냥꾼이어야 한다");
        let errors = validate_quest_def(&def, qi());
        assert!(errors.is_empty(), "시맨틱 검증 통과해야 한다: {:?}", errors);
    }

    #[test]
    fn 파밍퀘스트의_목표전리품ID는_드롭가능한_실재_방어구로_매핑된다() {
        // 목표는 quest item 이 아니라 armors.ron 의 실재 방어구 id 여야
        // "사냥으로 파밍" 서사가 HasItem 으로 성립한다.
        let _ = qi();
        let def = load_loot_farming_quest();
        let cond = def.transitions.iter()
            .find(|t| t.from == "farming" && t.trigger == TriggerKind::Auto)
            .and_then(|t| t.when.as_ref())
            .expect("파밍 완료 자동 전이에 HasItem 조건이 있어야 한다");
        let target = match cond {
            QuestCondition::HasItem(id) => id.as_str(),
            other => panic!("목표 조건은 HasItem 이어야 한다: {:?}", other),
        };
        assert_eq!(target, "knight_armor", "목표 전리품은 기사 갑옷이어야 한다");
        // item_id_to_kind 가 armor 레지스트리에서 조회해 Armor 로 매핑한다.
        assert_eq!(
            item_id_to_kind(target, qi()),
            Some(ItemKind::Armor(crate::modules::item::ArmorKind("knight_armor"))),
            "목표 id 는 실재 방어구로 매핑돼야 한다",
        );
    }

    #[test]
    fn 파밍퀘스트의_목표전리품은_레벨스케일_드롭으로_실제로_나올_수_있다() {
        // pick_leveled_armor 가 knight_armor 를 뽑을 수 있어야 "사냥으로 파밍" 이 성립.
        // 충분히 많이 굴려 통계적으로 한 번이라도 목표 id 가 나오는지 확인한다.
        use crate::modules::item::{pick_leveled_armor, ArmorKind};
        let mut rng = rand::thread_rng();
        let target = ArmorKind("knight_armor");
        // knight_armor 는 T3. 그 티어가 충분히 잘 뽑히는 레벨대(중간 레벨)에서 굴린다.
        let appeared = (0..5000).any(|_| pick_leveled_armor(7, qi(), &mut rng) == Some(target));
        assert!(appeared, "knight_armor 가 레벨스케일 드롭으로 나올 수 있어야 한다(파밍 가능 근거)");
    }

    #[test]
    fn 파밍퀘스트의_보상장비ID도_드롭레지스트리의_실재_방어구다() {
        // 보상은 상위 티어 장비여야 한다(paladin_armor T5).
        let _ = qi();
        let def = load_loot_farming_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "gathered" && t.trigger == TriggerKind::Interact)
            .expect("완료 전이가 있어야 한다");
        let gives_paladin = t.actions.iter().any(|a| matches!(a,
            QuestAction::GiveItem(id) if id == "paladin_armor"));
        assert!(gives_paladin, "완료 시 상위 티어 장비(paladin_armor)를 보상으로 줘야 한다");
        assert_eq!(
            item_id_to_kind("paladin_armor", qi()),
            Some(ItemKind::Armor(crate::modules::item::ArmorKind("paladin_armor"))),
            "보상 id 도 실재 방어구로 매핑돼야 한다",
        );
    }

    #[test]
    fn 파밍퀘스트_수락전이는_사냥터포탈을_등록생성기로_연다() {
        let def = load_loot_farming_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "not_started" && t.trigger == TriggerKind::Interact)
            .expect("수락 전이가 있어야 한다");
        let gen = t.actions.iter().find_map(|a| match a {
            QuestAction::OpenPortal { zone, generator, .. } if zone == "hunting_ground" => Some(generator.as_str()),
            _ => None,
        }).expect("hunting_ground 포탈을 여는 OpenPortal 이 있어야 한다");
        assert_eq!(gen, "forest", "사냥터는 forest 생성기로 열어야 한다");
    }

    #[test]
    fn 파밍퀘스트는_전리품을_파밍하면_자동으로_귀환단계로_전진한다() {
        let def = load_loot_farming_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "farming" && t.trigger == TriggerKind::Auto)
            .expect("파밍 중 자동 전이가 있어야 한다");
        assert_eq!(t.to, "gathered", "파밍 시 귀환(gathered) 단계로 가야 한다");

        let cond = t.when.as_ref().expect("파밍 자동 전이에 HasItem 조건이 있어야 한다");
        let world = make_world();
        let state = QuestState::default();
        let empty = PlayerInventory::default();
        assert!(!eval_condition(cond, &empty, &world, &state, qi()),
            "전리품이 없으면 전진하지 않아야 한다");
        let with_loot = make_inventory_with(&["knight_armor"]);
        assert!(eval_condition(cond, &with_loot, &world, &state, qi()),
            "전리품을 파밍하면 전진 조건이 충족돼야 한다");
    }

    #[test]
    fn 파밍퀘스트_완료전이는_전리품반납과_포탈정리와_보상지급으로_done에_도달한다() {
        let def = load_loot_farming_quest();
        let t = def.transitions.iter()
            .find(|t| t.from == "gathered" && t.trigger == TriggerKind::Interact)
            .expect("완료 전이가 있어야 한다");
        assert_eq!(t.to, "done", "넘기면 완료(done)에 도달해야 한다");
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::RemoveItem(id) if id == "knight_armor")),
            "완료 시 전리품을 반납해야 한다",
        );
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::ClosePortal(z) if z == "hunting_ground")),
            "완료 시 사냥터 포탈을 닫아 정리해야 한다",
        );
        assert!(
            t.actions.iter().any(|a| matches!(a, QuestAction::GiveItem(_) | QuestAction::GiveItems { .. })),
            "완료 시 보상을 지급해야 한다",
        );
    }

    #[test]
    fn 파밍퀘스트의_giver_보물사냥꾼은_다른_퀘스트의_giver와_겹치지_않는다() {
        let mut reg = QuestRegistry::default();
        for (path, def) in load_all_quest_defs() {
            assert!(
                reg.quests.insert(def.id.clone(), def).is_none(),
                "{}: 퀘스트 id 가 중복된다", path
            );
        }
        let givers: Vec<&str> = reg.quests.values()
            .filter(|q| q.giver_npc == "treasure_hunter")
            .map(|q| q.id.as_str())
            .collect();
        assert_eq!(givers, vec!["loot_farming_quest"],
            "보물사냥꾼을 giver 로 쓰는 퀘스트는 loot_farming_quest 하나뿐이어야 한다");
    }
}
