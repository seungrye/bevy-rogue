use bevy::prelude::*;
use crate::modules::{
    item::{ItemKind, PlayerInventory},
    quest::{item_id_to_kind, QuestCondition, QuestDef, QuestRegistry, QuestSpawn, QuestState},
    ui::minimap::{DiscoveredMarkers, MarkerKind, MINIMAP_DISPLAY_SIZE},
    zone::{WorldState, ZoneId},
};

const PANEL_WIDTH: f32 = MINIMAP_DISPLAY_SIZE + 10.0;
const FONT_SIZE: f32 = 13.5;

const C_HEADER:   Color = Color::rgba(0.3, 1.0, 0.3, 1.0);
const C_TITLE:    Color = Color::rgba(0.9, 0.9, 0.5, 1.0);
const C_OBJ:      Color = Color::rgba(0.82, 0.82, 0.82, 1.0);
const C_META:     Color = Color::rgba(0.55, 0.78, 1.0, 1.0);
const C_PROGRESS: Color = Color::rgba(0.75, 1.0, 0.65, 1.0);
const C_DONE:     Color = Color::rgba(0.35, 0.35, 0.35, 0.9);
const C_EMPTY:    Color = Color::rgba(0.45, 0.45, 0.45, 0.8);

#[derive(Component)] pub struct QuestPanel;
#[derive(Component)] struct QuestPanelContent;
#[derive(Resource, Default)] pub struct QuestPanelOpen(pub bool);

pub struct QuestPanelPlugin;

impl Plugin for QuestPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuestPanelOpen>()
            .add_systems(Startup, setup_quest_panel)
            .add_systems(Update, (toggle_quest_panel, update_quest_panel).chain());
    }
}

/// 이후 풍부한 퀘스트 텍스트를 넣을 숨겨진 퀘스트 패널 껍데기를 만든다.
fn setup_quest_panel(mut commands: Commands) {
    commands.spawn((
        NodeBundle {
            style: Style {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(PANEL_WIDTH),
                padding: UiRect::all(Val::Px(10.0)),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            },
            background_color: Color::rgba(0.0, 0.05, 0.0, 0.97).into(),
            z_index: ZIndex::Global(100),
            visibility: Visibility::Hidden,
            ..default()
        },
        QuestPanel,
    )).with_children(|parent| {
        parent.spawn((
            TextBundle::from_section("", TextStyle {
                font: Handle::default(),
                font_size: FONT_SIZE,
                color: Color::WHITE,
            }),
            QuestPanelContent,
        ));
    });
}

/// Q 입력으로 퀘스트 패널을 열고 닫으며, 즉각적인 반응을 위해 visibility를 바로 갱신한다.
fn toggle_quest_panel(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut panel_open: ResMut<QuestPanelOpen>,
    mut panel_q: Query<&mut Visibility, With<QuestPanel>>,
) {
    if !keyboard.just_pressed(KeyCode::KeyQ) { return; }
    panel_open.0 = !panel_open.0;
    if let Ok(mut vis) = panel_q.get_single_mut() {
        *vis = if panel_open.0 { Visibility::Inherited } else { Visibility::Hidden };
    }
}

