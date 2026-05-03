use bevy::prelude::*;
use crate::modules::{
    item::{
        ItemKind, PlayerInventory, PlayerEquipment, EquipmentPanelOpen,
        weapon_attack, armor_defense_bonus,
    },
    player::Player,
    combat::CombatStats,
};
use super::LogMessage;

#[derive(Resource, Default)]
pub struct EquipmentUiState {
    pub cursor: usize,
}

#[derive(Component)]
struct EquipmentPanel;

#[derive(Component)]
struct EquipmentPanelContent;

pub struct EquipmentPlugin;

impl Plugin for EquipmentPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EquipmentUiState>()
            .add_systems(Startup, setup_equipment_panel)
            .add_systems(Update, (
                toggle_equipment_panel,
                handle_equipment_input,
                update_equipment_panel,
            ).chain());
    }
}

fn setup_equipment_panel(mut commands: Commands, asset_server: Res<AssetServer>) {
    let font = asset_server.load("fonts/NanumSquareNeo-bRg.ttf");
    commands.spawn((
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            z_index: ZIndex::Global(100),
            visibility: Visibility::Hidden,
            ..default()
        },
        EquipmentPanel,
    )).with_children(|root| {
        root.spawn(NodeBundle {
            style: Style {
                min_width: Val::Px(320.0),
                padding: UiRect::all(Val::Px(18.0)),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            background_color: Color::rgba(0.05, 0.05, 0.08, 0.95).into(),
            ..default()
        }).with_children(|panel| {
            panel.spawn((
                TextBundle::from_section(
                    "",
                    TextStyle { font, font_size: 16.0, color: Color::WHITE },
                ),
                EquipmentPanelContent,
            ));
        });
    });
}

fn toggle_equipment_panel(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut panel_open: ResMut<EquipmentPanelOpen>,
    mut ui_state: ResMut<EquipmentUiState>,
) {
    if keyboard.just_pressed(KeyCode::KeyE) {
        panel_open.0 = !panel_open.0;
        if panel_open.0 {
            ui_state.cursor = 0;
        }
    }
}

fn handle_equipment_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut panel_open: ResMut<EquipmentPanelOpen>,
    mut ui_state: ResMut<EquipmentUiState>,
    mut inventory: ResMut<PlayerInventory>,
    mut equipment: ResMut<PlayerEquipment>,
    mut player_query: Query<&mut CombatStats, With<Player>>,
    mut log: EventWriter<LogMessage>,
) {
    if !panel_open.0 { return; }

    if keyboard.just_pressed(KeyCode::Escape) {
        panel_open.0 = false;
        return;
    }

    let total = inventory.items.len() + inventory.consumables.len();

    if keyboard.just_pressed(KeyCode::ArrowUp) && ui_state.cursor > 0 {
        ui_state.cursor -= 1;
    }
    if keyboard.just_pressed(KeyCode::ArrowDown) && total > 0 && ui_state.cursor + 1 < total {
        ui_state.cursor += 1;
    }

    if keyboard.just_pressed(KeyCode::Enter) {
        let cursor = ui_state.cursor;
        let eq_len = inventory.items.len();
        if cursor < eq_len {
            let kind = inventory.items[cursor].kind;
            match kind {
                ItemKind::Weapon(w) => {
                    if equipment.weapon == Some(w) {
                        equipment.weapon = None;
                        log.send(LogMessage(format!("{} 해제.", w.display_name())));
                    } else {
                        equipment.weapon = Some(w);
                        log.send(LogMessage(format!("{} 장착.", w.display_name())));
                    }
                }
                ItemKind::Armor(a) => {
                    if equipment.armor == Some(a) {
                        equipment.armor = None;
                        log.send(LogMessage(format!("{} 해제.", a.display_name())));
                    } else {
                        equipment.armor = Some(a);
                        log.send(LogMessage(format!("{} 장착.", a.display_name())));
                    }
                }
                ItemKind::Consumable(_) => {}
            }
        } else {
            let ci = cursor - eq_len;
            if ci < inventory.consumables.len() {
                let (ck, _) = inventory.consumables[ci];
                if inventory.use_consumable(ck) {
                    if let Ok(mut stats) = player_query.get_single_mut() {
                        let heal = ck.heal_amount();
                        stats.hp = (stats.hp + heal).min(stats.max_hp);
                        log.send(LogMessage(format!(
                            "{} 사용. (HP +{}, {}/{})",
                            ck.display_name(), heal, stats.hp, stats.max_hp
                        )));
                    }
                    let new_total = inventory.items.len() + inventory.consumables.len();
                    if new_total > 0 && ui_state.cursor >= new_total {
                        ui_state.cursor = new_total - 1;
                    } else if new_total == 0 {
                        ui_state.cursor = 0;
                    }
                }
            }
        }
    }
}

fn update_equipment_panel(
    panel_open: Res<EquipmentPanelOpen>,
    inventory: Res<PlayerInventory>,
    equipment: Res<PlayerEquipment>,
    ui_state: Res<EquipmentUiState>,
    mut panel_q: Query<&mut Visibility, With<EquipmentPanel>>,
    mut text_q: Query<&mut Text, With<EquipmentPanelContent>>,
) {
    let Ok(mut vis) = panel_q.get_single_mut() else { return };

    if panel_open.is_changed() {
        *vis = if panel_open.0 { Visibility::Inherited } else { Visibility::Hidden };
    }

    if !panel_open.0 { return; }

    if panel_open.is_changed() || inventory.is_changed() || equipment.is_changed() || ui_state.is_changed() {
        if let Ok(mut text) = text_q.get_single_mut() {
            text.sections[0].value = build_panel_text(&inventory, &equipment, ui_state.cursor);
        }
    }
}

