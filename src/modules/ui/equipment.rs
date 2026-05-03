use bevy::prelude::*;
use crate::modules::{
    item::{
        ItemKind, PlayerInventory, PlayerEquipment, EquipmentPanelOpen,
        weapon_attack, armor_defense_bonus, WeaponKind,
    },
    player::Player,
    combat::CombatStats,
};
use super::{LogMessage, minimap::MINIMAP_DISPLAY_SIZE};

const WEAPON_ICON: &str = "\u{E946}";
const SPEAR_ICON:  &str = "\u{EAAC}";
const BOW_ICON:    &str = "\u{E978}";
const ARMOR_ICON:  &str = "\u{EA96}";
const POTION_ICON: &str = "\u{EA72}";

// 미니맵 너비(180) + 오른쪽 여백(5) + 여유(5) = 190
const PANEL_WIDTH: f32 = MINIMAP_DISPLAY_SIZE + 10.0;
const FONT_SIZE:   f32 = 13.5;

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
                position_type: PositionType::Absolute,
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
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
        EquipmentPanel,
    )).with_children(|panel| {
        panel.spawn((
            TextBundle::from_section(
                "",
                TextStyle { font, font_size: FONT_SIZE, color: Color::WHITE },
            ),
            EquipmentPanelContent,
        ));
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
                ItemKind::Consumable(_) | ItemKind::QuestItem(_) => {}
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
    asset_server: Res<AssetServer>,
    mut panel_q: Query<&mut Visibility, With<EquipmentPanel>>,
    mut text_q: Query<&mut Text, With<EquipmentPanelContent>>,
) {
    let Ok(mut vis) = panel_q.get_single_mut() else { return };

    if panel_open.is_changed() {
        *vis = if panel_open.0 { Visibility::Inherited } else { Visibility::Hidden };
    }

    if !panel_open.0 { return; }

    if panel_open.is_changed() || inventory.is_changed() || equipment.is_changed() || ui_state.is_changed() {
        let kr_font  = asset_server.load("fonts/NanumSquareNeo-bRg.ttf");
        let rpg_font = asset_server.load("fonts/rpg-awesome.ttf");
        if let Ok(mut text) = text_q.get_single_mut() {
            text.sections = build_panel_sections(&inventory, &equipment, ui_state.cursor, &kr_font, &rpg_font);
        }
    }
}

fn ts(value: impl Into<String>, font: Handle<Font>, size: f32, color: Color) -> TextSection {
    TextSection { value: value.into(), style: TextStyle { font, font_size: size, color } }
}

fn item_kind_icon(kind: &ItemKind) -> &'static str {
    match kind {
        ItemKind::Weapon(WeaponKind::Sword) => WEAPON_ICON,
        ItemKind::Weapon(WeaponKind::Spear) => SPEAR_ICON,
        ItemKind::Weapon(WeaponKind::Bow)   => BOW_ICON,
        ItemKind::Armor(_)                  => ARMOR_ICON,
        ItemKind::Consumable(_)             => POTION_ICON,
        ItemKind::QuestItem(_)              => "*",
    }
}

