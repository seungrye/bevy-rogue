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

#[allow(dead_code)] // 테스트에서만 참조되는 공개 상수 (프로덕션 미사용)
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
    #[allow(dead_code)] // 테스트에서만 참조되는 공개 접근자 (프로덕션 미사용)
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
    // wasm32: 브라우저 런타임은 std::fs 가 없으므로 컴파일 시 RON 을 임베드한다.
    // 네이티브는 기존 동작(파일 시스템 읽기) 그대로.
    #[cfg(target_arch = "wasm32")]
    let text: String = include_str!("../../../assets/items/quest_items.ron").to_string();
    #[cfg(not(target_arch = "wasm32"))]
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
            attack_power_min: def.attack_power_min,
            attack_power_max: def.attack_power_max,
            tier: def.tier,
            element,
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
            defense_bonus_min: def.defense_bonus_min,
            defense_bonus_max: def.defense_bonus_max,
            tier: def.tier,
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
    #[allow(dead_code)] // 테스트에서만 참조되는 공개 접근자 (프로덕션 미사용)
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
    #[allow(dead_code)] // 테스트에서만 참조되는 공개 접근자 (프로덕션 미사용)
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
    #[allow(dead_code)] // 테스트에서만 참조되는 공개 접근자 (프로덕션 미사용)
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
    pub attack_power_min: i32,
    pub attack_power_max: i32,
    pub tier: u8,
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
    pub defense_bonus_min: i32,
    pub defense_bonus_max: i32,
    pub tier: u8,
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
    pub attack_power_min: i32,
    pub attack_power_max: i32,
    /// 레벨 스케일 드롭 시 티어 그룹화·가중치에 사용 (§7-B).
    pub tier: u8,
    pub element: Option<&'static str>,
}

impl WeaponMeta {
    /// 롤 없이 표시·기본값으로 쓰는 범위 중앙값 (min+max)/2 (정수 내림).
    pub fn attack_mid(&self) -> i32 {
        (self.attack_power_min + self.attack_power_max) / 2
    }
}

#[derive(Debug, Clone)]
pub struct ArmorMeta {
    pub display_name: &'static str,
    pub glyph_ascii: &'static str,
    pub glyph_unicode: &'static str,
    pub glyph_game_icon: &'static str,
    pub pickup_message: &'static str,
    pub defense_bonus_min: i32,
    pub defense_bonus_max: i32,
    /// 레벨 스케일 드롭 시 티어 그룹화·가중치에 사용 (§7-B).
    pub tier: u8,
}

impl ArmorMeta {
    /// 롤 없이 표시·기본값으로 쓰는 범위 중앙값.
    pub fn defense_mid(&self) -> i32 {
        (self.defense_bonus_min + self.defense_bonus_max) / 2
    }
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

/// 지정 경로에서 시작 로드아웃을 읽는다. 읽기/파싱 실패 시 기본 로드아웃(gold 50)으로 폴백.
/// 경로를 인자로 받아 테스트에서 임의 파일(정상/깨진/없는)을 주입할 수 있게 한다.
fn read_start_loadout(path: &str) -> StartLoadout {
    match std::fs::read_to_string(path) {
        Ok(text) => match ron::de::from_str::<StartLoadout>(&text) {
            Ok(loadout) => {
                info!("start_loadout 로드 완료 (gold: {}, items: {}, consumables: {})", loadout.gold, loadout.items.len(), loadout.consumables.len());
                loadout
            }
            Err(e) => {
                warn!("{} 파싱 실패, 기본 로드아웃 사용: {}", path, e);
                StartLoadout { gold: 50, ..Default::default() }
            }
        },
        Err(_) => StartLoadout { gold: 50, ..Default::default() },
    }
}

fn load_start_loadout_system(mut registry: ResMut<StartLoadoutRegistry>) {
    registry.0 = read_start_loadout(START_LOADOUT_PATH);
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
            inv.items.push(InventoryItem::new(ItemKind::Weapon(WeaponKind(intern))));
        } else if let Some(intern) = registry.intern_armor(id) {
            inv.items.push(InventoryItem::new(ItemKind::Armor(ArmorKind(intern))));
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
    apply_loadout_unless_save(&mut inv, &mut eq, &loadout.0, &registry, crate::modules::save::SAVE_PATH);
}

/// 세이브 경로에 파일이 있으면 아무것도 안 하고, 없으면 로드아웃을 적용한다.
/// save_path 를 인자로 받아 테스트에서 임의 경로로 양쪽 분기를 검증할 수 있게 한다.
fn apply_loadout_unless_save(
    inv: &mut PlayerInventory,
    eq: &mut PlayerEquipment,
    loadout: &StartLoadout,
    registry: &ItemRegistry,
    save_path: &str,
) {
    if std::path::Path::new(save_path).exists() {
        return;
    }
    apply_start_loadout(inv, eq, loadout, registry);
}

// ── 로드 시스템 ────────────────────────────────────────────────────────────
fn load_weapons_system(mut registry: ResMut<ItemRegistry>) {
    let path = "assets/items/weapons.ron";
    #[cfg(target_arch = "wasm32")]
    let text: String = include_str!("../../../assets/items/weapons.ron").to_string();
    #[cfg(not(target_arch = "wasm32"))]
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
            attack_power_min: def.attack_power_min,
            attack_power_max: def.attack_power_max,
            tier: def.tier,
            element,
        });
    }
    info!("weapon 로드: {} 종", map.len());
    registry.weapons = map;
}

fn load_armors_system(mut registry: ResMut<ItemRegistry>) {
    let path = "assets/items/armors.ron";
    #[cfg(target_arch = "wasm32")]
    let text: String = include_str!("../../../assets/items/armors.ron").to_string();
    #[cfg(not(target_arch = "wasm32"))]
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
            defense_bonus_min: def.defense_bonus_min,
            defense_bonus_max: def.defense_bonus_max,
            tier: def.tier,
        });
    }
    info!("armor 로드: {} 종", map.len());
    registry.armors = map;
}

fn load_consumables_system(mut registry: ResMut<ItemRegistry>) {
    let path = "assets/items/consumables.ron";
    #[cfg(target_arch = "wasm32")]
    let text: String = include_str!("../../../assets/items/consumables.ron").to_string();
    #[cfg(not(target_arch = "wasm32"))]
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

// ── 레어도 등급 (롤 백분위 파생) — specs/item-random-stats.md §2 ────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Epic,
    Legendary,
}

impl Rarity {
    /// 롤된 값이 [min, max] 안에서 차지하는 백분위로 등급을 매긴다.
    /// min == max 면 분모가 0 이므로 백분위 0(Common) 으로 처리한다.
    pub fn from_roll(rolled: i32, min: i32, max: i32) -> Rarity {
        let p = if max <= min {
            0.0
        } else {
            (rolled - min) as f32 / (max - min) as f32
        };
        if p < 0.40 {
            Rarity::Common
        } else if p < 0.70 {
            Rarity::Uncommon
        } else if p < 0.90 {
            Rarity::Rare
        } else if p < 0.98 {
            Rarity::Epic
        } else {
            Rarity::Legendary
        }
    }

    pub fn name_ko(self) -> &'static str {
        match self {
            Rarity::Common    => "일반",
            Rarity::Uncommon  => "고급",
            Rarity::Rare      => "희귀",
            Rarity::Epic      => "영웅",
            Rarity::Legendary => "전설",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Rarity::Common    => Color::rgb(0.7, 0.7, 0.7),
            Rarity::Uncommon  => Color::rgb(0.3, 0.9, 0.3),
            Rarity::Rare      => Color::rgb(0.3, 0.5, 1.0),
            Rarity::Epic      => Color::rgb(0.7, 0.3, 1.0),
            Rarity::Legendary => Color::rgb(1.0, 0.8, 0.2),
        }
    }
}

/// 무기 단일 공격력 — 범위의 중앙값 (registry 단일값이 필요한 호출처용).
pub fn weapon_attack(kind: WeaponKind, r: &ItemRegistry) -> i32 {
    r.weapon(kind).map(|m| m.attack_mid()).unwrap_or(0)
}

/// 방어구 단일 방어보너스 — 범위의 중앙값.
pub fn armor_defense_bonus(kind: ArmorKind, r: &ItemRegistry) -> i32 {
    r.armor(kind).map(|m| m.defense_mid()).unwrap_or(0)
}

/// 무기 종류의 롤된 공격력으로 레어도를 계산한다. (장비창 표시용)
pub fn weapon_rarity(kind: WeaponKind, rolled: i32, r: &ItemRegistry) -> Option<Rarity> {
    r.weapon(kind).map(|m| Rarity::from_roll(rolled, m.attack_power_min, m.attack_power_max))
}

/// 방어구 종류의 롤된 방어보너스로 레어도를 계산한다.
pub fn armor_rarity(kind: ArmorKind, rolled: i32, r: &ItemRegistry) -> Option<Rarity> {
    r.armor(kind).map(|m| Rarity::from_roll(rolled, m.defense_bonus_min, m.defense_bonus_max))
}

/// 드롭/인벤토리 아이템의 글리프·표시 색. 무기/방어구는 롤값+range 로 레어도 색을
/// 계산하고, 롤값이 없거나(소비/퀘스트) range 조회 실패면 기존 카테고리 색으로 폴백한다.
pub fn item_display_color(
    kind: ItemKind,
    rolled_attack: Option<i32>,
    rolled_defense: Option<i32>,
    r: &ItemRegistry,
) -> Color {
    match kind {
        ItemKind::Weapon(w) => match rolled_attack.and_then(|v| weapon_rarity(w, v, r)) {
            Some(rarity) => rarity.color(),
            None => kind.color(),
        },
        ItemKind::Armor(a) => match rolled_defense.and_then(|v| armor_rarity(a, v, r)) {
            Some(rarity) => rarity.color(),
            None => kind.color(),
        },
        ItemKind::Consumable(_) | ItemKind::QuestItem(_) => kind.color(),
    }
}

/// 무기/방어구 드롭 시 tier 범위 안에서 스탯을 롤한다.
/// Consumable/QuestItem 은 (None, None). registry 조회 실패도 (None, None).
pub fn roll_item_stats<R: Rng + ?Sized>(
    kind: ItemKind,
    rng: &mut R,
    r: &ItemRegistry,
) -> (Option<i32>, Option<i32>) {
    match kind {
        ItemKind::Weapon(w) => match r.weapon(w) {
            Some(m) => (Some(rng.gen_range(m.attack_power_min..=m.attack_power_max)), None),
            None => (None, None),
        },
        ItemKind::Armor(a) => match r.armor(a) {
            Some(m) => (None, Some(rng.gen_range(m.defense_bonus_min..=m.defense_bonus_max))),
            None => (None, None),
        },
        ItemKind::Consumable(_) | ItemKind::QuestItem(_) => (None, None),
    }
}

// ── 레벨 스케일 드롭 티어 선택 — specs/item-random-stats.md §7-B ────────────
// 플레이어 레벨에 따라 드롭되는 장비의 티어 분포를 조정한다.
// 결정 로직은 순수 함수로 분리해 경계를 단위 테스트로 전부 커버한다.

/// 최소/최대 티어 (RON 데이터의 tier 범위).
const MIN_TIER: u8 = 1;
const MAX_TIER: u8 = 5;
/// 티어 밴드 중심이 한 단계 오르는 데 필요한 레벨 간격 (≈ 3레벨마다 +1티어).
const LEVELS_PER_TIER: u32 = 3;

/// 플레이어 레벨 L 에 대응하는 "티어 밴드 중심".
/// `center = (1 + (L-1)/LEVELS_PER_TIER).clamp(MIN_TIER, MAX_TIER)`.
/// 레벨 1~3 → 1, 4~6 → 2, ... 13 이상 → 5 로 상한 클램프.
pub fn tier_band_center(level: u32) -> u8 {
    let raw = 1 + level.saturating_sub(1) / LEVELS_PER_TIER;
    (raw as u8).clamp(MIN_TIER, MAX_TIER)
}

/// 아이템 티어가 레벨 대비 받는 드롭 가중치.
/// `d = item_tier - center` 에 따라 스펙 §7-B 표 그대로:
/// d≥+2 → 0.0(게이트), +1 → 0.5, 0 → 1.0, -1 → 0.6, -2 → 0.3, ≤-3 → 0.1.
pub fn tier_weight(item_tier: u8, level: u32) -> f32 {
    let center = tier_band_center(level);
    let d = item_tier as i32 - center as i32;
    if d >= 2 {
        0.0
    } else if d == 1 {
        0.5
    } else if d == 0 {
        1.0
    } else if d == -1 {
        0.6
    } else if d == -2 {
        0.3
    } else {
        // d <= -3
        0.1
    }
}

