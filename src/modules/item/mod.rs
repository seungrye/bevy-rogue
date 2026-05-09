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

fn glyph_unicode(kind: ItemKind, r: &ItemRegistry) -> &'static str {
    match kind {
        ItemKind::Weapon(w)     => r.weapon(w).map(|m| m.glyph_unicode).unwrap_or("?"),
        ItemKind::Armor(a)      => r.armor(a).map(|m| m.glyph_unicode).unwrap_or("?"),
        ItemKind::Consumable(c) => r.consumable(c).map(|m| m.glyph_unicode).unwrap_or("?"),
        ItemKind::QuestItem(qk) => r.quest_item(qk).map(|m| m.glyph_unicode).unwrap_or("?"),
    }
}

fn glyph_game_icon(kind: ItemKind, r: &ItemRegistry) -> &'static str {
    match kind {
        ItemKind::Weapon(w)     => r.weapon(w).map(|m| m.glyph_game_icon).unwrap_or("?"),
        ItemKind::Armor(a)      => r.armor(a).map(|m| m.glyph_game_icon).unwrap_or("?"),
        ItemKind::Consumable(c) => r.consumable(c).map(|m| m.glyph_game_icon).unwrap_or("?"),
        ItemKind::QuestItem(qk) => r.quest_item(qk).map(|m| m.glyph_game_icon).unwrap_or("?"),
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

/// 모든 아이템(weapons / armors / consumables / quest items) 의 메타데이터를 보유하는
/// Bevy Resource. 4 개의 sub-map 으로 카테고리를 구분한다.
/// VillagerRegistry 와 동일한 Resource 패턴으로 일관성 유지.
#[derive(Resource, Default)]
pub struct ItemRegistry {
    pub quest_items: HashMap<&'static str, QuestItemMeta>,
    pub weapons:     HashMap<&'static str, WeaponMeta>,
    pub armors:      HashMap<&'static str, ArmorMeta>,
    pub consumables: HashMap<&'static str, ConsumableMeta>,
}

impl ItemRegistry {
    pub fn quest_item(&self, kind: QuestItemKind) -> Option<&QuestItemMeta> {
        self.quest_items.get(kind.0)
    }
    pub fn weapon(&self, kind: WeaponKind) -> Option<&WeaponMeta> {
        self.weapons.get(kind.0)
    }
    pub fn armor(&self, kind: ArmorKind) -> Option<&ArmorMeta> {
        self.armors.get(kind.0)
    }
    pub fn consumable(&self, kind: ConsumableKind) -> Option<&ConsumableMeta> {
        self.consumables.get(kind.0)
    }

    /// 등록된 quest item ID 의 leak 된 &'static str 반환 (item_id_to_kind 에서 사용)
    pub fn intern_quest_item(&self, id: &str) -> Option<&'static str> {
        self.quest_items.get_key_value(id).map(|(k, _)| *k)
    }
    pub fn intern_weapon(&self, id: &str) -> Option<&'static str> {
        self.weapons.get_key_value(id).map(|(k, _)| *k)
    }
    pub fn intern_armor(&self, id: &str) -> Option<&'static str> {
        self.armors.get_key_value(id).map(|(k, _)| *k)
    }
    pub fn intern_consumable(&self, id: &str) -> Option<&'static str> {
        self.consumables.get_key_value(id).map(|(k, _)| *k)
    }
}

/// 하위 호환 — 기존 QuestItemRegistry 사용처가 점진 이주할 수 있도록 type alias 유지
pub type QuestItemRegistry = ItemRegistry;

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
    registry.quest_items = map;
}

