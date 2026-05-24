use bevy::prelude::*;
use crate::modules::{
    item::{
        ItemKind, PlayerInventory, PlayerEquipment, EquipmentPanelOpen,
        weapon_attack, armor_defense_bonus, weapon_rarity, armor_rarity,
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
    defeated_q: Query<(), With<crate::modules::combat::Defeated>>,
) {
    if !defeated_q.is_empty() { return; }
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
    items: Res<crate::modules::item::ItemRegistry>,
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
            let inv_item = inventory.items[cursor].clone();
            let kind = inv_item.kind;
            match kind {
                ItemKind::Weapon(w) => {
                    if equipment.weapon == Some(w) {
                        equipment.weapon = None;
                        equipment.weapon_rolled_attack = None;
                        log.send(LogMessage(format!("{} 해제.", w.display_name(&items))));
                    } else {
                        equipment.weapon = Some(w);
                        equipment.weapon_rolled_attack = inv_item.rolled_attack;
                        log.send(LogMessage(format!("{} 장착.", w.display_name(&items))));
                    }
                }
                ItemKind::Armor(a) => {
                    if equipment.armor == Some(a) {
                        equipment.armor = None;
                        equipment.armor_rolled_defense = None;
                        log.send(LogMessage(format!("{} 해제.", a.display_name(&items))));
                    } else {
                        equipment.armor = Some(a);
                        equipment.armor_rolled_defense = inv_item.rolled_defense;
                        log.send(LogMessage(format!("{} 장착.", a.display_name(&items))));
                    }
                }
                ItemKind::Consumable(_) | ItemKind::QuestItem(_) => {}
            }
        } else {
            let ci = cursor - eq_len;
            if ci < inventory.consumables.len() {
                let (ck, _) = inventory.consumables[ci];
                // 회복 효과가 없는 소모품(함정 키트/해제 도구 등)은 장비 패널의
                // "사용(회복)" 경로로는 소비하지 않는다. 전용 단축키로만 쓴다.
                if ck.heal_amount(&items) <= 0 {
                    log.send(LogMessage(format!(
                        "{}은(는) 여기서 사용할 수 없다.", ck.display_name(&items)
                    )));
                } else if inventory.use_consumable(ck) {
                    if let Ok(mut stats) = player_query.get_single_mut() {
                        let heal = ck.heal_amount(&items);
                        stats.hp = (stats.hp + heal).min(stats.max_hp);
                        log.send(LogMessage(format!(
                            "{} 사용. (HP +{}, {}/{})",
                            ck.display_name(&items), heal, stats.hp, stats.max_hp
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
    quest_items: Res<crate::modules::item::QuestItemRegistry>,
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
            text.sections = build_panel_sections(&inventory, &equipment, ui_state.cursor, &kr_font, &rpg_font, &quest_items);
        }
    }
}

fn ts(value: impl Into<String>, font: Handle<Font>, size: f32, color: Color) -> TextSection {
    TextSection { value: value.into(), style: TextStyle { font, font_size: size, color } }
}

fn item_kind_icon(kind: &ItemKind) -> &'static str {
    match kind {
        ItemKind::Weapon(w) => match w.0 {
            "sword" => WEAPON_ICON,
            "spear" => SPEAR_ICON,
            "bow"   => BOW_ICON,
            _       => WEAPON_ICON,
        },
        ItemKind::Armor(_)     => ARMOR_ICON,
        ItemKind::Consumable(_) => POTION_ICON,
        ItemKind::QuestItem(_)  => "*",
    }
}

pub(crate) fn build_panel_sections(
    inventory: &PlayerInventory,
    equipment: &PlayerEquipment,
    cursor: usize,
    kr_font: &Handle<Font>,
    rpg_font: &Handle<Font>,
    quest_items: &crate::modules::item::QuestItemRegistry,
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
            let icon_str = match w.0 {
                "sword" => WEAPON_ICON,
                "spear" => SPEAR_ICON,
                "bow"   => BOW_ICON,
                _       => WEAPON_ICON,
            };
            let atk = equipment.weapon_rolled_attack.unwrap_or_else(|| weapon_attack(w, quest_items));
            let bar = "|".repeat(atk as usize);
            // 롤된 공격력이 있으면 레어도 색·이름 표시.
            let rarity = equipment.weapon_rolled_attack.and_then(|v| weapon_rarity(w, v, quest_items));
            let (name_color, prefix) = match rarity {
                Some(r) => (r.color(), format!("[{}] ", r.name_ko())),
                None => (c_active, String::new()),
            };
            s.push(ico(&format!("  {} ", icon_str),                        c_active));
            s.push(kr(&format!("{}{}  ({})", prefix, w.display_name(quest_items), bar), name_color));
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
            let def = equipment.armor_rolled_defense.unwrap_or_else(|| armor_defense_bonus(a, quest_items));
            let bar = "|".repeat((def * 3) as usize);
            let rarity = equipment.armor_rolled_defense.and_then(|v| armor_rarity(a, v, quest_items));
            let (name_color, prefix) = match rarity {
                Some(r) => (r.color(), format!("[{}] ", r.name_ko())),
                None => (c_active, String::new()),
            };
            s.push(ico(&format!("  {} ", ARMOR_ICON),                      c_active));
            s.push(kr(&format!("{}{}  ({})", prefix, a.display_name(quest_items), bar), name_color));
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
            // [등급] 접두사·이름 색은 무기/방어구의 롤값+range 로 계산한 레어도에서 가져온다.
            let (rarity, stat) = match inv_item.kind {
                ItemKind::Weapon(w) => {
                    let atk = inv_item.rolled_attack.unwrap_or_else(|| weapon_attack(w, quest_items));
                    let r = inv_item.rolled_attack.and_then(|v| weapon_rarity(w, v, quest_items));
                    (r, format!(" (ATK {})", atk))
                }
                ItemKind::Armor(a) => {
                    let def = inv_item.rolled_defense.unwrap_or_else(|| armor_defense_bonus(a, quest_items));
                    let r = inv_item.rolled_defense.and_then(|v| armor_rarity(a, v, quest_items));
                    (r, format!(" (+{}DEF)", def))
                }
                _ => (None, String::new()),
            };
            let (name_color, prefix) = match rarity {
                Some(r) if i != cursor => (r.color(), format!("[{}] ", r.name_ko())),
                Some(r)                => (color, format!("[{}] ", r.name_ko())),
                None                   => (color, String::new()),
            };
            let icon_str = item_kind_icon(&inv_item.kind);
            s.push(kr(&format!("{} {} ", sel, i + 1),                     color));
            s.push(ico(&format!("{} ", icon_str),                          color));
            s.push(kr(&format!("{}{}{}", prefix, inv_item.kind.display_name(quest_items), stat), name_color));
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
            s.push(kr(&format!("{}  x{}\n", ck.display_name(quest_items), count), color));
        }
    }

    // 푸터
    s.push(kr("\n─────────────────────\n",          c_sep));
    s.push(kr("↑↓ 이동  Enter 장착/사용\nEsc·E 닫기", c_hint));

    s
}

#[cfg(test)]
pub(crate) fn build_panel_text(
    inventory: &PlayerInventory,
    equipment: &PlayerEquipment,
    cursor: usize,
    quest_items: &crate::modules::item::QuestItemRegistry,
) -> String {
    let kr:  Handle<Font> = Handle::default();
    let rpg: Handle<Font> = Handle::default();
    build_panel_sections(inventory, equipment, cursor, &kr, &rpg, quest_items)
        .into_iter()
        .map(|s| s.value)
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;
    use crate::modules::item::{InventoryItem, WeaponKind, ArmorKind, ConsumableKind, QuestItemKind};
    use std::sync::OnceLock;

    static TEST_QI: OnceLock<crate::modules::item::QuestItemRegistry> = OnceLock::new();
    fn qi() -> &'static crate::modules::item::QuestItemRegistry {
        TEST_QI.get_or_init(|| crate::modules::item::build_test_registry())
    }

    fn empty_inv() -> PlayerInventory { PlayerInventory::default() }
    fn empty_eq()  -> PlayerEquipment { PlayerEquipment::default() }

    // ── 패널 텍스트 빌더 ─────────────────────────────────────────────────

    #[test]
    fn 빈_인벤토리는_비어있음_안내를_보여준다() {
        let text = build_panel_text(&empty_inv(), &empty_eq(), 0, qi());
        assert!(text.contains("비어있음"));
    }

    #[test]
    fn 장착된_무기에는_장착_표식이_붙는다() {
        let mut inv = empty_inv();
        inv.items.push(InventoryItem::new(ItemKind::Weapon(WeaponKind::SWORD)));
        let mut eq = empty_eq();
        eq.weapon = Some(WeaponKind::SWORD);
        let text = build_panel_text(&inv, &eq, 0, qi());
        assert!(text.contains("[장착]"));
    }

    #[test]
    fn 장착되지_않은_무기에는_장착_표식이_없다() {
        let mut inv = empty_inv();
        inv.items.push(InventoryItem::new(ItemKind::Weapon(WeaponKind::SWORD)));
        let text = build_panel_text(&inv, &empty_eq(), 0, qi());
        assert!(!text.contains("[장착]"));
    }

    #[test]
    fn 커서가_가리키는_줄은_화살표로_표시된다() {
        let mut inv = empty_inv();
        inv.items.push(InventoryItem::new(ItemKind::Weapon(WeaponKind::SWORD)));
        inv.items.push(InventoryItem::new(ItemKind::Weapon(WeaponKind::SPEAR)));
        let text = build_panel_text(&inv, &empty_eq(), 1, qi());
        let spear_line = text.lines().find(|l| l.contains("창")).unwrap_or("");
        assert!(spear_line.starts_with("> "), "spear line was: {spear_line:?}");
        assert!(spear_line.contains("창"), "spear line was: {spear_line:?}");
    }

    #[test]
    fn 소모품은_보유_수량을_표시한다() {
        let mut inv = empty_inv();
        inv.add_consumable(ConsumableKind::HEALTH_POTION);
        inv.add_consumable(ConsumableKind::HEALTH_POTION);
        let text = build_panel_text(&inv, &empty_eq(), 0, qi());
        assert!(text.contains("x2"));
    }

    #[test]
    fn 패널은_롤값없는_무기의_중앙값_공격력을_표시한다() {
        // 롤값 없는 창 → 중앙값 10.
        let mut inv = empty_inv();
        inv.items.push(InventoryItem::new(ItemKind::Weapon(WeaponKind::SPEAR)));
        let text = build_panel_text(&inv, &empty_eq(), 0, qi());
        assert!(text.contains("ATK 10"));
    }

    #[test]
    fn 패널은_롤값없는_방어구의_중앙값_방어력을_표시한다() {
        // 롤값 없는 가죽 → 중앙값 3.
        let mut inv = empty_inv();
        inv.items.push(InventoryItem::new(ItemKind::Armor(ArmorKind::LEATHER_ARMOR)));
        let text = build_panel_text(&inv, &empty_eq(), 0, qi());
        assert!(text.contains("+3DEF"));
    }

    #[test]
    fn 같은_종류_무기가_여러개여도_장착_표식은_하나만_붙는다() {
        let mut inv = empty_inv();
        inv.items.push(InventoryItem::new(ItemKind::Weapon(WeaponKind::SWORD)));
        inv.items.push(InventoryItem::new(ItemKind::Weapon(WeaponKind::SWORD)));
        let mut eq = empty_eq();
        eq.weapon = Some(WeaponKind::SWORD);
        let text = build_panel_text(&inv, &eq, 0, qi());
        assert_eq!(text.matches("[장착]").count(), 1);
    }

    #[test]
    fn 무기와_방어구를_장착하면_슬롯에_각각_표시된다() {
        let mut eq = empty_eq();
        eq.weapon = Some(WeaponKind::SWORD);
        eq.armor  = Some(ArmorKind::LEATHER_ARMOR);
        let text = build_panel_text(&empty_inv(), &eq, 0, qi());
        assert!(text.contains("ATK"));
        assert!(text.contains("DEF"));
        // 빈 슬롯 "없음" 은 없어야 한다.
        assert!(!text.contains("없음"));
    }

    #[test]
    fn 빈_장비_슬롯은_없음으로_표시된다() {
        let text = build_panel_text(&empty_inv(), &empty_eq(), 0, qi());
        assert_eq!(text.matches("없음").count(), 2);
    }

    #[test]
    fn 창과_활은_각자의_아이콘을_쓴다() {
        // 창/활 장착 슬롯 아이콘 분기를 양방향으로 도달시킨다.
        let mut eq_spear = empty_eq();
        eq_spear.weapon = Some(WeaponKind::SPEAR);
        let spear = build_panel_text(&empty_inv(), &eq_spear, 0, qi());
        assert!(spear.contains(SPEAR_ICON));

        let mut eq_bow = empty_eq();
        eq_bow.weapon = Some(WeaponKind::BOW);
        let bow = build_panel_text(&empty_inv(), &eq_bow, 0, qi());
        assert!(bow.contains(BOW_ICON));
    }

    #[test]
    fn 알수없는_무기는_기본_무기_아이콘으로_표시된다() {
        // item_kind_icon / 장착 슬롯 모두의 `_ => WEAPON_ICON` 폴백 분기를 도달시킨다.
        let unknown = WeaponKind("trident");
        let mut inv = empty_inv();
        inv.items.push(InventoryItem::new(ItemKind::Weapon(unknown)));
        let mut eq = empty_eq();
        eq.weapon = Some(unknown);
        let text = build_panel_text(&inv, &eq, 0, qi());
        assert!(text.contains(WEAPON_ICON));
    }

    #[test]
    fn 인벤토리의_장착중인_방어구에는_장착_표식이_붙는다() {
        // 인벤토리 목록 루프의 Armor equip 표식 분기(armor_marked).
        let mut inv = empty_inv();
        inv.items.push(InventoryItem::new(ItemKind::Armor(ArmorKind::LEATHER_ARMOR)));
        let mut eq = empty_eq();
        eq.armor = Some(ArmorKind::LEATHER_ARMOR);
        let text = build_panel_text(&inv, &eq, 0, qi());
        assert!(text.contains("[장착]"));
        assert!(text.contains("DEF"));
    }

    #[test]
    fn 인벤토리의_퀘스트아이템은_능력치표기없이_표시된다() {
        // 인벤토리 목록 루프의 stat 폴백(_ => String::new()) 분기.
        let mut inv = empty_inv();
        inv.items.push(InventoryItem::new(ItemKind::QuestItem(QuestItemKind("eternal_gem"))));
        let text = build_panel_text(&inv, &empty_eq(), 0, qi());
        // 퀘스트 아이템 줄에는 ATK/DEF 표기가 없다.
        assert!(!text.contains("ATK"));
        assert!(!text.contains("DEF"));
        assert!(!text.contains("[장착]"));
    }

    #[test]
    fn 같은_방어구가_둘이면_뒤엣것은_장착_표식이_없다() {
        // armor equip-tag 의 `&& !armor_marked` 거짓(둘째 동일 방어구) 분기.
        let mut inv = empty_inv();
        inv.items.push(InventoryItem::new(ItemKind::Armor(ArmorKind::LEATHER_ARMOR)));
        inv.items.push(InventoryItem::new(ItemKind::Armor(ArmorKind::LEATHER_ARMOR)));
        let mut eq = empty_eq();
        eq.armor = Some(ArmorKind::LEATHER_ARMOR);
        let text = build_panel_text(&inv, &eq, 0, qi());
        assert_eq!(text.matches("[장착]").count(), 1);
    }

    #[test]
    fn 커서가_없는_소모품_줄은_화살표없이_표시된다() {
        // 소모품 루프 cursor 강조 분기의 양방향(커서 줄 / 아닌 줄).
        let mut inv = empty_inv();
        inv.consumables.push((ConsumableKind::HEALTH_POTION, 1));
        inv.consumables.push((ConsumableKind::HEALTH_POTION, 1));
        // 커서를 0번 소모품에 → 1번은 비커서.
        let text = build_panel_text(&inv, &empty_eq(), 0, qi());
        assert!(text.lines().any(|l| l.starts_with("> ")));
        assert!(text.lines().filter(|l| l.contains("x1")).any(|l| !l.starts_with(">")));
    }

    #[test]
    fn 퀘스트_아이템_아이콘은_별표다() {
        assert_eq!(item_kind_icon(&ItemKind::QuestItem(QuestItemKind("eternal_gem"))), "*");
    }

    #[test]
    fn 소모품_아이콘은_물약_아이콘이다() {
        assert_eq!(item_kind_icon(&ItemKind::Consumable(ConsumableKind::HEALTH_POTION)), POTION_ICON);
    }

    // ── App 하네스 ──────────────────────────────────────────────────────

    fn 렌더_하네스() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app.init_asset::<Image>();
        app.add_event::<LogMessage>();
        app.init_resource::<EquipmentPanelOpen>()
            .init_resource::<EquipmentUiState>()
            .init_resource::<PlayerInventory>()
            .init_resource::<PlayerEquipment>()
            .insert_resource(crate::modules::item::build_test_registry());
        app
    }

    fn 키_입력_하네스() -> App {
        let mut app = 렌더_하네스();
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app
    }

    #[test]
    fn 플러그인을_등록하면_장비_상태_리소스가_초기화된다() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app.add_event::<LogMessage>();
        app.init_resource::<EquipmentPanelOpen>()
            .init_resource::<PlayerInventory>()
            .init_resource::<PlayerEquipment>()
            .insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(crate::modules::item::build_test_registry());
        app.add_plugins(EquipmentPlugin);
        assert!(app.world.contains_resource::<EquipmentUiState>());
    }

    #[test]
    fn 시작시_장비_패널과_텍스트가_생성된다() {
        let mut app = 렌더_하네스();
        app.add_systems(Startup, setup_equipment_panel);
        app.update();
        assert_eq!(app.world.query_filtered::<(), With<EquipmentPanel>>().iter(&app.world).count(), 1);
        assert_eq!(app.world.query_filtered::<(), With<EquipmentPanelContent>>().iter(&app.world).count(), 1);
    }

    #[test]
    fn E키를_누르면_장비_패널이_열린다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, toggle_equipment_panel);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyE);
        app.update();
        assert!(app.world.resource::<EquipmentPanelOpen>().0);
        assert_eq!(app.world.resource::<EquipmentUiState>().cursor, 0);
    }

    #[test]
    fn 열린_상태에서_E키를_다시_누르면_닫힌다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, toggle_equipment_panel);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<EquipmentUiState>().cursor = 3;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyE);
        app.update();
        assert!(!app.world.resource::<EquipmentPanelOpen>().0);
        // 닫을 때는 커서를 건드리지 않는다.
        assert_eq!(app.world.resource::<EquipmentUiState>().cursor, 3);
    }

    #[test]
    fn 아무_키도_안누르면_토글은_상태를_유지한다() {
        // just_pressed(KeyE) 거짓 분기.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, toggle_equipment_panel);
        app.update();
        assert!(!app.world.resource::<EquipmentPanelOpen>().0);
    }

    #[test]
    fn 패배_상태면_E키_토글이_무시된다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, toggle_equipment_panel);
        app.world.spawn(crate::modules::combat::Defeated);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyE);
        app.update();
        assert!(!app.world.resource::<EquipmentPanelOpen>().0);
    }

    #[test]
    fn 패널이_닫혀있으면_입력_핸들러는_아무것도_안한다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        // 패널이 닫혀 있으니 인벤토리는 그대로다.
        assert!(app.world.resource::<PlayerEquipment>().weapon.is_none());
    }

    #[test]
    fn 패널이_열린_상태에서_Esc를_누르면_닫힌다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
        app.update();
        assert!(!app.world.resource::<EquipmentPanelOpen>().0);
    }

    #[test]
    fn 아래위_화살표로_커서가_이동한다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        {
            let mut inv = app.world.resource_mut::<PlayerInventory>();
            inv.items.push(InventoryItem::new(ItemKind::Weapon(WeaponKind::SWORD)));
            inv.items.push(InventoryItem::new(ItemKind::Weapon(WeaponKind::SPEAR)));
        }
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowDown);
        app.update();
        assert_eq!(app.world.resource::<EquipmentUiState>().cursor, 1);

        app.world.resource_mut::<ButtonInput<KeyCode>>().clear();
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowUp);
        app.update();
        assert_eq!(app.world.resource::<EquipmentUiState>().cursor, 0);
    }

    #[test]
    fn 첫_줄에서_위로는_더_올라가지_않고_마지막_줄에서_아래로도_안내려간다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().items
            .push(InventoryItem::new(ItemKind::Weapon(WeaponKind::SWORD)));
        // 첫 줄에서 위 → 그대로
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowUp);
        app.update();
        assert_eq!(app.world.resource::<EquipmentUiState>().cursor, 0);
        // 마지막(유일) 줄에서 아래 → 그대로
        app.world.resource_mut::<ButtonInput<KeyCode>>().clear();
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowDown);
        app.update();
        assert_eq!(app.world.resource::<EquipmentUiState>().cursor, 0);
    }

    #[test]
    fn 빈_인벤토리에서_아래화살표는_커서를_움직이지_않는다() {
        // total == 0 분기를 도달시킨다.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowDown);
        app.update();
        assert_eq!(app.world.resource::<EquipmentUiState>().cursor, 0);
    }

    #[test]
    fn 빈슬롯_무기를_선택하고_엔터를_누르면_장착된다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().items
            .push(InventoryItem::new(ItemKind::Weapon(WeaponKind::SWORD)));
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert_eq!(app.world.resource::<PlayerEquipment>().weapon, Some(WeaponKind::SWORD));
    }

    #[test]
    fn 이미_장착한_무기에_엔터를_누르면_해제된다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().items
            .push(InventoryItem::new(ItemKind::Weapon(WeaponKind::SWORD)));
        app.world.resource_mut::<PlayerEquipment>().weapon = Some(WeaponKind::SWORD);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert!(app.world.resource::<PlayerEquipment>().weapon.is_none());
    }

    #[test]
    fn 빈슬롯_방어구를_선택하고_엔터를_누르면_장착된다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().items
            .push(InventoryItem::new(ItemKind::Armor(ArmorKind::LEATHER_ARMOR)));
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert_eq!(app.world.resource::<PlayerEquipment>().armor, Some(ArmorKind::LEATHER_ARMOR));
    }

    #[test]
    fn 이미_장착한_방어구에_엔터를_누르면_해제된다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().items
            .push(InventoryItem::new(ItemKind::Armor(ArmorKind::LEATHER_ARMOR)));
        app.world.resource_mut::<PlayerEquipment>().armor = Some(ArmorKind::LEATHER_ARMOR);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert!(app.world.resource::<PlayerEquipment>().armor.is_none());
    }

    #[test]
    fn 인벤토리_퀘스트아이템에_엔터를_눌러도_아무_일도_없다() {
        // Consumable/QuestItem arm 의 no-op 분기를 인벤토리 항목으로 도달시킨다.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().items
            .push(InventoryItem::new(ItemKind::QuestItem(QuestItemKind("eternal_gem"))));
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert!(app.world.resource::<PlayerEquipment>().weapon.is_none());
        assert!(app.world.resource::<PlayerEquipment>().armor.is_none());
    }

    #[test]
    fn 소모품에_엔터를_누르면_사용되어_HP가_회복되고_수량이_준다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().add_consumable(ConsumableKind::HEALTH_POTION);
        let player = app.world.spawn((Player, CombatStats { hp: 1, max_hp: 100, mp: 0, max_mp: 0, attack: 1, defense: 0 })).id();
        // 커서를 소모품(인덱스 0, items 가 비었으므로) 으로
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert!(app.world.get::<CombatStats>(player).unwrap().hp > 1);
        assert!(app.world.resource::<PlayerInventory>().consumables.is_empty());
    }

    #[test]
    fn 회복효과없는_소모품은_장비패널_엔터로_소비되지_않는다() {
        // 함정 키트(§B-2, Heal(0))는 장비 패널에서 엔터를 눌러도 소모되지 않는다
        // — 전용 단축키로만 쓰인다.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().add_consumable(ConsumableKind("trap_kit"));
        let player = app.world.spawn((Player, CombatStats { hp: 1, max_hp: 100, mp: 0, max_mp: 0, attack: 1, defense: 0 })).id();
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert_eq!(app.world.get::<CombatStats>(player).unwrap().hp, 1, "HP 변화 없음");
        assert_eq!(app.world.resource::<PlayerInventory>().consumables.len(), 1, "키트 소비 안 됨");
    }

    #[test]
    fn 빈_소모품슬롯을_사용하면_사용에_실패해_상태가_그대로다() {
        // use_consumable 거짓 분기: count 0 인 슬롯을 직접 만들어 도달.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        // count 0 슬롯 직접 삽입(정상 흐름에선 안 생기지만 use_consumable false 분기를 도달).
        app.world.resource_mut::<PlayerInventory>().consumables.push((ConsumableKind::HEALTH_POTION, 0));
        app.world.spawn((Player, CombatStats { hp: 50, max_hp: 100, mp: 0, max_mp: 0, attack: 1, defense: 0 }));
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        // 슬롯은 그대로(remove 되지 않음), HP 변화 없음.
        assert_eq!(app.world.resource::<PlayerInventory>().consumables.len(), 1);
    }

    #[test]
    fn 소모품이_여러개일때_앞엣것을_쓰면_커서는_그대로다() {
        // 소모품 사용 후 new_total > 0 && cursor < new_total → 양쪽 보정 모두 안함.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        {
            let mut inv = app.world.resource_mut::<PlayerInventory>();
            inv.consumables.push((ConsumableKind::HEALTH_POTION, 2));
        }
        app.world.spawn((Player, CombatStats { hp: 1, max_hp: 100, mp: 0, max_mp: 0, attack: 1, defense: 0 }));
        app.world.resource_mut::<EquipmentUiState>().cursor = 0;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        // 슬롯은 1개 남고(수량 1), 커서 0 유지.
        assert_eq!(app.world.resource::<EquipmentUiState>().cursor, 0);
        assert_eq!(app.world.resource::<PlayerInventory>().consumables[0].1, 1);
    }

    #[test]
    fn 마지막_소모품을_사용하면_커서가_0으로_보정된다() {
        // new_total == 0 분기.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().add_consumable(ConsumableKind::HEALTH_POTION);
        app.world.spawn((Player, CombatStats { hp: 50, max_hp: 100, mp: 0, max_mp: 0, attack: 1, defense: 0 }));
        app.world.resource_mut::<EquipmentUiState>().cursor = 0;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert_eq!(app.world.resource::<EquipmentUiState>().cursor, 0);
        assert!(app.world.resource::<PlayerInventory>().consumables.is_empty());
    }

    #[test]
    fn 여러_소모품_중_마지막을_사용하면_커서가_새_끝으로_보정된다() {
        // new_total > 0 && cursor >= new_total 분기.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        {
            let mut inv = app.world.resource_mut::<PlayerInventory>();
            inv.items.push(InventoryItem::new(ItemKind::Weapon(WeaponKind::SWORD)));
            inv.consumables.push((ConsumableKind::HEALTH_POTION, 1));
        }
        app.world.spawn((Player, CombatStats { hp: 50, max_hp: 100, mp: 0, max_mp: 0, attack: 1, defense: 0 }));
        // 커서를 소모품(index 1) 에 둔다.
        app.world.resource_mut::<EquipmentUiState>().cursor = 1;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        // 소모품이 사라져 총 1개(무기) → 커서는 0 으로 보정.
        assert_eq!(app.world.resource::<EquipmentUiState>().cursor, 0);
    }

    #[test]
    fn 플레이어가_없으면_소모품_사용시_HP갱신은_건너뛴다() {
        // player_query.get_single_mut() Err 분기.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().add_consumable(ConsumableKind::HEALTH_POTION);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        // 플레이어가 없어도 소모품은 소비된다.
        assert!(app.world.resource::<PlayerInventory>().consumables.is_empty());
    }

    #[test]
    fn 범위밖_소모품_인덱스에_엔터를_눌러도_안전하다() {
        // ci >= consumables.len() 분기 (소모품 0개인데 커서가 items 끝 이후).
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().items
            .push(InventoryItem::new(ItemKind::Weapon(WeaponKind::SWORD)));
        // 커서를 items.len()(=1) 로: cursor >= eq_len 이면서 소모품은 없음.
        app.world.resource_mut::<EquipmentUiState>().cursor = 1;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        // 변경 없음.
        assert!(app.world.resource::<PlayerEquipment>().weapon.is_none());
    }

    // ── update_equipment_panel (렌더) ─────────────────────────────────────

    fn 패널_하네스() -> App {
        let mut app = 렌더_하네스();
        app.add_systems(Update, update_equipment_panel);
        app.world.spawn((Visibility::Hidden, EquipmentPanel));
        app.world.spawn((Text::default(), EquipmentPanelContent));
        app
    }

    #[test]
    fn 패널이_열리면_visibility가_보임으로_바뀐다() {
        let mut app = 패널_하네스();
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.update();
        let vis = *app.world.query_filtered::<&Visibility, With<EquipmentPanel>>().single(&app.world);
        assert!(matches!(vis, Visibility::Inherited));
    }

    #[test]
    fn 패널이_닫히면_visibility가_숨김으로_바뀐다() {
        let mut app = 패널_하네스();
        // 한 번 열고
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.update();
        // 닫는다
        app.world.resource_mut::<EquipmentPanelOpen>().0 = false;
        app.update();
        let vis = *app.world.query_filtered::<&Visibility, With<EquipmentPanel>>().single(&app.world);
        assert!(matches!(vis, Visibility::Hidden));
    }

    #[test]
    fn 패널이_열리면_텍스트가_채워진다() {
        let mut app = 패널_하네스();
        app.world.resource_mut::<PlayerInventory>().items
            .push(InventoryItem::new(ItemKind::Weapon(WeaponKind::SWORD)));
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.update();
        let text = app.world.query_filtered::<&Text, With<EquipmentPanelContent>>().single(&app.world);
        assert!(!text.sections.is_empty());
    }

    #[test]
    fn 패널이_열린채로_인벤토리만_바뀌어도_텍스트가_갱신된다() {
        // panel_open.is_changed() 거짓이지만 inventory.is_changed() 참인 경로
        // (189 의 OR 뒤쪽 피연산자, 183 의 거짓 분기).
        let mut app = 패널_하네스();
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.update(); // 1회: panel_open 변경됨 + 채움
        // 텍스트를 비우고, panel_open 은 건드리지 않은 채 인벤토리만 변경.
        {
            let mut text = app.world.query_filtered::<&mut Text, With<EquipmentPanelContent>>().single_mut(&mut app.world);
            text.sections.clear();
        }
        app.world.resource_mut::<PlayerInventory>().items
            .push(InventoryItem::new(ItemKind::Weapon(WeaponKind::SWORD)));
        app.update();
        let text = app.world.query_filtered::<&Text, With<EquipmentPanelContent>>().single(&app.world);
        assert!(!text.sections.is_empty());
    }

    #[test]
    fn 열린_패널에서_장비만_바뀌어도_텍스트가_갱신된다() {
        // 189 OR 의 equipment.is_changed() 피연산자 True 도달.
        let mut app = 패널_하네스();
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.update();
        {
            let mut text = app.world.query_filtered::<&mut Text, With<EquipmentPanelContent>>().single_mut(&mut app.world);
            text.sections.clear();
        }
        app.world.resource_mut::<PlayerEquipment>().weapon = Some(WeaponKind::SWORD);
        app.update();
        let text = app.world.query_filtered::<&Text, With<EquipmentPanelContent>>().single(&app.world);
        assert!(!text.sections.is_empty());
    }

    #[test]
    fn 열린_패널에서_커서만_바뀌어도_텍스트가_갱신된다() {
        // 189 OR 의 ui_state.is_changed() 피연산자 True 도달.
        let mut app = 패널_하네스();
        app.world.resource_mut::<PlayerInventory>().items
            .push(InventoryItem::new(ItemKind::Weapon(WeaponKind::SWORD)));
        app.world.resource_mut::<PlayerInventory>().items
            .push(InventoryItem::new(ItemKind::Weapon(WeaponKind::SPEAR)));
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.update();
        {
            let mut text = app.world.query_filtered::<&mut Text, With<EquipmentPanelContent>>().single_mut(&mut app.world);
            text.sections.clear();
        }
        app.world.resource_mut::<EquipmentUiState>().cursor = 1;
        app.update();
        let text = app.world.query_filtered::<&Text, With<EquipmentPanelContent>>().single(&app.world);
        assert!(!text.sections.is_empty());
    }

    #[test]
    fn 열린_패널에서_아무것도_안바뀌면_텍스트를_다시_채우지_않는다() {
        // 189 의 모든 피연산자 거짓 → 갱신 스킵.
        let mut app = 패널_하네스();
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.update(); // 채움
        {
            let mut text = app.world.query_filtered::<&mut Text, With<EquipmentPanelContent>>().single_mut(&mut app.world);
            text.sections.clear();
        }
        app.update(); // 변경 없음
        let text = app.world.query_filtered::<&Text, With<EquipmentPanelContent>>().single(&app.world);
        assert!(text.sections.is_empty());
    }

    #[test]
    fn 텍스트_엔티티가_없어도_렌더는_안전하다() {
        // text_q.get_single_mut() Err 분기(패널은 있고 텍스트만 없음).
        let mut app = 렌더_하네스();
        app.add_systems(Update, update_equipment_panel);
        app.world.spawn((Visibility::Hidden, EquipmentPanel));
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.update();
    }

    #[test]
    fn 패널_엔티티가_없으면_렌더는_조기_반환한다() {
        // panel_q.get_single_mut() Err 분기.
        let mut app = 렌더_하네스();
        app.add_systems(Update, update_equipment_panel);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.update(); // 패널 엔티티 없음 → 패닉 없이 통과
    }

    #[test]
    fn 닫힌_패널은_텍스트를_갱신하지_않는다() {
        // !panel_open.0 의 조기 반환(is_changed false 경로 포함).
        let mut app = 패널_하네스();
        app.update(); // open=false
        let text = app.world.query_filtered::<&Text, With<EquipmentPanelContent>>().single(&app.world);
        assert!(text.sections.is_empty());
    }

    // ── 레어도/롤값 장착 흐름 + 표시 ────────────────────────────────────────

    fn rolled_weapon(rolled: i32) -> InventoryItem {
        InventoryItem { kind: ItemKind::Weapon(WeaponKind::SWORD), rolled_attack: Some(rolled), rolled_defense: None }
    }
    fn rolled_armor(rolled: i32) -> InventoryItem {
        InventoryItem { kind: ItemKind::Armor(ArmorKind::LEATHER_ARMOR), rolled_attack: None, rolled_defense: Some(rolled) }
    }

    #[test]
    fn 무기를_장착하면_롤된_공격력이_장비로_복사된다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().items.push(rolled_weapon(8));
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        let eq = app.world.resource::<PlayerEquipment>();
        assert_eq!(eq.weapon, Some(WeaponKind::SWORD));
        assert_eq!(eq.weapon_rolled_attack, Some(8), "롤값이 장비로 복사됨");
    }

    #[test]
    fn 무기를_해제하면_롤된_공격력도_사라진다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().items.push(rolled_weapon(8));
        {
            let mut eq = app.world.resource_mut::<PlayerEquipment>();
            eq.weapon = Some(WeaponKind::SWORD);
            eq.weapon_rolled_attack = Some(8);
        }
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        let eq = app.world.resource::<PlayerEquipment>();
        assert!(eq.weapon.is_none());
        assert_eq!(eq.weapon_rolled_attack, None, "해제 시 롤값도 None");
    }

    #[test]
    fn 방어구를_장착하면_롤된_방어보너스가_장비로_복사된다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().items.push(rolled_armor(4));
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        let eq = app.world.resource::<PlayerEquipment>();
        assert_eq!(eq.armor, Some(ArmorKind::LEATHER_ARMOR));
        assert_eq!(eq.armor_rolled_defense, Some(4));
    }

    #[test]
    fn 방어구를_해제하면_롤된_방어보너스도_사라진다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_equipment_input);
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().items.push(rolled_armor(4));
        {
            let mut eq = app.world.resource_mut::<PlayerEquipment>();
            eq.armor = Some(ArmorKind::LEATHER_ARMOR);
            eq.armor_rolled_defense = Some(4);
        }
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        let eq = app.world.resource::<PlayerEquipment>();
        assert!(eq.armor.is_none());
        assert_eq!(eq.armor_rolled_defense, None);
    }

    #[test]
    fn 인벤토리의_롤된_무기는_등급과_롤공격력을_표시한다() {
        // 검 5~9, 롤 9 → 전설. 패널에 [전설] 과 ATK 9 가 표시되어야 한다.
        let mut inv = empty_inv();
        inv.items.push(rolled_weapon(9));
        let text = build_panel_text(&inv, &empty_eq(), 9, qi()); // 커서를 항목 밖에 둬 색만 검증 회피
        assert!(text.contains("[전설]"), "레어도 이름 표시: {text:?}");
        assert!(text.contains("ATK 9"), "롤된 공격력 표시: {text:?}");
    }

    #[test]
    fn 인벤토리의_롤된_방어구는_등급과_롤방어력을_표시한다() {
        let mut inv = empty_inv();
        inv.items.push(rolled_armor(2)); // 가죽 2~4, 롤 2 → 일반
        let text = build_panel_text(&inv, &empty_eq(), 9, qi());
        assert!(text.contains("[일반]"), "레어도 이름 표시: {text:?}");
        assert!(text.contains("+2DEF"), "롤된 방어력 표시: {text:?}");
    }

    #[test]
    fn 커서가_가리키는_롤된_무기도_등급접두사를_표시한다() {
        // cursor == i 분기(Some(r) if i != cursor 의 거짓 → 두 번째 arm).
        let mut inv = empty_inv();
        inv.items.push(rolled_weapon(9));
        let text = build_panel_text(&inv, &empty_eq(), 0, qi());
        assert!(text.contains("[전설]"));
    }

    #[test]
    fn 장착슬롯의_롤된_무기는_등급이름과_롤공격력을_표시한다() {
        let mut eq = empty_eq();
        eq.weapon = Some(WeaponKind::SWORD);
        eq.weapon_rolled_attack = Some(9);
        let text = build_panel_text(&empty_inv(), &eq, 0, qi());
        assert!(text.contains("[전설]"));
        assert!(text.contains("ATK 9"));
    }

    #[test]
    fn 장착슬롯의_롤된_방어구는_등급이름과_롤방어력을_표시한다() {
        let mut eq = empty_eq();
        eq.armor = Some(ArmorKind::LEATHER_ARMOR);
        eq.armor_rolled_defense = Some(4);
        let text = build_panel_text(&empty_inv(), &eq, 0, qi());
        assert!(text.contains("[전설]"));
        assert!(text.contains("+4DEF"));
    }

    #[test]
    fn 롤값없는_장착무기는_등급표기없이_중앙값을_표시한다() {
        // weapon_rolled_attack None → 레어도 접두사 없음, 중앙값(7) 표시.
        let mut eq = empty_eq();
        eq.weapon = Some(WeaponKind::SWORD);
        let text = build_panel_text(&empty_inv(), &eq, 0, qi());
        assert!(!text.contains("[전설]") && !text.contains("[일반]"), "등급 접두사 없음: {text:?}");
        assert!(text.contains("ATK 7"), "중앙값 표시: {text:?}");
    }

    #[test]
    fn 롤값없는_장착방어구는_등급표기없이_중앙값을_표시한다() {
        let mut eq = empty_eq();
        eq.armor = Some(ArmorKind::LEATHER_ARMOR);
        let text = build_panel_text(&empty_inv(), &eq, 0, qi());
        assert!(!text.contains("[일반]") && !text.contains("[전설]"));
        assert!(text.contains("+3DEF"), "중앙값 3 표시: {text:?}");
    }
}
