use bevy::prelude::*;
use crate::modules::{
    quest::{QuestRegistry, QuestState},
    ui::minimap::MINIMAP_DISPLAY_SIZE,
};

const PANEL_WIDTH: f32 = MINIMAP_DISPLAY_SIZE + 10.0;
const FONT_SIZE: f32 = 13.5;

const C_HEADER:   Color = Color::rgba(0.3, 1.0, 0.3, 1.0);
const C_TITLE:    Color = Color::rgba(0.9, 0.9, 0.5, 1.0);
const C_OBJ:      Color = Color::rgba(0.82, 0.82, 0.82, 1.0);
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

fn update_quest_panel(
    panel_open: Res<QuestPanelOpen>,
    registry: Res<QuestRegistry>,
    state: Res<QuestState>,
    asset_server: Res<AssetServer>,
    mut text_q: Query<&mut Text, With<QuestPanelContent>>,
) {
    if !panel_open.0 { return; }
    if !panel_open.is_changed() && !state.is_changed() { return; }

    let font = asset_server.load("fonts/NanumSquareNeo-bRg.ttf");
    let Ok(mut text) = text_q.get_single_mut() else { return };
    text.sections = build_quest_sections(&registry, &state, &font);
}

fn ts(value: impl Into<String>, font: Handle<Font>, color: Color) -> TextSection {
    TextSection { value: value.into(), style: TextStyle { font, font_size: FONT_SIZE, color } }
}

fn build_quest_sections(
    registry: &QuestRegistry,
    state: &QuestState,
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

        let mark = if done { "[완료] " } else { "" };
        s.push(ts(format!("{}{}\n", mark, def.title), f.clone(), title_color));

        let obj_text = phase.objective.as_deref()
            .unwrap_or(phase_id);
        s.push(ts(format!("  → {}\n\n", obj_text), f.clone(), obj_color));
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use crate::modules::quest::{QuestDef, QuestPhaseDef, QuestRegistry, QuestState};

    fn make_registry_and_state(phase_id: &str) -> (QuestRegistry, QuestState) {
        let mut phases = HashMap::new();
        phases.insert("active".to_string(), QuestPhaseDef {
            dialog: vec![],
            on_interact: vec![],
            auto_advance: vec![],
            objective: Some("보석을 찾아라".to_string()),
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
            spawns: vec![],
        };
        let mut reg = QuestRegistry::default();
        reg.quests.insert("gem_quest".into(), def);

        let mut st = QuestState::default();
        st.set_phase("gem_quest", phase_id);
        (reg, st)
    }

    #[test]
    fn empty_state_shows_no_quests_message() {
        let reg = QuestRegistry::default();
        let st  = QuestState::default();
        let sections = build_quest_sections(&reg, &st, &Handle::default());
        let all_text: String = sections.iter().map(|s| s.value.as_str()).collect();
        assert!(all_text.contains("진행 중인 퀘스트 없음"));
    }

    #[test]
    fn active_quest_shows_title_and_objective() {
        let (reg, st) = make_registry_and_state("active");
        let sections = build_quest_sections(&reg, &st, &Handle::default());
        let all_text: String = sections.iter().map(|s| s.value.as_str()).collect();
        assert!(all_text.contains("잃어버린 보석"));
        assert!(all_text.contains("보석을 찾아라"));
    }

    #[test]
    fn done_quest_shows_completed_mark() {
        let (reg, st) = make_registry_and_state("done");
        let sections = build_quest_sections(&reg, &st, &Handle::default());
        let all_text: String = sections.iter().map(|s| s.value.as_str()).collect();
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
        });
        let mut st = QuestState::default();
        st.set_phase("q", "active");
        let sections = build_quest_sections(&reg, &st, &Handle::default());
        let all_text: String = sections.iter().map(|s| s.value.as_str()).collect();
        assert!(all_text.contains("active"));
    }
}
