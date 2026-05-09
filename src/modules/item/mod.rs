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
        ItemKind::QuestItem(q) => match q {
            QuestItemKind::EternalGem        => "\u{25C6}", // ◆ 검은 마름모
            QuestItemKind::PhilosophersStone => "\u{2295}", // ⊕ 원 안의 더하기
            QuestItemKind::DragonScale       => "\u{25B2}", // ▲ 삼각형
            QuestItemKind::AncientScroll     => "\u{2393}", // ⎓ 두루마리 느낌
            QuestItemKind::PrologueGreatsword => "\u{2694}", // ⚔ 교차한 검
            QuestItemKind::PrologueDaggers    => "\u{25B8}", // ▸ 작은 오른쪽 삼각형
            QuestItemKind::PrologueBowTorch   => "\u{2600}", // ☀ 불/태양
            QuestItemKind::FamilyCrest        => "\u{269C}", // ⚜ 백합 문장
            QuestItemKind::IceSword           => "\u{2746}", // ❆ 눈송이
            QuestItemKind::DragonEgg          => "\u{25CE}", // ◎ 과녁 모양
            QuestItemKind::GhostWolf          => "\u{25D4}", // ◔ 원호 모양
            QuestItemKind::LordsOath          => "\u{2709}", // ✉ 봉투
            QuestItemKind::JaimeSword         => "\u{2020}", // † 단검 기호
            QuestItemKind::KingsNorthCrown    => "\u{265A}", // ♚ 체스 왕 말
            QuestItemKind::WarlockKey         => "\u{2318}", // ⌘ 명령/열쇠 느낌
            QuestItemKind::DragonChain        => "\u{26D3}", // ⛓ 사슬
            QuestItemKind::EssosSailMap       => "\u{2742}", // ❂ 나침반 느낌
            QuestItemKind::DragonglassArrows  => "\u{25B6}", // ▶ 채워진 화살표
            QuestItemKind::RangersNote        => "\u{2767}", // ❧ 장식 하트
            QuestItemKind::YgrittesBow        => "\u{2640}", // ♀ 이그리트 상징
            QuestItemKind::SilverBellRoot    => "\u{2698}", // ⚘ 꽃
            QuestItemKind::EllenElixir       => "\u{2697}", // ⚗ 증류기
            QuestItemKind::PoisonedHerb      => "\u{2620}", // ☠ 해골
            QuestItemKind::DemonSword        => "\u{2694}", // ⚔ 교차한 검 (마검)
            QuestItemKind::ElenasMemo        => "\u{270E}", // ✎ 연필 (메모)
            QuestItemKind::AncientRitualBook => "\u{2720}", // ✠ 몰타 십자 (의식서)
            QuestItemKind::PrototypeHammer   => "\u{2692}", // ⚒ 곡괭이 (파암추)
            QuestItemKind::SteelCore         => "\u{2B22}", // ⬢ 육각형 (강철 심장)
            QuestItemKind::PilotBadge        => "\u{2605}", // ★ 별 (인증서)
        },
    }
}