pub(crate) fn build_panel_text(
    inventory: &PlayerInventory,
    equipment: &PlayerEquipment,
    cursor: usize,
) -> String {
    let mut lines: Vec<String> = vec![
        "=== 장비 관리 ===".into(),
        "↑↓: 선택  Enter: 장착/사용  E·Esc: 닫기".into(),
        String::new(),
    ];

    let weapon_str = match equipment.weapon {
        None    => "없음".to_string(),
        Some(w) => format!("{} (ATK {})", w.display_name(), weapon_attack(w)),
    };
    let armor_str = match equipment.armor {
        None    => "없음".to_string(),
        Some(a) => format!("{} (+{}DEF)", a.display_name(), armor_defense_bonus(a)),
    };
    lines.push(format!("무기: {}", weapon_str));
    lines.push(format!("방어구: {}", armor_str));
    lines.push(String::new());
    lines.push("--- 인벤토리 ---".into());

    let total = inventory.items.len() + inventory.consumables.len();
    if total == 0 {
        lines.push("  (비어있음)".into());
    } else {
        let mut weapon_marked = false;
        let mut armor_marked  = false;
        for (i, inv_item) in inventory.items.iter().enumerate() {
            let prefix = if i == cursor { ">" } else { " " };
            let equipped = match inv_item.kind {
                ItemKind::Weapon(w) if equipment.weapon == Some(w) && !weapon_marked => {
                    weapon_marked = true;
                    " [장착]"
                }
                ItemKind::Armor(a) if equipment.armor == Some(a) && !armor_marked => {
                    armor_marked = true;
                    " [장착]"
                }
                _ => "",
            };
            let stat = match inv_item.kind {
                ItemKind::Weapon(w) => format!(" (ATK {})", weapon_attack(w)),
                ItemKind::Armor(a)  => format!(" (+{}DEF)", armor_defense_bonus(a)),
                _                   => String::new(),
            };
            lines.push(format!("{} {}{}{}", prefix, inv_item.kind.display_name(), stat, equipped));
        }
        let base = inventory.items.len();
        for (i, (ck, count)) in inventory.consumables.iter().enumerate() {
            let prefix = if base + i == cursor { ">" } else { " " };
            lines.push(format!("{} {} x{}", prefix, ck.display_name(), count));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::item::{InventoryItem, WeaponKind, ArmorKind, ConsumableKind};

    fn empty_inv() -> PlayerInventory { PlayerInventory::default() }
    fn empty_eq()  -> PlayerEquipment { PlayerEquipment::default() }

    #[test]
    fn empty_inventory_shows_empty_message() {
        let text = build_panel_text(&empty_inv(), &empty_eq(), 0);
        assert!(text.contains("비어있음"));
    }

    #[test]
    fn equipped_weapon_shows_equip_tag() {
        let mut inv = empty_inv();
        inv.items.push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::Sword) });
        let mut eq = empty_eq();
        eq.weapon = Some(WeaponKind::Sword);
        let text = build_panel_text(&inv, &eq, 0);
        assert!(text.contains("[장착]"));
    }

    #[test]
    fn unequipped_weapon_no_equip_tag() {
        let mut inv = empty_inv();
        inv.items.push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::Sword) });
        let text = build_panel_text(&inv, &empty_eq(), 0);
        assert!(!text.contains("[장착]"));
    }

    #[test]
    fn cursor_marks_selected_row_with_arrow() {
        let mut inv = empty_inv();
        inv.items.push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::Sword) });
        inv.items.push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::Spear) });
        let text = build_panel_text(&inv, &empty_eq(), 1);
        let spear_line = text.lines().find(|l| l.contains("창")).unwrap_or("");
        assert!(spear_line.contains("> 창"), "spear line was: {spear_line:?}");
    }

    #[test]
    fn consumable_shows_count() {
        let mut inv = empty_inv();
        inv.add_consumable(ConsumableKind::HealthPotion);
        inv.add_consumable(ConsumableKind::HealthPotion);
        let text = build_panel_text(&inv, &empty_eq(), 0);
        assert!(text.contains("x2"));
    }

    #[test]
    fn weapon_atk_shown_in_panel() {
        let mut inv = empty_inv();
        inv.items.push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::Spear) });
        let text = build_panel_text(&inv, &empty_eq(), 0);
        assert!(text.contains("ATK 9"));
    }

    #[test]
    fn armor_def_shown_in_panel() {
        let mut inv = empty_inv();
        inv.items.push(InventoryItem { kind: ItemKind::Armor(ArmorKind::LeatherArmor) });
        let text = build_panel_text(&inv, &empty_eq(), 0);
        assert!(text.contains("+2DEF"));
    }

    #[test]
    fn only_first_of_same_weapon_kind_gets_equip_tag() {
        let mut inv = empty_inv();
        inv.items.push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::Sword) });
        inv.items.push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::Sword) });
        let mut eq = empty_eq();
        eq.weapon = Some(WeaponKind::Sword);
        let text = build_panel_text(&inv, &eq, 0);
        assert_eq!(text.matches("[장착]").count(), 1);
    }
}
