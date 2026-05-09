use bevy::prelude::*;
use crate::modules::item::{
    ItemKind, PlayerInventory, PlayerEquipment,
    WeaponKind, ArmorKind, ConsumableKind,
};
use super::LogMessage;

const PANEL_WIDTH: f32 = 280.0;
const FONT_SIZE: f32 = 13.5;

#[derive(Event)]
pub struct ShopOpenEvent;

#[derive(Resource, Default)]
pub struct ShopPanelOpen(pub bool);

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ShopMode {
    #[default]
    Buy,
    Sell,
}

#[derive(Resource, Default)]
pub struct ShopUiState {
    pub cursor: usize,
    pub mode: ShopMode,
}

pub struct ShopCatalogItem {
    pub kind: ItemKind,
    pub name: &'static str,
    pub buy_price: u32,
    pub sell_price: u32,
}

pub const SHOP_CATALOG: &[ShopCatalogItem] = &[
    ShopCatalogItem { kind: ItemKind::Consumable(ConsumableKind::HEALTH_POTION), name: "체력 물약", buy_price: 50, sell_price: 25 },
    ShopCatalogItem { kind: ItemKind::Weapon(WeaponKind::SWORD),                name: "검",       buy_price: 100, sell_price: 50 },
    ShopCatalogItem { kind: ItemKind::Weapon(WeaponKind::SPEAR),                name: "창",       buy_price: 150, sell_price: 75 },
    ShopCatalogItem { kind: ItemKind::Weapon(WeaponKind::BOW),                  name: "활",       buy_price: 80,  sell_price: 40 },
    ShopCatalogItem { kind: ItemKind::Armor(ArmorKind::LEATHER_ARMOR),           name: "가죽 갑옷", buy_price: 100, sell_price: 50 },
];

fn catalog_sell_price(kind: ItemKind) -> u32 {
    SHOP_CATALOG.iter()
        .find(|i| i.kind == kind)
        .map(|i| i.sell_price)
        .unwrap_or(10)
}

/// 인벤토리 내 판매 가능 아이템 목록: (ItemKind, 표시명, 가격, count)
fn build_sell_list(inventory: &PlayerInventory, items: &crate::modules::item::ItemRegistry) -> Vec<(ItemKind, String, u32)> {
    let mut list = Vec::new();
    for item in &inventory.items {
        if matches!(item.kind, ItemKind::QuestItem(_)) { continue; }
        let price = catalog_sell_price(item.kind);
        let name = item_display_name(item.kind, items).to_string();
        list.push((item.kind, name, price));
    }
    for (kind, count) in &inventory.consumables {
        let item_kind = ItemKind::Consumable(*kind);
        let price = catalog_sell_price(item_kind);
        let name = if *count > 1 {
            format!("{} x{}", item_display_name(item_kind, items), count)
        } else {
            item_display_name(item_kind, items).to_string()
        };
        list.push((item_kind, name, price));
    }
    list
}

fn item_display_name(kind: ItemKind, items: &crate::modules::item::ItemRegistry) -> &'static str {
    match kind {
        ItemKind::Weapon(w)     => items.weapon(w).map(|m| m.display_name).unwrap_or("???"),
        ItemKind::Armor(a)      => items.armor(a).map(|m| m.display_name).unwrap_or("???"),
        ItemKind::Consumable(c) => items.consumable(c).map(|m| m.display_name).unwrap_or("???"),
        ItemKind::QuestItem(_)  => "퀘스트 아이템",
    }
}

#[derive(Component)]
struct ShopPanel;

#[derive(Component)]
struct ShopPanelContent;

#[derive(Resource)]
struct ShopFont(Handle<Font>);

pub struct ShopPlugin;

impl Plugin for ShopPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<ShopOpenEvent>()
            .init_resource::<ShopPanelOpen>()
            .init_resource::<ShopUiState>()
            .add_systems(Startup, setup_shop_panel)
            .add_systems(Update, (
                on_shop_open,
                handle_shop_input,
                update_shop_panel,
            ).chain());
    }
}

