use bevy::prelude::*;
use rand::Rng;
use serde::Deserialize;
use std::collections::HashMap;
use crate::modules::{
    map::{tile_to_world_coords, world_to_tile_coords, TILE_SIZE, PlayerActedEvent},
    player::{Player, MovingTo, PlayerSystemSet, PLAYER_ATK, PLAYER_DEF},
    combat::CombatStats,
    ui::LogMessage,
    quest::{DespawnWorldItemEvent, item_id_to_kind},
};

pub const POTION_HEAL: i32 = 8;
const Z_ITEM: f32 = 0.3;
const Z_QUEST_POPUP: i32 = 100;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GlyphStyle {
    #[default]
    Ascii,
    Unicode,
    GameIcon,
}

impl GlyphStyle {
    pub fn next(self) -> Self {
        match self {
            GlyphStyle::Ascii    => GlyphStyle::Unicode,
            GlyphStyle::Unicode  => GlyphStyle::GameIcon,
            GlyphStyle::GameIcon => GlyphStyle::Ascii,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            GlyphStyle::Ascii    => "ASCII",
            GlyphStyle::Unicode  => "유니코드",
            GlyphStyle::GameIcon => "RPG 아이콘",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "ascii"              => Some(GlyphStyle::Ascii),
            "unicode"            => Some(GlyphStyle::Unicode),
            "icon" | "gameicon"  => Some(GlyphStyle::GameIcon),
            _                    => None,
        }
    }
}

#[derive(Resource)]
pub struct GlyphConfig {
    pub style: GlyphStyle,
}

impl Default for GlyphConfig {
    fn default() -> Self { Self { style: GlyphStyle::default() } }
}

#[derive(Resource)]
pub struct GlyphFontHandles {
    pub ascii:     Handle<Font>,
    pub unicode:   Handle<Font>,
    pub game_icon: Handle<Font>,
}

impl GlyphFontHandles {
    pub fn for_style(&self, style: GlyphStyle) -> Handle<Font> {
        match style {
            GlyphStyle::Ascii    => self.ascii.clone(),
            GlyphStyle::Unicode  => self.unicode.clone(),
            GlyphStyle::GameIcon => self.game_icon.clone(),
        }
    }
}

pub fn glyph_for_style(kind: ItemKind, style: GlyphStyle, registry: &QuestItemRegistry) -> &'static str {
    match style {
        GlyphStyle::Ascii    => kind.glyph(registry),
        GlyphStyle::Unicode  => glyph_unicode(kind, registry),
        GlyphStyle::GameIcon => glyph_game_icon(kind, registry),
    }
}

fn glyph_unicode(kind: ItemKind, registry: &QuestItemRegistry) -> &'static str {
    match kind {
        ItemKind::Weapon(w) => match w {
            WeaponKind::Sword => "\u{1F5E1}", // 🗡 단검 모양
            WeaponKind::Spear => "\u{2B06}",  // ⬆ 위쪽 화살표
            WeaponKind::Bow   => "\u{27A4}",  // ➤ 오른쪽 화살촉
        },
        ItemKind::Armor(a) => match a {
            ArmorKind::LeatherArmor => "\u{1F6E1}", // 🛡 방패
        },
        ItemKind::Consumable(c) => match c {
            ConsumableKind::HealthPotion => "\u{2764}", // ❤ 굵은 하트
        },
        ItemKind::QuestItem(qk) => registry.lookup(qk).map(|m| m.glyph_unicode).unwrap_or("?"),
    }
}

fn glyph_game_icon(kind: ItemKind, registry: &QuestItemRegistry) -> &'static str {
    match kind {
        ItemKind::Weapon(w) => match w {
            WeaponKind::Sword => "\u{E946}", // RPG Awesome 넓은 검 아이콘
            WeaponKind::Spear => "\u{EAAC}", // RPG Awesome 창끝 아이콘
            WeaponKind::Bow   => "\u{E978}", // RPG Awesome 석궁 아이콘
        },
        ItemKind::Armor(a) => match a {
            ArmorKind::LeatherArmor => "\u{EA96}", // RPG Awesome 방패 아이콘
        },
        ItemKind::Consumable(c) => match c {
            ConsumableKind::HealthPotion => "\u{EA72}", // RPG Awesome 물약 아이콘
        },
        ItemKind::QuestItem(qk) => registry.lookup(qk).map(|m| m.glyph_game_icon).unwrap_or("?"),
    }
}

/// 퀘스트 아이템 ID — &'static str 기반 newtype (Copy 유지를 위해)
/// 등록된 ID 는 startup 시점에 Box::leak 으로 영속화되어 registry 의 키와 동일.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct QuestItemKind(pub &'static str);

