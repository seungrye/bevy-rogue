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

const C_JOURNAL: Color = Color::rgba(1.0, 0.85, 0.4, 1.0);

#[derive(Component)] pub struct QuestPanel;
#[derive(Component)] struct QuestPanelContent;
#[derive(Resource, Default)] pub struct QuestPanelOpen(pub bool);

/// 진행 중(활성)인 퀘스트의 `(제목, 현재 목표)` 목록을 순수 함수로 만든다.
///
/// 저널은 **읽기 전용** 으로, 지금 플레이어가 추적해야 할 목표만 보여준다.
/// 따라서 다음 퀘스트는 제외한다:
/// - **미시작**: `QuestState.phases` 에 없거나 현재 페이즈가 `initial_phase` 인 퀘스트
///   (아직 수락 전이므로 추적할 목표가 없다).
/// - **완료(터미널)**: 현재 페이즈에서 시작하는 전환이 하나도 없는 퀘스트
///   (`done` 등 종착 페이즈). villager 모듈의 터미널 판정과 같은 규칙을 쓴다.
///
/// 목표 문구는 현재 페이즈의 `objective` 를 쓰되, 없으면 페이즈 ID 로 대체한다
/// (패널 렌더(`build_quest_sections`)의 fallback 과 동일하게 일관 유지).
///
/// 실제 패널과 단위 테스트가 같은 로직을 공유하도록 순수 함수로 분리한다.
pub fn journal_entries(
    quest_state: &QuestState,
    registry: &QuestRegistry,
) -> Vec<(String, String)> {
    quest_state.phases.iter()
        .filter_map(|(qid, phase_id)| {
            let def = registry.get(qid)?;
            // 미시작(initial_phase) 제외 — 아직 수락 전이라 추적 목표 없음.
            if phase_id == &def.initial_phase { return None; }
            let phase = def.phases.get(phase_id)?;
            // 완료(터미널: 현재 페이즈에서 나가는 전환 없음) 제외.
            if is_terminal_phase(def, phase_id) { return None; }
            let objective = phase.objective.clone().unwrap_or_else(|| phase_id.clone());
            Some((def.title.clone(), objective))
        })
        .collect()
}

/// 현재 페이즈에서 시작하는 전환이 하나도 없으면 터미널(완료) 페이즈다.
/// villager 모듈의 `is_quest_terminal_def` 와 같은 규칙을 quest_panel 안에서 재사용한다.
fn is_terminal_phase(def: &QuestDef, phase_id: &str) -> bool {
    !def.transitions.iter().any(|t| t.from == phase_id)
}

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

/// `Q` 또는 `J`(저널) 입력으로 퀘스트 패널을 열고 닫으며, 즉각적인 반응을 위해 visibility를 바로 갱신한다.
///
/// 저널은 별도 패널이 아니라 같은 퀘스트 패널이므로 두 키 모두 동일 패널을 토글한다
/// (모달/입력 흐름은 기존 패널과 동일하게 유지).
fn toggle_quest_panel(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut panel_open: ResMut<QuestPanelOpen>,
    mut panel_q: Query<&mut Visibility, With<QuestPanel>>,
    defeated_q: Query<(), With<crate::modules::combat::Defeated>>,
) {
    if !defeated_q.is_empty() { return; }
    if !keyboard.just_pressed(KeyCode::KeyQ) && !keyboard.just_pressed(KeyCode::KeyJ) { return; }
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
    quest_items: Res<crate::modules::item::QuestItemRegistry>,
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
    text.sections = build_quest_sections(&registry, &state, &inventory, &world, &markers, &font, &quest_items);
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
    quest_items: &crate::modules::item::QuestItemRegistry,
) -> Vec<TextSection> {
    let f = font.clone();
    let mut s = vec![ts("/ Q U E S T S /\n\n", f.clone(), C_HEADER)];

    // 진행 중(활성) 퀘스트 목표만 모은 읽기 전용 저널 요약 (Q/J 공용 패널 상단).
    push_journal_summary(&mut s, registry, state, &f);

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
        for line in quest_progress_hints(def, phase_id, state, inventory, quest_items) {
            s.push(ts(format!("    {}\n", line), f.clone(), progress_color));
        }
        if !done && should_hint_giver_dialogue(def, phase_id) {
            let marker_hint = quest_giver_marker_hint(def, world, markers);
            s.push(ts(format!("    다음: {}와 대화{}\n", def.giver_npc, marker_hint), f.clone(), meta_color));
        }
        s.push(ts("\n", f.clone(), obj_color));
    }

    s
}