/// 테스트용 — 4 카테고리 모두 inline 으로 적재한 registry 를 구성한다
#[cfg(test)]
pub fn build_test_registry() -> ItemRegistry {
    let mut r = ItemRegistry::default();
    // quest items
    let text = std::fs::read_to_string("assets/items/quest_items.ron").expect("quest_items.ron");
    let defs: Vec<QuestItemDef> = ron::de::from_str(&text).expect("quest_items.ron 파싱");
    for def in defs {
        let id: &'static str = Box::leak(def.id.into_boxed_str());
        r.quest_items.insert(id, QuestItemMeta {
            display_name:    Box::leak(def.display_name.into_boxed_str()),
            glyph_ascii:     Box::leak(def.glyph_ascii.into_boxed_str()),
            glyph_unicode:   Box::leak(def.glyph_unicode.into_boxed_str()),
            glyph_game_icon: Box::leak(def.glyph_game_icon.into_boxed_str()),
            pickup_message:  Box::leak(def.pickup_message.into_boxed_str()),
            image_path:      Box::leak(def.image_path.into_boxed_str()),
        });
    }
    // weapons
    let text = std::fs::read_to_string("assets/items/weapons.ron").expect("weapons.ron");
    let defs: Vec<WeaponDef> = ron::de::from_str(&text).expect("weapons.ron 파싱");
    for def in defs {
        let id: &'static str = Box::leak(def.id.into_boxed_str());
        let element = def.element.map(|e| -> &'static str { Box::leak(e.into_boxed_str()) });
        r.weapons.insert(id, WeaponMeta {
            display_name:    Box::leak(def.display_name.into_boxed_str()),
            glyph_ascii:     Box::leak(def.glyph_ascii.into_boxed_str()),
            glyph_unicode:   Box::leak(def.glyph_unicode.into_boxed_str()),
            glyph_game_icon: Box::leak(def.glyph_game_icon.into_boxed_str()),
            pickup_message:  Box::leak(def.pickup_message.into_boxed_str()),
            attack_power: def.attack_power, element,
        });
    }
    // armors
    let text = std::fs::read_to_string("assets/items/armors.ron").expect("armors.ron");
    let defs: Vec<ArmorDef> = ron::de::from_str(&text).expect("armors.ron 파싱");
    for def in defs {
        let id: &'static str = Box::leak(def.id.into_boxed_str());
        r.armors.insert(id, ArmorMeta {
            display_name:    Box::leak(def.display_name.into_boxed_str()),
            glyph_ascii:     Box::leak(def.glyph_ascii.into_boxed_str()),
            glyph_unicode:   Box::leak(def.glyph_unicode.into_boxed_str()),
            glyph_game_icon: Box::leak(def.glyph_game_icon.into_boxed_str()),
            pickup_message:  Box::leak(def.pickup_message.into_boxed_str()),
            defense_bonus: def.defense_bonus,
        });
    }
    // consumables
    let text = std::fs::read_to_string("assets/items/consumables.ron").expect("consumables.ron");
    let defs: Vec<ConsumableDef> = ron::de::from_str(&text).expect("consumables.ron 파싱");
    for def in defs {
        let id: &'static str = Box::leak(def.id.into_boxed_str());
        r.consumables.insert(id, ConsumableMeta {
            display_name:    Box::leak(def.display_name.into_boxed_str()),
            glyph_ascii:     Box::leak(def.glyph_ascii.into_boxed_str()),
            glyph_unicode:   Box::leak(def.glyph_unicode.into_boxed_str()),
            glyph_game_icon: Box::leak(def.glyph_game_icon.into_boxed_str()),
            pickup_message:  Box::leak(def.pickup_message.into_boxed_str()),
            effect: def.effect,
        });
    }
    r
}

// ── Weapon / Armor / Consumable: ID 기반 newtype + Registry 패턴 ──────────────
// QuestItemKind 와 동일한 Resource 패턴으로 통일.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WeaponKind(pub &'static str);

impl WeaponKind {
    pub fn id(self) -> &'static str { self.0 }

    /// 호환 편의: 자주 쓰이는 검/창/활 상수 (ID 기반)
    pub const SWORD: WeaponKind = WeaponKind("sword");
    pub const SPEAR: WeaponKind = WeaponKind("spear");
    pub const BOW:   WeaponKind = WeaponKind("bow");
}

impl serde::Serialize for WeaponKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { s.serialize_str(self.0) }
}
impl<'de> serde::Deserialize<'de> for WeaponKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(WeaponKind(Box::leak(s.into_boxed_str())))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ArmorKind(pub &'static str);

impl ArmorKind {
    pub fn id(self) -> &'static str { self.0 }
    pub const LEATHER_ARMOR: ArmorKind = ArmorKind("leather_armor");
}

impl serde::Serialize for ArmorKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { s.serialize_str(self.0) }
}
impl<'de> serde::Deserialize<'de> for ArmorKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(ArmorKind(Box::leak(s.into_boxed_str())))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ConsumableKind(pub &'static str);

impl ConsumableKind {
    pub fn id(self) -> &'static str { self.0 }
    pub const HEALTH_POTION: ConsumableKind = ConsumableKind("health_potion");
}

impl serde::Serialize for ConsumableKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { s.serialize_str(self.0) }
}
impl<'de> serde::Deserialize<'de> for ConsumableKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(ConsumableKind(Box::leak(s.into_boxed_str())))
    }
}