/// 퀘스트 관련 상태가 바뀌면 패널 표시 여부와 텍스트를 갱신한다.
///
/// visibility 동기화는 키보드 토글뿐 아니라 여기에도 둔다.
/// 새 run 시작처럼 키보드 토글을 거치지 않는 흐름도 패널을 깔끔하게 닫기 위해서다.
fn update_quest_panel(
    panel_open: Res<QuestPanelOpen>,
    registry: Res<QuestRegistry>,
    state: Res<QuestState>,
    inventory: Res<PlayerInventory>,
    world: Res<WorldState>,
    markers: Res<DiscoveredMarkers>,
    asset_server: Res<AssetServer>,
    mut panel_q: Query<&mut Visibility, With<QuestPanel>>,
    mut text_q: Query<&mut Text, With<QuestPanelContent>>,
) {
    if panel_open.is_changed() {
        if let Ok(mut vis) = panel_q.get_single_mut() {
            *vis = if panel_open.0 { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
    if !panel_open.0 { return; }
    if !panel_open.is_changed()
        && !state.is_changed()
        && !inventory.is_changed()
        && !world.is_changed()
        && !markers.is_changed()
    {
        return;
    }

    let font = asset_server.load("fonts/NanumSquareNeo-bRg.ttf");
    let Ok(mut text) = text_q.get_single_mut() else { return };
    text.sections = build_quest_sections(&registry, &state, &inventory, &world, &markers, &font);
}

/// 패널 공통 글꼴 크기를 적용한 텍스트 섹션 하나를 만든다.
fn ts(value: impl Into<String>, font: Handle<Font>, color: Color) -> TextSection {
    TextSection { value: value.into(), style: TextStyle { font, font_size: FONT_SIZE, color } }
}

/// 목표 문장, 대상 존, 진행도, 다음 행동 힌트를 포함한 퀘스트 패널 텍스트를 만든다.
fn build_quest_sections(
    registry: &QuestRegistry,
    state: &QuestState,
    inventory: &PlayerInventory,
    world: &WorldState,
    markers: &DiscoveredMarkers,
    font: &Handle<Font>,
) -> Vec<TextSection> {
    let f = font.clone();
    let mut s = vec![ts("/ Q U E S T S /\n\n", f.clone(), C_HEADER)];

    let active_quests: Vec<_> = state.phases.iter()
        .filter_map(|(qid, phase_id)| {
            let def = registry.get(qid)?;
            let phase = def.phases.get(phase_id)?;
            Some((def, phase_id.as_str(), phase))
        })
        .collect();

    if active_quests.is_empty() {
        s.push(ts("진행 중인 퀘스트 없음", f.clone(), C_EMPTY));
        return s;
    }

    for (def, phase_id, phase) in active_quests {
        let done = phase_id == "done";
        let title_color = if done { C_DONE } else { C_TITLE };
        let obj_color   = if done { C_DONE } else { C_OBJ };
        let meta_color  = if done { C_DONE } else { C_META };
        let progress_color = if done { C_DONE } else { C_PROGRESS };

        let mark = if done { "[완료] " } else { "" };
        s.push(ts(format!("{}{}\n", mark, def.title), f.clone(), title_color));

        let obj_text = phase.objective.as_deref()
            .unwrap_or(phase_id);
        s.push(ts(format!("  → {}\n", obj_text), f.clone(), obj_color));

        for line in quest_location_hints(def, phase_id, state, world) {
            s.push(ts(format!("    {}\n", line), f.clone(), meta_color));
        }
        for line in quest_progress_hints(def, phase_id, state, inventory) {
            s.push(ts(format!("    {}\n", line), f.clone(), progress_color));
        }
        if !done && should_hint_giver_dialogue(def, phase) {
            let marker_hint = quest_giver_marker_hint(def, world, markers);
            s.push(ts(format!("    다음: {}와 대화{}\n", def.giver_npc, marker_hint), f.clone(), meta_color));
        }
        s.push(ts("\n", f.clone(), obj_color));
    }

    s
}

/// phase 스폰과 존 기반 자동 진행 조건에서 추론한 목표 존 힌트를 반환한다.
fn quest_location_hints(
    def: &QuestDef,
    phase_id: &str,
    state: &QuestState,
    world: &WorldState,
) -> Vec<String> {
    let mut zones = Vec::new();

    for spawn in pending_phase_spawns(def, phase_id, state) {
        push_unique_zone(&mut zones, &spawn.zone);
    }

    if let Some(phase) = def.phases.get(phase_id) {
        for auto in &phase.auto_advance {
            collect_condition_zones(&auto.condition, &mut zones);
        }
    }

    zones.into_iter()
        .map(|zone| {
            let here = if zone == world.current { " / 현재 위치" } else { "" };
            format!("위치: {}{}", zone.display_name(), here)
        })
        .collect()
}

/// 현재 phase에 속한 퀘스트 스폰의 아이템 수집 진행도를 반환한다.
fn quest_progress_hints(
    def: &QuestDef,
    phase_id: &str,
    state: &QuestState,
    inventory: &PlayerInventory,
) -> Vec<String> {
    let mut lines = Vec::new();

    for spawn in pending_phase_spawns(def, phase_id, state) {
        let Some(kind) = item_id_to_kind(&spawn.item) else { continue };
        let have = inventory_count(inventory, kind);
        let need = spawn.count.max(1);
        let status = if have >= need { "완료" } else { "진행" };
        lines.push(format!("{}: {} {}/{}", status, kind.display_name(), have.min(need), need));
    }

    lines
}

/// 아직 플레이어에게 의미가 남아 있는 현재 phase 스폰만 고른다.
fn pending_phase_spawns<'a>(
    def: &'a QuestDef,
    phase_id: &str,
    state: &QuestState,
) -> Vec<&'a QuestSpawn> {
    def.spawns.iter()
        .filter(|spawn| spawn.phase == phase_id)
        .filter(|spawn| !state.is_spawn_done(&def.id, &spawn.item))
        .collect()
}

/// 플레이어가 현재 들고 있는 같은 퀘스트 아이템 수를 센다.
fn inventory_count(inventory: &PlayerInventory, kind: ItemKind) -> u32 {
    inventory.items.iter()
        .filter(|item| item.kind == kind)
        .count() as u32
}

/// 퀘스트 데이터에서 나온 순서를 유지하면서 같은 존은 한 번만 추가한다.
fn push_unique_zone(zones: &mut Vec<ZoneId>, zone: &ZoneId) {
    if !zones.iter().any(|known| known == zone) {
        zones.push(zone.clone());
    }
}

/// 중첩된 퀘스트 조건을 순회하며 길 안내에 쓸 수 있는 모든 존 조건을 추출한다.
fn collect_condition_zones(condition: &QuestCondition, zones: &mut Vec<ZoneId>) {
    match condition {
        QuestCondition::InZone(zone) => push_unique_zone(zones, zone),
        QuestCondition::And(conditions) | QuestCondition::Or(conditions) => {
            for condition in conditions {
                collect_condition_zones(condition, zones);
            }
        }
        QuestCondition::Not(inner) => collect_condition_zones(inner, zones),
        QuestCondition::HasItem(_)
        | QuestCondition::PhaseIs { .. }
        | QuestCondition::FlagIs { .. }
        | QuestCondition::HasFlag(_) => {}
    }
}


/// 현재 존에서 발견된 퀘스트 제공자 마커가 있으면 대화 힌트에 붙일 짧은 상태 문구를 만든다.
fn quest_giver_marker_hint(
    def: &QuestDef,
    world: &WorldState,
    markers: &DiscoveredMarkers,
) -> &'static str {
    if def.giver_npc.trim().is_empty() {
        return "";
    }
    let has_marker_here = markers.0.iter().any(|marker| {
        marker.zone == world.current && marker.kind == MarkerKind::QuestGiver
    });
    if has_marker_here { " (현재 존 / 미니맵 표시)" } else { "" }
}