fn setup_shop_panel(mut commands: Commands, asset_server: Res<AssetServer>) {
    let font: Handle<Font> = asset_server.load("fonts/NanumSquareNeo-bRg.ttf");
    commands.insert_resource(ShopFont(font.clone()));
    // 전체 화면 투명 컨테이너 (수평 중앙 정렬)
    commands.spawn((
        NodeBundle {
            style: Style {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            z_index: ZIndex::Global(200),
            visibility: Visibility::Hidden,
            ..default()
        },
        ShopPanel,
    )).with_children(|parent| {
        parent.spawn(NodeBundle {
            style: Style {
                width: Val::Px(PANEL_WIDTH),
                padding: UiRect::all(Val::Px(10.0)),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            background_color: Color::rgba(0.0, 0.05, 0.0, 0.97).into(),
            ..default()
        }).with_children(|panel| {
            panel.spawn((
                TextBundle::from_section(
                    "",
                    TextStyle { font, font_size: FONT_SIZE, color: Color::WHITE },
                ),
                ShopPanelContent,
            ));
        });
    });
}

fn on_shop_open(
    mut events: EventReader<ShopOpenEvent>,
    mut open: ResMut<ShopPanelOpen>,
    mut state: ResMut<ShopUiState>,
) {
    if events.read().next().is_none() { return; }
    open.0 = true;
    state.cursor = 0;
    state.mode = ShopMode::Buy;
}

fn handle_shop_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut open: ResMut<ShopPanelOpen>,
    mut state: ResMut<ShopUiState>,
    mut inventory: ResMut<PlayerInventory>,
    mut equipment: ResMut<PlayerEquipment>,
    mut log: EventWriter<LogMessage>,
    items: Res<crate::modules::item::ItemRegistry>,
) {
    if !open.0 { return; }

    if keyboard.just_pressed(KeyCode::Escape) {
        open.0 = false;
        return;
    }

    if keyboard.just_pressed(KeyCode::Tab) {
        state.mode = if state.mode == ShopMode::Buy { ShopMode::Sell } else { ShopMode::Buy };
        state.cursor = 0;
        return;
    }

    let list_len = match state.mode {
        ShopMode::Buy  => SHOP_CATALOG.len(),
        ShopMode::Sell => build_sell_list(&inventory, &items).len(),
    };

    if keyboard.just_pressed(KeyCode::ArrowUp) && state.cursor > 0 {
        state.cursor -= 1;
    }
    if keyboard.just_pressed(KeyCode::ArrowDown) && list_len > 0 && state.cursor + 1 < list_len {
        state.cursor += 1;
    }

    if keyboard.just_pressed(KeyCode::Enter) {
        match state.mode {
            ShopMode::Buy => {
                let Some(entry) = SHOP_CATALOG.get(state.cursor) else { return };
                if !inventory.spend_gold(entry.buy_price) {
                    log.send(LogMessage(format!("금화가 부족합니다. (필요: {}G)", entry.buy_price)));
                    return;
                }
                match entry.kind {
                    ItemKind::Consumable(c) => inventory.add_consumable(c),
                    kind => inventory.items.push(crate::modules::item::InventoryItem { kind }),
                }
                // 장착 아이템 자동 장착 (빈 슬롯일 경우)
                match entry.kind {
                    ItemKind::Weapon(w) if equipment.weapon.is_none() => { equipment.weapon = Some(w); }
                    ItemKind::Armor(a) if equipment.armor.is_none() => { equipment.armor = Some(a); }
                    _ => {}
                }
                log.send(LogMessage(format!("{} 구매 완료. (-{}G, 잔액: {}G)", entry.name, entry.buy_price, inventory.gold)));
            }
            ShopMode::Sell => {
                let sell_list = build_sell_list(&inventory, &items);
                let Some(&(kind, _, price)) = sell_list.get(state.cursor) else { return };
                let name = item_display_name(kind, &items).to_string();
                match kind {
                    ItemKind::Consumable(c) => { inventory.use_consumable(c); }
                    _ => {
                        if let Some(pos) = inventory.items.iter().position(|i| i.kind == kind) {
                            // 장착 해제
                            match kind {
                                ItemKind::Weapon(w) => { if equipment.weapon == Some(w) { equipment.weapon = None; } }
                                ItemKind::Armor(a) => { if equipment.armor == Some(a) { equipment.armor = None; } }
                                _ => {}
                            }
                            inventory.items.remove(pos);
                        }
                    }
                }
                inventory.earn_gold(price);
                if state.cursor > 0 && state.cursor >= build_sell_list(&inventory, &items).len() {
                    state.cursor -= 1;
                }
                log.send(LogMessage(format!("{} 판매 완료. (+{}G, 잔액: {}G)", name, price, inventory.gold)));
            }
        }
    }
}

fn update_shop_panel(
    open: Res<ShopPanelOpen>,
    state: Res<ShopUiState>,
    inventory: Res<PlayerInventory>,
    shop_font: Res<ShopFont>,
    items: Res<crate::modules::item::ItemRegistry>,
    mut panel_q: Query<&mut Visibility, With<ShopPanel>>,
    mut content_q: Query<&mut Text, With<ShopPanelContent>>,
) {
    let Ok(mut vis) = panel_q.get_single_mut() else { return };
    *vis = if open.0 { Visibility::Visible } else { Visibility::Hidden };
    if !open.0 { return; }

    let Ok(mut text) = content_q.get_single_mut() else { return };
    let yellow = Color::rgb(1.0, 0.9, 0.1);
    let green  = Color::rgb(0.3, 1.0, 0.6);
    let dim    = Color::rgba(0.6, 0.6, 0.6, 1.0);
    let white  = Color::WHITE;

    let mut sections: Vec<TextSection> = Vec::new();
    let font = shop_font.0.clone();

    let make = |s: String, c: Color| TextSection::new(s, TextStyle { font: font.clone(), font_size: FONT_SIZE, color: c });

    // 헤더
    sections.push(make("═══ 상인의 상점 ═══\n".into(), yellow));

    // 탭
    let (buy_color, sell_color) = if state.mode == ShopMode::Buy { (yellow, dim) } else { (dim, yellow) };
    sections.push(make("  ".into(), white));
    sections.push(make("[구매]".into(), buy_color));
    sections.push(make("  ".into(), white));
    sections.push(make("[판매]".into(), sell_color));
    sections.push(make("  (Tab)\n\n".into(), dim));

    match state.mode {
        ShopMode::Buy => {
            for (i, entry) in SHOP_CATALOG.iter().enumerate() {
                let cursor_str = if i == state.cursor { "> " } else { "  " };
                let row_color = if i == state.cursor { green } else { white };
                let line = format!("{}{:<10} {:>4}G\n", cursor_str, entry.name, entry.buy_price);
                sections.push(make(line, row_color));
            }
        }
        ShopMode::Sell => {
            let sell_list = build_sell_list(&inventory, &items);
            if sell_list.is_empty() {
                sections.push(make("  판매할 아이템이 없습니다\n".into(), dim));
            } else {
                for (i, (_, name, price)) in sell_list.iter().enumerate() {
                    let cursor_str = if i == state.cursor { "> " } else { "  " };
                    let row_color = if i == state.cursor { green } else { white };
                    let line = format!("{}{:<14} {:>4}G\n", cursor_str, name, price);
                    sections.push(make(line, row_color));
                }
            }
        }
    }

    sections.push(make("\n".into(), white));
    sections.push(make(format!("보유 금화: {}G\n", inventory.gold), yellow));
    sections.push(make("──────────────────\n".into(), dim));
    sections.push(make("↑↓이동  Enter확인  Esc닫기\n".into(), dim));

    text.sections = sections;
}

// ── 단위 테스트 ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_inventory(gold: u32) -> PlayerInventory {
        PlayerInventory { items: vec![], consumables: vec![], gold }
    }

    #[test]
    fn spend_gold_succeeds_when_sufficient() {
        let mut inv = make_inventory(100);
        assert!(inv.spend_gold(50));
        assert_eq!(inv.gold, 50);
    }

    #[test]
    fn spend_gold_fails_when_insufficient() {
        let mut inv = make_inventory(30);
        assert!(!inv.spend_gold(50));
        assert_eq!(inv.gold, 30);
    }

    #[test]
    fn earn_gold_increases_balance() {
        let mut inv = make_inventory(10);
        inv.earn_gold(40);
        assert_eq!(inv.gold, 50);
    }

    #[test]
    fn default_inventory_starts_with_50_gold() {
        let inv = PlayerInventory::default();
        assert_eq!(inv.gold, 50);
    }

    #[test]
    fn catalog_has_expected_items() {
        assert!(SHOP_CATALOG.iter().any(|i| i.name == "체력 물약"));
        assert!(SHOP_CATALOG.iter().any(|i| i.name == "검"));
        assert!(SHOP_CATALOG.iter().any(|i| i.name == "가죽 갑옷"));
    }

    use std::sync::OnceLock;
    static TEST_QI: OnceLock<crate::modules::item::ItemRegistry> = OnceLock::new();
    fn qi() -> &'static crate::modules::item::ItemRegistry {
        TEST_QI.get_or_init(|| crate::modules::item::build_test_registry())
    }

    #[test]
    fn sell_list_excludes_quest_items() {
        use crate::modules::item::{InventoryItem, QuestItemKind};
        let mut inv = make_inventory(0);
        inv.items.push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::SWORD) });
        inv.items.push(InventoryItem { kind: ItemKind::QuestItem(QuestItemKind("eternal_gem")) });
        let list = build_sell_list(&inv, qi());
        assert_eq!(list.len(), 1);
        assert!(matches!(list[0].0, ItemKind::Weapon(WeaponKind::SWORD)));
    }

    #[test]
    fn sell_list_shows_consumable_count() {
        let mut inv = make_inventory(0);
        inv.consumables.push((ConsumableKind::HEALTH_POTION, 3));
        let list = build_sell_list(&inv, qi());
        assert_eq!(list.len(), 1);
        assert!(list[0].1.contains("x3"));
    }

    #[test]
    fn catalog_sell_price_returns_default_for_unknown() {
        // QuestItem은 카탈로그에 없으므로 기본값 10
        use crate::modules::item::QuestItemKind;
        let price = catalog_sell_price(ItemKind::QuestItem(QuestItemKind("eternal_gem")));
        assert_eq!(price, 10);
    }
}