impl QuestItemKind {
    pub fn id(self) -> &'static str { self.0 }
}

// serde: 단순 문자열로 직렬화/역직렬화 (저장 데이터 호환)
impl serde::Serialize for QuestItemKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.0)
    }
}

impl<'de> serde::Deserialize<'de> for QuestItemKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // serde 컨텍스트에서는 registry 접근이 어려워 leak 으로 fallback.
        // 저장 데이터 로드 시 사용되며, leak 은 save 데이터 크기에 의해 bounded.
        // PartialEq/Hash 는 내용 비교라 registry 의 leak 된 키와도 동등하게 동작.
        let s = String::deserialize(d)?;
        Ok(QuestItemKind(Box::leak(s.into_boxed_str())))
    }
}

/// RON 에서 불러오는 퀘스트 아이템 메타데이터 (raw)
#[derive(Debug, Deserialize, Clone)]
pub struct QuestItemDef {
    pub id: String,
    pub display_name: String,
    pub glyph_ascii: String,
    pub glyph_unicode: String,
    pub glyph_game_icon: String,
    pub pickup_message: String,
    pub image_path: String,
}

/// leak 된 &'static 메타데이터 — registry 가 보유
#[derive(Debug, Clone)]
pub struct QuestItemMeta {
    pub display_name: &'static str,
    pub glyph_ascii: &'static str,
    pub glyph_unicode: &'static str,
    pub glyph_game_icon: &'static str,
    pub pickup_message: &'static str,
    pub image_path: &'static str,
}

/// Bevy Resource — startup 시점에 RON 에서 로드되어 init 됨.
/// VillagerRegistry 와 동일한 Resource 패턴으로 일관성 유지.
#[derive(Resource, Default)]
pub struct QuestItemRegistry {
    pub items: HashMap<&'static str, QuestItemMeta>,
}

impl QuestItemRegistry {
    pub fn lookup(&self, kind: QuestItemKind) -> Option<&QuestItemMeta> {
        self.items.get(kind.0)
    }

    pub fn lookup_id(&self, id: &str) -> Option<&QuestItemMeta> {
        self.items.get(id)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.items.contains_key(id)
    }

    /// 같은 ID 의 leak 된 &'static str 을 반환한다 (registry 에 등록된 경우)
    pub fn intern(&self, id: &str) -> Option<&'static str> {
        self.items.get_key_value(id).map(|(k, _)| *k)
    }
}

/// item 시스템 Startup 단계 ordering
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ItemSystemSet {
    Load,
}

/// RON 에서 quest item 정의를 읽어 Resource 에 적재한다
fn load_quest_items_system(mut registry: ResMut<QuestItemRegistry>) {
    let path = "assets/items/quest_items.ron";
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("[치명적] {} 읽기 실패: {}", path, e));
    let defs: Vec<QuestItemDef> = ron::de::from_str(&text)
        .unwrap_or_else(|e| panic!("[치명적] {} RON 파싱 실패: {}", path, e));

    let mut map: HashMap<&'static str, QuestItemMeta> = HashMap::new();
    for def in defs {
        let id: &'static str = Box::leak(def.id.into_boxed_str());
        let meta = QuestItemMeta {
            display_name:    Box::leak(def.display_name.into_boxed_str()),
            glyph_ascii:     Box::leak(def.glyph_ascii.into_boxed_str()),
            glyph_unicode:   Box::leak(def.glyph_unicode.into_boxed_str()),
            glyph_game_icon: Box::leak(def.glyph_game_icon.into_boxed_str()),
            pickup_message:  Box::leak(def.pickup_message.into_boxed_str()),
            image_path:      Box::leak(def.image_path.into_boxed_str()),
        };
        map.insert(id, meta);
    }
    info!("quest item 로드: {} 종", map.len());
    registry.items = map;
}