fn glyph_game_icon(kind: ItemKind) -> &'static str {
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
        ItemKind::QuestItem(q) => match q {
            QuestItemKind::EternalGem        => "\u{25C6}", // 대체 글리프: ◆
            QuestItemKind::PhilosophersStone => "\u{2295}", // 대체 글리프: ⊕
            QuestItemKind::DragonScale       => "\u{25B2}", // 대체 글리프: ▲
            QuestItemKind::AncientScroll     => "\u{2393}", // 대체 글리프: ⎓
            QuestItemKind::PrologueGreatsword => "\u{2694}", // 대체 글리프: ⚔
            QuestItemKind::PrologueDaggers    => "\u{25B8}", // 대체 글리프: ▸
            QuestItemKind::PrologueBowTorch   => "\u{2600}", // 대체 글리프: ☀
            QuestItemKind::FamilyCrest        => "\u{269C}", // 대체 글리프: ⚜
            QuestItemKind::IceSword           => "\u{2746}", // 대체 글리프: ❆
            QuestItemKind::DragonEgg          => "\u{25CE}", // 대체 글리프: ◎
            QuestItemKind::GhostWolf          => "\u{25D4}", // 대체 글리프: ◔
            QuestItemKind::LordsOath          => "\u{2709}", // 대체 글리프: ✉
            QuestItemKind::JaimeSword         => "\u{2020}", // 대체 글리프: †
            QuestItemKind::KingsNorthCrown    => "\u{265A}", // 대체 글리프: ♚
            QuestItemKind::WarlockKey         => "\u{2318}", // 대체 글리프: ⌘
            QuestItemKind::DragonChain        => "\u{26D3}", // 대체 글리프: ⛓
            QuestItemKind::EssosSailMap       => "\u{2742}", // 대체 글리프: ❂
            QuestItemKind::DragonglassArrows  => "\u{25B6}", // 대체 글리프: ▶
            QuestItemKind::RangersNote        => "\u{2767}", // 대체 글리프: ❧
            QuestItemKind::YgrittesBow        => "\u{2640}", // 대체 글리프: ♀
            QuestItemKind::SilverBellRoot    => "\u{2698}", // 대체 글리프: ⚘
            QuestItemKind::EllenElixir       => "\u{2697}", // 대체 글리프: ⚗
            QuestItemKind::PoisonedHerb      => "\u{2620}", // 대체 글리프: ☠
            QuestItemKind::DemonSword        => "\u{2694}", // 대체 글리프: ⚔
            QuestItemKind::ElenasMemo        => "\u{270E}", // 대체 글리프: ✎
            QuestItemKind::AncientRitualBook => "\u{2720}", // 대체 글리프: ✠
            QuestItemKind::PrototypeHammer   => "\u{2692}", // 대체 글리프: ⚒
            QuestItemKind::SteelCore         => "\u{2B22}", // 대체 글리프: ⬢
            QuestItemKind::PilotBadge        => "\u{2605}", // 대체 글리프: ★
        },
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum QuestItemKind {
    // 세계 균열 퀘스트
    EternalGem,
    PhilosophersStone,
    DragonScale,
    AncientScroll,
    // 안개 프롤로그 퀘스트 — 무기 선택
    PrologueGreatsword,
    PrologueDaggers,
    PrologueBowTorch,
    // 안개 프롤로그 퀘스트 — 스토리 아이템
    FamilyCrest,
    // 안개 프롤로그 퀘스트 — 각성 보상
    IceSword,
    DragonEgg,
    GhostWolf,
    // 스타크 퀘스트 — 전쟁의 서막
    LordsOath,
    JaimeSword,
    KingsNorthCrown,
    // 타르가르옌 퀘스트 — 재생의 불꽃
    WarlockKey,
    DragonChain,
    EssosSailMap,
    // 존 스노우 퀘스트 — 장벽 너머의 그림자
    DragonglassArrows,
    RangersNote,
    YgrittesBow,
    // 약초 퀘스트 — 은방울 뿌리 채집
    SilverBellRoot,
    EllenElixir,
    PoisonedHerb,
    // 마검 퀘스트 — 성기사가 마검을 들다
    DemonSword,
    ElenasMemo,
    AncientRitualBook,
    // 파리 퀘스트 — 파리 고수와 기계 공학자
    PrototypeHammer,
    SteelCore,
    PilotBadge,
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
            QuestItemKind::SilverBellRoot    => "은방울 뿌리",
            QuestItemKind::EllenElixir       => "엘렌의 특제 영약",
            QuestItemKind::PoisonedHerb      => "독초",
            QuestItemKind::DemonSword        => "마검",
            QuestItemKind::ElenasMemo        => "엘레나의 메모",
            QuestItemKind::AncientRitualBook => "고대 의식서",
            QuestItemKind::PrototypeHammer   => "시제 6식 파암추",
            QuestItemKind::SteelCore         => "강철 갑주 심장",
            QuestItemKind::PilotBadge        => "전속 파일럿 인증서",
        }
    }
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
                QuestItemKind::SilverBellRoot    => ";",
                QuestItemKind::EllenElixir       => "&",
                QuestItemKind::PoisonedHerb      => "?",
                QuestItemKind::DemonSword        => "D",
                QuestItemKind::ElenasMemo        => "e",
                QuestItemKind::AncientRitualBook => "R",
                QuestItemKind::PrototypeHammer   => "H",
                QuestItemKind::SteelCore         => "#",
                QuestItemKind::PilotBadge        => "P",
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
                QuestItemKind::SilverBellRoot    => "은방울 뿌리를 채집했다. 맑은 향기가 퍼진다.",
                QuestItemKind::EllenElixir       => "엘렌의 특제 영약. 온 몸에 활력이 흐른다.",
                QuestItemKind::PoisonedHerb      => "독초를 발견했다. 조심해야 할 것 같다.",
                QuestItemKind::DemonSword        => "마검을 집어들었다. 손에서 어둠의 기운이 흐른다...",
                QuestItemKind::ElenasMemo        => "엘레나의 메모를 발견했다. 폐허 요새에 의식서가 있다고 한다.",
                QuestItemKind::AncientRitualBook => "고대 의식서를 손에 넣었다. 봉인 의식의 방법이 담겨 있다.",
                QuestItemKind::PrototypeHammer   => "시제 6식 파암추를 받았다. 내부에 화약식 파일뱅커가 내장되어 있다.",
                QuestItemKind::SteelCore         => "강철 갑주의 심장을 획득했다. 보스 격파의 확실한 증거다.",
                QuestItemKind::PilotBadge        => "전속 파일럿 인증서를 받았다. 그레체의 무기 테스터가 되었다.",
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
            .add_systems(Startup, setup_glyph_fonts)
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
        QuestItemKind::SilverBellRoot    => "scene/open-chest.png",
        QuestItemKind::EllenElixir       => "scene/open-chest.png",
        QuestItemKind::PoisonedHerb      => "scene/open-chest.png",
        QuestItemKind::DemonSword        => "scene/open-chest.png",
        QuestItemKind::ElenasMemo        => "scene/open-chest.png",
        QuestItemKind::AncientRitualBook => "scene/open-chest.png",
        QuestItemKind::PrototypeHammer   => "scene/open-chest.png",
        QuestItemKind::SteelCore         => "scene/open-chest.png",
        QuestItemKind::PilotBadge        => "scene/open-chest.png",
    }
}