/// 사용 가능한 티어 목록에서 레벨 가중치로 하나를 추첨한다.
/// 모든 후보의 가중치 합이 0(전부 게이트)이면 None.
pub fn weighted_tier_pick<R: Rng + ?Sized>(
    level: u32,
    available_tiers: &[u8],
    rng: &mut R,
) -> Option<u8> {
    // 양수 가중치 티어만 후보로 모은다 (가중치 0=게이트 제외).
    let candidates: Vec<(u8, f32)> = available_tiers.iter()
        .map(|&t| (t, tier_weight(t, level)))
        .filter(|&(_, w)| w > 0.0)
        .collect();
    let total: f32 = candidates.iter().map(|&(_, w)| w).sum();
    if total <= 0.0 {
        return None;
    }
    // 마지막 후보를 catch-all 로 두어 부동소수 누적 오차와 무관하게 항상 선택이 된다.
    let mut roll = rng.gen::<f32>() * total;
    let last = candidates.len() - 1;
    for &(t, w) in &candidates[..last] {
        if roll < w {
            return Some(t);
        }
        roll -= w;
    }
    Some(candidates[last].0)
}

/// ID→tier 메타 접근을 추상화해 무기/방어구 양쪽에 동일 그룹화·선택 로직을 재사용한다.
/// (tier 로 그룹화 → 레벨 가중 추첨 → 그 티어 중 랜덤 ID 선택).
/// 후보가 없거나 전부 게이트되면 None.
fn pick_leveled_id<'a, R: Rng + ?Sized, I>(level: u32, entries: I, rng: &mut R) -> Option<&'a str>
where
    I: Iterator<Item = (&'a str, u8)>,
{
    let mut by_tier: HashMap<u8, Vec<&'a str>> = HashMap::new();
    for (id, tier) in entries {
        by_tier.entry(tier).or_default().push(id);
    }
    let mut tiers: Vec<u8> = by_tier.keys().copied().collect();
    tiers.sort_unstable();
    let tier = weighted_tier_pick(level, &tiers, rng)?;
    // tier 는 by_tier 의 키에서 왔으므로 ids 는 항상 존재하고 비어 있지 않다.
    let ids = &by_tier[&tier];
    Some(ids[rng.gen_range(0..ids.len())])
}

/// 레벨 가중으로 무기 한 종류를 고른다.
/// 티어로 그룹화 → 레벨 가중 추첨 → 그 티어 무기 중 랜덤 선택.
/// 후보 티어가 없거나 전부 게이트되면 None.
pub fn pick_leveled_weapon<R: Rng + ?Sized>(
    level: u32,
    r: &ItemRegistry,
    rng: &mut R,
) -> Option<WeaponKind> {
    let id = pick_leveled_id(level, r.weapons.iter().map(|(id, m)| (*id, m.tier)), rng)?;
    Some(WeaponKind(id))
}

/// 레벨 가중으로 방어구 한 종류를 고른다 (무기와 동일 흐름).
pub fn pick_leveled_armor<R: Rng + ?Sized>(
    level: u32,
    r: &ItemRegistry,
    rng: &mut R,
) -> Option<ArmorKind> {
    let id = pick_leveled_id(level, r.armors.iter().map(|(id, m)| (*id, m.tier)), rng)?;
    Some(ArmorKind(id))
}

/// 유효 공격력: 무기 장착 시 롤값(있으면) 또는 범위 중앙값, 무기 없으면 기본 ATK.
pub fn effective_attack(equipment: &PlayerEquipment, r: &ItemRegistry) -> i32 {
    match equipment.weapon {
        Some(w) => equipment.weapon_rolled_attack.unwrap_or_else(|| weapon_attack(w, r)),
        None => PLAYER_ATK,
    }
}

/// 유효 방어력: 방어구 장착 시 롤값(있으면) 또는 범위 중앙값 보너스 + 기본 DEF.
pub fn effective_defense(equipment: &PlayerEquipment, r: &ItemRegistry) -> i32 {
    let bonus = match equipment.armor {
        Some(a) => equipment.armor_rolled_defense.unwrap_or_else(|| armor_defense_bonus(a, r)),
        None => 0,
    };
    PLAYER_DEF + bonus
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct InventoryItem {
    pub kind: ItemKind,
    /// 무기일 때 드롭 시 롤된 공격력. serde default → 구 세이브는 None.
    #[serde(default)]
    pub rolled_attack: Option<i32>,
    /// 방어구일 때 드롭 시 롤된 방어보너스. serde default → 구 세이브는 None.
    #[serde(default)]
    pub rolled_defense: Option<i32>,
}

impl InventoryItem {
    /// 롤값 없이 종류만으로 인벤토리 아이템을 만든다 (시작 로드아웃·테스트용).
    pub fn new(kind: ItemKind) -> Self {
        Self { kind, rolled_attack: None, rolled_defense: None }
    }
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
    /// 장착한 무기의 롤된 공격력. serde default → 구 세이브는 None(중앙값 사용).
    #[serde(default)]
    pub weapon_rolled_attack: Option<i32>,
    /// 장착한 방어구의 롤된 방어보너스. serde default → 구 세이브는 None.
    #[serde(default)]
    pub armor_rolled_defense: Option<i32>,
}

#[derive(Resource, Default)]
pub struct EquipmentPanelOpen(pub bool);

#[derive(Component)]
pub struct Item {
    pub kind:   ItemKind,
    pub tile_x: usize,
    pub tile_y: usize,
    /// 드롭 시 롤된 무기 공격력 (무기일 때만 Some).
    pub rolled_attack: Option<i32>,
    /// 드롭 시 롤된 방어구 방어보너스 (방어구일 때만 Some).
    pub rolled_defense: Option<i32>,
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

/// 몬스터 드롭 카테고리. 구체적 아이템 대신 "무엇을" 떨구나만 표현한다.
/// 실제 장비 아이템은 플레이어 레벨 가중(`weighted_tier_pick`)으로 선택되어
/// 신규 아이템도 tier 만 맞으면 자동 편입되는 데이터 주도 방식이 된다.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropCategory {
    /// 특정 소비아이템 드롭.
    Consumable(ConsumableKind),
    /// 무기 드롭 — 종류는 레벨 가중으로 선택.
    Weapon,
    /// 방어구 드롭 — 종류는 레벨 가중으로 선택.
    Armor,
}

/// 몬스터별 드롭 테이블 — 각 항목은 독립 확률로 롤된다.
/// 몬스터는 드롭 빈도/카테고리(포션·장비드롭 여부)에만 영향을 주고,
/// 구체적 장비 아이템·티어는 플레이어 레벨 가중으로 결정된다(역할 분리).
/// specs/item-random-stats.md §7 / §7-B 참고
pub fn monster_drop_table(monster_name: &str) -> &'static [(DropCategory, f32)] {
    match monster_name {
        // 약한 몬스터 — 포션 위주, 장비 드롭 확률 낮음
        "고블린" => &[
            (DropCategory::Consumable(ConsumableKind::HEALTH_POTION), 0.30),
            (DropCategory::Weapon, 0.20),
            (DropCategory::Armor,  0.15),
        ],
        // 중간 몬스터 — 장비 드롭 확률 상승
        "오크" => &[
            (DropCategory::Consumable(ConsumableKind::HEALTH_POTION), 0.40),
            (DropCategory::Weapon, 0.28),
            (DropCategory::Armor,  0.22),
        ],
        // 강한 몬스터 — 포션·장비 모두 높음
        "트롤" => &[
            (DropCategory::Consumable(ConsumableKind::HEALTH_POTION), 0.50),
            (DropCategory::Weapon, 0.35),
            (DropCategory::Armor,  0.28),
        ],
        _ => &[
            (DropCategory::Consumable(ConsumableKind::HEALTH_POTION), 0.25),
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

/// 한 번의 드롭 이벤트에 대해 실제로 떨어질 아이템들을 결정한다 (rng 주입 seam).
/// 몬스터 드롭 테이블로 카테고리·빈도를 굴리고, 장비 카테고리는 플레이어 레벨
/// 가중으로 구체 아이템·티어를 정한 뒤 스탯을 롤한다.
/// 시스템과 분리해 통계적 커버 + 레벨별 분포 검증을 용이하게 한다.
fn resolve_drops<R: Rng + ?Sized>(
    monster_name: &str,
    level: u32,
    r: &ItemRegistry,
    rng: &mut R,
) -> Vec<(ItemKind, Option<i32>, Option<i32>)> {
    let mut out = Vec::new();
    for &(category, rate) in monster_drop_table(monster_name) {
        if rng.gen::<f32>() >= rate { continue; }
        let kind = match category {
            DropCategory::Consumable(ck) => ItemKind::Consumable(ck),
            DropCategory::Weapon => match pick_leveled_weapon(level, r, rng) {
                Some(w) => ItemKind::Weapon(w),
                None => continue,
            },
            DropCategory::Armor => match pick_leveled_armor(level, r, rng) {
                Some(a) => ItemKind::Armor(a),
                None => continue,
            },
        };
        let (rolled_attack, rolled_defense) = roll_item_stats(kind, rng, r);
        out.push((kind, rolled_attack, rolled_defense));
    }
    out
}

fn spawn_dropped_items(
    mut events: EventReader<ItemDropEvent>,
    mut commands: Commands,
    config: Res<GlyphConfig>,
    font_handles: Res<GlyphFontHandles>,
    quest_items: Res<QuestItemRegistry>,
    progress: Res<crate::modules::player::PlayerProgress>,
) {
    let mut rng = rand::thread_rng();
    for event in events.read() {
        for (kind, rolled_attack, rolled_defense) in
            resolve_drops(&event.monster_name, progress.level, &quest_items, &mut rng)
        {
            let pos = tile_to_world_coords(event.tile_x, event.tile_y);
            commands.spawn((
                Text2dBundle {
                    text: Text::from_section(
                        glyph_for_style(kind, config.style, &quest_items),
                        TextStyle {
                            font: font_handles.for_style(config.style),
                            font_size: TILE_SIZE,
                            color: item_display_color(kind, rolled_attack, rolled_defense, &quest_items),
                        },
                    ),
                    transform: Transform::from_xyz(pos.x, pos.y, Z_ITEM),
                    ..default()
                },
                Item { kind, tile_x: event.tile_x, tile_y: event.tile_y, rolled_attack, rolled_defense },
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
    defeated_q: Query<(), With<crate::modules::combat::Defeated>>,
) {
    if !defeated_q.is_empty() { return; }
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

    let at_tile: Vec<(Entity, ItemKind, usize, usize, Option<i32>, Option<i32>)> = item_query.iter()
        .filter(|(_, item)| item.tile_x == px && item.tile_y == py)
        .map(|(e, item)| (e, item.kind, item.tile_x, item.tile_y, item.rolled_attack, item.rolled_defense))
        .collect();

    for (entity, kind, tx, ty, rolled_attack, rolled_defense) in at_tile {
        match kind {
            ItemKind::Weapon(_) | ItemKind::Armor(_) | ItemKind::QuestItem(_) => {
                inventory.items.push(InventoryItem { kind, rolled_attack, rolled_defense });
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
    #![allow(non_snake_case)]
    use super::*;
    use std::sync::OnceLock;
    static TEST_QI: OnceLock<ItemRegistry> = OnceLock::new();
    fn qi() -> &'static ItemRegistry {
        TEST_QI.get_or_init(|| build_test_registry())
    }

    #[test]
    fn 검의_공격력은_범위_중앙값_7이다() {
        // 검 5~9 → 중앙값 (5+9)/2 = 7
        assert_eq!(weapon_attack(WeaponKind::SWORD, qi()), 7);
    }

    #[test]
    fn 창의_공격력은_범위_중앙값_10이다() {
        // 창 8~12 → 중앙값 10
        assert_eq!(weapon_attack(WeaponKind::SPEAR, qi()), 10);
    }

    #[test]
    fn 활의_공격력은_범위_중앙값_9이다() {
        // 활 7~11 → 중앙값 9
        assert_eq!(weapon_attack(WeaponKind::BOW, qi()), 9);
    }

    #[test]
    fn 가죽갑옷의_방어보너스는_범위_중앙값_3이다() {
        // 가죽 2~4 → 중앙값 3
        assert_eq!(armor_defense_bonus(ArmorKind::LEATHER_ARMOR, qi()), 3);
    }

    #[test]
    fn 무기가_없으면_유효공격력은_플레이어_기본값이다() {
        let eq = PlayerEquipment { weapon: None, armor: None, ..Default::default() };
        assert_eq!(effective_attack(&eq, qi()), PLAYER_ATK);
    }

    #[test]
    fn 롤값없이_검을_장착하면_유효공격력은_중앙값_7이다() {
        // 롤값(weapon_rolled_attack) 이 None 이면 검 범위 중앙값 7 을 쓴다.
        let eq = PlayerEquipment { weapon: Some(WeaponKind::SWORD), armor: None, ..Default::default() };
        assert_eq!(effective_attack(&eq, qi()), 7);
    }

    #[test]
    fn 방어구가_없으면_유효방어력은_플레이어_기본값이다() {
        let eq = PlayerEquipment { weapon: None, armor: None, ..Default::default() };
        assert_eq!(effective_defense(&eq, qi()), PLAYER_DEF);
    }

    #[test]
    fn 롤값없이_가죽갑옷을_장착하면_방어보너스_중앙값이_더해진다() {
        // 롤값 None → 가죽 범위 중앙값 3 이 더해진다.
        let eq = PlayerEquipment { weapon: None, armor: Some(ArmorKind::LEATHER_ARMOR), ..Default::default() };
        assert_eq!(effective_defense(&eq, qi()), PLAYER_DEF + 3);
    }

    #[test]
    fn 고블린_드롭테이블은_포션과_무기방어구_카테고리로_구성된다() {
        // 드롭 테이블은 카테고리·빈도만 정의하고, 구체 아이템/티어는 레벨이 정한다.
        // 고블린은 포션 + 무기 + 방어구 드롭 카테고리를 갖는다.
        let t = monster_drop_table("고블린");
        assert!(t.iter().any(|(c, _)| matches!(c, DropCategory::Consumable(ConsumableKind::HEALTH_POTION))));
        assert!(t.iter().any(|(c, _)| matches!(c, DropCategory::Weapon)));
        assert!(t.iter().any(|(c, _)| matches!(c, DropCategory::Armor)));
    }

    #[test]
    fn 강한_몬스터일수록_장비드롭_확률이_높다() {
        // 몬스터는 드롭 빈도에만 영향을 준다 — 무기 드롭 확률은 고블린<오크<트롤.
        let weapon_rate = |name: &str| -> f32 {
            monster_drop_table(name).iter()
                .find(|(c, _)| matches!(c, DropCategory::Weapon))
                .map(|(_, r)| *r).unwrap()
        };
        assert!(weapon_rate("고블린") < weapon_rate("오크"));
        assert!(weapon_rate("오크") < weapon_rate("트롤"));
    }

    #[test]
    fn 포션드롭_확률은_강한_몬스터일수록_높다() {
        let potion_rate = |name: &str| -> f32 {
            monster_drop_table(name).iter()
                .find(|(c, _)| matches!(c, DropCategory::Consumable(_)))
                .map(|(_, r)| *r).unwrap()
        };
        assert!(potion_rate("고블린") < potion_rate("오크"));
        assert!(potion_rate("오크") < potion_rate("트롤"));
    }

    #[test]
    fn 같은_소비아이템을_추가하면_수량이_누적된다() {
        let mut inv = PlayerInventory::default();
        inv.add_consumable(ConsumableKind::HEALTH_POTION);
        inv.add_consumable(ConsumableKind::HEALTH_POTION);
        assert_eq!(inv.consumables.len(), 1);
        assert_eq!(inv.consumables[0].1, 2);
    }

    #[test]
    fn 소비아이템을_사용하면_수량이_감소한다() {
        let mut inv = PlayerInventory::default();
        inv.add_consumable(ConsumableKind::HEALTH_POTION);
        inv.add_consumable(ConsumableKind::HEALTH_POTION);
        assert!(inv.use_consumable(ConsumableKind::HEALTH_POTION));
        assert_eq!(inv.consumables[0].1, 1);
    }

    #[test]
    fn 소비아이템_수량이_0이되면_슬롯이_제거된다() {
        let mut inv = PlayerInventory::default();
        inv.add_consumable(ConsumableKind::HEALTH_POTION);
        inv.use_consumable(ConsumableKind::HEALTH_POTION);
        assert!(inv.consumables.is_empty());
    }

    #[test]
    fn 비어있을때_소비아이템_사용은_실패한다() {
        let mut inv = PlayerInventory::default();
        assert!(!inv.use_consumable(ConsumableKind::HEALTH_POTION));
    }

    #[test]
    fn 체력물약의_회복량은_상수와_일치한다() {
        assert_eq!(ConsumableKind::HEALTH_POTION.heal_amount(qi()), POTION_HEAL);
    }

    #[test]
    fn 장비창_열림상태의_기본값은_거짓이다() {
        assert!(!EquipmentPanelOpen::default().0);
    }

    #[test]
    fn 글리프스타일은_모든_종류를_순환한다() {
        assert_eq!(GlyphStyle::Ascii.next(),    GlyphStyle::Unicode);
        assert_eq!(GlyphStyle::Unicode.next(),  GlyphStyle::GameIcon);
        assert_eq!(GlyphStyle::GameIcon.next(), GlyphStyle::Ascii);
    }

    #[test]
    fn 유효한_문자열은_글리프스타일로_파싱된다() {
        assert_eq!(GlyphStyle::from_str("ascii"),   Some(GlyphStyle::Ascii));
        assert_eq!(GlyphStyle::from_str("unicode"), Some(GlyphStyle::Unicode));
        assert_eq!(GlyphStyle::from_str("icon"),    Some(GlyphStyle::GameIcon));
    }

    #[test]
    fn 잘못된_문자열은_글리프스타일_파싱에_실패한다() {
        assert_eq!(GlyphStyle::from_str("unknown"), None);
    }

    #[test]
    fn 시작_로드아웃_ron이_정상적으로_파싱된다() {
        let text = std::fs::read_to_string("assets/items/start_loadout.ron")
            .expect("start_loadout.ron 읽기 실패");
        let loadout: StartLoadout = ron::de::from_str(&text).expect("start_loadout.ron 파싱 실패");
        assert_eq!(loadout.gold, 50);
        assert_eq!(loadout.items, vec!["sword", "spear", "bow"]);
        assert_eq!(loadout.consumables, vec![
            ("health_potion".to_string(), 10),
            ("trap_kit".to_string(), 3),
            ("disarm_tool".to_string(), 1),
        ]);
    }

    #[test]
    fn 시작_로드아웃을_적용하면_인벤토리에_아이템이_들어간다() {
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
    fn 시작_로드아웃의_알수없는_id는_건너뛴다() {
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

    #[test]
    fn 시작_로드아웃의_items에_담긴_방어구id는_인벤토리에_방어구로_들어간다() {
        // items 목록에 방어구 id 가 들어오면 무기가 아니라 Armor 로 push 되고,
        // 미등록 id 는 건너뛴다 (apply_start_loadout 의 intern_armor / else 분기).
        let mut inv = PlayerInventory::default();
        let mut eq = PlayerEquipment::default();
        let loadout = StartLoadout {
            gold: 0,
            weapon: None,
            armor: None,
            items: vec!["leather_armor".into(), "없는거".into()],
            consumables: vec![],
        };
        apply_start_loadout(&mut inv, &mut eq, &loadout, qi());
        assert_eq!(inv.items.len(), 1, "미등록 id 는 스킵되고 방어구만 남는다");
        assert!(
            matches!(inv.items[0].kind, ItemKind::Armor(ArmorKind::LEATHER_ARMOR)),
            "items 의 방어구 id 는 Armor 로 인벤토리에 들어가야 한다",
        );
    }

    fn lookup_display_name(qk: QuestItemKind) -> &'static str {
        qi().quest_item(qk).map(|m| m.display_name).unwrap_or("???")
    }

    #[test]
    fn ascii_스타일은_ascii_문자를_반환한다() {
        assert_eq!(glyph_for_style(ItemKind::Weapon(WeaponKind::SWORD), GlyphStyle::Ascii, qi()), "/");
        assert_eq!(glyph_for_style(ItemKind::Weapon(WeaponKind::SPEAR), GlyphStyle::Ascii, qi()), "|");
        assert_eq!(glyph_for_style(ItemKind::Weapon(WeaponKind::BOW),   GlyphStyle::Ascii, qi()), ")");
    }

    #[test]
    fn 유니코드_스타일은_심볼문자를_반환한다() {
        let s = glyph_for_style(ItemKind::Weapon(WeaponKind::SWORD), GlyphStyle::Unicode, qi());
        assert_eq!(s, "\u{1F5E1}");
        let shield = glyph_for_style(ItemKind::Armor(ArmorKind::LEATHER_ARMOR), GlyphStyle::Unicode, qi());
        assert_eq!(shield, "\u{1F6E1}");
    }

    #[test]
    fn 게임아이콘_스타일은_pua_코드포인트를_반환한다() {
        let s = glyph_for_style(ItemKind::Weapon(WeaponKind::SWORD), GlyphStyle::GameIcon, qi());
        assert_eq!(s, "\u{E946}");
        let potion = glyph_for_style(ItemKind::Consumable(ConsumableKind::HEALTH_POTION), GlyphStyle::GameIcon, qi());
        assert_eq!(potion, "\u{EA72}");
    }

    #[test]
    fn 글리프스타일의_기본값은_ascii이다() {
        assert_eq!(GlyphStyle::default(), GlyphStyle::Ascii);
    }

    #[test]
    fn 퀘스트아이템의_표시이름이_올바르다() {
        assert_eq!(lookup_display_name(QuestItemKind("eternal_gem")), "영원의 보석");
        assert_eq!(lookup_display_name(QuestItemKind("philosophers_stone")), "현자의 돌");
    }

    #[test]
    fn 퀘스트아이템의_글리프와_획득메시지가_올바르다() {
        let gem = ItemKind::QuestItem(QuestItemKind("eternal_gem"));
        assert_eq!(gem.glyph(qi()), "*");
        assert_eq!(gem.pickup_message(qi()), "영원의 보석을 획득했다!");
        let stone = ItemKind::QuestItem(QuestItemKind("philosophers_stone"));
        assert_eq!(stone.pickup_message(qi()), "현자의 돌을 획득했다!");
    }

    #[test]
    fn 마검_퀘스트아이템들의_글리프와_이름이_올바르다() {
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
    fn 패링_퀘스트아이템들의_글리프와_이름이_올바르다() {
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
    fn 마검_퀘스트아이템들의_유니코드_글리프가_올바르다() {
        let sword = glyph_for_style(ItemKind::QuestItem(QuestItemKind("demon_sword")), GlyphStyle::Unicode, qi());
        assert_eq!(sword, "\u{2694}");
        let memo = glyph_for_style(ItemKind::QuestItem(QuestItemKind("elenas_memo")), GlyphStyle::Unicode, qi());
        assert_eq!(memo, "\u{270E}");
        let book = glyph_for_style(ItemKind::QuestItem(QuestItemKind("ancient_ritual_book")), GlyphStyle::Unicode, qi());
        assert_eq!(book, "\u{2720}");
    }

    #[test]
    fn 퀘스트아이템_ron은_36종을_모두_로드한다() {
        // 기존 35종 + 신규 퀘스트 아이템 1종(arcane_focus, 원소의 시험장 퀘스트 목표) = 36종.
        let registry = qi();
        assert_eq!(registry.quest_items.len(), 36, "quest_items.ron 에 36 종이 정의되어야 한다");
    }

    #[test]
    fn 알수없는_id의_퀘스트아이템_메타는_없음을_반환한다() {
        let unknown = QuestItemKind("does_not_exist");
        assert!(qi().quest_item(unknown).is_none());
    }

    #[test]
    fn 같은_퀘스트id는_인턴시_동일한_포인터를_반환한다() {
        let a = qi().intern_quest_item("eternal_gem").expect("등록된 ID 여야 한다");
        let b = qi().intern_quest_item("eternal_gem").expect("등록된 ID 여야 한다");
        // registry 에 등록된 ID 는 동일 &'static str (포인터 일치)
        assert_eq!(a.as_ptr(), b.as_ptr(), "같은 등록된 ID 는 같은 포인터여야 한다");
    }

    #[test]
    fn 무기_ron은_5티어_풀세트_열다섯_종류를_로드한다() {
        // 5티어 × 3종 = 15 무기.
        assert_eq!(qi().weapons.len(), 15);
        assert!(qi().weapon(WeaponKind::SWORD).is_some());
        assert!(qi().weapon(WeaponKind::SPEAR).is_some());
        assert!(qi().weapon(WeaponKind::BOW).is_some());
        // 각 티어 대표 신규 무기도 로드된다.
        assert!(qi().weapon(WeaponKind("dagger")).is_some());
        assert!(qi().weapon(WeaponKind("holy_sword")).is_some());
    }

    #[test]
    fn 무기들의_공격력_범위와_중앙값이_정확하다() {
        let sword = qi().weapon(WeaponKind::SWORD).unwrap();
        assert_eq!((sword.attack_power_min, sword.attack_power_max), (5, 9));
        assert_eq!(sword.attack_mid(), 7);
        let spear = qi().weapon(WeaponKind::SPEAR).unwrap();
        assert_eq!((spear.attack_power_min, spear.attack_power_max), (8, 12));
        assert_eq!(spear.attack_mid(), 10);
        let bow = qi().weapon(WeaponKind::BOW).unwrap();
        assert_eq!((bow.attack_power_min, bow.attack_power_max), (7, 11));
        assert_eq!(bow.attack_mid(), 9);
    }

    #[test]
    fn 무기들의_티어가_정확하다() {
        assert_eq!(qi().weapon(WeaponKind("dagger")).unwrap().tier, 1);
        assert_eq!(qi().weapon(WeaponKind::SPEAR).unwrap().tier, 2);
        assert_eq!(qi().weapon(WeaponKind("crossbow")).unwrap().tier, 3);
        assert_eq!(qi().weapon(WeaponKind("greatsword")).unwrap().tier, 4);
        assert_eq!(qi().weapon(WeaponKind("dragon_spear")).unwrap().tier, 5);
    }

    #[test]
    fn 무기들이_원소_문자열을_가진다() {
        assert_eq!(qi().weapon(WeaponKind::SWORD).unwrap().element, Some("fire"));
        assert_eq!(qi().weapon(WeaponKind::SPEAR).unwrap().element, Some("ice"));
        assert_eq!(qi().weapon(WeaponKind::BOW).unwrap().element, Some("lightning"));
        // 원소 없는 무기(단검/몽둥이/전쟁해머)도 있다.
        assert_eq!(qi().weapon(WeaponKind("dagger")).unwrap().element, None);
    }

    #[test]
    fn 방어구_ron은_5티어_풀세트_열_종류를_로드한다() {
        // 5티어 × 2종 = 10 방어구.
        assert_eq!(qi().armors.len(), 10);
        let leather = qi().armor(ArmorKind::LEATHER_ARMOR).unwrap();
        assert_eq!(leather.display_name, "가죽 갑옷");
        assert_eq!((leather.defense_bonus_min, leather.defense_bonus_max), (2, 4));
        assert_eq!(leather.defense_mid(), 3);
    }

    #[test]
    fn 방어구들의_티어가_정확하다() {
        assert_eq!(qi().armor(ArmorKind("cloth_armor")).unwrap().tier, 1);
        assert_eq!(qi().armor(ArmorKind("light_armor")).unwrap().tier, 2);
        assert_eq!(qi().armor(ArmorKind("scale_armor")).unwrap().tier, 3);
        assert_eq!(qi().armor(ArmorKind("plate_armor")).unwrap().tier, 4);
        assert_eq!(qi().armor(ArmorKind("paladin_armor")).unwrap().tier, 5);
    }

    #[test]
    fn 소비아이템_ron은_회복효과를_가진_체력물약을_로드한다() {
        // 체력 물약 + 함정 키트 + 해제 도구(§B-2) 가 모두 로드된다.
        assert_eq!(qi().consumables.len(), 3);
        let potion = qi().consumable(ConsumableKind::HEALTH_POTION).unwrap();
        assert!(matches!(potion.effect, ConsumableEffect::Heal(8)));
    }

    #[test]
    fn 소비아이템_ron은_함정키트와_해제도구를_효과없이_로드한다() {
        // §B-2 아이템은 회복 효과가 없어(Heal(0)) 장비 패널 회복 경로로는 소비되지 않는다.
        let kit = qi().consumable(ConsumableKind("trap_kit")).expect("함정 키트 로드");
        assert!(matches!(kit.effect, ConsumableEffect::Heal(0)));
        let tool = qi().consumable(ConsumableKind("disarm_tool")).expect("해제 도구 로드");
        assert!(matches!(tool.effect, ConsumableEffect::Heal(0)));
    }

    #[test]
    fn 무기종류는_serde_직렬화_왕복이_보존된다() {
        let wk = WeaponKind::SWORD;
        let s = ron::ser::to_string(&wk).unwrap();
        assert_eq!(s, "\"sword\"");
        let parsed: WeaponKind = ron::de::from_str(&s).unwrap();
        assert_eq!(parsed, wk);
    }

    #[test]
    fn 퀘스트아이템종류는_serde_직렬화_왕복이_보존된다() {
        let qk = QuestItemKind("eternal_gem");
        let s = ron::ser::to_string(&qk).unwrap();
        assert_eq!(s, "\"eternal_gem\"");
        let parsed: QuestItemKind = ron::de::from_str(&s).unwrap();
        assert_eq!(parsed, qk);
    }

    // ── 추가: 순수 로직 분기 커버리지 ───────────────────────────────────────

    #[test]
    fn 아이템_색상은_카테고리마다_구분된다() {
        assert_eq!(ItemKind::Weapon(WeaponKind::SWORD).color(),     Color::rgb(1.0, 1.0, 0.2));
        assert_eq!(ItemKind::Armor(ArmorKind::LEATHER_ARMOR).color(), Color::rgb(0.2, 0.4, 1.0));
        assert_eq!(ItemKind::Consumable(ConsumableKind::HEALTH_POTION).color(), Color::rgb(0.2, 0.9, 0.2));
        assert_eq!(ItemKind::QuestItem(QuestItemKind("eternal_gem")).color(), Color::rgb(0.8, 0.3, 1.0));
    }

    #[test]
    fn 골드를_획득하면_잔액이_증가한다() {
        let mut inv = PlayerInventory::default();
        let before = inv.gold;
        inv.earn_gold(25);
        assert_eq!(inv.gold, before + 25);
    }

    #[test]
    fn 잔액이_충분하면_골드_소비에_성공한다() {
        let mut inv = PlayerInventory::default();
        inv.gold = 100;
        assert!(inv.spend_gold(40));
        assert_eq!(inv.gold, 60);
    }

    #[test]
    fn 잔액이_부족하면_골드_소비에_실패하고_잔액이_유지된다() {
        let mut inv = PlayerInventory::default();
        inv.gold = 30;
        assert!(!inv.spend_gold(40));
        assert_eq!(inv.gold, 30, "실패 시 잔액은 변하지 않아야 한다");
    }

    #[test]
    fn 잔액과_정확히_같은_금액은_소비에_성공한다() {
        let mut inv = PlayerInventory::default();
        inv.gold = 50;
        assert!(inv.spend_gold(50));
        assert_eq!(inv.gold, 0);
    }

    #[test]
    fn 알수없는_몬스터의_드롭테이블은_포션만_반환한다() {
        let t = monster_drop_table("듣보잡몬스터");
        assert_eq!(t.len(), 1);
        assert!(matches!(t[0].0, DropCategory::Consumable(ConsumableKind::HEALTH_POTION)));
    }

    #[test]
    fn 방어구종류는_serde_직렬화_왕복이_보존된다() {
        let ak = ArmorKind::LEATHER_ARMOR;
        let s = ron::ser::to_string(&ak).unwrap();
        assert_eq!(s, "\"leather_armor\"");
        let parsed: ArmorKind = ron::de::from_str(&s).unwrap();
        assert_eq!(parsed, ak);
    }

    #[test]
    fn 소비아이템종류는_serde_직렬화_왕복이_보존된다() {
        let ck = ConsumableKind::HEALTH_POTION;
        let s = ron::ser::to_string(&ck).unwrap();
        assert_eq!(s, "\"health_potion\"");
        let parsed: ConsumableKind = ron::de::from_str(&s).unwrap();
        assert_eq!(parsed, ck);
    }

    #[test]
    fn 모든_아이템종류가_serde_직렬화_왕복이_보존된다() {
        for kind in [
            ItemKind::Weapon(WeaponKind::SWORD),
            ItemKind::Armor(ArmorKind::LEATHER_ARMOR),
            ItemKind::Consumable(ConsumableKind::HEALTH_POTION),
            ItemKind::QuestItem(QuestItemKind("eternal_gem")),
        ] {
            let s = ron::ser::to_string(&kind).unwrap();
            let parsed: ItemKind = ron::de::from_str(&s).unwrap();
            assert_eq!(parsed, kind);
        }
    }

    #[test]
    fn 등록되지_않은_무기의_공격력은_0이다() {
        assert_eq!(weapon_attack(WeaponKind("does_not_exist"), qi()), 0);
    }

    #[test]
    fn 등록되지_않은_방어구의_방어보너스는_0이다() {
        assert_eq!(armor_defense_bonus(ArmorKind("does_not_exist"), qi()), 0);
    }

    #[test]
    fn 등록되지_않은_종류의_글리프는_물음표로_폴백된다() {
        // 각 카테고리의 미등록 ID 는 "?" 로 폴백.
        assert_eq!(ItemKind::Weapon(WeaponKind("nope")).glyph(qi()), "?");
        assert_eq!(ItemKind::Armor(ArmorKind("nope")).glyph(qi()), "?");
        assert_eq!(ItemKind::Consumable(ConsumableKind("nope")).glyph(qi()), "?");
        assert_eq!(ItemKind::QuestItem(QuestItemKind("nope")).glyph(qi()), "?");
    }

    #[test]
    fn 등록되지_않은_종류의_유니코드와_아이콘_글리프도_폴백된다() {
        for style in [GlyphStyle::Unicode, GlyphStyle::GameIcon] {
            assert_eq!(glyph_for_style(ItemKind::Weapon(WeaponKind("nope")), style, qi()), "?");
            assert_eq!(glyph_for_style(ItemKind::Armor(ArmorKind("nope")), style, qi()), "?");
            assert_eq!(glyph_for_style(ItemKind::Consumable(ConsumableKind("nope")), style, qi()), "?");
            assert_eq!(glyph_for_style(ItemKind::QuestItem(QuestItemKind("nope")), style, qi()), "?");
        }
    }

    #[test]
    fn 등록되지_않은_종류의_표시이름은_물음표로_폴백된다() {
        assert_eq!(ItemKind::Weapon(WeaponKind("nope")).display_name(qi()), "???");
        assert_eq!(ItemKind::Armor(ArmorKind("nope")).display_name(qi()), "???");
        assert_eq!(ItemKind::Consumable(ConsumableKind("nope")).display_name(qi()), "???");
        assert_eq!(ItemKind::QuestItem(QuestItemKind("nope")).display_name(qi()), "???");
    }

    #[test]
    fn 등록되지_않은_종류의_획득메시지는_카테고리별_기본문구로_폴백된다() {
        assert_eq!(ItemKind::Weapon(WeaponKind("nope")).pickup_message(qi()), "무기를 획득했다!");
        assert_eq!(ItemKind::Armor(ArmorKind("nope")).pickup_message(qi()), "방어구를 획득했다!");
        assert_eq!(ItemKind::Consumable(ConsumableKind("nope")).pickup_message(qi()), "소모품을 획득했다!");
        assert_eq!(ItemKind::QuestItem(QuestItemKind("nope")).pickup_message(qi()), "아이템을 획득했다!");
    }

    #[test]
    fn 글리프폰트핸들이_각_스타일에_맞는_핸들을_돌려준다() {
        // GlyphFontHandles::for_style 의 3개 arm 매핑 (핸들 동등성).
        let h = GlyphFontHandles {
            ascii: Handle::weak_from_u128(1),
            unicode: Handle::weak_from_u128(2),
            game_icon: Handle::weak_from_u128(3),
        };
        assert_eq!(h.for_style(GlyphStyle::Ascii), h.ascii);
        assert_eq!(h.for_style(GlyphStyle::Unicode), h.unicode);
        assert_eq!(h.for_style(GlyphStyle::GameIcon), h.game_icon);
    }

    #[test]
    fn 다른_종류만_있을때_해당_소비아이템_사용은_실패한다() {
        // consumables 에 슬롯은 있지만 요청한 kind 가 아닌 경우 (position None 경로)
        let mut inv = PlayerInventory::default();
        inv.add_consumable(ConsumableKind::HEALTH_POTION);
        assert!(!inv.use_consumable(ConsumableKind("mana_potion")));
        assert_eq!(inv.consumables[0].1, 1, "다른 kind 사용 실패는 기존 수량 보존");
    }

    // ── 추가: Bevy 시스템 App 하네스 테스트 ─────────────────────────────────
    use crate::modules::combat::Defeated;

    #[test]
    fn 등록되지_않은_퀘스트아이템의_이미지경로는_기본값으로_폴백된다() {
        // 미등록 quest item → 기본 이미지 경로 폴백
        assert_eq!(quest_item_image_path(QuestItemKind("nope"), qi()), "scene/open-chest.png");
    }

    #[test]
    fn 등록된_퀘스트아이템은_지정된_이미지경로를_반환한다() {
        // 등록된 quest item 은 registry 의 image_path 를 그대로 반환 (Some 경로)
        let expected = qi().quest_item(QuestItemKind("eternal_gem")).unwrap().image_path;
        assert_eq!(quest_item_image_path(QuestItemKind("eternal_gem"), qi()), expected);
    }

    // ── load_*_system: RON 파일을 registry 로 적재 ──
    #[test]
    fn 무기_로드_시스템이_레지스트리를_채운다() {
        let mut app = App::new();
        app.init_resource::<ItemRegistry>()
            .add_systems(Startup, load_weapons_system);
        app.update();
        let r = app.world.resource::<ItemRegistry>();
        assert!(r.weapon(WeaponKind::SWORD).is_some());
    }

    #[test]
    fn 방어구_로드_시스템이_레지스트리를_채운다() {
        let mut app = App::new();
        app.init_resource::<ItemRegistry>()
            .add_systems(Startup, load_armors_system);
        app.update();
        assert!(app.world.resource::<ItemRegistry>().armor(ArmorKind::LEATHER_ARMOR).is_some());
    }

    #[test]
    fn 소비아이템_로드_시스템이_레지스트리를_채운다() {
        let mut app = App::new();
        app.init_resource::<ItemRegistry>()
            .add_systems(Startup, load_consumables_system);
        app.update();
        assert!(app.world.resource::<ItemRegistry>().consumable(ConsumableKind::HEALTH_POTION).is_some());
    }

    #[test]
    fn 퀘스트아이템_로드_시스템이_레지스트리를_채운다() {
        let mut app = App::new();
        app.init_resource::<QuestItemRegistry>()
            .add_systems(Startup, load_quest_items_system);
        app.update();
        assert!(app.world.resource::<QuestItemRegistry>().quest_items.len() >= 1);
    }

    #[test]
    fn 시작로드아웃_로드_시스템이_시작골드를_읽어온다() {
        let mut app = App::new();
        app.init_resource::<StartLoadoutRegistry>()
            .add_systems(Startup, load_start_loadout_system);
        app.update();
        assert_eq!(app.world.resource::<StartLoadoutRegistry>().0.gold, 50);
    }

    // ── cycle_glyph_style ──
    fn glyph_app() -> App {
        let mut app = App::new();
        app.insert_resource(GlyphConfig { style: GlyphStyle::Ascii })
            .insert_resource(ButtonInput::<KeyCode>::default())
            .add_event::<LogMessage>()
            .add_systems(Update, cycle_glyph_style);
        app
    }

    #[test]
    fn G키를_누르면_글리프스타일이_다음으로_바뀐다() {
        let mut app = glyph_app();
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyG);
        app.update();
        assert_eq!(app.world.resource::<GlyphConfig>().style, GlyphStyle::Unicode);
    }

    #[test]
    fn 키_입력이_없으면_글리프스타일이_바뀌지_않는다() {
        let mut app = glyph_app();
        app.update();
        assert_eq!(app.world.resource::<GlyphConfig>().style, GlyphStyle::Ascii);
    }

    #[test]
    fn 사망_상태에서는_글리프스타일_전환이_차단된다() {
        let mut app = glyph_app();
        app.world.spawn(Defeated);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyG);
        app.update();
        assert_eq!(app.world.resource::<GlyphConfig>().style, GlyphStyle::Ascii, "사망 시 글리프 전환 차단");
    }

    // ── apply_equipment_stats ──
    #[test]
    fn 장비가_바뀌면_플레이어_전투스탯이_갱신된다() {
        let mut app = App::new();
        app.insert_resource(PlayerEquipment { weapon: Some(WeaponKind::SPEAR), armor: Some(ArmorKind::LEATHER_ARMOR), ..Default::default() })
            .insert_resource(build_test_registry());
        let e = app.world.spawn((
            Player,
            CombatStats { hp: 30, max_hp: 30, mp: 0, max_mp: 0, attack: PLAYER_ATK, defense: PLAYER_DEF },
        )).id();
        app.add_systems(Update, apply_equipment_stats);
        app.update();
        let stats = app.world.get::<CombatStats>(e).unwrap();
        // 롤값 없는 장비 → 범위 중앙값: 창 8~12 → 10, 가죽 2~4 → +3.
        assert_eq!(stats.attack, 10, "창 공격력 중앙값");
        assert_eq!(stats.defense, PLAYER_DEF + 3, "가죽 갑옷 중앙값 +3");
    }

    #[test]
    fn 플레이어가_없으면_장비스탯_적용은_아무일도_하지_않는다() {
        let mut app = App::new();
        app.insert_resource(PlayerEquipment::default())
            .insert_resource(build_test_registry())
            .add_systems(Update, apply_equipment_stats);
        app.update(); // 플레이어 없음 → get_single_mut Err → 조용히 반환 (panic 없음)
    }

    #[test]
    fn 장비가_바뀌지_않으면_전투스탯을_재계산하지_않는다() {
        let mut app = App::new();
        app.insert_resource(PlayerEquipment { weapon: Some(WeaponKind::SWORD), armor: None, ..Default::default() })
            .insert_resource(build_test_registry());
        let e = app.world.spawn((
            Player,
            CombatStats { hp: 30, max_hp: 30, mp: 0, max_mp: 0, attack: PLAYER_ATK, defense: PLAYER_DEF },
        )).id();
        app.add_systems(Update, apply_equipment_stats);
        app.update(); // 1회차: 변경됨 → 적용 (attack=7)
        assert_eq!(app.world.get::<CombatStats>(e).unwrap().attack, 7);
        // 외부에서 attack 을 임의로 바꿔도 2회차엔 equipment 미변경 → 덮어쓰지 않음
        app.world.get_mut::<CombatStats>(e).unwrap().attack = 999;
        app.update();
        assert_eq!(app.world.get::<CombatStats>(e).unwrap().attack, 999, "equipment 미변경 시 재계산 안 함");
    }

    // ── handle_despawn_world_item ──
    #[test]
    fn 월드아이템_제거이벤트는_일치하는_종류의_아이템을_제거한다() {
        let mut app = App::new();
        app.insert_resource(build_test_registry())
            .add_event::<crate::modules::quest::DespawnWorldItemEvent>()
            .add_systems(Update, handle_despawn_world_item);
        let e = app.world.spawn(Item {
            kind: ItemKind::QuestItem(QuestItemKind("eternal_gem")),
            tile_x: 1, tile_y: 1,
            rolled_attack: None, rolled_defense: None,
        }).id();
        app.world.send_event(crate::modules::quest::DespawnWorldItemEvent("eternal_gem".to_string()));
        app.update();
        assert!(app.world.get_entity(e).is_none(), "해당 아이템 엔티티 제거됨");
    }

    #[test]
    fn 월드아이템_제거이벤트는_종류가_다른_아이템은_남겨둔다() {
        // 일치하는 종류만 제거하고, kind 가 다른 아이템은 보존 (item.kind == kind 의 False 분기).
        let mut app = App::new();
        app.insert_resource(build_test_registry())
            .add_event::<crate::modules::quest::DespawnWorldItemEvent>()
            .add_systems(Update, handle_despawn_world_item);
        let target = app.world.spawn(Item {
            kind: ItemKind::QuestItem(QuestItemKind("eternal_gem")),
            tile_x: 1, tile_y: 1,
            rolled_attack: None, rolled_defense: None,
        }).id();
        let other = app.world.spawn(Item {
            kind: ItemKind::QuestItem(QuestItemKind("philosophers_stone")),
            tile_x: 2, tile_y: 2,
            rolled_attack: None, rolled_defense: None,
        }).id();
        app.world.send_event(crate::modules::quest::DespawnWorldItemEvent("eternal_gem".to_string()));
        app.update();
        assert!(app.world.get_entity(target).is_none(), "일치 종류는 제거");
        assert!(app.world.get_entity(other).is_some(), "다른 종류는 보존");
    }

    #[test]
    fn 월드아이템_제거이벤트는_알수없는_id를_무시한다() {
        let mut app = App::new();
        app.insert_resource(build_test_registry())
            .add_event::<crate::modules::quest::DespawnWorldItemEvent>()
            .add_systems(Update, handle_despawn_world_item);
        let e = app.world.spawn(Item {
            kind: ItemKind::QuestItem(QuestItemKind("eternal_gem")),
            tile_x: 1, tile_y: 1,
            rolled_attack: None, rolled_defense: None,
        }).id();
        app.world.send_event(crate::modules::quest::DespawnWorldItemEvent("does_not_exist".to_string()));
        app.update();
        assert!(app.world.get_entity(e).is_some(), "미등록 id 는 무시");
    }

    // ── pickup_items ──
    fn pickup_app() -> App {
        let mut app = App::new();
        app.insert_resource(build_test_registry())
            .init_resource::<PlayerInventory>()
            .init_resource::<crate::modules::ui::minimap::DiscoveredMarkers>()
            .init_resource::<crate::modules::zone::WorldState>()
            .add_event::<PlayerActedEvent>()
            .add_event::<LogMessage>()
            .add_event::<QuestItemAcquiredEvent>()
            .add_systems(Update, pickup_items);
        app
    }

    fn spawn_player_at(app: &mut App, px: usize, py: usize) {
        let pos = tile_to_world_coords(px, py).extend(0.0);
        app.world.spawn((Player, Transform::from_translation(pos)));
    }

    #[test]
    fn 턴_이벤트가_없으면_아이템을_줍지_않는다() {
        let mut app = pickup_app();
        spawn_player_at(&mut app, 5, 5);
        let item = app.world.spawn(Item { kind: ItemKind::Weapon(WeaponKind::SWORD), tile_x: 5, tile_y: 5, rolled_attack: None, rolled_defense: None }).id();
        app.update(); // PlayerActedEvent 없음 → 반환
        assert!(app.world.get_entity(item).is_some());
        assert!(app.world.resource::<PlayerInventory>().items.is_empty());
    }

    #[test]
    fn 플레이어가_없으면_아이템을_줍지_않는다() {
        // 턴 이벤트는 있지만 플레이어가 없으면 get_single Err → 조용히 반환 (panic 없음).
        let mut app = pickup_app();
        app.world.spawn(Item { kind: ItemKind::Weapon(WeaponKind::SWORD), tile_x: 5, tile_y: 5, rolled_attack: None, rolled_defense: None });
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!(app.world.resource::<PlayerInventory>().items.is_empty());
    }

    #[test]
    fn 주운_무기는_인벤토리에_들어간다() {
        let mut app = pickup_app();
        spawn_player_at(&mut app, 5, 5);
        app.world.spawn(Item { kind: ItemKind::Weapon(WeaponKind::SWORD), tile_x: 5, tile_y: 5, rolled_attack: None, rolled_defense: None });
        app.world.send_event(PlayerActedEvent);
        app.update();
        let inv = app.world.resource::<PlayerInventory>();
        assert_eq!(inv.items.len(), 1);
        assert!(matches!(inv.items[0].kind, ItemKind::Weapon(_)));
    }

    #[test]
    fn 주운_방어구는_인벤토리에_들어간다() {
        let mut app = pickup_app();
        spawn_player_at(&mut app, 5, 5);
        app.world.spawn(Item { kind: ItemKind::Armor(ArmorKind::LEATHER_ARMOR), tile_x: 5, tile_y: 5, rolled_attack: None, rolled_defense: None });
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.resource::<PlayerInventory>().items.len(), 1);
    }

    #[test]
    fn 주운_소비아이템은_인벤토리에서_누적된다() {
        let mut app = pickup_app();
        spawn_player_at(&mut app, 5, 5);
        app.world.spawn(Item { kind: ItemKind::Consumable(ConsumableKind::HEALTH_POTION), tile_x: 5, tile_y: 5, rolled_attack: None, rolled_defense: None });
        app.world.send_event(PlayerActedEvent);
        app.update();
        let inv = app.world.resource::<PlayerInventory>();
        assert!(inv.items.is_empty());
        assert_eq!(inv.consumables.len(), 1);
    }

    #[test]
    fn 퀘스트아이템을_주우면_획득이벤트가_발행되고_수집된다() {
        let mut app = pickup_app();
        spawn_player_at(&mut app, 5, 5);
        app.world.spawn(Item { kind: ItemKind::QuestItem(QuestItemKind("eternal_gem")), tile_x: 5, tile_y: 5, rolled_attack: None, rolled_defense: None });
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.resource::<PlayerInventory>().items.len(), 1);
        let events = app.world.resource::<Events<QuestItemAcquiredEvent>>();
        assert!(events.len() >= 1, "퀘스트 아이템 획득 이벤트 발행");
    }

    #[test]
    fn 플레이어_타일에_없는_아이템은_줍지_않는다() {
        let mut app = pickup_app();
        spawn_player_at(&mut app, 5, 5);
        app.world.spawn(Item { kind: ItemKind::Weapon(WeaponKind::SWORD), tile_x: 9, tile_y: 9, rolled_attack: None, rolled_defense: None });
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!(app.world.resource::<PlayerInventory>().items.is_empty(), "다른 타일 아이템은 안 주움");
    }

    #[test]
    fn 이동중이면_목적지_타일을_기준으로_아이템을_줍는다() {
        // MovingTo 가 있으면 그 목적지 타일 기준으로 줍는다
        let mut app = pickup_app();
        let pos = tile_to_world_coords(5, 5).extend(0.0);
        let target = tile_to_world_coords(7, 7).extend(0.0);
        app.world.spawn((Player, Transform::from_translation(pos), MovingTo { target }));
        app.world.spawn(Item { kind: ItemKind::Weapon(WeaponKind::SWORD), tile_x: 7, tile_y: 7, rolled_attack: None, rolled_defense: None });
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.resource::<PlayerInventory>().items.len(), 1);
    }

    // ── spawn_dropped_items (rng 기반 — 통계적 커버) ──
    use crate::modules::player::PlayerProgress;

    #[test]
    fn 드롭_이벤트가_충분히_많으면_아이템이_월드에_스폰된다() {
        let mut app = App::new();
        app.insert_resource(GlyphConfig { style: GlyphStyle::Ascii })
            .insert_resource(GlyphFontHandles {
                ascii: Handle::default(), unicode: Handle::default(), game_icon: Handle::default(),
            })
            .insert_resource(build_test_registry())
            .insert_resource(PlayerProgress::default())
            .add_event::<ItemDropEvent>()
            .add_systems(Update, spawn_dropped_items);
        // 트롤은 포션 0.5 확률 — 200개 이벤트면 사실상 확실히 일부 드롭
        for _ in 0..200 {
            app.world.send_event(ItemDropEvent { tile_x: 3, tile_y: 3, monster_name: "트롤".into() });
        }
        app.update();
        let count = app.world.query::<&Item>().iter(&app.world).count();
        assert!(count > 0, "다수 이벤트 시 최소 한 개는 드롭되어야 한다");
    }

    // ── update_item_glyphs ──
    #[test]
    fn 글리프설정이_바뀌면_월드_아이템_글리프가_갱신된다() {
        let mut app = App::new();
        app.insert_resource(GlyphConfig { style: GlyphStyle::Unicode })
            .insert_resource(GlyphFontHandles {
                ascii: Handle::default(), unicode: Handle::default(), game_icon: Handle::default(),
            })
            .insert_resource(build_test_registry());
        let e = app.world.spawn((
            Item { kind: ItemKind::Weapon(WeaponKind::SWORD), tile_x: 0, tile_y: 0, rolled_attack: None, rolled_defense: None },
            Text::from_section("/", TextStyle::default()),
        )).id();
        app.add_systems(Update, update_item_glyphs);
        app.update();
        let text = app.world.get::<Text>(e).unwrap();
        assert_eq!(text.sections[0].value, "\u{1F5E1}", "유니코드 글리프로 갱신");
    }

    #[test]
    fn 글리프설정이_바뀌지_않으면_아이템_글리프를_갱신하지_않는다() {
        let mut app = App::new();
        app.insert_resource(GlyphConfig { style: GlyphStyle::Ascii })
            .insert_resource(GlyphFontHandles {
                ascii: Handle::default(), unicode: Handle::default(), game_icon: Handle::default(),
            })
            .insert_resource(build_test_registry());
        let e = app.world.spawn((
            Item { kind: ItemKind::Weapon(WeaponKind::SWORD), tile_x: 0, tile_y: 0, rolled_attack: None, rolled_defense: None },
            Text::from_section("X", TextStyle::default()),
        )).id();
        app.add_systems(Update, update_item_glyphs);
        app.update(); // 1회차: config 변경됨 → "/" 로 갱신
        assert_eq!(app.world.get::<Text>(e).unwrap().sections[0].value, "/");
        app.world.get_mut::<Text>(e).unwrap().sections[0].value = "Z".into();
        app.update(); // 2회차: config 미변경 → 갱신 안 함
        assert_eq!(app.world.get::<Text>(e).unwrap().sections[0].value, "Z");
    }

    // ── AssetServer 의존 시스템 (폰트/팝업) ──
    fn asset_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app.init_asset::<Image>();
        app
    }

    #[test]
    fn 글리프폰트_셋업이_폰트핸들_리소스를_삽입한다() {
        let mut app = asset_app();
        app.add_systems(Startup, setup_glyph_fonts);
        app.update();
        assert!(app.world.get_resource::<GlyphFontHandles>().is_some());
    }

    #[test]
    fn 퀘스트아이템_획득이벤트가_오면_팝업이_생성된다() {
        let mut app = asset_app();
        app.insert_resource(build_test_registry())
            .add_event::<QuestItemAcquiredEvent>()
            .add_systems(Update, spawn_quest_item_popup);
        app.world.spawn((Player, Transform::default()));
        app.world.send_event(QuestItemAcquiredEvent(QuestItemKind("eternal_gem")));
        app.update();
        assert_eq!(app.world.query::<&QuestItemPopup>().iter(&app.world).count(), 1);
    }

    #[test]
    fn 이벤트가_없으면_퀘스트아이템_팝업을_만들지_않는다() {
        let mut app = asset_app();
        app.insert_resource(build_test_registry())
            .add_event::<QuestItemAcquiredEvent>()
            .add_systems(Update, spawn_quest_item_popup);
        app.world.spawn((Player, Transform::default()));
        app.update();
        assert_eq!(app.world.query::<&QuestItemPopup>().iter(&app.world).count(), 0);
    }

    #[test]
    fn 팝업이_이미_열려있으면_새_팝업을_만들지_않는다() {
        let mut app = asset_app();
        app.insert_resource(build_test_registry())
            .add_event::<QuestItemAcquiredEvent>()
            .add_systems(Update, spawn_quest_item_popup);
        app.world.spawn((Player, Transform::default()));
        app.world.spawn(QuestItemPopup { tile_x: 0, tile_y: 0 });
        app.world.send_event(QuestItemAcquiredEvent(QuestItemKind("eternal_gem")));
        app.update();
        assert_eq!(app.world.query::<&QuestItemPopup>().iter(&app.world).count(), 1, "이미 열려 있으면 추가 안 함");
    }

    #[test]
    fn 플레이어가_없으면_퀘스트아이템_팝업을_만들지_않는다() {
        let mut app = asset_app();
        app.insert_resource(build_test_registry())
            .add_event::<QuestItemAcquiredEvent>()
            .add_systems(Update, spawn_quest_item_popup);
        app.world.send_event(QuestItemAcquiredEvent(QuestItemKind("eternal_gem")));
        app.update();
        assert_eq!(app.world.query::<&QuestItemPopup>().iter(&app.world).count(), 0);
    }

    // ── close_quest_item_popup ──
    fn close_popup_app() -> App {
        let mut app = App::new();
        app.insert_resource(ButtonInput::<KeyCode>::default())
            .add_systems(Update, close_quest_item_popup);
        app
    }

    #[test]
    fn 팝업이_없으면_닫기_시스템은_아무일도_하지_않는다() {
        let mut app = close_popup_app();
        app.world.spawn((Player, Transform::default()));
        app.update(); // 팝업 없음 → 즉시 반환
    }

    #[test]
    fn ESC를_누르면_퀘스트아이템_팝업이_닫힌다() {
        let mut app = close_popup_app();
        app.world.spawn((Player, Transform::default()));
        let p = app.world.spawn(QuestItemPopup { tile_x: 0, tile_y: 0 }).id();
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
        app.update();
        assert!(app.world.get_entity(p).is_none());
    }

    #[test]
    fn 플레이어가_타일을_벗어나면_팝업이_닫힌다() {
        let mut app = close_popup_app();
        app.world.spawn((Player, Transform::from_translation(tile_to_world_coords(0, 0).extend(0.0))));
        let p = app.world.spawn(QuestItemPopup { tile_x: 5, tile_y: 5 }).id();
        app.update();
        assert!(app.world.get_entity(p).is_none(), "다른 타일로 이동 시 닫힘");
    }

    #[test]
    fn 플레이어가_같은_타일에_있으면_팝업이_유지된다() {
        let mut app = close_popup_app();
        app.world.spawn((Player, Transform::from_translation(tile_to_world_coords(3, 3).extend(0.0))));
        let p = app.world.spawn(QuestItemPopup { tile_x: 3, tile_y: 3 }).id();
        app.update();
        assert!(app.world.get_entity(p).is_some(), "같은 타일이면 유지");
    }

    #[test]
    fn x는_같지만_y가_다르면_팝업이_닫힌다() {
        // px == tile_x 이지만 py != tile_y → `px != tile_x || py != tile_y` 의 우변을 True 로 평가.
        let mut app = close_popup_app();
        app.world.spawn((Player, Transform::from_translation(tile_to_world_coords(3, 3).extend(0.0))));
        let p = app.world.spawn(QuestItemPopup { tile_x: 3, tile_y: 8 }).id();
        app.update();
        assert!(app.world.get_entity(p).is_none(), "x 같고 y 다르면 닫힘");
    }

    #[test]
    fn 팝업이_있어도_플레이어가_없으면_닫기는_조용히_반환한다() {
        // ESC 없이 팝업만 있고 플레이어가 없으면 get_single Err → 조용히 반환 (panic 없음).
        let mut app = close_popup_app();
        let p = app.world.spawn(QuestItemPopup { tile_x: 3, tile_y: 3 }).id();
        app.update();
        assert!(app.world.get_entity(p).is_some(), "플레이어 없으면 팝업 유지");
    }

    // ── 나머지 분기 보강 (display_name / default / id / heal / 로드아웃 seam / 플러그인) ──

    #[test]
    fn 글리프스타일의_표시이름이_종류별로_올바르다() {
        assert_eq!(GlyphStyle::Ascii.display_name(),    "ASCII");
        assert_eq!(GlyphStyle::Unicode.display_name(),  "유니코드");
        assert_eq!(GlyphStyle::GameIcon.display_name(), "RPG 아이콘");
    }

    #[test]
    fn 글리프설정의_기본값은_ascii이다() {
        assert_eq!(GlyphConfig::default().style, GlyphStyle::Ascii);
    }

    #[test]
    fn 각_아이템종류의_id가_내부식별자를_돌려준다() {
        assert_eq!(WeaponKind::SWORD.id(), "sword");
        assert_eq!(ArmorKind::LEATHER_ARMOR.id(), "leather_armor");
        assert_eq!(ConsumableKind::HEALTH_POTION.id(), "health_potion");
        assert_eq!(QuestItemKind("eternal_gem").id(), "eternal_gem");
    }

    #[test]
    fn 미등록_소비아이템의_회복량은_0이다() {
        assert_eq!(ConsumableKind("nope").heal_amount(qi()), 0);
    }

    #[test]
    fn 시작로드아웃은_유효한_무기와_방어구를_장착한다() {
        let mut inv = PlayerInventory::default();
        let mut eq = PlayerEquipment::default();
        let loadout = StartLoadout {
            gold: 10,
            weapon: Some("sword".into()),
            armor: Some("leather_armor".into()),
            items: vec![],
            consumables: vec![],
        };
        apply_start_loadout(&mut inv, &mut eq, &loadout, qi());
        assert_eq!(eq.weapon, Some(WeaponKind::SWORD));
        assert_eq!(eq.armor, Some(ArmorKind::LEATHER_ARMOR));
    }

    #[test]
    fn 시작로드아웃의_알수없는_방어구id는_장착하지_않는다() {
        let mut inv = PlayerInventory::default();
        let mut eq = PlayerEquipment::default();
        let loadout = StartLoadout {
            gold: 0,
            weapon: None,
            armor: Some("ghost_armor".into()),
            items: vec![],
            consumables: vec![],
        };
        apply_start_loadout(&mut inv, &mut eq, &loadout, qi());
        assert!(eq.armor.is_none());
    }

    #[test]
    fn 시작로드아웃파일을_정상적으로_읽는다() {
        let l = read_start_loadout("assets/items/start_loadout.ron");
        assert!(!l.items.is_empty(), "실제 파일엔 시작 무기들이 있다");
    }

    #[test]
    fn 없는_시작로드아웃파일은_기본값으로_폴백한다() {
        let l = read_start_loadout("/no/such/start_loadout_xyz.ron");
        assert_eq!(l.gold, 50);
        assert!(l.items.is_empty());
    }

    #[test]
    fn 깨진_시작로드아웃파일은_기본값으로_폴백한다() {
        let p = std::env::temp_dir().join("bevy_rogue_bad_loadout_test.ron");
        std::fs::write(&p, "not valid ron {{{ <<<").unwrap();
        let l = read_start_loadout(p.to_str().unwrap());
        assert_eq!(l.gold, 50);
        assert!(l.items.is_empty());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn 세이브가_없으면_로드아웃을_적용한다() {
        let mut inv = PlayerInventory::default();
        let mut eq = PlayerEquipment::default();
        let loadout = StartLoadout {
            gold: 99, weapon: None, armor: None,
            items: vec!["sword".into()], consumables: vec![],
        };
        apply_loadout_unless_save(&mut inv, &mut eq, &loadout, qi(), "/no/such/save_path.ron");
        assert_eq!(inv.gold, 99);
        assert_eq!(inv.items.len(), 1);
    }

    #[test]
    fn 세이브가_있으면_로드아웃을_적용하지_않는다() {
        let p = std::env::temp_dir().join("bevy_rogue_fake_save_test.ron");
        std::fs::write(&p, "x").unwrap();
        let mut inv = PlayerInventory::default();
        let before = inv.gold;
        let mut eq = PlayerEquipment::default();
        let loadout = StartLoadout {
            gold: 99, weapon: None, armor: None,
            items: vec!["sword".into()], consumables: vec![],
        };
        apply_loadout_unless_save(&mut inv, &mut eq, &loadout, qi(), p.to_str().unwrap());
        assert_eq!(inv.gold, before, "세이브가 있으면 로드아웃 미적용");
        assert!(inv.items.is_empty());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn 무세이브_로드아웃_시스템이_패닉없이_실행된다() {
        let mut app = App::new();
        app.insert_resource(build_test_registry())
            .init_resource::<PlayerInventory>()
            .init_resource::<PlayerEquipment>()
            .init_resource::<StartLoadoutRegistry>()
            .add_systems(Update, apply_start_loadout_if_no_save);
        app.update();
    }

    #[test]
    fn 아이템플러그인이_정상적으로_빌드된다() {
        let mut app = App::new();
        app.add_plugins(ItemPlugin::default());
    }

    // ── 레어도 등급 (Rarity::from_roll) — §2 백분위 경계 ─────────────────────

    #[test]
    fn 최저롤은_일반등급이다() {
        // p = 0.0 < 0.40 → Common
        assert_eq!(Rarity::from_roll(0, 0, 100), Rarity::Common);
    }

    #[test]
    fn 백분위_40미만은_일반_40이상은_고급이다() {
        // 경계 0.40: 미만(39%) Common, 정확히 40% Uncommon.
        assert_eq!(Rarity::from_roll(39, 0, 100), Rarity::Common);
        assert_eq!(Rarity::from_roll(40, 0, 100), Rarity::Uncommon);
    }

    #[test]
    fn 백분위_70미만은_고급_70이상은_희귀이다() {
        assert_eq!(Rarity::from_roll(69, 0, 100), Rarity::Uncommon);
        assert_eq!(Rarity::from_roll(70, 0, 100), Rarity::Rare);
    }

    #[test]
    fn 백분위_90미만은_희귀_90이상은_영웅이다() {
        assert_eq!(Rarity::from_roll(89, 0, 100), Rarity::Rare);
        assert_eq!(Rarity::from_roll(90, 0, 100), Rarity::Epic);
    }

    #[test]
    fn 백분위_98미만은_영웅_98이상은_전설이다() {
        assert_eq!(Rarity::from_roll(97, 0, 100), Rarity::Epic);
        assert_eq!(Rarity::from_roll(98, 0, 100), Rarity::Legendary);
    }

    #[test]
    fn 최고롤은_전설등급이다() {
        // p = 1.0 → Legendary
        assert_eq!(Rarity::from_roll(100, 0, 100), Rarity::Legendary);
    }

    #[test]
    fn 최소최대가_같으면_백분위_0으로_일반등급이다() {
        // min == max → 분모 0, 백분위 0 으로 처리(Common).
        assert_eq!(Rarity::from_roll(5, 5, 5), Rarity::Common);
    }

    #[test]
    fn 각_레어도의_한글이름이_정확하다() {
        assert_eq!(Rarity::Common.name_ko(),    "일반");
        assert_eq!(Rarity::Uncommon.name_ko(),  "고급");
        assert_eq!(Rarity::Rare.name_ko(),      "희귀");
        assert_eq!(Rarity::Epic.name_ko(),      "영웅");
        assert_eq!(Rarity::Legendary.name_ko(), "전설");
    }

    #[test]
    fn 각_레어도의_색이_스펙과_일치한다() {
        assert_eq!(Rarity::Common.color(),    Color::rgb(0.7, 0.7, 0.7));
        assert_eq!(Rarity::Uncommon.color(),  Color::rgb(0.3, 0.9, 0.3));
        assert_eq!(Rarity::Rare.color(),      Color::rgb(0.3, 0.5, 1.0));
        assert_eq!(Rarity::Epic.color(),      Color::rgb(0.7, 0.3, 1.0));
        assert_eq!(Rarity::Legendary.color(), Color::rgb(1.0, 0.8, 0.2));
    }

    // ── weapon_rarity / armor_rarity ────────────────────────────────────────

    #[test]
    fn 무기_레어도는_롤값과_무기범위로_계산된다() {
        // 검 5~9. 롤 5(최저)→일반, 롤 9(최고)→전설.
        assert_eq!(weapon_rarity(WeaponKind::SWORD, 5, qi()), Some(Rarity::Common));
        assert_eq!(weapon_rarity(WeaponKind::SWORD, 9, qi()), Some(Rarity::Legendary));
    }

    #[test]
    fn 방어구_레어도는_롤값과_방어구범위로_계산된다() {
        // 가죽 2~4. 롤 2→일반, 롤 4→전설.
        assert_eq!(armor_rarity(ArmorKind::LEATHER_ARMOR, 2, qi()), Some(Rarity::Common));
        assert_eq!(armor_rarity(ArmorKind::LEATHER_ARMOR, 4, qi()), Some(Rarity::Legendary));
    }

    #[test]
    fn 미등록_무기방어구의_레어도는_없음이다() {
        assert_eq!(weapon_rarity(WeaponKind("nope"), 5, qi()), None);
        assert_eq!(armor_rarity(ArmorKind("nope"), 5, qi()), None);
    }

    // ── roll_item_stats: 범위 내 롤 ────────────────────────────────────────

    #[test]
    fn 무기_롤은_무기범위_안의_공격력만_방어는_없음을_반환한다() {
        let mut rng = rand::thread_rng();
        let m = qi().weapon(WeaponKind::SWORD).unwrap();
        for _ in 0..500 {
            let (atk, def) = roll_item_stats(ItemKind::Weapon(WeaponKind::SWORD), &mut rng, qi());
            let atk = atk.expect("무기는 공격력이 롤된다");
            assert!(atk >= m.attack_power_min, "롤 {atk} 이 최소 미만");
            assert!(atk <= m.attack_power_max, "롤 {atk} 이 최대 초과");
            assert!(def.is_none(), "무기는 방어롤이 없다");
        }
    }

    #[test]
    fn 방어구_롤은_방어범위_안의_방어보너스만_공격은_없음을_반환한다() {
        let mut rng = rand::thread_rng();
        let m = qi().armor(ArmorKind::LEATHER_ARMOR).unwrap();
        for _ in 0..500 {
            let (atk, def) = roll_item_stats(ItemKind::Armor(ArmorKind::LEATHER_ARMOR), &mut rng, qi());
            let def = def.expect("방어구는 방어보너스가 롤된다");
            assert!(def >= m.defense_bonus_min, "롤 {def} 이 최소 미만");
            assert!(def <= m.defense_bonus_max, "롤 {def} 이 최대 초과");
            assert!(atk.is_none(), "방어구는 공격롤이 없다");
        }
    }

    #[test]
    fn 소비_퀘스트아이템은_롤되지_않는다() {
        let mut rng = rand::thread_rng();
        assert_eq!(roll_item_stats(ItemKind::Consumable(ConsumableKind::HEALTH_POTION), &mut rng, qi()), (None, None));
        assert_eq!(roll_item_stats(ItemKind::QuestItem(QuestItemKind("eternal_gem")), &mut rng, qi()), (None, None));
    }

    #[test]
    fn 미등록_무기방어구_롤은_없음을_반환한다() {
        let mut rng = rand::thread_rng();
        assert_eq!(roll_item_stats(ItemKind::Weapon(WeaponKind("nope")), &mut rng, qi()), (None, None));
        assert_eq!(roll_item_stats(ItemKind::Armor(ArmorKind("nope")), &mut rng, qi()), (None, None));
    }

    // ── 레벨 스케일 드롭: tier_band_center (§7-B) ──────────────────────────────

    #[test]
    fn 티어밴드중심은_3레벨마다_한단계씩_오른다() {
        // (1 + (L-1)/3).clamp(1,5): 1~3→1, 4~6→2, 7~9→3, 10~12→4.
        assert_eq!(tier_band_center(1), 1);
        assert_eq!(tier_band_center(3), 1, "3레벨까지는 여전히 중심 1");
        assert_eq!(tier_band_center(4), 2, "4레벨에서 중심 2 로 상승");
        assert_eq!(tier_band_center(6), 2);
        assert_eq!(tier_band_center(7), 3, "7레벨에서 중심 3");
        assert_eq!(tier_band_center(10), 4);
    }

    #[test]
    fn 티어밴드중심은_상한_5로_클램프된다() {
        // 13레벨에서 raw=5, 그 이상은 5 로 클램프.
        assert_eq!(tier_band_center(13), 5);
        assert_eq!(tier_band_center(100), 5, "고레벨도 상한 5");
    }

    // ── 레벨 스케일 드롭: tier_weight 각 d 구간 (§7-B 표) ──────────────────────

    #[test]
    fn 티어가_중심보다_2이상_높으면_가중치는_0이다() {
        // center(레벨1)=1. tier 3 → d=+2 → 0.0 (게이트). tier 5 → d=+4 → 0.0.
        assert_eq!(tier_weight(3, 1), 0.0);
        assert_eq!(tier_weight(5, 1), 0.0);
    }

    #[test]
    fn 티어가_중심보다_한단계_높으면_가중치는_절반이다() {
        // center(레벨1)=1. tier 2 → d=+1 → 0.5.
        assert_eq!(tier_weight(2, 1), 0.5);
    }

    #[test]
    fn 티어가_중심과_같으면_가중치는_최대1이다() {
        // center(레벨1)=1. tier 1 → d=0 → 1.0.
        assert_eq!(tier_weight(1, 1), 1.0);
    }

    #[test]
    fn 티어가_중심보다_한단계_낮으면_가중치는_06이다() {
        // center(레벨4)=2. tier 1 → d=-1 → 0.6.
        assert_eq!(tier_weight(1, 4), 0.6);
    }

    #[test]
    fn 티어가_중심보다_두단계_낮으면_가중치는_03이다() {
        // center(레벨7)=3. tier 1 → d=-2 → 0.3.
        assert_eq!(tier_weight(1, 7), 0.3);
    }

    #[test]
    fn 티어가_중심보다_세단계_이상_낮으면_가중치는_01로_줄어든다() {
        // center(레벨10)=4. tier 1 → d=-3 → 0.1. center(레벨13)=5 → tier 1 d=-4 → 0.1.
        assert_eq!(tier_weight(1, 10), 0.1);
        assert_eq!(tier_weight(1, 13), 0.1);
    }

    // ── 레벨 스케일 드롭: weighted_tier_pick ──────────────────────────────────

    #[test]
    fn 가중추첨은_양수가중치_티어만_고른다() {
        // 레벨1 중심1. 후보 [1,2,3,4,5] → 1(1.0),2(0.5) 만 양수, 3+ 는 0.
        let mut rng = rand::thread_rng();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..2000 {
            let t = weighted_tier_pick(1, &[1, 2, 3, 4, 5], &mut rng).expect("양수 가중치 존재");
            seen.insert(t);
        }
        assert!(seen.contains(&1), "적정 티어 1 은 뽑혀야 한다");
        assert!(seen.contains(&2), "한 단계 위 티어 2 도 가끔 뽑힌다");
        assert!(!seen.contains(&3), "게이트된 티어 3 은 절대 안 뽑힌다");
        assert!(!seen.contains(&4));
        assert!(!seen.contains(&5));
    }

    #[test]
    fn 모든_후보가_게이트되면_가중추첨은_없음이다() {
        // 레벨1 중심1. 후보가 전부 d>=+2(가중치 0) → 합 0 → None.
        let mut rng = rand::thread_rng();
        assert_eq!(weighted_tier_pick(1, &[3, 4, 5], &mut rng), None);
    }

    #[test]
    fn 후보가_비어있으면_가중추첨은_없음이다() {
        let mut rng = rand::thread_rng();
        assert_eq!(weighted_tier_pick(1, &[], &mut rng), None);
    }

    #[test]
    fn 단일_티어만_있으면_항상_그_티어를_고른다() {
        let mut rng = rand::thread_rng();
        for _ in 0..50 {
            assert_eq!(weighted_tier_pick(1, &[1], &mut rng), Some(1));
        }
    }

    #[test]
    fn 마지막_catch_all_후보도_정상적으로_선택된다() {
        // 레벨1 양수 가중치 후보 [1(1.0), 2(0.5)]. 충분히 추첨하면 앞 후보의
        // `roll < w` 가 모두 False 인 경우(마지막 catch-all 2 선택)도 반드시 발생한다.
        let mut rng = rand::thread_rng();
        let mut saw_first = false;
        let mut saw_last = false;
        for _ in 0..2000 {
            let t = weighted_tier_pick(1, &[1, 2], &mut rng).unwrap();
            if t == 1 { saw_first = true; }
            if t == 2 { saw_last = true; }
        }
        assert!(saw_first, "앞 후보(티어1) 선택 경로");
        assert!(saw_last, "마지막 catch-all(티어2) 선택 경로");
    }

    // ── 레벨 스케일 드롭: pick_leveled_weapon / armor ─────────────────────────

    #[test]
    fn 저레벨_플레이어는_고티어_무기를_받지_못한다() {
        // 레벨1 중심1. T3+ 무기(d>=+2)는 가중치 0 → 절대 안 나온다.
        let mut rng = rand::thread_rng();
        for _ in 0..3000 {
            let w = pick_leveled_weapon(1, qi(), &mut rng).expect("저레벨도 T1/T2 는 나온다");
            let tier = qi().weapon(w).unwrap().tier;
            assert!(tier <= 2, "레벨1 에서 T{tier} 무기가 나오면 안 된다");
        }
    }

    #[test]
    fn 저레벨_플레이어는_고티어_방어구를_받지_못한다() {
        let mut rng = rand::thread_rng();
        for _ in 0..3000 {
            let a = pick_leveled_armor(1, qi(), &mut rng).expect("저레벨도 T1/T2 는 나온다");
            let tier = qi().armor(a).unwrap().tier;
            assert!(tier <= 2, "레벨1 에서 T{tier} 방어구가 나오면 안 된다");
        }
    }

    #[test]
    fn 무기가_없는_레지스트리에서는_레벨드롭이_없음이다() {
        // 후보 티어가 비어 weighted_tier_pick 이 None → pick_leveled_weapon None.
        let empty = ItemRegistry::default();
        let mut rng = rand::thread_rng();
        assert_eq!(pick_leveled_weapon(1, &empty, &mut rng), None);
    }

    #[test]
    fn 방어구가_없는_레지스트리에서는_레벨드롭이_없음이다() {
        let empty = ItemRegistry::default();
        let mut rng = rand::thread_rng();
        assert_eq!(pick_leveled_armor(1, &empty, &mut rng), None);
    }

    #[test]
    fn 고레벨_플레이어는_저티어_무기비중이_크게_감소한다() {
        // 레벨13 중심5. T1(d=-4)=0.1 vs T5(d=0)=1.0 → T1 빈도 ≪ T5 빈도.
        let mut rng = rand::thread_rng();
        let mut t1 = 0;
        let mut t5 = 0;
        for _ in 0..5000 {
            let w = pick_leveled_weapon(13, qi(), &mut rng).unwrap();
            match qi().weapon(w).unwrap().tier {
                1 => t1 += 1,
                5 => t5 += 1,
                _ => {}
            }
        }
        assert!(t5 > t1 * 3, "고레벨에선 T5({t5}) 가 T1({t1}) 보다 훨씬 흔해야 한다");
    }

    // ── 레벨 스케일 드롭: resolve_drops (드롭 흐름) ───────────────────────────

    #[test]
    fn 드롭_해소는_저레벨에서_고티어_장비를_절대_떨구지_않는다() {
        // 트롤(무기/방어구 드롭 확률 높음) × 레벨1 → 장비는 T1/T2 만 나온다.
        let mut rng = rand::thread_rng();
        let mut weapon_drops = 0;
        for _ in 0..2000 {
            for (kind, _, _) in resolve_drops("트롤", 1, qi(), &mut rng) {
                match kind {
                    ItemKind::Weapon(w) => {
                        weapon_drops += 1;
                        assert!(qi().weapon(w).unwrap().tier <= 2, "레벨1 장비드롭은 T1/T2");
                    }
                    ItemKind::Armor(a) => {
                        assert!(qi().armor(a).unwrap().tier <= 2, "레벨1 장비드롭은 T1/T2");
                    }
                    ItemKind::Consumable(_) | ItemKind::QuestItem(_) => {}
                }
            }
        }
        assert!(weapon_drops > 0, "충분한 시도면 무기 드롭이 발생한다");
    }

    #[test]
    fn 드롭_해소는_무기드롭시_공격력을_롤하고_방어구드롭시_방어를_롤한다() {
        // 장비가 떨어지면 Phase1 의 roll_item_stats 로 스탯이 채워진다.
        let mut rng = rand::thread_rng();
        let mut saw_weapon_roll = false;
        let mut saw_armor_roll = false;
        for _ in 0..3000 {
            for (kind, atk, def) in resolve_drops("트롤", 1, qi(), &mut rng) {
                match kind {
                    ItemKind::Weapon(_) => {
                        assert!(atk.is_some(), "무기는 공격력이 롤된다");
                        assert!(def.is_none(), "무기는 방어롤이 없다");
                        saw_weapon_roll = true;
                    }
                    ItemKind::Armor(_) => {
                        assert!(def.is_some(), "방어구는 방어가 롤된다");
                        assert!(atk.is_none(), "방어구는 공격롤이 없다");
                        saw_armor_roll = true;
                    }
                    // 트롤은 무기/방어구/포션만 드롭 — 포션은 롤 없음.
                    _ => {
                        assert!(atk.is_none(), "포션은 공격롤 없음");
                        assert!(def.is_none(), "포션은 방어롤 없음");
                    }
                }
            }
        }
        assert!(saw_weapon_roll, "무기 드롭 + 공격력 롤 확인");
        assert!(saw_armor_roll, "방어구 드롭 + 방어 롤 확인");
    }

    #[test]
    fn 장비가_없는_레지스트리면_장비카테고리_드롭은_건너뛴다() {
        // pick_leveled_weapon/armor 가 None → resolve_drops 의 `None => continue` 분기.
        // 빈 레지스트리라 무기/방어구는 못 만들고 포션만 나올 수 있다.
        let empty = ItemRegistry::default();
        let mut rng = rand::thread_rng();
        for _ in 0..2000 {
            for (kind, _, _) in resolve_drops("트롤", 1, &empty, &mut rng) {
                assert!(matches!(kind, ItemKind::Consumable(_)), "장비 없으면 포션만");
            }
        }
    }

    #[test]
    fn 알수없는_몬스터는_장비를_드롭하지_않고_포션만_떨군다() {
        // 기타 몬스터 테이블엔 포션만 있다 → 무기/방어구는 절대 안 나온다.
        let mut rng = rand::thread_rng();
        for _ in 0..1000 {
            for (kind, _, _) in resolve_drops("기타몬스터", 5, qi(), &mut rng) {
                assert!(
                    matches!(kind, ItemKind::Consumable(_)),
                    "기타 몬스터는 포션만 드롭",
                );
            }
        }
    }

    // ── item_display_color: 드롭 글리프 색 ─────────────────────────────────

    #[test]
    fn 롤된_무기의_표시색은_레어도색이다() {
        // 검 5~9, 롤 9 → 전설(금색).
        let c = item_display_color(ItemKind::Weapon(WeaponKind::SWORD), Some(9), None, qi());
        assert_eq!(c, Rarity::Legendary.color());
    }

    #[test]
    fn 롤된_방어구의_표시색은_레어도색이다() {
        let c = item_display_color(ItemKind::Armor(ArmorKind::LEATHER_ARMOR), None, Some(2), qi());
        assert_eq!(c, Rarity::Common.color());
    }

    #[test]
    fn 롤없는_무기방어구의_표시색은_카테고리색으로_폴백한다() {
        let w = item_display_color(ItemKind::Weapon(WeaponKind::SWORD), None, None, qi());
        assert_eq!(w, ItemKind::Weapon(WeaponKind::SWORD).color());
        let a = item_display_color(ItemKind::Armor(ArmorKind::LEATHER_ARMOR), None, None, qi());
        assert_eq!(a, ItemKind::Armor(ArmorKind::LEATHER_ARMOR).color());
    }

    #[test]
    fn 미등록_무기방어구의_표시색은_카테고리색으로_폴백한다() {
        // 롤값은 있으나 range 조회 실패 → 카테고리 색.
        let w = item_display_color(ItemKind::Weapon(WeaponKind("nope")), Some(5), None, qi());
        assert_eq!(w, ItemKind::Weapon(WeaponKind("nope")).color());
        let a = item_display_color(ItemKind::Armor(ArmorKind("nope")), None, Some(5), qi());
        assert_eq!(a, ItemKind::Armor(ArmorKind("nope")).color());
    }

    #[test]
    fn 소비_퀘스트아이템의_표시색은_항상_카테고리색이다() {
        let c = item_display_color(ItemKind::Consumable(ConsumableKind::HEALTH_POTION), None, None, qi());
        assert_eq!(c, ItemKind::Consumable(ConsumableKind::HEALTH_POTION).color());
        let q = item_display_color(ItemKind::QuestItem(QuestItemKind("eternal_gem")), None, None, qi());
        assert_eq!(q, ItemKind::QuestItem(QuestItemKind("eternal_gem")).color());
    }

    // ── effective_attack/defense: 롤값 우선 ─────────────────────────────────

    #[test]
    fn 롤값이_있으면_유효공격력은_롤값을_쓴다() {
        let eq = PlayerEquipment {
            weapon: Some(WeaponKind::SWORD), armor: None,
            weapon_rolled_attack: Some(9), armor_rolled_defense: None,
        };
        assert_eq!(effective_attack(&eq, qi()), 9, "중앙값 7 이 아니라 롤값 9");
    }

    #[test]
    fn 롤값이_있으면_유효방어력은_롤값을_더한다() {
        let eq = PlayerEquipment {
            weapon: None, armor: Some(ArmorKind::LEATHER_ARMOR),
            weapon_rolled_attack: None, armor_rolled_defense: Some(4),
        };
        assert_eq!(effective_defense(&eq, qi()), PLAYER_DEF + 4, "중앙값 3 이 아니라 롤값 4");
    }

    // ── pickup_items: rolled 이전 ──────────────────────────────────────────

    #[test]
    fn 주운_무기는_롤된_공격력을_인벤토리로_이전한다() {
        let mut app = pickup_app();
        spawn_player_at(&mut app, 5, 5);
        app.world.spawn(Item {
            kind: ItemKind::Weapon(WeaponKind::SWORD), tile_x: 5, tile_y: 5,
            rolled_attack: Some(8), rolled_defense: None,
        });
        app.world.send_event(PlayerActedEvent);
        app.update();
        let inv = app.world.resource::<PlayerInventory>();
        assert_eq!(inv.items[0].rolled_attack, Some(8), "드롭 롤값이 인벤토리로 이전됨");
    }

    #[test]
    fn 주운_방어구는_롤된_방어보너스를_인벤토리로_이전한다() {
        let mut app = pickup_app();
        spawn_player_at(&mut app, 5, 5);
        app.world.spawn(Item {
            kind: ItemKind::Armor(ArmorKind::LEATHER_ARMOR), tile_x: 5, tile_y: 5,
            rolled_attack: None, rolled_defense: Some(4),
        });
        app.world.send_event(PlayerActedEvent);
        app.update();
        let inv = app.world.resource::<PlayerInventory>();
        assert_eq!(inv.items[0].rolled_defense, Some(4));
    }

    // ── 세이브 호환: rolled 없는 구 데이터 역직렬화 ─────────────────────────

    #[test]
    fn rolled필드없는_구_인벤토리아이템도_역직렬화된다() {
        // 구 세이브: rolled_attack/rolled_defense 필드가 없는 RON → serde default 로 None.
        let old = "(kind: Weapon(\"sword\"))";
        let parsed: InventoryItem = ron::de::from_str(old).expect("구 InventoryItem 역직렬화");
        assert!(matches!(parsed.kind, ItemKind::Weapon(WeaponKind("sword"))));
        assert_eq!(parsed.rolled_attack, None);
        assert_eq!(parsed.rolled_defense, None);
    }

    #[test]
    fn rolled필드없는_구_장비도_역직렬화된다() {
        let old = "(weapon: Some(\"sword\"), armor: None)";
        let parsed: PlayerEquipment = ron::de::from_str(old).expect("구 PlayerEquipment 역직렬화");
        assert_eq!(parsed.weapon, Some(WeaponKind::SWORD));
        assert_eq!(parsed.weapon_rolled_attack, None);
        assert_eq!(parsed.armor_rolled_defense, None);
    }

    #[test]
    fn rolled필드포함_인벤토리아이템은_직렬화_왕복이_보존된다() {
        let item = InventoryItem { kind: ItemKind::Weapon(WeaponKind::SWORD), rolled_attack: Some(8), rolled_defense: None };
        let s = ron::ser::to_string(&item).unwrap();
        let parsed: InventoryItem = ron::de::from_str(&s).unwrap();
        assert_eq!(parsed.rolled_attack, Some(8));
    }

    #[test]
    fn 생성자로_만든_인벤토리아이템은_롤값이_없다() {
        let item = InventoryItem::new(ItemKind::Weapon(WeaponKind::SWORD));
        assert_eq!(item.rolled_attack, None);
        assert_eq!(item.rolled_defense, None);
    }
}