// ── 메타데이터 (RON 로드용) ────────────────────────────────────────────────
#[derive(Debug, Deserialize, Clone)]
pub struct WeaponDef {
    pub id: String,
    pub display_name: String,
    pub glyph_ascii: String,
    pub glyph_unicode: String,
    pub glyph_game_icon: String,
    pub pickup_message: String,
    pub attack_power: i32,
    pub element: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ArmorDef {
    pub id: String,
    pub display_name: String,
    pub glyph_ascii: String,
    pub glyph_unicode: String,
    pub glyph_game_icon: String,
    pub pickup_message: String,
    pub defense_bonus: i32,
}

#[derive(Debug, Deserialize, Clone)]
pub enum ConsumableEffect {
    Heal(i32),
}

#[derive(Debug, Deserialize, Clone)]
pub struct ConsumableDef {
    pub id: String,
    pub display_name: String,
    pub glyph_ascii: String,
    pub glyph_unicode: String,
    pub glyph_game_icon: String,
    pub pickup_message: String,
    pub effect: ConsumableEffect,
}

// ── leak 된 메타 (런타임 사용) ─────────────────────────────────────────────
#[derive(Debug, Clone)]
pub struct WeaponMeta {
    pub display_name: &'static str,
    pub glyph_ascii: &'static str,
    pub glyph_unicode: &'static str,
    pub glyph_game_icon: &'static str,
    pub pickup_message: &'static str,
    pub attack_power: i32,
    pub element: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct ArmorMeta {
    pub display_name: &'static str,
    pub glyph_ascii: &'static str,
    pub glyph_unicode: &'static str,
    pub glyph_game_icon: &'static str,
    pub pickup_message: &'static str,
    pub defense_bonus: i32,
}

#[derive(Debug, Clone)]
pub struct ConsumableMeta {
    pub display_name: &'static str,
    pub glyph_ascii: &'static str,
    pub glyph_unicode: &'static str,
    pub glyph_game_icon: &'static str,
    pub pickup_message: &'static str,
    pub effect: ConsumableEffect,
}

// ── 시작 로드아웃 (assets/items/start_loadout.ron) ─────────────────────────

/// 새 게임 시작 시 적용되는 기본 인벤토리·장비·금화.
/// id 는 weapons.ron / armors.ron / consumables.ron 의 식별자.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct StartLoadout {
    pub gold: u32,
    #[serde(default)]
    pub weapon: Option<String>,
    #[serde(default)]
    pub armor: Option<String>,
    #[serde(default)]
    pub items: Vec<String>,
    #[serde(default)]
    pub consumables: Vec<(String, u32)>,
}

#[derive(Resource, Default)]
pub struct StartLoadoutRegistry(pub StartLoadout);

const START_LOADOUT_PATH: &str = "assets/items/start_loadout.ron";

fn load_start_loadout_system(mut registry: ResMut<StartLoadoutRegistry>) {
    match std::fs::read_to_string(START_LOADOUT_PATH) {
        Ok(text) => match ron::de::from_str::<StartLoadout>(&text) {
            Ok(loadout) => {
                info!("start_loadout 로드 완료 (gold: {}, items: {}, consumables: {})",
                    loadout.gold, loadout.items.len(), loadout.consumables.len());
                registry.0 = loadout;
            }
            Err(e) => {
                warn!("{} 파싱 실패, 기본 로드아웃 사용: {}", START_LOADOUT_PATH, e);
                registry.0 = StartLoadout { gold: 50, ..Default::default() };
            }
        },
        Err(_) => {
            registry.0 = StartLoadout { gold: 50, ..Default::default() };
        }
    }
}

/// loadout 을 inventory / equipment 에 적용한다.
/// 호출자가 미리 inventory / equipment 를 default 로 초기화한 뒤 호출.
/// 등록되지 않은 id 는 warn 로그 후 스킵.
pub fn apply_start_loadout(
    inv: &mut PlayerInventory,
    eq: &mut PlayerEquipment,
    loadout: &StartLoadout,
    registry: &ItemRegistry,
) {
    inv.gold = loadout.gold;

    if let Some(id) = &loadout.weapon {
        match registry.intern_weapon(id) {
            Some(intern) => eq.weapon = Some(WeaponKind(intern)),
            None => warn!("start_loadout: 알 수 없는 weapon id '{}'", id),
        }
    }
    if let Some(id) = &loadout.armor {
        match registry.intern_armor(id) {
            Some(intern) => eq.armor = Some(ArmorKind(intern)),
            None => warn!("start_loadout: 알 수 없는 armor id '{}'", id),
        }
    }

    for id in &loadout.items {
        if let Some(intern) = registry.intern_weapon(id) {
            inv.items.push(InventoryItem { kind: ItemKind::Weapon(WeaponKind(intern)) });
        } else if let Some(intern) = registry.intern_armor(id) {
            inv.items.push(InventoryItem { kind: ItemKind::Armor(ArmorKind(intern)) });
        } else {
            warn!("start_loadout: 알 수 없는 item id '{}'", id);
        }
    }

    for (id, count) in &loadout.consumables {
        match registry.intern_consumable(id) {
            Some(intern) => {
                for _ in 0..*count {
                    inv.add_consumable(ConsumableKind(intern));
                }
            }
            None => warn!("start_loadout: 알 수 없는 consumable id '{}'", id),
        }
    }
}

/// 세이브 파일이 없을 때만 시작 로드아웃을 적용한다.
/// 세이브가 있으면 `save::load_if_save_exists` 가 inventory 를 덮어쓴다.
fn apply_start_loadout_if_no_save(
    mut inv: ResMut<PlayerInventory>,
    mut eq: ResMut<PlayerEquipment>,
    loadout: Res<StartLoadoutRegistry>,
    registry: Res<ItemRegistry>,
) {
    if std::path::Path::new(crate::modules::save::SAVE_PATH).exists() {
        return;
    }
    apply_start_loadout(&mut inv, &mut eq, &loadout.0, &registry);
}

// ── 로드 시스템 ────────────────────────────────────────────────────────────
fn load_weapons_system(mut registry: ResMut<ItemRegistry>) {
    let path = "assets/items/weapons.ron";
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("[치명적] {} 읽기 실패: {}", path, e));
    let defs: Vec<WeaponDef> = ron::de::from_str(&text)
        .unwrap_or_else(|e| panic!("[치명적] {} RON 파싱 실패: {}", path, e));
    let mut map = HashMap::new();
    for def in defs {
        let id: &'static str = Box::leak(def.id.into_boxed_str());
        let element = def.element.map(|e| -> &'static str { Box::leak(e.into_boxed_str()) });
        map.insert(id, WeaponMeta {
            display_name:    Box::leak(def.display_name.into_boxed_str()),
            glyph_ascii:     Box::leak(def.glyph_ascii.into_boxed_str()),
            glyph_unicode:   Box::leak(def.glyph_unicode.into_boxed_str()),
            glyph_game_icon: Box::leak(def.glyph_game_icon.into_boxed_str()),
            pickup_message:  Box::leak(def.pickup_message.into_boxed_str()),
            attack_power: def.attack_power,
            element,
        });
    }
    info!("weapon 로드: {} 종", map.len());
    registry.weapons = map;
}

fn load_armors_system(mut registry: ResMut<ItemRegistry>) {
    let path = "assets/items/armors.ron";
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("[치명적] {} 읽기 실패: {}", path, e));
    let defs: Vec<ArmorDef> = ron::de::from_str(&text)
        .unwrap_or_else(|e| panic!("[치명적] {} RON 파싱 실패: {}", path, e));
    let mut map = HashMap::new();
    for def in defs {
        let id: &'static str = Box::leak(def.id.into_boxed_str());
        map.insert(id, ArmorMeta {
            display_name:    Box::leak(def.display_name.into_boxed_str()),
            glyph_ascii:     Box::leak(def.glyph_ascii.into_boxed_str()),
            glyph_unicode:   Box::leak(def.glyph_unicode.into_boxed_str()),
            glyph_game_icon: Box::leak(def.glyph_game_icon.into_boxed_str()),
            pickup_message:  Box::leak(def.pickup_message.into_boxed_str()),
            defense_bonus: def.defense_bonus,
        });
    }
    info!("armor 로드: {} 종", map.len());
    registry.armors = map;
}