fn spawn_quest_item_popup(
    mut commands: Commands,
    mut events: EventReader<QuestItemAcquiredEvent>,
    asset_server: Res<AssetServer>,
    popup_q: Query<(), With<QuestItemPopup>>,
    player_q: Query<(Option<&MovingTo>, &Transform), With<Player>>,
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

    #[test]
    fn demonsword_items_have_correct_glyphs_and_names() {
        assert_eq!(QuestItemKind::DemonSword.display_name(), "마검");
        assert_eq!(QuestItemKind::ElenasMemo.display_name(), "엘레나의 메모");
        assert_eq!(QuestItemKind::AncientRitualBook.display_name(), "고대 의식서");

        let sword = ItemKind::QuestItem(QuestItemKind::DemonSword);
        let memo  = ItemKind::QuestItem(QuestItemKind::ElenasMemo);
        let book  = ItemKind::QuestItem(QuestItemKind::AncientRitualBook);

        assert_eq!(sword.glyph(), "D");
        assert_eq!(memo.glyph(),  "e");
        assert_eq!(book.glyph(),  "R");

        assert!(sword.pickup_message().contains("마검"));
        assert!(memo.pickup_message().contains("폐허 요새"));
        assert!(book.pickup_message().contains("봉인 의식"));
    }

    #[test]
    fn parry_quest_items_have_correct_glyphs_and_names() {
        assert_eq!(QuestItemKind::PrototypeHammer.display_name(), "시제 6식 파암추");
        assert_eq!(QuestItemKind::SteelCore.display_name(),       "강철 갑주 심장");
        assert_eq!(QuestItemKind::PilotBadge.display_name(),      "전속 파일럿 인증서");

        let hammer = ItemKind::QuestItem(QuestItemKind::PrototypeHammer);
        let core   = ItemKind::QuestItem(QuestItemKind::SteelCore);
        let badge  = ItemKind::QuestItem(QuestItemKind::PilotBadge);

        assert_eq!(hammer.glyph(), "H");
        assert_eq!(core.glyph(),   "#");
        assert_eq!(badge.glyph(),  "P");

        assert!(hammer.pickup_message().contains("파암추"));
        assert!(core.pickup_message().contains("보스 격파"));
        assert!(badge.pickup_message().contains("파일럿"));
    }

    #[test]
    fn demonsword_items_unicode_glyphs() {
        let sword = glyph_for_style(ItemKind::QuestItem(QuestItemKind::DemonSword), GlyphStyle::Unicode);
        assert_eq!(sword, "\u{2694}");
        let memo = glyph_for_style(ItemKind::QuestItem(QuestItemKind::ElenasMemo), GlyphStyle::Unicode);
        assert_eq!(memo, "\u{270E}");
        let book = glyph_for_style(ItemKind::QuestItem(QuestItemKind::AncientRitualBook), GlyphStyle::Unicode);
        assert_eq!(book, "\u{2720}");
    }
}
