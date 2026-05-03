use bevy::prelude::*;
use std::collections::HashMap;
use serde::Deserialize;
use crate::modules::{
    item::{PlayerInventory, ItemKind, QuestItemKind, InventoryItem, Item},
    map::{MapResource, TILE_SIZE, tile_to_world_coords},
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
}

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
}

impl QuestRegistry {
    pub fn get(&self, id: &str) -> Option<&QuestDef> {
        self.quests.get(id)
    }

    #[allow(dead_code)]
    pub fn phase<'a>(&'a self, quest_id: &str, phase_id: &str) -> Option<&'a QuestPhaseDef> {
        self.quests.get(quest_id)?.phases.get(phase_id)
    }
}

#[derive(Resource, Default)]
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
            .add_systems(Startup, load_quests)
            .add_systems(Update, (check_auto_advance, spawn_quest_items));
    }
}

// ── Systems ──────────────────────────────────────────────────────────────────

fn load_quests(mut registry: ResMut<QuestRegistry>) {
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
        let errors = validate_quest_def(&def);
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
}

/// QuestDef 의 내부 일관성을 검증하고 오류 메시지 목록을 반환한다
pub fn validate_quest_def(def: &QuestDef) -> Vec<String> {
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
            collect_action_errors(action, &def.phases, phase_id, &mut errors);
        }
        // auto_advance next_phase 참조 확인
        for auto in &phase.auto_advance {
            if !def.phases.contains_key(&auto.next_phase) {
                errors.push(format!(
                    "페이즈 '{}': auto_advance next_phase '{}' 이 없습니다",
                    phase_id, auto.next_phase
                ));
            }
        }
    }

    // spawns 아이템 ID 확인
    for spawn in &def.spawns {
        if item_id_to_kind(&spawn.item).is_none() {
            errors.push(format!(
                "spawns: item_id '{}' 를 인식할 수 없습니다", spawn.item
            ));
        }
        if !def.phases.contains_key(&spawn.phase) {
            errors.push(format!(
                "spawns: phase '{}' 이 phases 에 없습니다", spawn.phase
            ));
        }
    }

    errors
}