/// 테스트용 — registry 를 inline 으로 구성한다
#[cfg(test)]
pub fn build_test_registry() -> QuestItemRegistry {
    let path = "assets/items/quest_items.ron";
    let text = std::fs::read_to_string(path).expect("quest_items.ron 읽기 실패");
    let defs: Vec<QuestItemDef> = ron::de::from_str(&text).expect("quest_items.ron 파싱 실패");
    let mut map: HashMap<&'static str, QuestItemMeta> = HashMap::new();
    for def in defs {
        let id: &'static str = Box::leak(def.id.into_boxed_str());
        let meta = QuestItemMeta {
            display_name:    Box::leak(def.display_name.into_boxed_str()),
            glyph_ascii:     Box::leak(def.glyph_ascii.into_boxed_str()),
            glyph_unicode:   Box::leak(def.glyph_unicode.into_boxed_str()),
            glyph_game_icon: Box::leak(def.glyph_game_icon.into_boxed_str()),
            pickup_message:  Box::leak(def.pickup_message.into_boxed_str()),
            image_path:      Box::leak(def.image_path.into_boxed_str()),
        };
        map.insert(id, meta);
    }
    QuestItemRegistry { items: map }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum WeaponKind {
    Sword,
    Spear,
    Bow,
}

impl WeaponKind {
    pub fn display_name(self) -> &'static str {
        match self {
            WeaponKind::Sword => "검",
            WeaponKind::Spear => "창",
            WeaponKind::Bow   => "활",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum ArmorKind {
    LeatherArmor,
}

impl ArmorKind {
    pub fn display_name(self) -> &'static str {
        match self {
            ArmorKind::LeatherArmor => "가죽 갑옷",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum ConsumableKind {
    HealthPotion,
}

impl ConsumableKind {
    pub fn display_name(self) -> &'static str {
        match self {
            ConsumableKind::HealthPotion => "체력 물약",
        }
    }

    pub fn heal_amount(self) -> i32 {
        match self {
            ConsumableKind::HealthPotion => POTION_HEAL,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum ItemKind {
    Weapon(WeaponKind),
    Armor(ArmorKind),
    Consumable(ConsumableKind),
    QuestItem(QuestItemKind),
}

impl ItemKind {
    pub fn glyph(self, registry: &QuestItemRegistry) -> &'static str {
        match self {
            ItemKind::Weapon(w) => match w {
                WeaponKind::Sword => "/",
                WeaponKind::Spear => "|",
                WeaponKind::Bow   => ")",
            },
            ItemKind::Armor(a) => match a {
                ArmorKind::LeatherArmor => "]",
            },
            ItemKind::Consumable(c) => match c {
                ConsumableKind::HealthPotion => "!",
            },
            ItemKind::QuestItem(qk) => registry.lookup(qk).map(|m| m.glyph_ascii).unwrap_or("?"),
        }
    }

    pub fn color(self) -> Color {
        match self {
            ItemKind::Weapon(_)     => Color::rgb(1.0, 1.0, 0.2),
            ItemKind::Armor(_)      => Color::rgb(0.2, 0.4, 1.0),
            ItemKind::Consumable(_) => Color::rgb(0.2, 0.9, 0.2),
            ItemKind::QuestItem(_)  => Color::rgb(0.8, 0.3, 1.0),
        }
    }

    pub fn display_name(self, registry: &QuestItemRegistry) -> &'static str {
        match self {
            ItemKind::Weapon(w)     => w.display_name(),
            ItemKind::Armor(a)      => a.display_name(),
            ItemKind::Consumable(c) => c.display_name(),
            ItemKind::QuestItem(qk) => registry.lookup(qk).map(|m| m.display_name).unwrap_or("???"),
        }
    }

    pub fn pickup_message(self, registry: &QuestItemRegistry) -> &'static str {
        match self {
            ItemKind::Weapon(w) => match w {
                WeaponKind::Sword => "검을 획득했다!",
                WeaponKind::Spear => "창을 획득했다!",
                WeaponKind::Bow   => "활을 획득했다!",
            },
            ItemKind::Armor(a) => match a {
                ArmorKind::LeatherArmor => "가죽 갑옷을 획득했다!",
            },
            ItemKind::Consumable(c) => match c {
                ConsumableKind::HealthPotion => "체력 물약을 획득했다!",
            },
            ItemKind::QuestItem(qk) => registry.lookup(qk).map(|m| m.pickup_message).unwrap_or("아이템을 획득했다!"),
        }
    }
}

pub fn weapon_attack(kind: WeaponKind) -> i32 {
    match kind {
        WeaponKind::Sword => 7,
        WeaponKind::Spear => 9,
        WeaponKind::Bow   => 5,
    }
}

pub fn armor_defense_bonus(kind: ArmorKind) -> i32 {
    match kind {
        ArmorKind::LeatherArmor => 2,
    }
}

pub fn effective_attack(equipment: &PlayerEquipment) -> i32 {
    equipment.weapon.map(weapon_attack).unwrap_or(PLAYER_ATK)
}

pub fn effective_defense(equipment: &PlayerEquipment) -> i32 {
    let bonus = equipment.armor.map(armor_defense_bonus).unwrap_or(0);
    PLAYER_DEF + bonus
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct InventoryItem {
    pub kind: ItemKind,
}

#[derive(Resource, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlayerInventory {
    pub items: Vec<InventoryItem>,
    pub consumables: Vec<(ConsumableKind, u32)>,
    pub gold: u32,
}

impl Default for PlayerInventory {
    fn default() -> Self {
        Self { items: Vec::new(), consumables: Vec::new(), gold: 50 }
    }
}

impl PlayerInventory {
    pub fn earn_gold(&mut self, amount: u32) { self.gold += amount; }
    pub fn spend_gold(&mut self, amount: u32) -> bool {
        if self.gold >= amount { self.gold -= amount; true } else { false }
    }

    pub fn add_consumable(&mut self, kind: ConsumableKind) {
        if let Some(slot) = self.consumables.iter_mut().find(|(k, _)| *k == kind) {
            slot.1 += 1;
        } else {
            self.consumables.push((kind, 1));
        }
    }

    pub fn use_consumable(&mut self, kind: ConsumableKind) -> bool {
        if let Some(pos) = self.consumables.iter().position(|(k, _)| *k == kind) {
            if self.consumables[pos].1 > 0 {
                self.consumables[pos].1 -= 1;
                if self.consumables[pos].1 == 0 {
                    self.consumables.remove(pos);
                }
                return true;
            }
        }
        false
    }
}

#[derive(Resource, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlayerEquipment {
    pub weapon: Option<WeaponKind>,
    pub armor:  Option<ArmorKind>,
}

#[derive(Resource, Default)]
pub struct EquipmentPanelOpen(pub bool);

#[derive(Component)]
pub struct Item {
    pub kind:   ItemKind,
    pub tile_x: usize,
    pub tile_y: usize,
}

#[derive(Event)]
pub struct QuestItemAcquiredEvent(pub QuestItemKind);

#[derive(Component)]
struct QuestItemPopup {
    tile_x: usize,
    tile_y: usize,
}

/// 몬스터 처치 시 아이템 드롭을 요청하는 이벤트
#[derive(Event)]
pub struct ItemDropEvent {
    pub tile_x:       usize,
    pub tile_y:       usize,
    pub monster_name: String,
}

/// 몬스터별 드롭 테이블 — 각 항목은 독립 확률로 롤된다
pub fn monster_drop_table(monster_name: &str) -> &'static [(ItemKind, f32)] {
    match monster_name {
        "고블린" => &[
            (ItemKind::Consumable(ConsumableKind::HealthPotion), 0.30),
            (ItemKind::Weapon(WeaponKind::Sword), 0.15),
        ],
        "오크" => &[
            (ItemKind::Consumable(ConsumableKind::HealthPotion), 0.40),
            (ItemKind::Weapon(WeaponKind::Spear), 0.20),
            (ItemKind::Armor(ArmorKind::LeatherArmor), 0.10),
        ],
        "트롤" => &[
            (ItemKind::Consumable(ConsumableKind::HealthPotion), 0.50),
            (ItemKind::Weapon(WeaponKind::Bow), 0.25),
            (ItemKind::Armor(ArmorKind::LeatherArmor), 0.20),
        ],
        _ => &[
            (ItemKind::Consumable(ConsumableKind::HealthPotion), 0.25),
        ],
    }
}

pub struct ItemPlugin {
    pub initial_glyph_style: GlyphStyle,
}

impl Default for ItemPlugin {
    fn default() -> Self { Self { initial_glyph_style: GlyphStyle::Ascii } }
}

impl Plugin for ItemPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GlyphConfig { style: self.initial_glyph_style })
            .add_event::<ItemDropEvent>()
            .add_event::<QuestItemAcquiredEvent>()
            .init_resource::<PlayerInventory>()
            .init_resource::<PlayerEquipment>()
            .init_resource::<EquipmentPanelOpen>()
            .init_resource::<QuestItemRegistry>()
            .add_systems(Startup, (
                load_quest_items_system.in_set(ItemSystemSet::Load),
                setup_glyph_fonts,
            ))
            .add_systems(Update, (
                spawn_dropped_items,
                pickup_items.after(PlayerSystemSet::MovementComplete),
                handle_despawn_world_item,
                apply_equipment_stats,
                update_item_glyphs,
                cycle_glyph_style,
                spawn_quest_item_popup,
                close_quest_item_popup,
            ));
    }
}