fn load_consumables_system(mut registry: ResMut<ItemRegistry>) {
    let path = "assets/items/consumables.ron";
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("[치명적] {} 읽기 실패: {}", path, e));
    let defs: Vec<ConsumableDef> = ron::de::from_str(&text)
        .unwrap_or_else(|e| panic!("[치명적] {} RON 파싱 실패: {}", path, e));
    let mut map = HashMap::new();
    for def in defs {
        let id: &'static str = Box::leak(def.id.into_boxed_str());
        map.insert(id, ConsumableMeta {
            display_name:    Box::leak(def.display_name.into_boxed_str()),
            glyph_ascii:     Box::leak(def.glyph_ascii.into_boxed_str()),
            glyph_unicode:   Box::leak(def.glyph_unicode.into_boxed_str()),
            glyph_game_icon: Box::leak(def.glyph_game_icon.into_boxed_str()),
            pickup_message:  Box::leak(def.pickup_message.into_boxed_str()),
            effect: def.effect,
        });
    }
    info!("consumable 로드: {} 종", map.len());
    registry.consumables = map;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum ItemKind {
    Weapon(WeaponKind),
    Armor(ArmorKind),
    Consumable(ConsumableKind),
    QuestItem(QuestItemKind),
}

impl ItemKind {
    pub fn glyph(self, r: &ItemRegistry) -> &'static str {
        match self {
            ItemKind::Weapon(w)     => r.weapon(w).map(|m| m.glyph_ascii).unwrap_or("?"),
            ItemKind::Armor(a)      => r.armor(a).map(|m| m.glyph_ascii).unwrap_or("?"),
            ItemKind::Consumable(c) => r.consumable(c).map(|m| m.glyph_ascii).unwrap_or("?"),
            ItemKind::QuestItem(qk) => r.quest_item(qk).map(|m| m.glyph_ascii).unwrap_or("?"),
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

    pub fn display_name(self, r: &ItemRegistry) -> &'static str {
        match self {
            ItemKind::Weapon(w)     => r.weapon(w).map(|m| m.display_name).unwrap_or("???"),
            ItemKind::Armor(a)      => r.armor(a).map(|m| m.display_name).unwrap_or("???"),
            ItemKind::Consumable(c) => r.consumable(c).map(|m| m.display_name).unwrap_or("???"),
            ItemKind::QuestItem(qk) => r.quest_item(qk).map(|m| m.display_name).unwrap_or("???"),
        }
    }

