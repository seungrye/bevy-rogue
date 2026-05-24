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
                        // 도달 불가 방어코드(None 분기): 판매 목록은 inventory.items 에서 만들어지므로
                        // 여기서 고른 무기/방어구 kind 는 항상 items 안에서 발견된다.
                        if let Some(pos) = inventory.items.iter().position(|i| i.kind == kind) {
                            // 장착 해제
                            match kind {
                                ItemKind::Weapon(w) => { if equipment.weapon == Some(w) { equipment.weapon = None; } }
                                ItemKind::Armor(a) => { if equipment.armor == Some(a) { equipment.armor = None; } }
                                // 도달 불가 방어코드: build_sell_list 가 QuestItem 을 제외하고,
                                // Consumable 은 위 바깥 match 에서 따로 처리되므로 여기 도달하는 kind 는 없다.
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
    use crate::modules::item::{InventoryItem, QuestItemKind};

    fn make_inventory(gold: u32) -> PlayerInventory {
        PlayerInventory { items: vec![], consumables: vec![], gold }
    }

    // ── 골드 / 카탈로그 / 판매목록 순수 로직 ─────────────────────────────

    #[test]
    fn 잔액이_충분하면_골드_지불에_성공한다() {
        let mut inv = make_inventory(100);
        assert!(inv.spend_gold(50));
        assert_eq!(inv.gold, 50);
    }

    #[test]
    fn 잔액이_부족하면_골드_지불에_실패한다() {
        let mut inv = make_inventory(30);
        assert!(!inv.spend_gold(50));
        assert_eq!(inv.gold, 30);
    }

    #[test]
    fn 골드_획득은_잔액을_늘린다() {
        let mut inv = make_inventory(10);
        inv.earn_gold(40);
        assert_eq!(inv.gold, 50);
    }

    #[test]
    fn 기본_인벤토리는_50골드로_시작한다() {
        let inv = PlayerInventory::default();
        assert_eq!(inv.gold, 50);
    }

    #[test]
    fn 카탈로그는_예상된_아이템들을_담고_있다() {
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
    fn 판매목록은_퀘스트_아이템을_제외한다() {
        let mut inv = make_inventory(0);
        inv.items.push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::SWORD) });
        inv.items.push(InventoryItem { kind: ItemKind::QuestItem(QuestItemKind("eternal_gem")) });
        let list = build_sell_list(&inv, qi());
        assert_eq!(list.len(), 1);
        assert!(matches!(list[0].0, ItemKind::Weapon(WeaponKind::SWORD)));
    }

    #[test]
    fn 판매목록은_소모품_수량을_표시한다() {
        let mut inv = make_inventory(0);
        inv.consumables.push((ConsumableKind::HEALTH_POTION, 3));
        let list = build_sell_list(&inv, qi());
        assert_eq!(list.len(), 1);
        assert!(list[0].1.contains("x3"));
    }

    #[test]
    fn 판매목록은_소모품이_하나면_수량표기를_생략한다() {
        // count == 1 분기 (수량 표기 없음).
        let mut inv = make_inventory(0);
        inv.consumables.push((ConsumableKind::HEALTH_POTION, 1));
        let list = build_sell_list(&inv, qi());
        assert_eq!(list.len(), 1);
        assert!(!list[0].1.contains("x"));
    }

    #[test]
    fn 카탈로그에_없는_아이템의_판매가는_기본값_10이다() {
        let price = catalog_sell_price(ItemKind::QuestItem(QuestItemKind("eternal_gem")));
        assert_eq!(price, 10);
    }

    #[test]
    fn 아이템_표시명은_종류별로_레지스트리에서_가져온다() {
        // item_display_name 의 Weapon/Armor/Consumable/QuestItem arm 을 모두 도달.
        assert_eq!(item_display_name(ItemKind::Weapon(WeaponKind::SWORD), qi()), "검");
        assert_eq!(item_display_name(ItemKind::Armor(ArmorKind::LEATHER_ARMOR), qi()), "가죽 갑옷");
        assert_eq!(item_display_name(ItemKind::Consumable(ConsumableKind::HEALTH_POTION), qi()), "체력 물약");
        assert_eq!(item_display_name(ItemKind::QuestItem(QuestItemKind("eternal_gem")), qi()), "퀘스트 아이템");
    }

    // ── App 하네스 ──────────────────────────────────────────────────────

    fn 렌더_하네스() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app.add_event::<LogMessage>();
        app.add_event::<ShopOpenEvent>();
        app.init_resource::<ShopPanelOpen>()
            .init_resource::<ShopUiState>()
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
    fn 플러그인을_등록하면_상점_리소스들이_초기화된다() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app.add_event::<LogMessage>();
        app.init_resource::<PlayerInventory>()
            .init_resource::<PlayerEquipment>()
            .insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(crate::modules::item::build_test_registry());
        app.add_plugins(ShopPlugin);
        assert!(app.world.contains_resource::<ShopPanelOpen>());
        assert!(app.world.contains_resource::<ShopUiState>());
    }

    #[test]
    fn 시작시_상점_패널과_텍스트와_폰트가_생성된다() {
        let mut app = 렌더_하네스();
        app.add_systems(Startup, setup_shop_panel);
        app.update();
        assert_eq!(app.world.query_filtered::<(), With<ShopPanel>>().iter(&app.world).count(), 1);
        assert_eq!(app.world.query_filtered::<(), With<ShopPanelContent>>().iter(&app.world).count(), 1);
        assert!(app.world.contains_resource::<ShopFont>());
    }

    #[test]
    fn 상점_열기_이벤트가_오면_구매모드로_상점이_열린다() {
        let mut app = 렌더_하네스();
        app.add_systems(Update, on_shop_open);
        app.world.resource_mut::<ShopUiState>().cursor = 3;
        app.world.resource_mut::<ShopUiState>().mode = ShopMode::Sell;
        app.world.send_event(ShopOpenEvent);
        app.update();
        assert!(app.world.resource::<ShopPanelOpen>().0);
        assert_eq!(app.world.resource::<ShopUiState>().cursor, 0);
        assert_eq!(app.world.resource::<ShopUiState>().mode, ShopMode::Buy);
    }

    #[test]
    fn 상점_열기_이벤트가_없으면_상점은_닫힌채_유지된다() {
        let mut app = 렌더_하네스();
        app.add_systems(Update, on_shop_open);
        app.update();
        assert!(!app.world.resource::<ShopPanelOpen>().0);
    }

    #[test]
    fn 닫힌_상점에서는_입력_핸들러가_아무것도_안한다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        // 닫힌 상태라 구매가 일어나지 않아 골드 그대로.
        assert_eq!(app.world.resource::<PlayerInventory>().gold, 50);
    }

    #[test]
    fn 열린_상점에서_Esc를_누르면_닫힌다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
        app.update();
        assert!(!app.world.resource::<ShopPanelOpen>().0);
    }

    #[test]
    fn Tab을_누르면_구매와_판매_모드가_번갈아_바뀐다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<ShopUiState>().cursor = 2;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Tab);
        app.update();
        assert_eq!(app.world.resource::<ShopUiState>().mode, ShopMode::Sell);
        assert_eq!(app.world.resource::<ShopUiState>().cursor, 0);

        // 같은 키를 다시 just_pressed 시키려면 떼었다가 다시 눌러야 한다.
        app.world.resource_mut::<ButtonInput<KeyCode>>().clear();
        app.world.resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::Tab);
        app.world.resource_mut::<ButtonInput<KeyCode>>().clear();
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Tab);
        app.update();
        assert_eq!(app.world.resource::<ShopUiState>().mode, ShopMode::Buy);
    }

    #[test]
    fn 구매모드에서_아래위_화살표로_커서가_이동한다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowDown);
        app.update();
        assert_eq!(app.world.resource::<ShopUiState>().cursor, 1);

        app.world.resource_mut::<ButtonInput<KeyCode>>().clear();
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowUp);
        app.update();
        assert_eq!(app.world.resource::<ShopUiState>().cursor, 0);
    }

    #[test]
    fn 첫_항목에서_위로는_안올라가고_마지막_항목에서_아래로도_안내려간다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        // 첫 항목에서 위
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowUp);
        app.update();
        assert_eq!(app.world.resource::<ShopUiState>().cursor, 0);
        // 마지막 항목에서 아래
        app.world.resource_mut::<ShopUiState>().cursor = SHOP_CATALOG.len() - 1;
        app.world.resource_mut::<ButtonInput<KeyCode>>().clear();
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowDown);
        app.update();
        assert_eq!(app.world.resource::<ShopUiState>().cursor, SHOP_CATALOG.len() - 1);
    }

    #[test]
    fn 판매할_물건이_없으면_아래화살표가_커서를_움직이지_않는다() {
        // Sell 모드 list_len == 0 분기.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<ShopUiState>().mode = ShopMode::Sell;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowDown);
        app.update();
        assert_eq!(app.world.resource::<ShopUiState>().cursor, 0);
    }

    #[test]
    fn 소모품을_구매하면_인벤토리에_추가되고_골드가_줄어든다() {
        // Buy + Consumable arm + 자동장착 _ => {} arm.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().gold = 1000;
        // 카탈로그 0번 = 체력 물약
        app.world.resource_mut::<ShopUiState>().cursor = 0;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert!(!app.world.resource::<PlayerInventory>().consumables.is_empty());
        assert_eq!(app.world.resource::<PlayerInventory>().gold, 950);
    }

    #[test]
    fn 빈손에_무기를_구매하면_자동으로_장착된다() {
        // Buy + Weapon 자동장착 분기 (weapon.is_none()).
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().gold = 1000;
        // 카탈로그 1번 = 검
        app.world.resource_mut::<ShopUiState>().cursor = 1;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert_eq!(app.world.resource::<PlayerEquipment>().weapon, Some(WeaponKind::SWORD));
    }

    #[test]
    fn 이미_무기를_들고있으면_새_무기는_자동장착되지_않는다() {
        // Weapon 자동장착 분기의 거짓(weapon 이미 있음) → _ => {} arm.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().gold = 1000;
        app.world.resource_mut::<PlayerEquipment>().weapon = Some(WeaponKind::BOW);
        app.world.resource_mut::<ShopUiState>().cursor = 1; // 검
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        // 기존 무기 유지.
        assert_eq!(app.world.resource::<PlayerEquipment>().weapon, Some(WeaponKind::BOW));
        // 그래도 인벤토리에는 검이 들어온다.
        assert!(app.world.resource::<PlayerInventory>().items.iter()
            .any(|i| i.kind == ItemKind::Weapon(WeaponKind::SWORD)));
    }

    #[test]
    fn 빈손에_방어구를_구매하면_자동으로_장착된다() {
        // Buy + Armor 자동장착 분기.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().gold = 1000;
        // 카탈로그 4번 = 가죽 갑옷
        app.world.resource_mut::<ShopUiState>().cursor = 4;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert_eq!(app.world.resource::<PlayerEquipment>().armor, Some(ArmorKind::LEATHER_ARMOR));
    }

    #[test]
    fn 이미_방어구를_입고있으면_새_방어구는_자동장착되지_않는다() {
        // Armor 자동장착 가드(armor.is_none()) 거짓 분기 → _ => {} arm.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().gold = 1000;
        app.world.resource_mut::<PlayerEquipment>().armor = Some(ArmorKind::LEATHER_ARMOR);
        app.world.resource_mut::<ShopUiState>().cursor = 4; // 가죽 갑옷
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        // 인벤토리에는 들어오지만 자동장착으로 바뀌진 않음(이미 같은 종류 장착중).
        assert!(app.world.resource::<PlayerInventory>().items.iter()
            .any(|i| i.kind == ItemKind::Armor(ArmorKind::LEATHER_ARMOR)));
    }

    #[test]
    fn 골드가_부족하면_구매가_거부된다() {
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().gold = 0;
        app.world.resource_mut::<ShopUiState>().cursor = 1; // 검 100G
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert!(app.world.resource::<PlayerInventory>().items.is_empty());
        assert_eq!(app.world.resource::<PlayerInventory>().gold, 0);
    }

    #[test]
    fn 구매모드_커서가_카탈로그_범위를_벗어나면_아무_일도_없다() {
        // Buy + SHOP_CATALOG.get(cursor) == None 분기 (도달 가능: 커서 강제 설정).
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<PlayerInventory>().gold = 1000;
        app.world.resource_mut::<ShopUiState>().cursor = 999;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        // 구매 없음.
        assert_eq!(app.world.resource::<PlayerInventory>().gold, 1000);
    }

    #[test]
    fn 판매모드에서_소모품을_팔면_수량이_줄고_골드를_받는다() {
        // Sell + Consumable arm.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<ShopUiState>().mode = ShopMode::Sell;
        app.world.resource_mut::<PlayerInventory>().gold = 0;
        app.world.resource_mut::<PlayerInventory>().consumables.push((ConsumableKind::HEALTH_POTION, 2));
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert_eq!(app.world.resource::<PlayerInventory>().consumables[0].1, 1);
        assert_eq!(app.world.resource::<PlayerInventory>().gold, 25);
    }

    #[test]
    fn 판매모드에서_장착중인_무기를_팔면_장착이_해제되고_제거된다() {
        // Sell + Weapon arm + 장착해제 분기(equipment.weapon == Some(w)).
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<ShopUiState>().mode = ShopMode::Sell;
        app.world.resource_mut::<PlayerInventory>().items
            .push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::SWORD) });
        app.world.resource_mut::<PlayerEquipment>().weapon = Some(WeaponKind::SWORD);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert!(app.world.resource::<PlayerEquipment>().weapon.is_none());
        assert!(app.world.resource::<PlayerInventory>().items.is_empty());
        assert_eq!(app.world.resource::<PlayerInventory>().gold, 100);
    }

    #[test]
    fn 판매모드에서_장착중인_방어구를_팔면_장착이_해제된다() {
        // Sell + Armor arm + 장착해제 분기.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<ShopUiState>().mode = ShopMode::Sell;
        app.world.resource_mut::<PlayerInventory>().items
            .push(InventoryItem { kind: ItemKind::Armor(ArmorKind::LEATHER_ARMOR) });
        app.world.resource_mut::<PlayerEquipment>().armor = Some(ArmorKind::LEATHER_ARMOR);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert!(app.world.resource::<PlayerEquipment>().armor.is_none());
        assert!(app.world.resource::<PlayerInventory>().items.is_empty());
    }

    #[test]
    fn 판매모드에서_장착하지_않은_무기를_팔면_장착상태는_그대로다() {
        // Sell + Weapon arm + 장착해제 분기의 거짓(다른 무기 장착중).
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<ShopUiState>().mode = ShopMode::Sell;
        app.world.resource_mut::<PlayerInventory>().items
            .push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::SWORD) });
        // 다른 무기(활)를 장착 중.
        app.world.resource_mut::<PlayerEquipment>().weapon = Some(WeaponKind::BOW);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        // 장착 무기는 유지되고, 검만 제거됨.
        assert_eq!(app.world.resource::<PlayerEquipment>().weapon, Some(WeaponKind::BOW));
        assert!(app.world.resource::<PlayerInventory>().items.is_empty());
    }

    #[test]
    fn 장착하지않은_방어구를_팔면_장착상태는_그대로다() {
        // Sell + Armor arm + 장착해제 조건(equipment.armor == Some(a)) 거짓.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<ShopUiState>().mode = ShopMode::Sell;
        app.world.resource_mut::<PlayerInventory>().items
            .push(InventoryItem { kind: ItemKind::Armor(ArmorKind::LEATHER_ARMOR) });
        // 방어구 미장착.
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert!(app.world.resource::<PlayerEquipment>().armor.is_none());
        assert!(app.world.resource::<PlayerInventory>().items.is_empty());
    }

    #[test]
    fn 첫번째_항목을_팔면_커서는_보정되지_않는다() {
        // 판매 후 커서 보정 조건(cursor > 0 && cursor >= 새 길이) 거짓: cursor == 0.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<ShopUiState>().mode = ShopMode::Sell;
        {
            let mut inv = app.world.resource_mut::<PlayerInventory>();
            inv.items.push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::SWORD) });
            inv.items.push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::SPEAR) });
        }
        app.world.resource_mut::<ShopUiState>().cursor = 0;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert_eq!(app.world.resource::<ShopUiState>().cursor, 0);
    }

    #[test]
    fn 마지막_항목을_팔면_커서가_새_끝으로_보정된다() {
        // cursor > 0 && cursor >= 새 길이 분기.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<ShopUiState>().mode = ShopMode::Sell;
        {
            let mut inv = app.world.resource_mut::<PlayerInventory>();
            inv.items.push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::SWORD) });
            inv.items.push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::SPEAR) });
        }
        app.world.resource_mut::<ShopUiState>().cursor = 1; // 마지막(창)
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        // 항목이 하나 남아 커서는 0 으로 보정.
        assert_eq!(app.world.resource::<ShopUiState>().cursor, 0);
    }

    #[test]
    fn 중간_항목을_팔면_커서는_여전히_유효해_보정되지_않는다() {
        // 보정 조건 cursor > 0 (참) && cursor >= 새 길이 (거짓) → 보정 안함.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<ShopUiState>().mode = ShopMode::Sell;
        {
            let mut inv = app.world.resource_mut::<PlayerInventory>();
            inv.items.push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::SWORD) });
            inv.items.push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::SPEAR) });
            inv.items.push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::BOW) });
        }
        // 커서 1(가운데) → 판매 후 2개 남으니 1 < 2, 보정 안함.
        app.world.resource_mut::<ShopUiState>().cursor = 1;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert_eq!(app.world.resource::<ShopUiState>().cursor, 1);
        assert_eq!(app.world.resource::<PlayerInventory>().items.len(), 2);
    }

    #[test]
    fn 판매모드_커서가_범위를_벗어나면_아무_일도_없다() {
        // Sell + sell_list.get(cursor) == None 분기.
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<ShopUiState>().mode = ShopMode::Sell;
        app.world.resource_mut::<ShopUiState>().cursor = 5; // 빈 판매목록
        app.world.resource_mut::<PlayerInventory>().gold = 7;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert_eq!(app.world.resource::<PlayerInventory>().gold, 7);
    }

    #[test]
    fn 판매모드에서_장착하지않은_무기를_팔면_장착해제없이_제거된다() {
        // Sell + Weapon arm + 장착해제 조건(equipment.weapon == Some(w)) 거짓(무기 미장착).
        let mut app = 키_입력_하네스();
        app.add_systems(Update, handle_shop_input);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<ShopUiState>().mode = ShopMode::Sell;
        app.world.resource_mut::<PlayerInventory>().items
            .push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::SWORD) });
        // 아무것도 장착하지 않은 상태.
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert!(app.world.resource::<PlayerEquipment>().weapon.is_none());
        assert!(app.world.resource::<PlayerInventory>().items.is_empty());
    }

    // ── update_shop_panel (렌더) ──────────────────────────────────────────

    fn 패널_하네스() -> App {
        let mut app = 렌더_하네스();
        app.insert_resource(ShopFont(Handle::default()));
        app.add_systems(Update, update_shop_panel);
        app.world.spawn((Visibility::Hidden, ShopPanel));
        app.world.spawn((Text::default(), ShopPanelContent));
        app
    }

    #[test]
    fn 상점이_열리면_패널이_보이고_구매목록_텍스트가_채워진다() {
        let mut app = 패널_하네스();
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.update();
        let vis = *app.world.query_filtered::<&Visibility, With<ShopPanel>>().single(&app.world);
        assert!(matches!(vis, Visibility::Visible));
        let text = app.world.query_filtered::<&Text, With<ShopPanelContent>>().single(&app.world);
        let joined: String = text.sections.iter().map(|s| s.value.as_str()).collect();
        assert!(joined.contains("상인의 상점"));
        assert!(joined.contains("검"));
    }

    #[test]
    fn 상점이_닫히면_패널이_숨겨진다() {
        let mut app = 패널_하네스();
        app.world.resource_mut::<ShopPanelOpen>().0 = false;
        app.update();
        let vis = *app.world.query_filtered::<&Visibility, With<ShopPanel>>().single(&app.world);
        assert!(matches!(vis, Visibility::Hidden));
    }

    #[test]
    fn 판매모드에서_팔게_없으면_안내문구가_나온다() {
        let mut app = 패널_하네스();
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<ShopUiState>().mode = ShopMode::Sell;
        app.update();
        let text = app.world.query_filtered::<&Text, With<ShopPanelContent>>().single(&app.world);
        let joined: String = text.sections.iter().map(|s| s.value.as_str()).collect();
        assert!(joined.contains("판매할 아이템이 없습니다"));
    }

    #[test]
    fn 판매모드에서_팔물건이_있으면_목록이_표시된다() {
        let mut app = 패널_하네스();
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<ShopUiState>().mode = ShopMode::Sell;
        app.world.resource_mut::<PlayerInventory>().items
            .push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::SWORD) });
        app.update();
        let text = app.world.query_filtered::<&Text, With<ShopPanelContent>>().single(&app.world);
        let joined: String = text.sections.iter().map(|s| s.value.as_str()).collect();
        assert!(joined.contains("검"));
    }

    #[test]
    fn 판매목록에서_커서가_가리키는_줄만_화살표로_표시된다() {
        // 판매 행 cursor 강조 분기 양방향(커서 줄 / 아닌 줄).
        let mut app = 패널_하네스();
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<ShopUiState>().mode = ShopMode::Sell;
        {
            let mut inv = app.world.resource_mut::<PlayerInventory>();
            inv.items.push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::SWORD) });
            inv.items.push(InventoryItem { kind: ItemKind::Weapon(WeaponKind::SPEAR) });
        }
        app.world.resource_mut::<ShopUiState>().cursor = 0;
        app.update();
        let text = app.world.query_filtered::<&Text, With<ShopPanelContent>>().single(&app.world);
        let joined: String = text.sections.iter().map(|s| s.value.as_str()).collect();
        // 한 줄은 화살표, 다른 한 줄은 공백 시작.
        assert!(joined.lines().any(|l| l.starts_with("> ")));
        assert!(joined.lines().filter(|l| l.contains("창") || l.contains("검")).any(|l| l.starts_with("  ")));
    }

    #[test]
    fn 구매모드에서_커서가_가리키는_줄은_화살표로_표시된다() {
        // i == cursor 분기 양방향(커서 줄/아닌 줄).
        let mut app = 패널_하네스();
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.world.resource_mut::<ShopUiState>().cursor = 1;
        app.update();
        let text = app.world.query_filtered::<&Text, With<ShopPanelContent>>().single(&app.world);
        let joined: String = text.sections.iter().map(|s| s.value.as_str()).collect();
        assert!(joined.lines().any(|l| l.starts_with("> ")));
    }

    #[test]
    fn 패널_엔티티가_없으면_렌더는_조기_반환한다() {
        // panel_q.get_single_mut() Err 분기.
        let mut app = 렌더_하네스();
        app.insert_resource(ShopFont(Handle::default()));
        app.add_systems(Update, update_shop_panel);
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.update(); // 패널 엔티티 없음 → 패닉 없음
    }

    #[test]
    fn 텍스트_엔티티가_없어도_렌더는_안전하다() {
        // content_q.get_single_mut() Err 분기.
        let mut app = 렌더_하네스();
        app.insert_resource(ShopFont(Handle::default()));
        app.add_systems(Update, update_shop_panel);
        app.world.spawn((Visibility::Hidden, ShopPanel));
        app.world.resource_mut::<ShopPanelOpen>().0 = true;
        app.update();
    }
}