fn setup_glyph_fonts(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.insert_resource(GlyphFontHandles {
        ascii:     asset_server.load("fonts/FiraMono-Medium.ttf"),
        unicode:   asset_server.load("fonts/NotoSansSymbols2-Regular.ttf"),
        game_icon: asset_server.load("fonts/rpg-awesome.ttf"),
    });
}

fn spawn_dropped_items(
    mut events: EventReader<ItemDropEvent>,
    mut commands: Commands,
    config: Res<GlyphConfig>,
    font_handles: Res<GlyphFontHandles>,
    quest_items: Res<QuestItemRegistry>,
) {
    let mut rng = rand::thread_rng();
    for event in events.read() {
        for &(kind, rate) in monster_drop_table(&event.monster_name) {
            if rng.gen::<f32>() >= rate { continue; }
            let pos = tile_to_world_coords(event.tile_x, event.tile_y);
            commands.spawn((
                Text2dBundle {
                    text: Text::from_section(
                        glyph_for_style(kind, config.style, &quest_items),
                        TextStyle {
                            font: font_handles.for_style(config.style),
                            font_size: TILE_SIZE,
                            color: kind.color(),
                        },
                    ),
                    transform: Transform::from_xyz(pos.x, pos.y, Z_ITEM),
                    ..default()
                },
                Item { kind, tile_x: event.tile_x, tile_y: event.tile_y },
            ));
        }
    }
}