/// `journal_entries` 로 뽑은 진행 중 퀘스트의 `(제목 — 목표)` 요약을 패널 상단에 추가한다.
///
/// 읽기 전용 저널: 지금 추적할 목표만 한눈에 보여주고(미시작/완료 제외),
/// 진행 중인 퀘스트가 하나도 없으면 안내 문구만 둔다.
fn push_journal_summary(
    s: &mut Vec<TextSection>,
    registry: &QuestRegistry,
    state: &QuestState,
    f: &Handle<Font>,
) {
    s.push(ts("[ 저널 ] (J / Q)\n", f.clone(), C_HEADER));
    let entries = journal_entries(state, registry);
    if entries.is_empty() {
        s.push(ts("진행 중인 목표 없음\n\n", f.clone(), C_EMPTY));
        return;
    }
    for (title, objective) in entries {
        s.push(ts(format!("• {} — {}\n", title, objective), f.clone(), C_JOURNAL));
    }
    s.push(ts("\n", f.clone(), C_JOURNAL));
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

    // 현재 phase 에서 시작하는 Auto transition 의 조건에서 존 힌트 추출
    for t in &def.transitions {
        if t.from == phase_id && t.trigger == crate::modules::quest::TriggerKind::Auto {
            if let Some(cond) = &t.when {
                collect_condition_zones(cond, &mut zones);
            }
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
    quest_items: &crate::modules::item::QuestItemRegistry,
) -> Vec<String> {
    let mut lines = Vec::new();

    for spawn in pending_phase_spawns(def, phase_id, state) {
        let Some(kind) = item_id_to_kind(&spawn.item, quest_items) else { continue };
        let have = inventory_count(inventory, kind);
        let need = spawn.count.max(1);
        let status = if have >= need { "완료" } else { "진행" };
        lines.push(format!("{}: {} {}/{}", status, kind.display_name(quest_items), have.min(need), need));
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
/// 현재 phase 에서 시작하는 Interact transition 이 있으면 giver 와 대화하라는 힌트를 띄운다.
fn should_hint_giver_dialogue(def: &QuestDef, phase_id: &str) -> bool {
    !def.giver_npc.trim().is_empty()
        && def.transitions.iter().any(|t|
            t.from == phase_id && t.trigger == crate::modules::quest::TriggerKind::Interact)
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;
    use std::collections::HashMap;
    use crate::modules::{
        item::{InventoryItem, QuestItemKind},
        quest::{QuestPhaseDef, QuestTransition, TriggerKind},
    };

    fn phase(objective: &str) -> QuestPhaseDef {
        QuestPhaseDef {
            dialog: vec![],
            objective: Some(objective.to_string()),
        }
    }

    fn make_registry_and_state(phase_id: &str) -> (QuestRegistry, QuestState) {
        let mut phases = HashMap::new();
        phases.insert("active".to_string(), phase("보석을 찾아라"));
        phases.insert("ready".to_string(), phase("보석을 장로에게 가져가라"));
        phases.insert("done".to_string(), phase("완료!"));
        let def = QuestDef {
            id: "gem_quest".into(),
            title: "잃어버린 보석".into(),
            giver_npc: "장로".into(),
            initial_phase: "active".into(),
            phases,
            transitions: vec![
                QuestTransition {
                    from: "active".into(),
                    trigger: TriggerKind::Auto,
                    when: Some(QuestCondition::HasItem("eternal_gem".into())),
                    actions: vec![],
                    to: "ready".into(),
                },
                QuestTransition {
                    from: "ready".into(),
                    trigger: TriggerKind::Interact,
                    when: None,
                    actions: vec![],
                    to: "done".into(),
                },
            ],
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

    use std::sync::OnceLock;
    static TEST_QI: OnceLock<crate::modules::item::QuestItemRegistry> = OnceLock::new();
    fn qi() -> &'static crate::modules::item::QuestItemRegistry {
        TEST_QI.get_or_init(|| crate::modules::item::build_test_registry())
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

    // ── 패널 텍스트 빌더 ─────────────────────────────────────────────────

    #[test]
    fn 진행중인_퀘스트가_없으면_없음_안내가_나온다() {
        let reg = QuestRegistry::default();
        let st  = QuestState::default();
        let sections = build_quest_sections(&reg, &st, &default_inventory(), &default_world(), &DiscoveredMarkers::default(), &Handle::default(), qi());
        let all_text = all_text(sections);
        assert!(all_text.contains("진행 중인 퀘스트 없음"));
    }

    #[test]
    fn 진행중인_퀘스트는_제목과_목표를_보여준다() {
        let (reg, st) = make_registry_and_state("active");
        let sections = build_quest_sections(&reg, &st, &default_inventory(), &default_world(), &DiscoveredMarkers::default(), &Handle::default(), qi());
        let all_text = all_text(sections);
        assert!(all_text.contains("잃어버린 보석"));
        assert!(all_text.contains("보석을 찾아라"));
    }

    #[test]
    fn 진행중인_퀘스트는_대상_존과_진행도를_보여준다() {
        let (reg, st) = make_registry_and_state("active");
        let sections = build_quest_sections(&reg, &st, &default_inventory(), &default_world(), &DiscoveredMarkers::default(), &Handle::default(), qi());
        let all_text = all_text(sections);
        assert!(all_text.contains("위치: 던전 2층"));
        assert!(all_text.contains("진행: 영원의 보석 0/1"));
    }

    #[test]
    fn 대상존이_현재위치와_같으면_현재위치로_표시한다() {
        let (reg, st) = make_registry_and_state("active");
        let mut world = default_world();
        world.current = ZoneId::Dungeon(2);
        let sections = build_quest_sections(&reg, &st, &default_inventory(), &world, &DiscoveredMarkers::default(), &Handle::default(), qi());
        let all_text = all_text(sections);
        assert!(all_text.contains("위치: 던전 2층 / 현재 위치"));
    }

    #[test]
    fn 진행도는_인벤토리의_퀘스트아이템_수를_센다() {
        let (reg, st) = make_registry_and_state("active");
        let mut inventory = default_inventory();
        let _ = qi();
        inventory.items.push(InventoryItem::new(ItemKind::QuestItem(QuestItemKind("eternal_gem"))));
        let sections = build_quest_sections(&reg, &st, &inventory, &default_world(), &DiscoveredMarkers::default(), &Handle::default(), qi());
        let all_text = all_text(sections);
        assert!(all_text.contains("완료: 영원의 보석 1/1"));
    }

    #[test]
    fn 제출_준비된_퀘스트는_제공자와_대화하라는_힌트를_준다() {
        let (reg, st) = make_registry_and_state("ready");
        let sections = build_quest_sections(&reg, &st, &default_inventory(), &default_world(), &DiscoveredMarkers::default(), &Handle::default(), qi());
        let all_text = all_text(sections);
        assert!(all_text.contains("다음: 장로와 대화"));
    }

    #[test]
    fn 제공자_마커가_현재존에서_발견되면_미니맵_표시_힌트가_붙는다() {
        let (reg, st) = make_registry_and_state("ready");
        let mut markers = DiscoveredMarkers::default();
        markers.add(4, 5, MarkerKind::QuestGiver, ZoneId::Town);

        let sections = build_quest_sections(&reg, &st, &default_inventory(), &default_world(), &markers, &Handle::default(), qi());
        let all_text = all_text(sections);

        assert!(all_text.contains("다음: 장로와 대화 (현재 존 / 미니맵 표시)"));
    }

    #[test]
    fn 완료된_퀘스트는_완료_표식을_보여준다() {
        let (reg, st) = make_registry_and_state("done");
        let sections = build_quest_sections(&reg, &st, &default_inventory(), &default_world(), &DiscoveredMarkers::default(), &Handle::default(), qi());
        let all_text = all_text(sections);
        assert!(all_text.contains("[완료]"));
    }

    #[test]
    fn 목표가_없는_페이즈는_페이즈ID로_대체_표시한다() {
        let mut reg = QuestRegistry::default();
        let mut phases = HashMap::new();
        phases.insert("active".to_string(), QuestPhaseDef {
            dialog: vec![],
            objective: None,
        });
        reg.quests.insert("q".into(), QuestDef {
            id: "q".into(),
            title: "테스트".into(),
            giver_npc: "npc".into(),
            initial_phase: "active".into(),
            phases,
            transitions: vec![],
            spawns: vec![],
            spawn_chance: 1.0,
        });
        let mut st = QuestState::default();
        st.set_phase("q", "active");
        let sections = build_quest_sections(&reg, &st, &default_inventory(), &default_world(), &DiscoveredMarkers::default(), &Handle::default(), qi());
        let all_text = all_text(sections);
        assert!(all_text.contains("active"));
    }

    #[test]
    fn 제공자가_없는_퀘스트는_대화_힌트를_띄우지_않는다() {
        // should_hint_giver_dialogue 의 거짓 분기 + giver 빈 문자열로 패널 빌드.
        let mut reg = QuestRegistry::default();
        let mut phases = HashMap::new();
        phases.insert("active".to_string(), phase("뭔가를 해라"));
        reg.quests.insert("q".into(), QuestDef {
            id: "q".into(),
            title: "이름없는 의뢰".into(),
            giver_npc: "".into(),
            initial_phase: "active".into(),
            phases,
            transitions: vec![
                QuestTransition {
                    from: "active".into(),
                    trigger: TriggerKind::Interact,
                    when: None,
                    actions: vec![],
                    to: "done".into(),
                },
            ],
            spawns: vec![],
            spawn_chance: 1.0,
        });
        let mut st = QuestState::default();
        st.set_phase("q", "active");
        let sections = build_quest_sections(&reg, &st, &default_inventory(), &default_world(), &DiscoveredMarkers::default(), &Handle::default(), qi());
        let all_text = all_text(sections);
        assert!(!all_text.contains("다음:"));
    }

    // ── 순수 함수 journal_entries (§D) ───────────────────────────────────

    #[test]
    fn 진행중인_퀘스트만_저널에_현재목표와_함께_표시된다() {
        // make_registry_and_state 의 phase "ready" 는 initial_phase("active")가 아니고
        // 터미널(전환 없음)도 아니므로 진행 중 → 저널에 포함된다.
        let (reg, st) = make_registry_and_state("ready");
        let entries = journal_entries(&st, &reg);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "잃어버린 보석");
        assert_eq!(entries[0].1, "보석을 장로에게 가져가라");
    }

    #[test]
    fn 아직_수락하지_않은_initial_phase_퀘스트는_저널에서_제외된다() {
        // initial_phase 와 같은 페이즈("active")는 미시작으로 간주 → 저널 제외.
        let (reg, st) = make_registry_and_state("active");
        assert!(journal_entries(&st, &reg).is_empty());
    }

    #[test]
    fn 완료된_터미널_퀘스트는_저널에서_제외된다() {
        // "done" 에서 시작하는 전환이 없으므로 터미널(완료) → 저널 제외.
        let (reg, st) = make_registry_and_state("done");
        assert!(journal_entries(&st, &reg).is_empty());
    }

    #[test]
    fn 진행중인_퀘스트가_하나도_없으면_저널은_빈_목록이다() {
        let reg = QuestRegistry::default();
        let st = QuestState::default();
        assert!(journal_entries(&st, &reg).is_empty());
    }

    #[test]
    fn 레지스트리에_없는_퀘스트는_저널에서_건너뛴다() {
        // registry.get(qid) == None 분기.
        let reg = QuestRegistry::default();
        let mut st = QuestState::default();
        st.set_phase("유령_퀘스트", "진행중");
        assert!(journal_entries(&st, &reg).is_empty());
    }

    #[test]
    fn 상태에_있지만_정의에_없는_페이즈는_저널에서_건너뛴다() {
        // def.phases.get(phase_id) == None 분기 (state 의 phase 가 def 에 없음).
        let (reg, _) = make_registry_and_state("ready");
        let mut st = QuestState::default();
        st.set_phase("gem_quest", "존재하지_않는_페이즈");
        assert!(journal_entries(&st, &reg).is_empty());
    }

    #[test]
    fn objective가_없는_진행페이즈는_페이즈ID를_목표로_대체한다() {
        // objective: None 인 진행 중 페이즈 → 페이즈 ID 로 fallback.
        let mut reg = QuestRegistry::default();
        let mut phases = HashMap::new();
        phases.insert("start".to_string(), phase("시작"));
        phases.insert("진행중페이즈".to_string(), QuestPhaseDef { dialog: vec![], objective: None });
        reg.quests.insert("q".into(), QuestDef {
            id: "q".into(),
            title: "목표없는 퀘스트".into(),
            giver_npc: "npc".into(),
            initial_phase: "start".into(),
            phases,
            transitions: vec![QuestTransition {
                from: "진행중페이즈".into(),
                trigger: TriggerKind::Interact,
                when: None,
                actions: vec![],
                to: "끝".into(),
            }],
            spawns: vec![],
            spawn_chance: 1.0,
        });
        let mut st = QuestState::default();
        st.set_phase("q", "진행중페이즈");
        let entries = journal_entries(&st, &reg);
        assert_eq!(entries, vec![("목표없는 퀘스트".to_string(), "진행중페이즈".to_string())]);
    }

    #[test]
    fn 저널은_진행중_퀘스트만_담고_미시작과_완료는_함께_있어도_제외한다() {
        // 한 상태에 미시작/진행중/완료 퀘스트를 모두 두고 진행 중만 남는지 확인.
        let mut reg = QuestRegistry::default();
        let three_phase = |start_obj: &str| {
            let mut p = HashMap::new();
            p.insert("start".to_string(), phase(start_obj));
            p.insert("mid".to_string(), phase("중간 목표"));
            p.insert("done".to_string(), phase("끝"));
            p
        };
        let make = |id: &str| QuestDef {
            id: id.into(),
            title: id.into(),
            giver_npc: "npc".into(),
            initial_phase: "start".into(),
            phases: three_phase("시작 목표"),
            transitions: vec![
                QuestTransition { from: "start".into(), trigger: TriggerKind::Interact, when: None, actions: vec![], to: "mid".into() },
                QuestTransition { from: "mid".into(), trigger: TriggerKind::Interact, when: None, actions: vec![], to: "done".into() },
            ],
            spawns: vec![],
            spawn_chance: 1.0,
        };
        reg.quests.insert("미시작".into(), make("미시작"));
        reg.quests.insert("진행중".into(), make("진행중"));
        reg.quests.insert("완료".into(), make("완료"));

        let mut st = QuestState::default();
        st.set_phase("미시작", "start"); // initial_phase → 제외
        st.set_phase("진행중", "mid");   // 진행 중 → 포함
        st.set_phase("완료", "done");    // 터미널 → 제외

        let entries = journal_entries(&st, &reg);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], ("진행중".to_string(), "중간 목표".to_string()));
    }

    // ── 저널 요약 패널 렌더 (push_journal_summary) ───────────────────────

    #[test]
    fn 패널_상단_저널요약은_진행중_퀘스트_목표를_보여준다() {
        let (reg, st) = make_registry_and_state("ready");
        let sections = build_quest_sections(&reg, &st, &default_inventory(), &default_world(), &DiscoveredMarkers::default(), &Handle::default(), qi());
        let all_text = all_text(sections);
        assert!(all_text.contains("[ 저널 ] (J / Q)"));
        assert!(all_text.contains("• 잃어버린 보석 — 보석을 장로에게 가져가라"));
    }

    #[test]
    fn 진행중_퀘스트가_없으면_저널요약은_진행중_목표_없음을_안내한다() {
        // 완료(done)만 있는 상태 → 저널 요약은 비어 안내 문구.
        let (reg, st) = make_registry_and_state("done");
        let sections = build_quest_sections(&reg, &st, &default_inventory(), &default_world(), &DiscoveredMarkers::default(), &Handle::default(), qi());
        let all_text = all_text(sections);
        assert!(all_text.contains("진행 중인 목표 없음"));
    }

    // ── 조건 존 수집 ─────────────────────────────────────────────────────

    #[test]
    fn 중첩된_존_조건은_중복없이_한번씩만_수집된다() {
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
    fn Not조건_안의_존도_수집된다() {
        // collect_condition_zones 의 Not arm 도달.
        let condition = QuestCondition::Not(Box::new(QuestCondition::InZone(ZoneId::Forest)));
        let mut zones = Vec::new();
        collect_condition_zones(&condition, &mut zones);
        assert_eq!(zones, vec![ZoneId::Forest]);
    }

    #[test]
    fn 존과_무관한_조건들은_존을_수집하지_않는다() {
        // HasItem/PhaseIs/FlagIs/HasFlag 의 빈 arm 도달.
        let mut zones = Vec::new();
        collect_condition_zones(&QuestCondition::HasItem("x".into()), &mut zones);
        collect_condition_zones(&QuestCondition::HasFlag("f".into()), &mut zones);
        assert!(zones.is_empty());
    }

    #[test]
    fn Auto전이의_존_조건은_위치_힌트가_된다() {
        let mut phases = HashMap::new();
        phases.insert("travel".to_string(), phase("숲으로 이동"));
        phases.insert("done".to_string(), phase("완료"));
        let def = QuestDef {
            id: "travel".into(),
            title: "여행".into(),
            giver_npc: "가이드".into(),
            initial_phase: "travel".into(),
            phases,
            transitions: vec![
                QuestTransition {
                    from: "travel".into(),
                    trigger: TriggerKind::Auto,
                    when: Some(QuestCondition::InZone(ZoneId::Forest)),
                    actions: vec![],
                    to: "done".into(),
                },
            ],
            spawns: vec![],
            spawn_chance: 1.0,
        };
        let hints = quest_location_hints(&def, "travel", &QuestState::default(), &default_world());
        assert_eq!(hints, vec!["위치: 숲".to_string()]);
    }

    #[test]
    fn 조건없는_Auto전이는_위치_힌트를_만들지_않는다() {
        // quest_location_hints: t.when == None 분기.
        let mut phases = HashMap::new();
        phases.insert("a".to_string(), phase("진행"));
        phases.insert("done".to_string(), phase("완료"));
        let def = QuestDef {
            id: "q".into(),
            title: "t".into(),
            giver_npc: "npc".into(),
            initial_phase: "a".into(),
            phases,
            transitions: vec![
                QuestTransition {
                    from: "a".into(),
                    trigger: TriggerKind::Auto,
                    when: None,
                    actions: vec![],
                    to: "done".into(),
                },
            ],
            spawns: vec![],
            spawn_chance: 1.0,
        };
        let hints = quest_location_hints(&def, "a", &QuestState::default(), &default_world());
        assert!(hints.is_empty());
    }

    #[test]
    fn 알수없는_아이템ID의_스폰은_진행도에서_건너뛴다() {
        // quest_progress_hints: item_id_to_kind None → continue 분기.
        let mut phases = HashMap::new();
        phases.insert("a".to_string(), phase("진행"));
        let def = QuestDef {
            id: "q".into(),
            title: "t".into(),
            giver_npc: "npc".into(),
            initial_phase: "a".into(),
            phases,
            transitions: vec![],
            spawns: vec![QuestSpawn {
                phase: "a".into(),
                item: "존재하지_않는_아이템".into(),
                zone: ZoneId::Town,
                count: 1,
                condition: None,
            }],
            spawn_chance: 1.0,
        };
        let lines = quest_progress_hints(&def, "a", &QuestState::default(), &default_inventory(), qi());
        assert!(lines.is_empty());
    }

    // ── 제공자 마커 힌트 순수 함수 ───────────────────────────────────────

    #[test]
    fn 제공자가_빈_문자열이면_마커_힌트는_빈_문자열이다() {
        // quest_giver_marker_hint 의 giver 비어있음 조기 반환.
        let def = QuestDef {
            id: "q".into(),
            title: "t".into(),
            giver_npc: "   ".into(),
            initial_phase: "a".into(),
            phases: HashMap::new(),
            transitions: vec![],
            spawns: vec![],
            spawn_chance: 1.0,
        };
        let hint = quest_giver_marker_hint(&def, &default_world(), &DiscoveredMarkers::default());
        assert_eq!(hint, "");
    }

    #[test]
    fn 현재존에_제공자가_아닌_마커만_있으면_힌트는_빈_문자열이다() {
        // has_marker_here 판정의 && 두번째 피연산자(kind == QuestGiver) 거짓.
        let def = QuestDef {
            id: "q".into(),
            title: "t".into(),
            giver_npc: "장로".into(),
            initial_phase: "a".into(),
            phases: HashMap::new(),
            transitions: vec![],
            spawns: vec![],
            spawn_chance: 1.0,
        };
        let mut markers = DiscoveredMarkers::default();
        // 현재 존(Town)에 있지만 QuestGiver 가 아닌 마커.
        markers.add(1, 1, MarkerKind::Portal, ZoneId::Town);
        let hint = quest_giver_marker_hint(&def, &default_world(), &markers);
        assert_eq!(hint, "");
    }

    #[test]
    fn 제공자_마커가_현재존에_없으면_힌트는_빈_문자열이다() {
        // has_marker_here == false 분기.
        let def = QuestDef {
            id: "q".into(),
            title: "t".into(),
            giver_npc: "장로".into(),
            initial_phase: "a".into(),
            phases: HashMap::new(),
            transitions: vec![],
            spawns: vec![],
            spawn_chance: 1.0,
        };
        let hint = quest_giver_marker_hint(&def, &default_world(), &DiscoveredMarkers::default());
        assert_eq!(hint, "");
    }

    #[test]
    fn 다른_존의_제공자_마커는_현재존_힌트를_만들지_않는다() {
        // && 첫 피연산자(marker.zone == world.current) 거짓 분기.
        let def = QuestDef {
            id: "q".into(),
            title: "t".into(),
            giver_npc: "장로".into(),
            initial_phase: "a".into(),
            phases: HashMap::new(),
            transitions: vec![],
            spawns: vec![],
            spawn_chance: 1.0,
        };
        let mut markers = DiscoveredMarkers::default();
        // 현재 존(Town)이 아닌 다른 존의 제공자 마커.
        markers.add(2, 2, MarkerKind::QuestGiver, ZoneId::Forest);
        let hint = quest_giver_marker_hint(&def, &default_world(), &markers);
        assert_eq!(hint, "");
    }

    // ── App 하네스: 플러그인 / 셋업 / 토글 / 갱신 ────────────────────────

    fn 렌더_하네스() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app.init_resource::<QuestPanelOpen>()
            .init_resource::<QuestRegistry>()
            .init_resource::<QuestState>()
            .init_resource::<PlayerInventory>()
            .init_resource::<WorldState>()
            .init_resource::<DiscoveredMarkers>()
            .insert_resource(crate::modules::item::build_test_registry());
        app
    }

    fn 키_입력_하네스() -> App {
        let mut app = 렌더_하네스();
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app
    }

    #[test]
    fn 플러그인을_등록하면_퀘스트패널_상태가_초기화된다() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app.init_resource::<QuestRegistry>()
            .init_resource::<QuestState>()
            .init_resource::<PlayerInventory>()
            .init_resource::<WorldState>()
            .init_resource::<DiscoveredMarkers>()
            .insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(crate::modules::item::build_test_registry());
        app.add_plugins(QuestPanelPlugin);
        assert!(app.world.contains_resource::<QuestPanelOpen>());
    }

    #[test]
    fn 시작시_퀘스트_패널과_텍스트가_생성된다() {
        let mut app = 렌더_하네스();
        app.add_systems(Startup, setup_quest_panel);
        app.update();
        assert_eq!(app.world.query_filtered::<(), With<QuestPanel>>().iter(&app.world).count(), 1);
        assert_eq!(app.world.query_filtered::<(), With<QuestPanelContent>>().iter(&app.world).count(), 1);
    }

    #[test]
    fn Q키를_누르면_패널이_열리고_보임으로_바뀐다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, toggle_quest_panel);
        app.world.spawn((Visibility::Hidden, QuestPanel));
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyQ);
        app.update();
        assert!(app.world.resource::<QuestPanelOpen>().0);
        let vis = *app.world.query_filtered::<&Visibility, With<QuestPanel>>().single(&app.world);
        assert!(matches!(vis, Visibility::Inherited));
    }

    #[test]
    fn J키를_누르면_저널_패널이_열리고_보임으로_바뀐다() {
        // 저널은 별도 패널이 아니라 같은 퀘스트 패널 → J 로도 토글된다.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, toggle_quest_panel);
        app.world.spawn((Visibility::Hidden, QuestPanel));
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyJ);
        app.update();
        assert!(app.world.resource::<QuestPanelOpen>().0);
        let vis = *app.world.query_filtered::<&Visibility, With<QuestPanel>>().single(&app.world);
        assert!(matches!(vis, Visibility::Inherited));
    }

    #[test]
    fn 열린_상태에서_J키를_다시_누르면_닫히고_숨김으로_바뀐다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, toggle_quest_panel);
        app.world.spawn((Visibility::Inherited, QuestPanel));
        app.world.resource_mut::<QuestPanelOpen>().0 = true;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyJ);
        app.update();
        assert!(!app.world.resource::<QuestPanelOpen>().0);
        let vis = *app.world.query_filtered::<&Visibility, With<QuestPanel>>().single(&app.world);
        assert!(matches!(vis, Visibility::Hidden));
    }

    #[test]
    fn 열린_상태에서_Q키를_다시_누르면_닫히고_숨김으로_바뀐다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, toggle_quest_panel);
        app.world.spawn((Visibility::Inherited, QuestPanel));
        app.world.resource_mut::<QuestPanelOpen>().0 = true;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyQ);
        app.update();
        assert!(!app.world.resource::<QuestPanelOpen>().0);
        let vis = *app.world.query_filtered::<&Visibility, With<QuestPanel>>().single(&app.world);
        assert!(matches!(vis, Visibility::Hidden));
    }

    #[test]
    fn Q키가_아니면_토글은_아무것도_안한다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, toggle_quest_panel);
        app.world.spawn((Visibility::Hidden, QuestPanel));
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        app.update();
        assert!(!app.world.resource::<QuestPanelOpen>().0);
    }

    #[test]
    fn 패배_상태면_Q키_토글이_무시된다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, toggle_quest_panel);
        app.world.spawn(crate::modules::combat::Defeated);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyQ);
        app.update();
        assert!(!app.world.resource::<QuestPanelOpen>().0);
    }

    #[test]
    fn 패널_엔티티가_없어도_토글은_안전하다() {
        // panel_q.get_single_mut() Err 분기.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, toggle_quest_panel);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyQ);
        app.update();
        assert!(app.world.resource::<QuestPanelOpen>().0);
    }

    fn 갱신_하네스() -> App {
        let mut app = 렌더_하네스();
        app.add_systems(Update, update_quest_panel);
        app.world.spawn((Visibility::Hidden, QuestPanel));
        app.world.spawn((Text::default(), QuestPanelContent));
        app
    }

    #[test]
    fn 패널이_열리면_갱신시_보임으로_바뀌고_텍스트가_채워진다() {
        let mut app = 갱신_하네스();
        let (reg, st) = make_registry_and_state("active");
        app.insert_resource(reg);
        app.insert_resource(st);
        app.world.resource_mut::<QuestPanelOpen>().0 = true;
        app.update();
        let vis = *app.world.query_filtered::<&Visibility, With<QuestPanel>>().single(&app.world);
        assert!(matches!(vis, Visibility::Inherited));
        let text = app.world.query_filtered::<&Text, With<QuestPanelContent>>().single(&app.world);
        assert!(!text.sections.is_empty());
    }

    #[test]
    fn 패널이_닫히면_갱신시_숨김으로_바뀐다() {
        let mut app = 갱신_하네스();
        app.world.resource_mut::<QuestPanelOpen>().0 = true;
        app.update();
        app.world.resource_mut::<QuestPanelOpen>().0 = false;
        app.update();
        let vis = *app.world.query_filtered::<&Visibility, With<QuestPanel>>().single(&app.world);
        assert!(matches!(vis, Visibility::Hidden));
    }

    #[test]
    fn 패널이_열린채_퀘스트상태만_바뀌어도_텍스트가_갱신된다() {
        // panel_open 은 변경 없지만 state 변경 → 갱신 가드의 뒤쪽 && 피연산자들 도달.
        let mut app = 갱신_하네스();
        app.world.resource_mut::<QuestPanelOpen>().0 = true;
        app.update(); // 1회 채움(panel_open 변경됨)
        {
            let mut text = app.world.query_filtered::<&mut Text, With<QuestPanelContent>>().single_mut(&mut app.world);
            text.sections.clear();
        }
        // panel_open 은 안 건드리고 state 만 변경.
        let (reg, st) = make_registry_and_state("active");
        app.insert_resource(reg);
        app.insert_resource(st);
        app.update();
        let text = app.world.query_filtered::<&Text, With<QuestPanelContent>>().single(&app.world);
        assert!(!text.sections.is_empty());
    }

    #[test]
    fn 패널이_열린채_인벤토리만_바뀌어도_텍스트가_갱신된다() {
        // 갱신 가드 && 체인의 !inventory.is_changed() 피연산자 False(=변경됨) 도달.
        let mut app = 갱신_하네스();
        app.world.resource_mut::<QuestPanelOpen>().0 = true;
        app.update();
        {
            let mut text = app.world.query_filtered::<&mut Text, With<QuestPanelContent>>().single_mut(&mut app.world);
            text.sections.clear();
        }
        // panel_open/state 는 그대로, 인벤토리만 변경.
        app.world.resource_mut::<PlayerInventory>().gold += 1;
        app.update();
        let text = app.world.query_filtered::<&Text, With<QuestPanelContent>>().single(&app.world);
        assert!(!text.sections.is_empty());
    }

    #[test]
    fn 패널이_열린채_세계상태만_바뀌어도_텍스트가_갱신된다() {
        // !world.is_changed() 피연산자 False 도달.
        let mut app = 갱신_하네스();
        app.world.resource_mut::<QuestPanelOpen>().0 = true;
        app.update();
        {
            let mut text = app.world.query_filtered::<&mut Text, With<QuestPanelContent>>().single_mut(&mut app.world);
            text.sections.clear();
        }
        app.world.resource_mut::<WorldState>().current = ZoneId::Forest;
        app.update();
        let text = app.world.query_filtered::<&Text, With<QuestPanelContent>>().single(&app.world);
        assert!(!text.sections.is_empty());
    }

    #[test]
    fn 패널이_열린채_마커만_바뀌어도_텍스트가_갱신된다() {
        // !markers.is_changed() 피연산자 False 도달.
        let mut app = 갱신_하네스();
        app.world.resource_mut::<QuestPanelOpen>().0 = true;
        app.update();
        {
            let mut text = app.world.query_filtered::<&mut Text, With<QuestPanelContent>>().single_mut(&mut app.world);
            text.sections.clear();
        }
        app.world.resource_mut::<DiscoveredMarkers>().add(1, 1, MarkerKind::QuestGiver, ZoneId::Town);
        app.update();
        let text = app.world.query_filtered::<&Text, With<QuestPanelContent>>().single(&app.world);
        assert!(!text.sections.is_empty());
    }

    #[test]
    fn 닫힌_패널은_갱신시_텍스트를_채우지_않는다() {
        let mut app = 갱신_하네스();
        app.update(); // open=false
        let text = app.world.query_filtered::<&Text, With<QuestPanelContent>>().single(&app.world);
        assert!(text.sections.is_empty());
    }

    #[test]
    fn 변경이_없으면_열린_패널도_텍스트를_다시_채우지_않는다() {
        // 두 번째 update: panel_open/state/inventory/world/markers 모두 변경 없음 → 조기 반환.
        let mut app = 갱신_하네스();
        app.world.resource_mut::<QuestPanelOpen>().0 = true;
        app.update(); // 첫 채움
        // 텍스트를 일부러 비운다.
        {
            let mut text = app.world.query_filtered::<&mut Text, With<QuestPanelContent>>().single_mut(&mut app.world);
            text.sections.clear();
        }
        app.update(); // 변경 없음 → 다시 채우지 않음
        let text = app.world.query_filtered::<&Text, With<QuestPanelContent>>().single(&app.world);
        assert!(text.sections.is_empty());
    }

    #[test]
    fn 텍스트_엔티티가_없으면_갱신은_조기_반환한다() {
        // text_q.get_single_mut() Err 분기.
        let mut app = 렌더_하네스();
        app.add_systems(Update, update_quest_panel);
        app.world.spawn((Visibility::Hidden, QuestPanel));
        app.world.resource_mut::<QuestPanelOpen>().0 = true;
        app.update(); // 텍스트 엔티티 없음 → 패닉 없음
    }

    #[test]
    fn 패널_엔티티가_없으면_갱신의_visibility_동기화는_건너뛴다() {
        // 첫 if 의 panel_q.get_single_mut() Err 분기.
        let mut app = 렌더_하네스();
        app.add_systems(Update, update_quest_panel);
        app.world.spawn((Text::default(), QuestPanelContent));
        app.world.resource_mut::<QuestPanelOpen>().0 = true;
        app.update();
        // 패널 엔티티가 없어도 텍스트는 채워진다(빈 퀘스트 메시지).
        let text = app.world.query_filtered::<&Text, With<QuestPanelContent>>().single(&app.world);
        assert!(!text.sections.is_empty());
    }
}
