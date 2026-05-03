use bevy::prelude::*;
use rand::Rng;
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

pub fn glyph_for_style(kind: ItemKind, style: GlyphStyle) -> &'static str {
    match style {
        GlyphStyle::Ascii    => kind.glyph(),
        GlyphStyle::Unicode  => glyph_unicode(kind),
        GlyphStyle::GameIcon => glyph_game_icon(kind),
    }
}

fn glyph_unicode(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Weapon(w) => match w {
            WeaponKind::Sword => "\u{1F5E1}", // 🗡 dagger
            WeaponKind::Spear => "\u{2B06}",  // ⬆ upward arrow
            WeaponKind::Bow   => "\u{27A4}",  // ➤ arrowhead right
        },
        ItemKind::Armor(a) => match a {
            ArmorKind::LeatherArmor => "\u{1F6E1}", // 🛡 shield
        },
        ItemKind::Consumable(c) => match c {
            ConsumableKind::HealthPotion => "\u{2764}", // ❤ heavy heart
        },
        ItemKind::QuestItem(q) => match q {
            QuestItemKind::EternalGem        => "\u{25C6}", // ◆ black diamond
            QuestItemKind::PhilosophersStone => "\u{2295}", // ⊕ circled plus
            QuestItemKind::DragonScale       => "\u{25B2}", // ▲ triangle
            QuestItemKind::AncientScroll     => "\u{2393}", // ⎓ scroll-like
            QuestItemKind::PrologueGreatsword => "\u{2694}", // ⚔ crossed swords
            QuestItemKind::PrologueDaggers    => "\u{25B8}", // ▸ small right triangle
            QuestItemKind::PrologueBowTorch   => "\u{2600}", // ☀ fire/sun
            QuestItemKind::FamilyCrest        => "\u{269C}", // ⚜ fleur-de-lis
            QuestItemKind::IceSword           => "\u{2746}", // ❆ snowflake
            QuestItemKind::DragonEgg          => "\u{25CE}", // ◎ bullseye
            QuestItemKind::GhostWolf          => "\u{25D4}", // ◔ circle arc
            QuestItemKind::LordsOath          => "\u{2709}", // ✉ envelope
            QuestItemKind::JaimeSword         => "\u{2020}", // † dagger
            QuestItemKind::KingsNorthCrown    => "\u{265A}", // ♚ king chess piece
            QuestItemKind::WarlockKey         => "\u{2318}", // ⌘ command/key
            QuestItemKind::DragonChain        => "\u{26D3}", // ⛓ chains
            QuestItemKind::EssosSailMap       => "\u{2742}", // ❂ compass-like
            QuestItemKind::DragonglassArrows  => "\u{25B6}", // ▶ solid arrow
            QuestItemKind::RangersNote        => "\u{2767}", // ❧ floral heart
            QuestItemKind::YgrittesBow        => "\u{2640}", // ♀ (ygritte's symbol)
        },
    }
}