fn collect_action_errors(
    action: &QuestAction,
    phases: &HashMap<String, QuestPhaseDef>,
    phase_id: &str,
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
        QuestAction::GiveItem(id) | QuestAction::RemoveItem(id) | QuestAction::DespawnWorldItem(id) => {
            if item_id_to_kind(id).is_none() {
                errors.push(format!(
                    "페이즈 '{}': item_id '{}' 를 인식할 수 없습니다", phase_id, id
                ));
            }
        }
        QuestAction::Branch { if_true, if_false, .. } => {
            for a in if_true.iter().chain(if_false.iter()) {
                collect_action_errors(a, phases, phase_id, errors);
            }
        }
        _ => {}
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
) {
    let mut advances: Vec<(String, String, Vec<QuestAction>)> = Vec::new();

    for (quest_id, quest_def) in &registry.quests {
        let current = match state.phases.get(quest_id) {
            Some(p) => p.clone(),
            None => continue,
        };
        let phase_def = match quest_def.phases.get(&current) {
            Some(p) => p,
            None => continue,
        };
        for auto in &phase_def.auto_advance {
            if eval_condition(&auto.condition, &inventory, &world, &state) {
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
                    if let Some(kind) = item_id_to_kind(item_id) {
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
) {
    for action in actions {
        match action {
            QuestAction::AdvancePhase(phase) => {
                state.set_phase(quest_id, phase);
                info!("퀘스트 [{}] 단계 전진: {}", quest_id, phase);
            }
            QuestAction::GiveItem(item_id) => {
                if let Some(kind) = item_id_to_kind(item_id) {
                    inventory.items.push(InventoryItem { kind });
                    log.send(crate::modules::ui::LogMessage(
                        format!("{} 획득!", kind.display_name())
                    ));
                }
            }
            QuestAction::RemoveItem(item_id) => {
                if let Some(kind) = item_id_to_kind(item_id) {
                    inventory.items.retain(|i| i.kind != kind);
                    log.send(crate::modules::ui::LogMessage(
                        format!("{} 반납.", kind.display_name())
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
                let branch = if eval_condition(condition, inventory, world, state) {
                    if_true.as_slice()
                } else {
                    if_false.as_slice()
                };
                execute_actions(branch, quest_id, state, inventory, log, world, kill_npc, open_portal, despawn_item);
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
) -> bool {
    match cond {
        QuestCondition::HasItem(item_id) => {
            let Some(kind) = item_id_to_kind(item_id) else { return false };
            inventory.items.iter().any(|i| i.kind == kind)
        }
        QuestCondition::InZone(zone) => &world.current == zone,
        QuestCondition::PhaseIs { quest, phase } => {
            quest_state.phases.get(quest).map(|p| p == phase).unwrap_or(false)
        }
        QuestCondition::FlagIs { flag, value } => quest_state.flag_is(flag, value),
        QuestCondition::HasFlag(flag) => quest_state.has_flag(flag),
        QuestCondition::And(conds) => {
            conds.iter().all(|c| eval_condition(c, inventory, world, quest_state))
        }
        QuestCondition::Or(conds) => {
            conds.iter().any(|c| eval_condition(c, inventory, world, quest_state))
        }
        QuestCondition::Not(inner) => {
            !eval_condition(inner, inventory, world, quest_state)
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
) {
    if !map_res.is_changed() { return; }

    for (quest_id, quest_def) in &registry.quests {
        let current_phase = match state.phases.get(quest_id) {
            Some(p) => p.clone(),
            None => continue,
        };

        for spawn in &quest_def.spawns {
            if spawn.phase != current_phase { continue; }
            if spawn.zone != world.current { continue; }
            if state.is_spawn_done(quest_id, &spawn.item) { continue; }

            let Some(kind) = item_id_to_kind(&spawn.item) else { continue };
            let map = &map_res.0;
            let (tx, ty) = map.rooms.last()
                .map(|r| r.center())
                .unwrap_or((map.width / 2, map.height / 2));

            let pos = tile_to_world_coords(tx, ty);
            let font = asset_server.load("fonts/FiraMono-Medium.ttf");

            commands.spawn((
                Text2dBundle {
                    text: Text::from_section(kind.glyph(), TextStyle {
                        font,
                        font_size: TILE_SIZE,
                        color: kind.color(),
                    }),
                    transform: Transform::from_xyz(pos.x, pos.y, 0.3),
                    ..default()
                },
                Item { kind, tile_x: tx, tile_y: ty },
            ));

            state.mark_spawned(quest_id, &spawn.item);
            info!("퀘스트 아이템 스폰: {} at ({}, {})", spawn.item, tx, ty);
        }
    }
}

// ── item_id 매핑 ─────────────────────────────────────────────────────────────

pub fn item_id_to_kind(id: &str) -> Option<ItemKind> {
    match id {
        "eternal_gem"         => Some(ItemKind::QuestItem(QuestItemKind::EternalGem)),
        "philosophers_stone"  => Some(ItemKind::QuestItem(QuestItemKind::PhilosophersStone)),
        "dragon_scale"        => Some(ItemKind::QuestItem(QuestItemKind::DragonScale)),
        "ancient_scroll"      => Some(ItemKind::QuestItem(QuestItemKind::AncientScroll)),
        "sword"               => Some(ItemKind::Weapon(crate::modules::item::WeaponKind::Sword)),
        "spear"               => Some(ItemKind::Weapon(crate::modules::item::WeaponKind::Spear)),
        "bow"                 => Some(ItemKind::Weapon(crate::modules::item::WeaponKind::Bow)),
        "leather_armor"       => Some(ItemKind::Armor(crate::modules::item::ArmorKind::LeatherArmor)),
        "health_potion"       => Some(ItemKind::Consumable(crate::modules::item::ConsumableKind::HealthPotion)),
        // stark_quest
        "lords_oath"          => Some(ItemKind::QuestItem(QuestItemKind::LordsOath)),
        "jaime_sword"         => Some(ItemKind::QuestItem(QuestItemKind::JaimeSword)),
        "kings_north_crown"   => Some(ItemKind::QuestItem(QuestItemKind::KingsNorthCrown)),
        // targaryen_quest
        "warlock_key"         => Some(ItemKind::QuestItem(QuestItemKind::WarlockKey)),
        "dragon_chain"        => Some(ItemKind::QuestItem(QuestItemKind::DragonChain)),
        "essos_sail_map"      => Some(ItemKind::QuestItem(QuestItemKind::EssosSailMap)),
        // jon_snow_quest
        "dragonglass_arrows"  => Some(ItemKind::QuestItem(QuestItemKind::DragonglassArrows)),
        "rangers_note"        => Some(ItemKind::QuestItem(QuestItemKind::RangersNote)),
        "ygritte_bow"         => Some(ItemKind::QuestItem(QuestItemKind::YgrittesBow)),
        // prologue_fog
        "prologue_greatsword" => Some(ItemKind::QuestItem(QuestItemKind::PrologueGreatsword)),
        "prologue_daggers"    => Some(ItemKind::QuestItem(QuestItemKind::PrologueDaggers)),
        "prologue_bowtorch"   => Some(ItemKind::QuestItem(QuestItemKind::PrologueBowTorch)),
        "family_crest"        => Some(ItemKind::QuestItem(QuestItemKind::FamilyCrest)),
        "ice_sword"           => Some(ItemKind::QuestItem(QuestItemKind::IceSword)),
        "dragon_egg"          => Some(ItemKind::QuestItem(QuestItemKind::DragonEgg)),
        "ghost_wolf"          => Some(ItemKind::QuestItem(QuestItemKind::GhostWolf)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(matches!(item_id_to_kind("eternal_gem"), Some(ItemKind::QuestItem(QuestItemKind::EternalGem))));
        assert!(matches!(item_id_to_kind("philosophers_stone"), Some(ItemKind::QuestItem(QuestItemKind::PhilosophersStone))));
        assert!(item_id_to_kind("unknown").is_none());
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
            if let Some(kind) = item_id_to_kind(id) {
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
        assert!(!eval_condition(&cond, &inv, &world, &state));

        let inv2 = make_inventory_with(&["dragon_scale", "ancient_scroll"]);
        assert!(eval_condition(&cond, &inv2, &world, &state));
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
        assert!(eval_condition(&cond, &inv, &world, &state));

        let empty = PlayerInventory::default();
        assert!(!eval_condition(&cond, &empty, &world, &state));
    }

    #[test]
    fn eval_not_inverts() {
        let inv = make_inventory_with(&["dragon_scale"]);
        let state = QuestState::default();
        let world = make_world();
        let cond = QuestCondition::Not(Box::new(QuestCondition::HasItem("ancient_scroll".into())));
        assert!(eval_condition(&cond, &inv, &world, &state));
        let cond2 = QuestCondition::Not(Box::new(QuestCondition::HasItem("dragon_scale".into())));
        assert!(!eval_condition(&cond2, &inv, &world, &state));
    }

    #[test]
    fn eval_phase_is_checks_quest_state() {
        let inv = PlayerInventory::default();
        let mut state = QuestState::default();
        let world = make_world();
        let cond = QuestCondition::PhaseIs { quest: "gem_quest".into(), phase: "done".into() };
        assert!(!eval_condition(&cond, &inv, &world, &state));
        state.set_phase("gem_quest", "done");
        assert!(eval_condition(&cond, &inv, &world, &state));
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
            .find(|a| eval_condition(&a.condition, &inv_both, &world, &state))
            .map(|a| a.next_phase.clone());
        assert_eq!(matched.as_deref(), Some("both_ready"), "둘 다 있으면 1순위가 선택돼야 한다");

        let inv_scale = make_inventory_with(&["dragon_scale"]);
        let matched2: Option<String> = phase.auto_advance.iter()
            .find(|a| eval_condition(&a.condition, &inv_scale, &world, &state))
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
            let branch = if eval_condition(condition, &inv, &world, &state) { if_true } else { if_false };
            for a in branch {
                match a {
                    QuestAction::RemoveItem(id) => {
                        if let Some(kind) = item_id_to_kind(id) {
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
        assert!(!inv.items.iter().any(|i| matches!(i.kind, ItemKind::QuestItem(QuestItemKind::DragonScale))));
    }

    #[test]
    fn new_item_ids_mapped_correctly() {
        assert!(matches!(item_id_to_kind("dragon_scale"), Some(ItemKind::QuestItem(QuestItemKind::DragonScale))));
        assert!(matches!(item_id_to_kind("ancient_scroll"), Some(ItemKind::QuestItem(QuestItemKind::AncientScroll))));
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
        assert!(!eval_condition(&cond, &inv, &world, &state));
        state.set_flag("npc_alive", "true");
        assert!(eval_condition(&cond, &inv, &world, &state));
    }

    #[test]
    fn eval_has_flag_condition() {
        let inv = PlayerInventory::default();
        let world = make_world();
        let mut state = QuestState::default();
        let cond = QuestCondition::HasFlag("village_burned".to_string());
        assert!(!eval_condition(&cond, &inv, &world, &state));
        state.set_flag("village_burned", "true");
        assert!(eval_condition(&cond, &inv, &world, &state));
        state.clear_flag("village_burned");
        assert!(!eval_condition(&cond, &inv, &world, &state));
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
        for (path, def) in load_all_quest_defs() {
            let errors = validate_quest_def(&def);
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
        for (path, def) in load_all_quest_defs() {
            for spawn in &def.spawns {
                assert!(
                    item_id_to_kind(&spawn.item).is_some(),
                    "{}: spawns 의 item_id '{}' 가 item_id_to_kind 에 없다",
                    path, spawn.item
                );
            }
        }
    }
}