/// 현재 phase가 퀘스트 제공자에게 돌아가야 하는 흐름인지 판단한다.
fn should_hint_giver_dialogue(def: &QuestDef, phase: &crate::modules::quest::QuestPhaseDef) -> bool {
    !phase.on_interact.is_empty() && def.giver_npc.trim().len() > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use crate::modules::{
        item::{InventoryItem, QuestItemKind},
        quest::{AutoAdvance, QuestAction, QuestPhaseDef},
    };

    fn make_registry_and_state(phase_id: &str) -> (QuestRegistry, QuestState) {
        let mut phases = HashMap::new();
        phases.insert("active".to_string(), QuestPhaseDef {
            dialog: vec![],
            on_interact: vec![],
            auto_advance: vec![],
            objective: Some("보석을 찾아라".to_string()),
        });
        phases.insert("ready".to_string(), QuestPhaseDef {
            dialog: vec![],
            on_interact: vec![QuestAction::AdvancePhase("done".into())],
            auto_advance: vec![],
            objective: Some("보석을 장로에게 가져가라".to_string()),
        });
        phases.insert("done".to_string(), QuestPhaseDef {
            dialog: vec![],
            on_interact: vec![],
            auto_advance: vec![],
            objective: Some("완료!".to_string()),
        });
        let def = QuestDef {
            id: "gem_quest".into(),
            title: "잃어버린 보석".into(),
            giver_npc: "장로".into(),
            initial_phase: "active".into(),
            phases,
            spawns: vec![QuestSpawn {
                phase: "active".into(),
                item: "eternal_gem".into(),
                zone: ZoneId::Dungeon(2),
                count: 1,
                condition: None,
            }],
            spawn_chance: 1.0,
        };
        let mut reg = QuestRegistry::default();
        reg.quests.insert("gem_quest".into(), def);

        let mut st = QuestState::default();
        st.set_phase("gem_quest", phase_id);
        (reg, st)
    }

    fn default_inventory() -> PlayerInventory {
        PlayerInventory::default()
    }

    fn default_world() -> WorldState {
        WorldState::default()
    }

    fn all_text(sections: Vec<TextSection>) -> String {
        sections.iter().map(|s| s.value.as_str()).collect()
    }

    #[test]
    fn empty_state_shows_no_quests_message() {
        let reg = QuestRegistry::default();
        let st  = QuestState::default();
        let sections = build_quest_sections(&reg, &st, &default_inventory(), &default_world(), &DiscoveredMarkers::default(), &Handle::default());
        let all_text = all_text(sections);
        assert!(all_text.contains("진행 중인 퀘스트 없음"));
    }

    #[test]
    fn active_quest_shows_title_and_objective() {
        let (reg, st) = make_registry_and_state("active");
        let sections = build_quest_sections(&reg, &st, &default_inventory(), &default_world(), &DiscoveredMarkers::default(), &Handle::default());
        let all_text = all_text(sections);
        assert!(all_text.contains("잃어버린 보석"));
        assert!(all_text.contains("보석을 찾아라"));
    }

    #[test]
    fn active_quest_shows_target_zone_and_progress() {
        let (reg, st) = make_registry_and_state("active");
        let sections = build_quest_sections(&reg, &st, &default_inventory(), &default_world(), &DiscoveredMarkers::default(), &Handle::default());
        let all_text = all_text(sections);
        assert!(all_text.contains("위치: 던전 2층"));
        assert!(all_text.contains("진행: 영원의 보석 0/1"));
    }

    #[test]
    fn active_quest_marks_current_zone_when_target_matches_world() {
        let (reg, st) = make_registry_and_state("active");
        let mut world = default_world();
        world.current = ZoneId::Dungeon(2);
        let sections = build_quest_sections(&reg, &st, &default_inventory(), &world, &DiscoveredMarkers::default(), &Handle::default());
        let all_text = all_text(sections);
        assert!(all_text.contains("위치: 던전 2층 / 현재 위치"));
    }

    #[test]
    fn active_quest_progress_counts_inventory_items() {
        let (reg, st) = make_registry_and_state("active");
        let mut inventory = default_inventory();
        crate::modules::item::load_quest_items();
        inventory.items.push(InventoryItem { kind: ItemKind::QuestItem(QuestItemKind("eternal_gem")) });
        let sections = build_quest_sections(&reg, &st, &inventory, &default_world(), &DiscoveredMarkers::default(), &Handle::default());
        let all_text = all_text(sections);
        assert!(all_text.contains("완료: 영원의 보석 1/1"));
    }

    #[test]
    fn ready_quest_hints_giver_dialogue() {
        let (reg, st) = make_registry_and_state("ready");
        let sections = build_quest_sections(&reg, &st, &default_inventory(), &default_world(), &DiscoveredMarkers::default(), &Handle::default());
        let all_text = all_text(sections);
        assert!(all_text.contains("다음: 장로와 대화"));
    }

    #[test]
    fn ready_quest_hints_current_zone_marker_when_giver_discovered() {
        let (reg, st) = make_registry_and_state("ready");
        let mut markers = DiscoveredMarkers::default();
        markers.add(4, 5, MarkerKind::QuestGiver, ZoneId::Town);

        let sections = build_quest_sections(&reg, &st, &default_inventory(), &default_world(), &markers, &Handle::default());
        let all_text = all_text(sections);

        assert!(all_text.contains("다음: 장로와 대화 (현재 존 / 미니맵 표시)"));
    }

    #[test]
    fn done_quest_shows_completed_mark() {
        let (reg, st) = make_registry_and_state("done");
        let sections = build_quest_sections(&reg, &st, &default_inventory(), &default_world(), &DiscoveredMarkers::default(), &Handle::default());
        let all_text = all_text(sections);
        assert!(all_text.contains("[완료]"));
    }

    #[test]
    fn phase_without_objective_falls_back_to_phase_id() {
        let mut reg = QuestRegistry::default();
        let mut phases = HashMap::new();
        phases.insert("active".to_string(), QuestPhaseDef {
            dialog: vec![],
            on_interact: vec![],
            auto_advance: vec![],
            objective: None,
        });
        reg.quests.insert("q".into(), QuestDef {
            id: "q".into(),
            title: "테스트".into(),
            giver_npc: "npc".into(),
            initial_phase: "active".into(),
            phases,
            spawns: vec![],
            spawn_chance: 1.0,
        });
        let mut st = QuestState::default();
        st.set_phase("q", "active");
        let sections = build_quest_sections(&reg, &st, &default_inventory(), &default_world(), &DiscoveredMarkers::default(), &Handle::default());
        let all_text = all_text(sections);
        assert!(all_text.contains("active"));
    }

    #[test]
    fn nested_zone_conditions_are_collected_once() {
        let condition = QuestCondition::And(vec![
            QuestCondition::InZone(ZoneId::Forest),
            QuestCondition::Or(vec![
                QuestCondition::InZone(ZoneId::Forest),
                QuestCondition::InZone(ZoneId::Dungeon(1)),
            ]),
        ]);
        let mut zones = Vec::new();
        collect_condition_zones(&condition, &mut zones);
        assert_eq!(zones, vec![ZoneId::Forest, ZoneId::Dungeon(1)]);
    }

    #[test]
    fn auto_advance_zone_conditions_become_location_hints() {
        let mut phases = HashMap::new();
        phases.insert("travel".to_string(), QuestPhaseDef {
            dialog: vec![],
            on_interact: vec![],
            auto_advance: vec![AutoAdvance {
                condition: QuestCondition::InZone(ZoneId::Forest),
                next_phase: "done".into(),
                actions: vec![],
            }],
            objective: Some("숲으로 이동".into()),
        });
        phases.insert("done".to_string(), QuestPhaseDef {
            dialog: vec![],
            on_interact: vec![],
            auto_advance: vec![],
            objective: Some("완료".into()),
        });
        let def = QuestDef {
            id: "travel".into(),
            title: "여행".into(),
            giver_npc: "가이드".into(),
            initial_phase: "travel".into(),
            phases,
            spawns: vec![],
            spawn_chance: 1.0,
        };
        let hints = quest_location_hints(&def, "travel", &QuestState::default(), &default_world());
        assert_eq!(hints, vec!["위치: 숲".to_string()]);
    }
}
