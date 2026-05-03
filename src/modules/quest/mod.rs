use bevy::prelude::*;
use std::collections::HashMap;
use serde::Deserialize;
use crate::modules::{
    item::{PlayerInventory, ItemKind, QuestItemKind, InventoryItem, Item},
    map::{MapResource, TILE_SIZE, tile_to_world_coords},
    zone::ZoneId,
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
    pub auto_advance: Option<AutoAdvance>,
    #[serde(default)]
    pub objective: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AutoAdvance {
    pub condition: QuestCondition,
    pub next_phase: String,
}

#[derive(Debug, Deserialize, Clone)]
pub enum QuestCondition {
    HasItem(String),
    InZone(ZoneId),
    PhaseIs { quest: String, phase: String },
}

#[derive(Debug, Deserialize, Clone)]
pub enum QuestAction {
    AdvancePhase(String),
    GiveItem(String),
    RemoveItem(String),
}

#[derive(Debug, Deserialize, Clone)]
pub struct QuestSpawn {
    pub phase: String,
    pub item: String,
    pub zone: ZoneId,
}

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
    pub phases: HashMap<String, String>,        // quest_id → current_phase_id
    pub spawned: std::collections::HashSet<String>, // "quest_id:item_id" 이미 스폰됨
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
}

// ── Plugin ───────────────────────────────────────────────────────────────────

pub struct QuestPlugin;

impl Plugin for QuestPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuestRegistry>()
            .init_resource::<QuestState>()
            .add_systems(Startup, load_quests)
            .add_systems(Update, (check_auto_advance, spawn_quest_items));
    }
}

// ── Systems ──────────────────────────────────────────────────────────────────

fn load_quests(mut registry: ResMut<QuestRegistry>) {
    let Ok(dir) = std::fs::read_dir("assets/quests") else {
        warn!("assets/quests 디렉터리를 찾을 수 없습니다.");
        return;
    };
    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ron") { continue; }
        let Ok(text) = std::fs::read_to_string(&path) else {
            warn!("퀘스트 파일 읽기 실패: {:?}", path);
            continue;
        };
        match ron::de::from_str::<QuestDef>(&text) {
            Ok(def) => {
                info!("퀘스트 로드: {} ({})", def.title, def.id);
                registry.quests.insert(def.id.clone(), def);
            }
            Err(e) => warn!("퀘스트 파싱 오류 {:?}: {}", path, e),
        }
    }
}

/// auto_advance 조건을 매 프레임 평가하여 단계를 자동 전진시킨다
fn check_auto_advance(
    registry: Res<QuestRegistry>,
    mut state: ResMut<QuestState>,
    inventory: Res<PlayerInventory>,
    world: Res<crate::modules::zone::WorldState>,
) {
    let mut advances: Vec<(String, String)> = Vec::new();

    for (quest_id, quest_def) in &registry.quests {
        let current = match state.phases.get(quest_id) {
            Some(p) => p.clone(),
            None => continue,
        };
        let phase_def = match quest_def.phases.get(&current) {
            Some(p) => p,
            None => continue,
        };
        if let Some(auto) = &phase_def.auto_advance {
            if eval_condition(&auto.condition, &inventory, &world) {
                advances.push((quest_id.clone(), auto.next_phase.clone()));
            }
        }
    }

    for (quest_id, next_phase) in advances {
        info!("퀘스트 [{}] 자동 전진: {}", quest_id, next_phase);
        state.set_phase(&quest_id, &next_phase);
    }
}

// ── 퀘스트 액션 실행 (빌리저 시스템에서 호출) ────────────────────────────────

pub fn execute_actions(
    actions: &[QuestAction],
    quest_id: &str,
    state: &mut QuestState,
    inventory: &mut PlayerInventory,
    log: &mut EventWriter<crate::modules::ui::LogMessage>,
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
        }
    }
}

// ── 조건 평가 ────────────────────────────────────────────────────────────────

pub fn eval_condition(
    cond: &QuestCondition,
    inventory: &PlayerInventory,
    world: &crate::modules::zone::WorldState,
) -> bool {
    match cond {
        QuestCondition::HasItem(item_id) => {
            let Some(kind) = item_id_to_kind(item_id) else { return false };
            inventory.items.iter().any(|i| i.kind == kind)
        }
        QuestCondition::InZone(zone) => &world.current == zone,
        QuestCondition::PhaseIs { quest, phase } => {
            // 다른 퀘스트 단계는 이 함수 밖에서 처리(전달된 state가 없음)
            // 단순화: 항상 false (복잡한 퀘스트 체인은 향후 확장)
            let _ = (quest, phase);
            false
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
        "sword"               => Some(ItemKind::Weapon(crate::modules::item::WeaponKind::Sword)),
        "spear"               => Some(ItemKind::Weapon(crate::modules::item::WeaponKind::Spear)),
        "bow"                 => Some(ItemKind::Weapon(crate::modules::item::WeaponKind::Bow)),
        "leather_armor"       => Some(ItemKind::Armor(crate::modules::item::ArmorKind::LeatherArmor)),
        "health_potion"       => Some(ItemKind::Consumable(crate::modules::item::ConsumableKind::HealthPotion)),
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
                    auto_advance: None,
                    objective: None,
                });
                m.insert("active".into(), QuestPhaseDef {
                    dialog: vec!["아직".into()],
                    on_interact: vec![],
                    auto_advance: Some(AutoAdvance {
                        condition: QuestCondition::HasItem("eternal_gem".into()),
                        next_phase: "ready".into(),
                    }),
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
}