fn glyph_game_icon(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Weapon(w) => match w {
            WeaponKind::Sword => "\u{E946}", // ra-broadsword
            WeaponKind::Spear => "\u{EAAC}", // ra-spear-head
            WeaponKind::Bow   => "\u{E978}", // ra-crossbow
        },
        ItemKind::Armor(a) => match a {
            ArmorKind::LeatherArmor => "\u{EA96}", // ra-shield
        },
        ItemKind::Consumable(c) => match c {
            ConsumableKind::HealthPotion => "\u{EA72}", // ra-potion
        },
        ItemKind::QuestItem(q) => match q {
            QuestItemKind::EternalGem        => "\u{25C6}", // fallback: ◆
            QuestItemKind::PhilosophersStone => "\u{2295}", // fallback: ⊕
            QuestItemKind::DragonScale       => "\u{25B2}", // fallback: ▲
            QuestItemKind::AncientScroll     => "\u{2393}", // fallback: ⎓
            QuestItemKind::PrologueGreatsword => "\u{2694}", // fallback: ⚔
            QuestItemKind::PrologueDaggers    => "\u{25B8}", // fallback: ▸
            QuestItemKind::PrologueBowTorch   => "\u{2600}", // fallback: ☀
            QuestItemKind::FamilyCrest        => "\u{269C}", // fallback: ⚜
            QuestItemKind::IceSword           => "\u{2746}", // fallback: ❆
            QuestItemKind::DragonEgg          => "\u{25CE}", // fallback: ◎
            QuestItemKind::GhostWolf          => "\u{25D4}", // fallback: ◔
            QuestItemKind::LordsOath          => "\u{2709}", // fallback: ✉
            QuestItemKind::JaimeSword         => "\u{2020}", // fallback: †
            QuestItemKind::KingsNorthCrown    => "\u{265A}", // fallback: ♚
            QuestItemKind::WarlockKey         => "\u{2318}", // fallback: ⌘
            QuestItemKind::DragonChain        => "\u{26D3}", // fallback: ⛓
            QuestItemKind::EssosSailMap       => "\u{2742}", // fallback: ❂
            QuestItemKind::DragonglassArrows  => "\u{25B6}", // fallback: ▶
            QuestItemKind::RangersNote        => "\u{2767}", // fallback: ❧
            QuestItemKind::YgrittesBow        => "\u{2640}", // fallback: ♀
        },
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuestItemKind {
    // world_fracture 퀘스트
    EternalGem,
    PhilosophersStone,
    DragonScale,
    AncientScroll,
    // prologue_fog 퀘스트 — 무기 선택
    PrologueGreatsword,
    PrologueDaggers,
    PrologueBowTorch,
    // prologue_fog 퀘스트 — 스토리 아이템
    FamilyCrest,
    // prologue_fog 퀘스트 — 각성 보상
    IceSword,
    DragonEgg,
    GhostWolf,
    // stark_quest — 전쟁의 서막
    LordsOath,
    JaimeSword,
    KingsNorthCrown,
    // targaryen_quest — 재생의 불꽃
    WarlockKey,
    DragonChain,
    EssosSailMap,
    // jon_snow_quest — 장벽 너머의 그림자
    DragonglassArrows,
    RangersNote,
    YgrittesBow,
}

impl QuestItemKind {
    pub fn display_name(self) -> &'static str {
        match self {
            QuestItemKind::EternalGem        => "영원의 보석",
            QuestItemKind::PhilosophersStone => "현자의 돌",
            QuestItemKind::DragonScale       => "용비늘",
            QuestItemKind::AncientScroll     => "고대 주문서",
            QuestItemKind::PrologueGreatsword => "대검",
            QuestItemKind::PrologueDaggers    => "단검과 투척물",
            QuestItemKind::PrologueBowTorch   => "부러진 활과 횃불",
            QuestItemKind::FamilyCrest        => "가문 문장 유물",
            QuestItemKind::IceSword           => "아이스",
            QuestItemKind::DragonEgg          => "용의 알",
            QuestItemKind::GhostWolf          => "고스트",
            QuestItemKind::LordsOath          => "충성 서약서",
            QuestItemKind::JaimeSword         => "제이미의 검",
            QuestItemKind::KingsNorthCrown    => "북부의 왕관",
            QuestItemKind::WarlockKey         => "마법사의 열쇠",
            QuestItemKind::DragonChain        => "드래곤 족쇄",
            QuestItemKind::EssosSailMap       => "에소스 항로도",
            QuestItemKind::DragonglassArrows  => "드래곤스톤 화살촉",
            QuestItemKind::RangersNote        => "죽은 레인저의 메모",
            QuestItemKind::YgrittesBow        => "이그리트의 활",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemKind {
    Weapon(WeaponKind),
    Armor(ArmorKind),
    Consumable(ConsumableKind),
    QuestItem(QuestItemKind),
}

impl ItemKind {
    pub fn glyph(self) -> &'static str {
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
            ItemKind::QuestItem(q) => match q {
                QuestItemKind::EternalGem        => "*",
                QuestItemKind::PhilosophersStone => "%",
                QuestItemKind::DragonScale       => "§",
                QuestItemKind::AncientScroll     => "~",
                QuestItemKind::PrologueGreatsword => "/",
                QuestItemKind::PrologueDaggers    => ")",
                QuestItemKind::PrologueBowTorch   => "}",
                QuestItemKind::FamilyCrest        => "^",
                QuestItemKind::IceSword           => "!",
                QuestItemKind::DragonEgg          => "o",
                QuestItemKind::GhostWolf          => "w",
                QuestItemKind::LordsOath          => "=",
                QuestItemKind::JaimeSword         => "|",
                QuestItemKind::KingsNorthCrown    => "&",
                QuestItemKind::WarlockKey         => "k",
                QuestItemKind::DragonChain        => "8",
                QuestItemKind::EssosSailMap       => "m",
                QuestItemKind::DragonglassArrows  => ">",
                QuestItemKind::RangersNote        => "n",
                QuestItemKind::YgrittesBow        => "q",
            },
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

    pub fn display_name(self) -> &'static str {
        match self {
            ItemKind::Weapon(w)     => w.display_name(),
            ItemKind::Armor(a)      => a.display_name(),
            ItemKind::Consumable(c) => c.display_name(),
            ItemKind::QuestItem(q)  => q.display_name(),
        }
    }

    pub fn pickup_message(self) -> &'static str {
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
            ItemKind::QuestItem(q) => match q {
                QuestItemKind::EternalGem        => "영원의 보석을 획득했다!",
                QuestItemKind::PhilosophersStone => "현자의 돌을 획득했다!",
                QuestItemKind::DragonScale       => "용비늘을 획득했다!",
                QuestItemKind::AncientScroll     => "고대 주문서를 획득했다!",
                QuestItemKind::PrologueGreatsword => "대검을 집어들었다. 손에 익숙하게 맞는다.",
                QuestItemKind::PrologueDaggers    => "단검과 투척물을 집었다. 가볍고 빠르다.",
                QuestItemKind::PrologueBowTorch   => "부러진 활과 횃불을 집었다. 거리가 곧 생명이다.",
                QuestItemKind::FamilyCrest        => "오래된 문장 유물을 발견했다. 어디선가 본 것 같다...",
                QuestItemKind::IceSword           => "아이스 — 스타크 가문의 검이 손 안에서 차갑게 빛난다.",
                QuestItemKind::DragonEgg          => "용의 알이 손바닥 위에서 뜨겁게 맥박친다.",
                QuestItemKind::GhostWolf          => "하얀 늑대 고스트가 곁에 나타났다.",
                QuestItemKind::LordsOath          => "충성 서약서를 받았다. 북부의 힘이 결집되고 있다.",
                QuestItemKind::JaimeSword         => "제이미 라니스터의 검을 손에 넣었다. 전장의 증거.",
                QuestItemKind::KingsNorthCrown    => "북부의 왕관. 이제 돌아올 수 없는 길에 섰다.",
                QuestItemKind::WarlockKey         => "마법사의 열쇠. 차갑고 이상한 냄새가 난다.",
                QuestItemKind::DragonChain        => "드래곤들을 묶었던 족쇄. 이제 그들은 자유롭다.",
                QuestItemKind::EssosSailMap       => "에소스 항로도. 정복의 시작점이 표시되어 있다.",
                QuestItemKind::DragonglassArrows  => "드래곤스톤 화살촉 다발. 와이트를 멈추는 유일한 방법.",
                QuestItemKind::RangersNote        => "죽은 레인저의 메모. 떨리는 손으로 쓴 마지막 경고.",
                QuestItemKind::YgrittesBow        => "이그리트의 활. 그녀는 항상 당신보다 빨리 쏜다.",
            },
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

#[derive(Clone, Debug)]
pub struct InventoryItem {
    pub kind: ItemKind,
}

#[derive(Resource, Default)]
pub struct PlayerInventory {
    pub items: Vec<InventoryItem>,
    pub consumables: Vec<(ConsumableKind, u32)>,
}

impl PlayerInventory {
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

#[derive(Resource, Default)]
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
struct QuestItemPopup;

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
            .add_systems(Startup, setup_glyph_fonts)
            .add_systems(Update, (
                spawn_dropped_items,
                pickup_items.after(PlayerSystemSet::Movement),
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
) {
    let mut rng = rand::thread_rng();
    for event in events.read() {
        for &(kind, rate) in monster_drop_table(&event.monster_name) {
            if rng.gen::<f32>() >= rate { continue; }
            let pos = tile_to_world_coords(event.tile_x, event.tile_y);
            commands.spawn((
                Text2dBundle {
                    text: Text::from_section(
                        glyph_for_style(kind, config.style),
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
    mut item_query: Query<(&Item, &mut Text)>,
) {
    if !config.is_changed() { return; }
    for (item, mut text) in item_query.iter_mut() {
        text.sections[0].value = glyph_for_style(item.kind, config.style).to_string();
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
        log.send(LogMessage(kind.pickup_message().to_string()));
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

fn quest_item_image_path(kind: QuestItemKind) -> &'static str {
    match kind {
        QuestItemKind::EternalGem        => "scene/open-chest.png",
        QuestItemKind::PhilosophersStone => "scene/open-chest.png",
        QuestItemKind::DragonScale       => "scene/open-chest.png",
        QuestItemKind::AncientScroll     => "scene/open-chest.png",
        QuestItemKind::PrologueGreatsword => "scene/open-chest.png",
        QuestItemKind::PrologueDaggers    => "scene/open-chest.png",
        QuestItemKind::PrologueBowTorch   => "scene/open-chest.png",
        QuestItemKind::FamilyCrest        => "scene/open-chest.png",
        QuestItemKind::IceSword           => "scene/open-chest.png",
        QuestItemKind::DragonEgg          => "scene/open-chest.png",
        QuestItemKind::GhostWolf          => "scene/open-chest.png",
        QuestItemKind::LordsOath          => "scene/open-chest.png",
        QuestItemKind::JaimeSword         => "scene/open-chest.png",
        QuestItemKind::KingsNorthCrown    => "scene/open-chest.png",
        QuestItemKind::WarlockKey         => "scene/open-chest.png",
        QuestItemKind::DragonChain        => "scene/open-chest.png",
        QuestItemKind::EssosSailMap       => "scene/open-chest.png",
        QuestItemKind::DragonglassArrows  => "scene/open-chest.png",
        QuestItemKind::RangersNote        => "scene/open-chest.png",
        QuestItemKind::YgrittesBow        => "scene/open-chest.png",
    }
}

fn spawn_quest_item_popup(
    mut commands: Commands,
    mut events: EventReader<QuestItemAcquiredEvent>,
    asset_server: Res<AssetServer>,
    popup_q: Query<(), With<QuestItemPopup>>,
) {
    for QuestItemAcquiredEvent(kind) in events.read() {
        if !popup_q.is_empty() { continue; }
        let image = asset_server.load(quest_item_image_path(*kind));
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
            QuestItemPopup,
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
}

fn close_quest_item_popup(
    mut commands: Commands,
    mut acted: EventReader<PlayerActedEvent>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    popup_q: Query<Entity, With<QuestItemPopup>>,
    mut seen: Local<bool>,
) {
    let Ok(entity) = popup_q.get_single() else {
        *seen = false; // 팝업 없으면 상태 초기화
        return;
    };

    // Escape 는 언제든 즉시 닫기
    if keyboard_input.just_pressed(KeyCode::Escape) {
        commands.entity(entity).despawn_recursive();
        *seen = false;
        return;
    }

    if !*seen {
        // 팝업이 처음 보이는 프레임 — 픽업 프레임의 오래된 acted 이벤트를 비워 무시
        *seen = true;
        acted.clear();
        return;
    }

    // 다음 프레임부터 플레이어가 행동하면 팝업 닫기
    if acted.read().next().is_some() {
        commands.entity(entity).despawn_recursive();
        *seen = false;
    }
}

/// DespawnWorldItemEvent 를 받아 월드에 있는 해당 아이템 엔티티를 제거한다
fn handle_despawn_world_item(
    mut events: EventReader<DespawnWorldItemEvent>,
    item_query: Query<(Entity, &Item)>,
    mut commands: Commands,
) {
    for DespawnWorldItemEvent(item_id) in events.read() {
        let Some(kind) = item_id_to_kind(item_id) else { continue };
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

    #[test]
    fn glyph_for_style_ascii_returns_ascii_chars() {
        assert_eq!(glyph_for_style(ItemKind::Weapon(WeaponKind::Sword), GlyphStyle::Ascii), "/");
        assert_eq!(glyph_for_style(ItemKind::Weapon(WeaponKind::Spear), GlyphStyle::Ascii), "|");
        assert_eq!(glyph_for_style(ItemKind::Weapon(WeaponKind::Bow),   GlyphStyle::Ascii), ")");
    }

    #[test]
    fn glyph_for_style_unicode_returns_symbols() {
        let s = glyph_for_style(ItemKind::Weapon(WeaponKind::Sword), GlyphStyle::Unicode);
        assert_eq!(s, "\u{1F5E1}");
        let shield = glyph_for_style(ItemKind::Armor(ArmorKind::LeatherArmor), GlyphStyle::Unicode);
        assert_eq!(shield, "\u{1F6E1}");
    }

    #[test]
    fn glyph_for_style_game_icon_returns_pua_codepoints() {
        let s = glyph_for_style(ItemKind::Weapon(WeaponKind::Sword), GlyphStyle::GameIcon);
        assert_eq!(s, "\u{E946}");
        let potion = glyph_for_style(ItemKind::Consumable(ConsumableKind::HealthPotion), GlyphStyle::GameIcon);
        assert_eq!(potion, "\u{EA72}");
    }

    #[test]
    fn glyph_style_default_is_ascii() {
        assert_eq!(GlyphStyle::default(), GlyphStyle::Ascii);
    }

    #[test]
    fn quest_item_display_names() {
        assert_eq!(QuestItemKind::EternalGem.display_name(), "영원의 보석");
        assert_eq!(QuestItemKind::PhilosophersStone.display_name(), "현자의 돌");
    }

    #[test]
    fn quest_item_glyph_and_pickup_message() {
        let gem = ItemKind::QuestItem(QuestItemKind::EternalGem);
        assert_eq!(gem.glyph(), "*");
        assert_eq!(gem.pickup_message(), "영원의 보석을 획득했다!");
        let stone = ItemKind::QuestItem(QuestItemKind::PhilosophersStone);
        assert_eq!(stone.pickup_message(), "현자의 돌을 획득했다!");
    }
}