    pub fn pickup_message(self, r: &ItemRegistry) -> &'static str {
        match self {
            ItemKind::Weapon(w)     => r.weapon(w).map(|m| m.pickup_message).unwrap_or("무기를 획득했다!"),
            ItemKind::Armor(a)      => r.armor(a).map(|m| m.pickup_message).unwrap_or("방어구를 획득했다!"),
            ItemKind::Consumable(c) => r.consumable(c).map(|m| m.pickup_message).unwrap_or("소모품을 획득했다!"),
            ItemKind::QuestItem(qk) => r.quest_item(qk).map(|m| m.pickup_message).unwrap_or("아이템을 획득했다!"),
        }
    }
}

impl WeaponKind {
    pub fn display_name(self, r: &ItemRegistry) -> &'static str {
        r.weapon(self).map(|m| m.display_name).unwrap_or("???")
    }
}
impl ArmorKind {
    pub fn display_name(self, r: &ItemRegistry) -> &'static str {
        r.armor(self).map(|m| m.display_name).unwrap_or("???")
    }
}
impl ConsumableKind {
    pub fn display_name(self, r: &ItemRegistry) -> &'static str {
        r.consumable(self).map(|m| m.display_name).unwrap_or("???")
    }
    pub fn heal_amount(self, r: &ItemRegistry) -> i32 {
        match r.consumable(self).map(|m| &m.effect) {
            Some(ConsumableEffect::Heal(n)) => *n,
            None => 0,
        }
    }
}

pub fn weapon_attack(kind: WeaponKind, r: &ItemRegistry) -> i32 {
    r.weapon(kind).map(|m| m.attack_power).unwrap_or(0)
}

pub fn armor_defense_bonus(kind: ArmorKind, r: &ItemRegistry) -> i32 {
    r.armor(kind).map(|m| m.defense_bonus).unwrap_or(0)
}

pub fn effective_attack(equipment: &PlayerEquipment, r: &ItemRegistry) -> i32 {
    equipment.weapon.map(|w| weapon_attack(w, r)).unwrap_or(PLAYER_ATK)
}