pub(crate) fn build_panel_sections(
    inventory: &PlayerInventory,
    equipment: &PlayerEquipment,
    cursor: usize,
    kr_font: &Handle<Font>,
    rpg_font: &Handle<Font>,
) -> Vec<TextSection> {
    let kr  = |v: &str, c: Color| ts(v,  kr_font.clone(),  FONT_SIZE, c);
    let ico = |v: &str, c: Color| ts(v,  rpg_font.clone(), FONT_SIZE, c);

    // Cogmind 색상 팔레트 (녹색 계열)
    let c_header   = Color::rgba(0.3, 1.0, 0.3, 1.0);
    let c_category = Color::rgba(0.55, 0.75, 0.55, 0.9);
    let c_active   = Color::rgba(0.45, 0.95, 0.45, 1.0);
    let c_stat     = Color::rgba(0.3,  1.0,  0.3,  1.0);
    let c_inactive = Color::rgba(0.28, 0.28, 0.28, 0.9);
    let c_cursor   = Color::rgba(1.0,  1.0,  0.2,  1.0);
    let c_normal   = Color::rgba(0.82, 0.82, 0.82, 1.0);
    let c_equipped = Color::rgba(0.3,  1.0,  0.3,  1.0);
    let c_sep      = Color::rgba(0.2,  0.45, 0.2,  0.8);
    let c_hint     = Color::rgba(0.3,  0.5,  0.3,  0.85);

    let mut s: Vec<TextSection> = Vec::new();

    // ── PARTS 헤더 ──
    s.push(kr("/ P A R T S /\n",         c_header));
    s.push(kr("─────────────────────\n", c_sep));

    // 무기 슬롯
    s.push(kr(" W e a p o n\n", c_category));
    match equipment.weapon {
        None => {
            s.push(ico(&format!("  {} ", WEAPON_ICON), c_inactive));
            s.push(kr("없음\n",                        c_inactive));
        }
        Some(w) => {
            let icon_str = match w {
                WeaponKind::Sword => WEAPON_ICON,
                WeaponKind::Spear => SPEAR_ICON,
                WeaponKind::Bow   => BOW_ICON,
            };
            let atk = weapon_attack(w);
            let bar = "|".repeat(atk as usize);
            s.push(ico(&format!("  {} ", icon_str),                        c_active));
            s.push(kr(&format!("{}  ({})", w.display_name(), bar),         c_active));
            s.push(kr(&format!("  ATK {}\n", atk),                        c_stat));
        }
    }

    // 방어구 슬롯
    s.push(kr("\n A r m o r\n", c_category));
    match equipment.armor {
        None => {
            s.push(ico(&format!("  {} ", ARMOR_ICON), c_inactive));
            s.push(kr("없음\n",                        c_inactive));
        }
        Some(a) => {
            let def = armor_defense_bonus(a);
            let bar = "|".repeat((def * 3) as usize);
            s.push(ico(&format!("  {} ", ARMOR_ICON),                      c_active));
            s.push(kr(&format!("{}  ({})", a.display_name(), bar),         c_active));
            s.push(kr(&format!("  +{}DEF\n", def),                        c_stat));
        }
    }

    // ── INVENTORY 헤더 ──
    s.push(kr("\n/ I N V E N T O R Y /\n", c_header));
    s.push(kr("─────────────────────\n",   c_sep));

    let total = inventory.items.len() + inventory.consumables.len();
    if total == 0 {
        s.push(kr("  (비어있음)\n", c_inactive));
    } else {
        let mut weapon_marked = false;
        let mut armor_marked  = false;
        for (i, inv_item) in inventory.items.iter().enumerate() {
            let (sel, color) = if i == cursor { (">", c_cursor) } else { (" ", c_normal) };
            let equipped = match inv_item.kind {
                ItemKind::Weapon(w) if equipment.weapon == Some(w) && !weapon_marked => {
                    weapon_marked = true; true
                }
                ItemKind::Armor(a) if equipment.armor == Some(a) && !armor_marked => {
                    armor_marked = true; true
                }
                _ => false,
            };
            let stat = match inv_item.kind {
                ItemKind::Weapon(w) => format!(" (ATK {})", weapon_attack(w)),
                ItemKind::Armor(a)  => format!(" (+{}DEF)", armor_defense_bonus(a)),
                _                   => String::new(),
            };
            let icon_str = item_kind_icon(&inv_item.kind);
            s.push(kr(&format!("{} {} ", sel, i + 1),                     color));
            s.push(ico(&format!("{} ", icon_str),                          color));
            s.push(kr(&format!("{}{}", inv_item.kind.display_name(), stat), color));
            if equipped {
                s.push(kr("  [장착]\n", c_equipped));
            } else {
                s.push(kr("\n", color));
            }
        }
        let base = inventory.items.len();
        for (i, (ck, count)) in inventory.consumables.iter().enumerate() {
            let idx = base + i;
            let (sel, color) = if idx == cursor { (">", c_cursor) } else { (" ", c_normal) };
            s.push(kr(&format!("{} {} ", sel, idx + 1),            color));
            s.push(ico(&format!("{} ", POTION_ICON),                color));
            s.push(kr(&format!("{}  x{}\n", ck.display_name(), count), color));
        }
    }

    // 푸터
    s.push(kr("\n─────────────────────\n",          c_sep));
    s.push(kr("↑↓ 이동  Enter 장착/사용\nEsc·E 닫기", c_hint));

    s
}

pub(crate) fn build_panel_text(
    inventory: &PlayerInventory,
    equipment: &PlayerEquipment,
    cursor: usize,
) -> String {
    let kr:  Handle<Font> = Handle::default();
    let rpg: Handle<Font> = Handle::default();
    build_panel_sections(inventory, equipment, cursor, &kr, &rpg)
        .into_iter()
        .map(|s| s.value)
        .collect()
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
        assert!(spear_line.starts_with("> ") && spear_line.contains("창"), "spear line was: {spear_line:?}");
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