fn update_item_glyphs(
    config: Res<GlyphConfig>,
    font_handles: Res<GlyphFontHandles>,
    quest_items: Res<QuestItemRegistry>,
    mut item_query: Query<(&Item, &mut Text)>,
) {
    if !config.is_changed() { return; }
    for (item, mut text) in item_query.iter_mut() {
        text.sections[0].value = glyph_for_style(item.kind, config.style, &quest_items).to_string();
        text.sections[0].style.font = font_handles.for_style(config.style);
    }
}

fn cycle_glyph_style(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut config: ResMut<GlyphConfig>,
    mut log: EventWriter<LogMessage>,
) {
    if keyboard.just_pressed(KeyCode::KeyG) {
        config.style = config.style.next();
        log.send(LogMessage(format!("글리프 스타일: {}", config.style.display_name())));
    }
}

fn pickup_items(
    mut commands: Commands,
    mut turn_events: EventReader<PlayerActedEvent>,
    player_query: Query<(Option<&MovingTo>, &Transform), With<Player>>,
    item_query: Query<(Entity, &Item)>,
    mut inventory: ResMut<PlayerInventory>,
    mut log: EventWriter<LogMessage>,
    mut quest_acquired: EventWriter<QuestItemAcquiredEvent>,
    quest_items: Res<QuestItemRegistry>,
) {
    if turn_events.read().next().is_none() { return; }
    let Ok((moving_to, transform)) = player_query.get_single() else { return };
    let (px, py) = moving_to
        .map(|m| world_to_tile_coords(m.target))
        .unwrap_or_else(|| world_to_tile_coords(transform.translation));

    let at_tile: Vec<(Entity, ItemKind)> = item_query.iter()
        .filter(|(_, item)| item.tile_x == px && item.tile_y == py)
        .map(|(e, item)| (e, item.kind))
        .collect();

    for (entity, kind) in at_tile {
        match kind {
            ItemKind::Weapon(_) | ItemKind::Armor(_) | ItemKind::QuestItem(_) => {
                inventory.items.push(InventoryItem { kind });
            }
            ItemKind::Consumable(ck) => {
                inventory.add_consumable(ck);
            }
        }
        if let ItemKind::QuestItem(qk) = kind {
            quest_acquired.send(QuestItemAcquiredEvent(qk));
        }
        log.send(LogMessage(kind.pickup_message(&quest_items).to_string()));
        commands.entity(entity).despawn();
    }
}

fn apply_equipment_stats(
    equipment: Res<PlayerEquipment>,
    mut player_query: Query<&mut CombatStats, With<Player>>,
) {
    if !equipment.is_changed() { return; }
    let Ok(mut stats) = player_query.get_single_mut() else { return };
    stats.attack  = effective_attack(&equipment);
    stats.defense = effective_defense(&equipment);
}

fn quest_item_image_path(kind: QuestItemKind, registry: &QuestItemRegistry) -> &'static str {
    registry.lookup(kind).map(|m| m.image_path).unwrap_or("scene/open-chest.png")
}

fn spawn_quest_item_popup(
    mut commands: Commands,
    mut events: EventReader<QuestItemAcquiredEvent>,
    asset_server: Res<AssetServer>,
    popup_q: Query<(), With<QuestItemPopup>>,
    player_q: Query<(Option<&MovingTo>, &Transform), With<Player>>,
    quest_items: Res<QuestItemRegistry>,
) {
    // 오래된 이벤트가 다음 프레임에 처리되지 않도록 먼저 모두 비운다.
    // 첫 번째 이벤트만 사용하고 나머지는 의도적으로 버린다.
    let all_events: Vec<_> = events.read().collect();
    if all_events.is_empty() || !popup_q.is_empty() { return; }

    let QuestItemAcquiredEvent(kind) = all_events[0];
    let Ok((moving_to, transform)) = player_q.get_single() else { return };
    let (tile_x, tile_y) = moving_to
        .map(|m| world_to_tile_coords(m.target))
        .unwrap_or_else(|| world_to_tile_coords(transform.translation));

    let image = asset_server.load(quest_item_image_path(*kind, &quest_items));
    commands.spawn((
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                position_type: PositionType::Absolute,
                ..default()
            },
            z_index: ZIndex::Global(Z_QUEST_POPUP),
            background_color: Color::NONE.into(),
            ..default()
        },
        QuestItemPopup { tile_x, tile_y },
    )).with_children(|parent| {
        parent.spawn(ImageBundle {
            image: image.into(),
            style: Style {
                width: Val::Percent(50.0),
                height: Val::Auto,
                ..default()
            },
            ..default()
        });
    });
}