pub fn effective_defense(equipment: &PlayerEquipment, r: &ItemRegistry) -> i32 {
    let bonus = equipment.armor.map(|a| armor_defense_bonus(a, r)).unwrap_or(0);
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
            (ItemKind::Consumable(ConsumableKind::HEALTH_POTION), 0.30),
            (ItemKind::Weapon(WeaponKind::SWORD), 0.15),
        ],
        "오크" => &[
            (ItemKind::Consumable(ConsumableKind::HEALTH_POTION), 0.40),
            (ItemKind::Weapon(WeaponKind::SPEAR), 0.20),
            (ItemKind::Armor(ArmorKind::LEATHER_ARMOR), 0.10),
        ],
        "트롤" => &[
            (ItemKind::Consumable(ConsumableKind::HEALTH_POTION), 0.50),
            (ItemKind::Weapon(WeaponKind::BOW), 0.25),
            (ItemKind::Armor(ArmorKind::LEATHER_ARMOR), 0.20),
        ],
        _ => &[
            (ItemKind::Consumable(ConsumableKind::HEALTH_POTION), 0.25),
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
            .init_resource::<StartLoadoutRegistry>()
            .add_systems(Startup, (
                load_quest_items_system.in_set(ItemSystemSet::Load),
                load_weapons_system.in_set(ItemSystemSet::Load),
                load_armors_system.in_set(ItemSystemSet::Load),
                load_consumables_system.in_set(ItemSystemSet::Load),
                load_start_loadout_system.in_set(ItemSystemSet::Load),
                setup_glyph_fonts,
            ))
            .add_systems(PostStartup, apply_start_loadout_if_no_save)
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
    mut markers: ResMut<crate::modules::ui::minimap::DiscoveredMarkers>,
    world: Res<crate::modules::zone::WorldState>,
) {
    if turn_events.read().next().is_none() { return; }
    let Ok((moving_to, transform)) = player_query.get_single() else { return };
    let (px, py) = moving_to
        .map(|m| world_to_tile_coords(m.target))
        .unwrap_or_else(|| world_to_tile_coords(transform.translation));

    let at_tile: Vec<(Entity, ItemKind, usize, usize)> = item_query.iter()
        .filter(|(_, item)| item.tile_x == px && item.tile_y == py)
        .map(|(e, item)| (e, item.kind, item.tile_x, item.tile_y))
        .collect();

    for (entity, kind, tx, ty) in at_tile {
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
            // 미니맵의 QuestTarget 마커 제거 — 획득한 아이템 위치는 더 이상 표시할 필요 없음
            markers.remove_at(tx, ty, crate::modules::ui::minimap::MarkerKind::QuestTarget, &world.current);
        }
        log.send(LogMessage(kind.pickup_message(&quest_items).to_string()));
        commands.entity(entity).despawn();
    }
}

fn apply_equipment_stats(
    equipment: Res<PlayerEquipment>,
    items: Res<ItemRegistry>,
    mut player_query: Query<&mut CombatStats, With<Player>>,
) {
    if !equipment.is_changed() { return; }
    let Ok(mut stats) = player_query.get_single_mut() else { return };
    stats.attack  = effective_attack(&equipment, &items);
    stats.defense = effective_defense(&equipment, &items);
}

fn quest_item_image_path(kind: QuestItemKind, registry: &ItemRegistry) -> &'static str {
    registry.quest_item(kind).map(|m| m.image_path).unwrap_or("scene/open-chest.png")
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
    use std::sync::OnceLock;
    static TEST_QI: OnceLock<ItemRegistry> = OnceLock::new();
    fn qi() -> &'static ItemRegistry {
        TEST_QI.get_or_init(|| build_test_registry())
    }

    #[test]
    fn weapon_attack_sword_is_7() {
        assert_eq!(weapon_attack(WeaponKind::SWORD, qi()), 7);
    }

    #[test]
    fn weapon_attack_spear_is_9() {
        assert_eq!(weapon_attack(WeaponKind::SPEAR, qi()), 9);
    }

    #[test]
    fn weapon_attack_bow_is_5() {
        assert_eq!(weapon_attack(WeaponKind::BOW, qi()), 5);
    }

    #[test]
    fn armor_defense_bonus_leather_is_2() {
        assert_eq!(armor_defense_bonus(ArmorKind::LEATHER_ARMOR, qi()), 2);
    }

    #[test]
    fn effective_attack_no_weapon_equals_player_default() {
        let eq = PlayerEquipment { weapon: None, armor: None };
        assert_eq!(effective_attack(&eq, qi()), PLAYER_ATK);
    }

    #[test]
    fn effective_attack_with_sword_is_7() {
        let eq = PlayerEquipment { weapon: Some(WeaponKind::SWORD), armor: None };
        assert_eq!(effective_attack(&eq, qi()), 7);
    }

    #[test]
    fn effective_defense_no_armor_equals_player_default() {
        let eq = PlayerEquipment { weapon: None, armor: None };
        assert_eq!(effective_defense(&eq, qi()), PLAYER_DEF);
    }

    #[test]
    fn effective_defense_with_leather_adds_bonus() {
        let eq = PlayerEquipment { weapon: None, armor: Some(ArmorKind::LEATHER_ARMOR) };
        assert_eq!(effective_defense(&eq, qi()), PLAYER_DEF + 2);
    }

    #[test]
    fn goblin_drop_table_has_potion_and_sword() {
        let t = monster_drop_table("고블린");
        assert!(t.iter().any(|(k, _)| matches!(k, ItemKind::Consumable(ConsumableKind::HEALTH_POTION))));
        assert!(t.iter().any(|(k, _)| matches!(k, ItemKind::Weapon(WeaponKind::SWORD))));
    }

    #[test]
    fn orc_drop_table_has_spear_and_armor() {
        let t = monster_drop_table("오크");
        assert!(t.iter().any(|(k, _)| matches!(k, ItemKind::Weapon(WeaponKind::SPEAR))));
        assert!(t.iter().any(|(k, _)| matches!(k, ItemKind::Armor(ArmorKind::LEATHER_ARMOR))));
    }

    #[test]
    fn troll_drop_table_has_bow_and_armor() {
        let t = monster_drop_table("트롤");
        assert!(t.iter().any(|(k, _)| matches!(k, ItemKind::Weapon(WeaponKind::BOW))));
        assert!(t.iter().any(|(k, _)| matches!(k, ItemKind::Armor(ArmorKind::LEATHER_ARMOR))));
    }

    #[test]
    fn add_consumable_stacks_same_kind() {
        let mut inv = PlayerInventory::default();
        inv.add_consumable(ConsumableKind::HEALTH_POTION);
        inv.add_consumable(ConsumableKind::HEALTH_POTION);
        assert_eq!(inv.consumables.len(), 1);
        assert_eq!(inv.consumables[0].1, 2);
    }

    #[test]
    fn use_consumable_decrements_count() {
        let mut inv = PlayerInventory::default();
        inv.add_consumable(ConsumableKind::HEALTH_POTION);
        inv.add_consumable(ConsumableKind::HEALTH_POTION);
        assert!(inv.use_consumable(ConsumableKind::HEALTH_POTION));
        assert_eq!(inv.consumables[0].1, 1);
    }

    #[test]
    fn use_consumable_removes_slot_when_count_zero() {
        let mut inv = PlayerInventory::default();
        inv.add_consumable(ConsumableKind::HEALTH_POTION);
        inv.use_consumable(ConsumableKind::HEALTH_POTION);
        assert!(inv.consumables.is_empty());
    }

    #[test]
    fn use_consumable_returns_false_when_empty() {
        let mut inv = PlayerInventory::default();
        assert!(!inv.use_consumable(ConsumableKind::HEALTH_POTION));
    }

    #[test]
    fn consumable_heal_amount_equals_constant() {
        assert_eq!(ConsumableKind::HEALTH_POTION.heal_amount(qi()), POTION_HEAL);
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

    #[test]
    fn start_loadout_ron_parses() {
        let text = std::fs::read_to_string("assets/items/start_loadout.ron")
            .expect("start_loadout.ron 읽기 실패");
        let loadout: StartLoadout = ron::de::from_str(&text).expect("start_loadout.ron 파싱 실패");
        assert_eq!(loadout.gold, 50);
        assert_eq!(loadout.items, vec!["sword", "spear", "bow"]);
        assert_eq!(loadout.consumables, vec![("health_potion".to_string(), 10)]);
    }

    #[test]
    fn apply_start_loadout_pushes_inventory_items() {
        let mut inv = PlayerInventory::default();
        let mut eq = PlayerEquipment::default();
        let loadout = StartLoadout {
            gold: 100,
            weapon: None,
            armor: None,
            items: vec!["sword".into(), "spear".into(), "bow".into()],
            consumables: vec![("health_potion".into(), 10)],
        };
        apply_start_loadout(&mut inv, &mut eq, &loadout, qi());
        assert_eq!(inv.gold, 100);
        assert_eq!(inv.items.len(), 3);
        let total_potions: u32 = inv.consumables.iter()
            .filter(|(k, _)| *k == ConsumableKind::HEALTH_POTION)
            .map(|(_, n)| *n).sum();
        assert_eq!(total_potions, 10);
    }

    #[test]
    fn apply_start_loadout_skips_unknown_id() {
        let mut inv = PlayerInventory::default();
        let mut eq = PlayerEquipment::default();
        let loadout = StartLoadout {
            gold: 0,
            weapon: Some("nonexistent".into()),
            armor: None,
            items: vec!["bogus".into(), "sword".into()],
            consumables: vec![("not_a_thing".into(), 5)],
        };
        apply_start_loadout(&mut inv, &mut eq, &loadout, qi());
        assert!(eq.weapon.is_none(), "알 수 없는 weapon id 는 스킵");
        assert_eq!(inv.items.len(), 1, "알 수 없는 item id 는 스킵");
        assert!(inv.consumables.is_empty(), "알 수 없는 consumable id 는 스킵");
    }

    fn lookup_display_name(qk: QuestItemKind) -> &'static str {
        qi().quest_item(qk).map(|m| m.display_name).unwrap_or("???")
    }

    #[test]
    fn glyph_for_style_ascii_returns_ascii_chars() {
        assert_eq!(glyph_for_style(ItemKind::Weapon(WeaponKind::SWORD), GlyphStyle::Ascii, qi()), "/");
        assert_eq!(glyph_for_style(ItemKind::Weapon(WeaponKind::SPEAR), GlyphStyle::Ascii, qi()), "|");
        assert_eq!(glyph_for_style(ItemKind::Weapon(WeaponKind::BOW),   GlyphStyle::Ascii, qi()), ")");
    }

    #[test]
    fn glyph_for_style_unicode_returns_symbols() {
        let s = glyph_for_style(ItemKind::Weapon(WeaponKind::SWORD), GlyphStyle::Unicode, qi());
        assert_eq!(s, "\u{1F5E1}");
        let shield = glyph_for_style(ItemKind::Armor(ArmorKind::LEATHER_ARMOR), GlyphStyle::Unicode, qi());
        assert_eq!(shield, "\u{1F6E1}");
    }

    #[test]
    fn glyph_for_style_game_icon_returns_pua_codepoints() {
        let s = glyph_for_style(ItemKind::Weapon(WeaponKind::SWORD), GlyphStyle::GameIcon, qi());
        assert_eq!(s, "\u{E946}");
        let potion = glyph_for_style(ItemKind::Consumable(ConsumableKind::HEALTH_POTION), GlyphStyle::GameIcon, qi());
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
        assert_eq!(registry.quest_items.len(), 29, "quest_items.ron 에 29 종이 정의되어야 한다");
    }

    #[test]
    fn quest_item_meta_returns_none_for_unknown_id() {
        let unknown = QuestItemKind("does_not_exist");
        assert!(qi().quest_item(unknown).is_none());
    }

    #[test]
    fn intern_quest_id_returns_same_pointer_for_same_id() {
        let a = qi().intern_quest_item("eternal_gem").expect("등록된 ID 여야 한다");
        let b = qi().intern_quest_item("eternal_gem").expect("등록된 ID 여야 한다");
        // registry 에 등록된 ID 는 동일 &'static str (포인터 일치)
        assert_eq!(a.as_ptr(), b.as_ptr(), "같은 등록된 ID 는 같은 포인터여야 한다");
    }

    #[test]
    fn weapons_ron_loads_three_weapons() {
        assert_eq!(qi().weapons.len(), 3);
        assert!(qi().weapon(WeaponKind::SWORD).is_some());
        assert!(qi().weapon(WeaponKind::SPEAR).is_some());
        assert!(qi().weapon(WeaponKind::BOW).is_some());
    }

    #[test]
    fn weapons_have_correct_attack_power() {
        assert_eq!(qi().weapon(WeaponKind::SWORD).unwrap().attack_power, 7);
        assert_eq!(qi().weapon(WeaponKind::SPEAR).unwrap().attack_power, 9);
        assert_eq!(qi().weapon(WeaponKind::BOW).unwrap().attack_power, 5);
    }

    #[test]
    fn weapons_have_element_strings() {
        assert_eq!(qi().weapon(WeaponKind::SWORD).unwrap().element, Some("fire"));
        assert_eq!(qi().weapon(WeaponKind::SPEAR).unwrap().element, Some("ice"));
        assert_eq!(qi().weapon(WeaponKind::BOW).unwrap().element, Some("lightning"));
    }

    #[test]
    fn armors_ron_loads_leather() {
        assert_eq!(qi().armors.len(), 1);
        let leather = qi().armor(ArmorKind::LEATHER_ARMOR).unwrap();
        assert_eq!(leather.display_name, "가죽 갑옷");
        assert_eq!(leather.defense_bonus, 2);
    }

    #[test]
    fn consumables_ron_loads_health_potion_with_heal_effect() {
        assert_eq!(qi().consumables.len(), 1);
        let potion = qi().consumable(ConsumableKind::HEALTH_POTION).unwrap();
        assert!(matches!(potion.effect, ConsumableEffect::Heal(8)));
    }

    #[test]
    fn weapon_kind_serde_roundtrip() {
        let wk = WeaponKind::SWORD;
        let s = ron::ser::to_string(&wk).unwrap();
        assert_eq!(s, "\"sword\"");
        let parsed: WeaponKind = ron::de::from_str(&s).unwrap();
        assert_eq!(parsed, wk);
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