fn close_quest_item_popup(
    mut commands: Commands,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    popup_q: Query<(Entity, &QuestItemPopup)>,
    player_q: Query<(Option<&MovingTo>, &Transform), With<Player>>,
) {
    if popup_q.is_empty() { return; }

    if keyboard_input.just_pressed(KeyCode::Escape) {
        for (entity, _) in popup_q.iter() {
            commands.entity(entity).despawn_recursive();
        }
        return;
    }

    let Ok((moving_to, transform)) = player_q.get_single() else { return };
    let (px, py) = moving_to
        .map(|m| world_to_tile_coords(m.target))
        .unwrap_or_else(|| world_to_tile_coords(transform.translation));

    for (entity, popup) in popup_q.iter() {
        if px != popup.tile_x || py != popup.tile_y {
            commands.entity(entity).despawn_recursive();
        }
    }
}

/// DespawnWorldItemEvent 를 받아 월드에 있는 해당 아이템 엔티티를 제거한다
fn handle_despawn_world_item(
    mut events: EventReader<DespawnWorldItemEvent>,
    item_query: Query<(Entity, &Item)>,
    mut commands: Commands,
    quest_items: Res<QuestItemRegistry>,
) {
    for DespawnWorldItemEvent(item_id) in events.read() {
        let Some(kind) = item_id_to_kind(item_id, &quest_items) else { continue };
        for (entity, item) in item_query.iter() {
            if item.kind == kind {
                commands.entity(entity).despawn();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weapon_attack_sword_is_7() {
        assert_eq!(weapon_attack(WeaponKind::Sword), 7);
    }

    #[test]
    fn weapon_attack_spear_is_9() {
        assert_eq!(weapon_attack(WeaponKind::Spear), 9);
    }

    #[test]
    fn weapon_attack_bow_is_5() {
        assert_eq!(weapon_attack(WeaponKind::Bow), 5);
    }

    #[test]
    fn armor_defense_bonus_leather_is_2() {
        assert_eq!(armor_defense_bonus(ArmorKind::LeatherArmor), 2);
    }

    #[test]
    fn effective_attack_no_weapon_equals_player_default() {
        let eq = PlayerEquipment { weapon: None, armor: None };
        assert_eq!(effective_attack(&eq), PLAYER_ATK);
    }

    #[test]
    fn effective_attack_with_sword_is_7() {
        let eq = PlayerEquipment { weapon: Some(WeaponKind::Sword), armor: None };
        assert_eq!(effective_attack(&eq), 7);
    }

    #[test]
    fn effective_defense_no_armor_equals_player_default() {
        let eq = PlayerEquipment { weapon: None, armor: None };
        assert_eq!(effective_defense(&eq), PLAYER_DEF);
    }

    #[test]
    fn effective_defense_with_leather_adds_bonus() {
        let eq = PlayerEquipment { weapon: None, armor: Some(ArmorKind::LeatherArmor) };
        assert_eq!(effective_defense(&eq), PLAYER_DEF + 2);
    }

    #[test]
    fn goblin_drop_table_has_potion_and_sword() {
        let t = monster_drop_table("고블린");
        assert!(t.iter().any(|(k, _)| matches!(k, ItemKind::Consumable(ConsumableKind::HealthPotion))));
        assert!(t.iter().any(|(k, _)| matches!(k, ItemKind::Weapon(WeaponKind::Sword))));
    }

    #[test]
    fn orc_drop_table_has_spear_and_armor() {
        let t = monster_drop_table("오크");
        assert!(t.iter().any(|(k, _)| matches!(k, ItemKind::Weapon(WeaponKind::Spear))));
        assert!(t.iter().any(|(k, _)| matches!(k, ItemKind::Armor(ArmorKind::LeatherArmor))));
    }

    #[test]
    fn troll_drop_table_has_bow_and_armor() {
        let t = monster_drop_table("트롤");
        assert!(t.iter().any(|(k, _)| matches!(k, ItemKind::Weapon(WeaponKind::Bow))));
        assert!(t.iter().any(|(k, _)| matches!(k, ItemKind::Armor(ArmorKind::LeatherArmor))));
    }

    #[test]
    fn add_consumable_stacks_same_kind() {
        let mut inv = PlayerInventory::default();
        inv.add_consumable(ConsumableKind::HealthPotion);
        inv.add_consumable(ConsumableKind::HealthPotion);
        assert_eq!(inv.consumables.len(), 1);
        assert_eq!(inv.consumables[0].1, 2);
    }

    #[test]
    fn use_consumable_decrements_count() {
        let mut inv = PlayerInventory::default();
        inv.add_consumable(ConsumableKind::HealthPotion);
        inv.add_consumable(ConsumableKind::HealthPotion);
        assert!(inv.use_consumable(ConsumableKind::HealthPotion));
        assert_eq!(inv.consumables[0].1, 1);
    }

    #[test]
    fn use_consumable_removes_slot_when_count_zero() {
        let mut inv = PlayerInventory::default();
        inv.add_consumable(ConsumableKind::HealthPotion);
        inv.use_consumable(ConsumableKind::HealthPotion);
        assert!(inv.consumables.is_empty());
    }

    #[test]
    fn use_consumable_returns_false_when_empty() {
        let mut inv = PlayerInventory::default();
        assert!(!inv.use_consumable(ConsumableKind::HealthPotion));
    }

    #[test]
    fn consumable_heal_amount_equals_constant() {
        assert_eq!(ConsumableKind::HealthPotion.heal_amount(), POTION_HEAL);
    }

    #[test]
    fn equipment_panel_open_default_is_false() {
        assert!(!EquipmentPanelOpen::default().0);
    }

    #[test]
    fn glyph_style_cycles_through_all_variants() {
        assert_eq!(GlyphStyle::Ascii.next(),    GlyphStyle::Unicode);
        assert_eq!(GlyphStyle::Unicode.next(),  GlyphStyle::GameIcon);
        assert_eq!(GlyphStyle::GameIcon.next(), GlyphStyle::Ascii);
    }

    #[test]
    fn glyph_style_from_str_valid() {
        assert_eq!(GlyphStyle::from_str("ascii"),   Some(GlyphStyle::Ascii));
        assert_eq!(GlyphStyle::from_str("unicode"), Some(GlyphStyle::Unicode));
        assert_eq!(GlyphStyle::from_str("icon"),    Some(GlyphStyle::GameIcon));
    }

    #[test]
    fn glyph_style_from_str_invalid_returns_none() {
        assert_eq!(GlyphStyle::from_str("unknown"), None);
    }

    use std::sync::OnceLock;
    static TEST_QI: OnceLock<QuestItemRegistry> = OnceLock::new();
    fn qi() -> &'static QuestItemRegistry {
        TEST_QI.get_or_init(|| build_test_registry())
    }

    fn lookup_display_name(qk: QuestItemKind) -> &'static str {
        qi().lookup(qk).map(|m| m.display_name).unwrap_or("???")
    }

    #[test]
    fn glyph_for_style_ascii_returns_ascii_chars() {
        assert_eq!(glyph_for_style(ItemKind::Weapon(WeaponKind::Sword), GlyphStyle::Ascii, qi()), "/");
        assert_eq!(glyph_for_style(ItemKind::Weapon(WeaponKind::Spear), GlyphStyle::Ascii, qi()), "|");
        assert_eq!(glyph_for_style(ItemKind::Weapon(WeaponKind::Bow),   GlyphStyle::Ascii, qi()), ")");
    }

    #[test]
    fn glyph_for_style_unicode_returns_symbols() {
        let s = glyph_for_style(ItemKind::Weapon(WeaponKind::Sword), GlyphStyle::Unicode, qi());
        assert_eq!(s, "\u{1F5E1}");
        let shield = glyph_for_style(ItemKind::Armor(ArmorKind::LeatherArmor), GlyphStyle::Unicode, qi());
        assert_eq!(shield, "\u{1F6E1}");
    }

    #[test]
    fn glyph_for_style_game_icon_returns_pua_codepoints() {
        let s = glyph_for_style(ItemKind::Weapon(WeaponKind::Sword), GlyphStyle::GameIcon, qi());
        assert_eq!(s, "\u{E946}");
        let potion = glyph_for_style(ItemKind::Consumable(ConsumableKind::HealthPotion), GlyphStyle::GameIcon, qi());
        assert_eq!(potion, "\u{EA72}");
    }

    #[test]
    fn glyph_style_default_is_ascii() {
        assert_eq!(GlyphStyle::default(), GlyphStyle::Ascii);
    }

    #[test]
    fn quest_item_display_names() {
        assert_eq!(lookup_display_name(QuestItemKind("eternal_gem")), "영원의 보석");
        assert_eq!(lookup_display_name(QuestItemKind("philosophers_stone")), "현자의 돌");
    }

    #[test]
    fn quest_item_glyph_and_pickup_message() {
        let gem = ItemKind::QuestItem(QuestItemKind("eternal_gem"));
        assert_eq!(gem.glyph(qi()), "*");
        assert_eq!(gem.pickup_message(qi()), "영원의 보석을 획득했다!");
        let stone = ItemKind::QuestItem(QuestItemKind("philosophers_stone"));
        assert_eq!(stone.pickup_message(qi()), "현자의 돌을 획득했다!");
    }

    #[test]
    fn demonsword_items_have_correct_glyphs_and_names() {
        assert_eq!(lookup_display_name(QuestItemKind("demon_sword")), "마검");
        assert_eq!(lookup_display_name(QuestItemKind("elenas_memo")), "엘레나의 메모");
        assert_eq!(lookup_display_name(QuestItemKind("ancient_ritual_book")), "고대 의식서");

        let sword = ItemKind::QuestItem(QuestItemKind("demon_sword"));
        let memo  = ItemKind::QuestItem(QuestItemKind("elenas_memo"));
        let book  = ItemKind::QuestItem(QuestItemKind("ancient_ritual_book"));

        assert_eq!(sword.glyph(qi()), "D");
        assert_eq!(memo.glyph(qi()),  "e");
        assert_eq!(book.glyph(qi()),  "R");

        assert!(sword.pickup_message(qi()).contains("마검"));
        assert!(memo.pickup_message(qi()).contains("폐허 요새"));
        assert!(book.pickup_message(qi()).contains("봉인 의식"));
    }

    #[test]
    fn parry_quest_items_have_correct_glyphs_and_names() {
        assert_eq!(lookup_display_name(QuestItemKind("prototype_hammer")), "시제 6식 파암추");
        assert_eq!(lookup_display_name(QuestItemKind("steel_core")),       "강철 갑주 심장");
        assert_eq!(lookup_display_name(QuestItemKind("pilot_badge")),      "전속 파일럿 인증서");

        let hammer = ItemKind::QuestItem(QuestItemKind("prototype_hammer"));
        let core   = ItemKind::QuestItem(QuestItemKind("steel_core"));
        let badge  = ItemKind::QuestItem(QuestItemKind("pilot_badge"));

        assert_eq!(hammer.glyph(qi()), "H");
        assert_eq!(core.glyph(qi()),   "#");
        assert_eq!(badge.glyph(qi()),  "P");

        assert!(hammer.pickup_message(qi()).contains("파암추"));
        assert!(core.pickup_message(qi()).contains("보스 격파"));
        assert!(badge.pickup_message(qi()).contains("파일럿"));
    }

    #[test]
    fn demonsword_items_unicode_glyphs() {
        let sword = glyph_for_style(ItemKind::QuestItem(QuestItemKind("demon_sword")), GlyphStyle::Unicode, qi());
        assert_eq!(sword, "\u{2694}");
        let memo = glyph_for_style(ItemKind::QuestItem(QuestItemKind("elenas_memo")), GlyphStyle::Unicode, qi());
        assert_eq!(memo, "\u{270E}");
        let book = glyph_for_style(ItemKind::QuestItem(QuestItemKind("ancient_ritual_book")), GlyphStyle::Unicode, qi());
        assert_eq!(book, "\u{2720}");
    }

    #[test]
    fn quest_items_ron_loads_all_29_items() {
        let registry = qi();
        assert_eq!(registry.items.len(), 29, "quest_items.ron 에 29 종이 정의되어야 한다");
    }

    #[test]
    fn quest_item_meta_returns_none_for_unknown_id() {
        let unknown = QuestItemKind("does_not_exist");
        assert!(qi().lookup(unknown).is_none());
    }

    #[test]
    fn intern_quest_id_returns_same_pointer_for_same_id() {
        let a = qi().intern("eternal_gem").expect("등록된 ID 여야 한다");
        let b = qi().intern("eternal_gem").expect("등록된 ID 여야 한다");
        // registry 에 등록된 ID 는 동일 &'static str (포인터 일치)
        assert_eq!(a.as_ptr(), b.as_ptr(), "같은 등록된 ID 는 같은 포인터여야 한다");
    }

    #[test]
    fn quest_item_kind_serde_roundtrip() {
        let qk = QuestItemKind("eternal_gem");
        let s = ron::ser::to_string(&qk).unwrap();
        assert_eq!(s, "\"eternal_gem\"");
        let parsed: QuestItemKind = ron::de::from_str(&s).unwrap();
        assert_eq!(parsed, qk);
    }
}
