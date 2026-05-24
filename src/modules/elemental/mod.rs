use bevy::prelude::*;
use std::collections::HashMap;
use crate::modules::{
    combat::{CombatStats, Defeated, Speed},
    item::WeaponKind,
    map::PlayerActedEvent,
    player::Player,
    ui::LogMessage,
};

pub struct ElementalPlugin;

impl Plugin for ElementalPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<ElementalApplyEvent>()
            .add_systems(Update, (
                apply_elements,
                process_elemental_turns,
                tick_stunned,
            ).chain());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Element {
    Fire,
    Ice,
    Poison,
    Lightning,
}

impl Element {
    pub fn name_ko(self) -> &'static str {
        match self {
            Element::Fire      => "불",
            Element::Ice       => "얼음",
            Element::Poison    => "독",
            Element::Lightning => "번개",
        }
    }

    fn default_duration(self) -> u32 {
        match self {
            Element::Fire      => 3,
            Element::Ice       => 4,
            Element::Poison    => 5,
            Element::Lightning => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reaction {
    Melt,
    ToxicFume,
    Shatter,
    Frostbite,
    Plasma,
    Electrotoxin,
}

impl Reaction {
    fn name_ko(self) -> &'static str {
        match self {
            Reaction::Melt         => "융해",
            Reaction::ToxicFume    => "독기",
            Reaction::Shatter      => "파쇄",
            Reaction::Frostbite    => "동상",
            Reaction::Plasma       => "플라즈마",
            Reaction::Electrotoxin => "전기독",
        }
    }

    fn from_pair(a: Element, b: Element) -> Option<Reaction> {
        match (a, b) {
            (Element::Fire, Element::Ice) | (Element::Ice, Element::Fire)
                => Some(Reaction::Melt),
            (Element::Fire, Element::Poison) | (Element::Poison, Element::Fire)
                => Some(Reaction::ToxicFume),
            (Element::Ice, Element::Lightning) | (Element::Lightning, Element::Ice)
                => Some(Reaction::Shatter),
            (Element::Ice, Element::Poison) | (Element::Poison, Element::Ice)
                => Some(Reaction::Frostbite),
            (Element::Fire, Element::Lightning) | (Element::Lightning, Element::Fire)
                => Some(Reaction::Plasma),
            (Element::Poison, Element::Lightning) | (Element::Lightning, Element::Poison)
                => Some(Reaction::Electrotoxin),
            _ => None,
        }
    }
}

#[derive(Component, Default)]
pub struct ElementalStatus {
    pub active: HashMap<Element, u32>,
}

#[derive(Component)]
pub struct Stunned {
    pub turns: u32,
}

#[derive(Component)]
pub struct Slowed {
    pub original_speed: f32,
}

#[derive(Event)]
pub struct ElementalApplyEvent {
    pub target: Entity,
    pub element: Element,
}

pub fn weapon_element(kind: WeaponKind, items: &crate::modules::item::ItemRegistry) -> Option<Element> {
    let meta = items.weapon(kind)?;
    match meta.element? {
        "fire"      => Some(Element::Fire),
        "ice"       => Some(Element::Ice),
        "lightning" => Some(Element::Lightning),
        _           => None,
    }
}

pub fn monster_element(name: &str) -> Option<Element> {
    match name {
        "고블린" => Some(Element::Poison),
        "오크"   => Some(Element::Fire),
        "트롤"   => Some(Element::Ice),
        _ => None,
    }
}

fn apply_elements(
    mut events: EventReader<ElementalApplyEvent>,
    mut commands: Commands,
    mut query: Query<(
        &mut ElementalStatus,
        &mut CombatStats,
        Option<&mut Speed>,
        Option<&Slowed>,
        Has<Player>,
    )>,
    mut log_writer: EventWriter<LogMessage>,
) {
    for ev in events.read() {
        let Ok((mut status, mut stats, speed_opt, slowed_opt, is_player)) =
            query.get_mut(ev.target) else { continue };

        let reaction_pair = status.active.keys().copied()
            .find_map(|existing| {
                Reaction::from_pair(existing, ev.element).map(|r| (r, existing))
            });

        if let Some((reaction, existing)) = reaction_pair {
            status.active.remove(&existing);
            status.active.remove(&ev.element);

            let target = if is_player { "당신" } else { "적" };

            match reaction {
                Reaction::Melt => {
                    stats.hp -= 15;
                    log_writer.send(LogMessage(format!(
                        "[{}] 불+얼음 → {}에게 15 피해!", reaction.name_ko(), target
                    )));
                }
                Reaction::ToxicFume => {
                    stats.hp -= 10;
                    log_writer.send(LogMessage(format!(
                        "[{}] 불+독 → {}에게 10 피해!", reaction.name_ko(), target
                    )));
                }
                Reaction::Shatter => {
                    stats.hp -= 20;
                    log_writer.send(LogMessage(format!(
                        "[{}] 얼음+번개 → {}에게 20 피해!", reaction.name_ko(), target
                    )));
                }
                Reaction::Frostbite => {
                    stats.hp -= 5;
                    if !is_player {
                        commands.entity(ev.target).insert(Stunned { turns: 2 });
                    }
                    log_writer.send(LogMessage(format!(
                        "[{}] 얼음+독 → {}에게 5 피해 + 2턴 행동불능!", reaction.name_ko(), target
                    )));
                }
                Reaction::Plasma => {
                    stats.hp -= 8;
                    if !is_player {
                        commands.entity(ev.target).insert(Stunned { turns: 3 });
                    }
                    log_writer.send(LogMessage(format!(
                        "[{}] 불+번개 → {}에게 8 피해 + 3턴 행동불능!", reaction.name_ko(), target
                    )));
                }
                Reaction::Electrotoxin => {
                    stats.hp -= 8;
                    status.active.insert(Element::Poison, 6);
                    log_writer.send(LogMessage(format!(
                        "[{}] 독+번개 → {}에게 8 피해 + 강화 독(6턴)!", reaction.name_ko(), target
                    )));
                }
            }

            if is_player && stats.hp <= 0 {
                commands.entity(ev.target).insert(Defeated);
                log_writer.send(LogMessage("원소 반응으로 사망했습니다...".to_string()));
            }
        } else {
            let was_new = !status.active.contains_key(&ev.element);
            status.active.insert(ev.element, ev.element.default_duration());

            if ev.element == Element::Ice && was_new && slowed_opt.is_none() {
                if let Some(mut speed) = speed_opt {
                    let original = speed.value;
                    speed.value = (original * 0.5).max(0.25);
                    commands.entity(ev.target).insert(Slowed { original_speed: original });
                }
            }

            let target = if is_player { "당신" } else { "적" };
            log_writer.send(LogMessage(format!(
                "{} 속성 → {} ({}턴)", ev.element.name_ko(), target, ev.element.default_duration()
            )));
        }
    }
}

fn process_elemental_turns(
    mut commands: Commands,
    mut turn_events: EventReader<PlayerActedEvent>,
    mut query: Query<(Entity, &mut ElementalStatus, &mut CombatStats, Option<&mut Speed>, Option<&Slowed>, Has<Player>)>,
    mut log_writer: EventWriter<LogMessage>,
) {
    if turn_events.read().next().is_none() { return; }

    for (entity, mut status, mut stats, mut speed_opt, slowed_opt, is_player) in query.iter_mut() {
        let mut fire_dmg = 0i32;
        let mut poison_dmg = 0i32;
        let mut ice_expired = false;

        status.active.retain(|&element, turns| {
            *turns = turns.saturating_sub(1);
            match element {
                Element::Fire   => fire_dmg += 2,
                Element::Poison => poison_dmg += 1,
                _ => {}
            }
            if *turns == 0 {
                if element == Element::Ice { ice_expired = true; }
                false
            } else {
                true
            }
        });

        let total_dot = fire_dmg + poison_dmg;
        if total_dot > 0 {
            stats.hp -= total_dot;
            if is_player {
                let mut parts = Vec::new();
                if fire_dmg > 0 { parts.push(format!("불 -{}", fire_dmg)); }
                if poison_dmg > 0 { parts.push(format!("독 -{}", poison_dmg)); }
                log_writer.send(LogMessage(format!(
                    "원소 피해: {} (HP {}/{})", parts.join(", "), stats.hp, stats.max_hp
                )));
                if stats.hp <= 0 {
                    commands.entity(entity).insert(Defeated);
                    log_writer.send(LogMessage("원소 피해로 사망했습니다...".to_string()));
                }
            }
        }

        if ice_expired {
            if let Some(slowed) = slowed_opt {
                if let Some(ref mut speed) = speed_opt {
                    speed.value = slowed.original_speed;
                }
                commands.entity(entity).remove::<Slowed>();
            }
        }
    }
}

fn tick_stunned(
    mut commands: Commands,
    mut turn_events: EventReader<PlayerActedEvent>,
    mut query: Query<(Entity, &mut Stunned)>,
) {
    if turn_events.read().next().is_none() { return; }

    for (entity, mut stunned) in query.iter_mut() {
        stunned.turns = stunned.turns.saturating_sub(1);
        if stunned.turns == 0 {
            commands.entity(entity).remove::<Stunned>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 불과_얼음이_만나면_융해반응이_일어난다() {
        assert_eq!(Reaction::from_pair(Element::Fire, Element::Ice), Some(Reaction::Melt));
        assert_eq!(Reaction::from_pair(Element::Ice, Element::Fire), Some(Reaction::Melt));
    }

    #[test]
    fn 얼음과_번개는_순서와_무관하게_파쇄반응이_일어난다() {
        assert_eq!(Reaction::from_pair(Element::Ice, Element::Lightning), Some(Reaction::Shatter));
        assert_eq!(Reaction::from_pair(Element::Lightning, Element::Ice), Some(Reaction::Shatter));
    }

    #[test]
    fn 같은_원소끼리는_반응하지_않는다() {
        assert_eq!(Reaction::from_pair(Element::Fire, Element::Fire), None);
    }

    #[test]
    fn 검은_화염_속성을_가진다() {
        let r = crate::modules::item::build_test_registry();
        assert_eq!(weapon_element(WeaponKind::SWORD, &r), Some(Element::Fire));
    }

    #[test]
    fn 활은_번개_속성을_가진다() {
        let r = crate::modules::item::build_test_registry();
        assert_eq!(weapon_element(WeaponKind::BOW, &r), Some(Element::Lightning));
    }

    #[test]
    fn 창은_얼음_속성을_가진다() {
        let r = crate::modules::item::build_test_registry();
        assert_eq!(weapon_element(WeaponKind::SPEAR, &r), Some(Element::Ice));
    }

    #[test]
    fn 고블린은_독_속성을_가진다() {
        assert_eq!(monster_element("고블린"), Some(Element::Poison));
    }

    #[test]
    fn 오크는_화염_속성을_가진다() {
        assert_eq!(monster_element("오크"), Some(Element::Fire));
    }

    #[test]
    fn 트롤은_얼음_속성을_가진다() {
        assert_eq!(monster_element("트롤"), Some(Element::Ice));
    }

    #[test]
    fn 모든_원소의_기본_지속시간은_양수이다() {
        for el in [Element::Fire, Element::Ice, Element::Poison, Element::Lightning] {
            assert!(el.default_duration() > 0);
        }
    }

    // ── 추가: 순수 로직 분기 커버리지 ───────────────────────────────────────

    #[test]
    fn 모든_원소의_한글이름이_올바르게_반환된다() {
        assert_eq!(Element::Fire.name_ko(),      "불");
        assert_eq!(Element::Ice.name_ko(),       "얼음");
        assert_eq!(Element::Poison.name_ko(),    "독");
        assert_eq!(Element::Lightning.name_ko(), "번개");
    }

    #[test]
    fn 각_원소의_기본_지속시간_값이_정확하다() {
        assert_eq!(Element::Fire.default_duration(),      3);
        assert_eq!(Element::Ice.default_duration(),       4);
        assert_eq!(Element::Poison.default_duration(),    5);
        assert_eq!(Element::Lightning.default_duration(), 1);
    }

    #[test]
    fn 모든_반응의_한글이름이_올바르게_반환된다() {
        assert_eq!(Reaction::Melt.name_ko(),         "융해");
        assert_eq!(Reaction::ToxicFume.name_ko(),    "독기");
        assert_eq!(Reaction::Shatter.name_ko(),      "파쇄");
        assert_eq!(Reaction::Frostbite.name_ko(),    "동상");
        assert_eq!(Reaction::Plasma.name_ko(),       "플라즈마");
        assert_eq!(Reaction::Electrotoxin.name_ko(), "전기독");
    }

    #[test]
    fn 모든_원소조합이_순서와_무관하게_올바른_반응을_만든다() {
        use Element::*;
        let cases = [
            (Fire, Ice,        Reaction::Melt),
            (Fire, Poison,     Reaction::ToxicFume),
            (Ice, Lightning,   Reaction::Shatter),
            (Ice, Poison,      Reaction::Frostbite),
            (Fire, Lightning,  Reaction::Plasma),
            (Poison, Lightning,Reaction::Electrotoxin),
        ];
        for (a, b, expected) in cases {
            assert_eq!(Reaction::from_pair(a, b), Some(expected), "{a:?}+{b:?}");
            assert_eq!(Reaction::from_pair(b, a), Some(expected), "{b:?}+{a:?} (역순)");
        }
    }

    #[test]
    fn 동일한_원소_조합은_어떤_반응도_만들지_않는다() {
        use Element::*;
        for el in [Fire, Ice, Poison, Lightning] {
            assert_eq!(Reaction::from_pair(el, el), None, "{el:?} 동일 원소");
        }
    }

    fn registry_with_one_weapon(id: &'static str, element: Option<&'static str>) -> crate::modules::item::ItemRegistry {
        use crate::modules::item::{ItemRegistry, WeaponMeta};
        let mut r = ItemRegistry::default();
        r.weapons.insert(id, WeaponMeta {
            display_name: "테스트무기",
            glyph_ascii: "T", glyph_unicode: "T", glyph_game_icon: "T",
            pickup_message: "획득", attack_power_min: 5, attack_power_max: 5, tier: 1, element,
        });
        r
    }

    #[test]
    fn 속성이_없는_무기는_원소를_반환하지_않는다() {
        let r = registry_with_one_weapon("plain", None);
        assert_eq!(weapon_element(WeaponKind("plain"), &r), None);
    }

    #[test]
    fn 인식되지_않는_속성문자열은_원소를_반환하지_않는다() {
        let r = registry_with_one_weapon("weird", Some("plasma_unknown"));
        assert_eq!(weapon_element(WeaponKind("weird"), &r), None);
    }

    #[test]
    fn 등록되지_않은_무기는_원소를_반환하지_않는다() {
        let r = crate::modules::item::ItemRegistry::default();
        assert_eq!(weapon_element(WeaponKind("missing"), &r), None);
    }

    #[test]
    fn 속성문자열이_화염_얼음_번개_원소로_매핑된다() {
        assert_eq!(weapon_element(WeaponKind("f"), &registry_with_one_weapon("f", Some("fire"))),      Some(Element::Fire));
        assert_eq!(weapon_element(WeaponKind("i"), &registry_with_one_weapon("i", Some("ice"))),       Some(Element::Ice));
        assert_eq!(weapon_element(WeaponKind("l"), &registry_with_one_weapon("l", Some("lightning"))), Some(Element::Lightning));
    }

    #[test]
    fn 알수없는_몬스터는_원소를_가지지_않는다() {
        assert_eq!(monster_element("슬라임"), None);
    }

    // ── 추가: Bevy 시스템 App 하네스 테스트 (모든 분기) ──────────────────────

    fn stats(hp: i32) -> CombatStats {
        CombatStats { hp, max_hp: 100, mp: 10, max_mp: 10, attack: 5, defense: 0 }
    }

    fn status_with(elements: &[(Element, u32)]) -> ElementalStatus {
        let mut s = ElementalStatus::default();
        for &(e, t) in elements { s.active.insert(e, t); }
        s
    }

    fn apply_app() -> App {
        let mut app = App::new();
        app.add_event::<ElementalApplyEvent>()
            .add_event::<LogMessage>()
            .add_systems(Update, apply_elements);
        app
    }

    #[test]
    fn 대상_컴포넌트가_없는_엔티티의_원소이벤트는_무시된다() {
        // ElementalStatus/CombatStats 없는 엔티티 → query.get_mut Err → continue
        let mut app = apply_app();
        let e = app.world.spawn(()).id();
        app.world.send_event(ElementalApplyEvent { target: e, element: Element::Fire });
        app.update(); // panic 없이 통과
    }

    #[test]
    fn 새로운_원소는_비플레이어의_상태에_추가된다() {
        let mut app = apply_app();
        let e = app.world.spawn((ElementalStatus::default(), stats(50))).id();
        app.world.send_event(ElementalApplyEvent { target: e, element: Element::Fire });
        app.update();
        let st = app.world.get::<ElementalStatus>(e).unwrap();
        assert_eq!(st.active.get(&Element::Fire), Some(&Element::Fire.default_duration()));
    }

    #[test]
    fn 새로운_원소가_플레이어에게_적용되어도_정상_처리된다() {
        // is_player == true 인 else(비반응) 경로 (line 222 "당신")
        let mut app = apply_app();
        let e = app.world.spawn((ElementalStatus::default(), stats(50), Player)).id();
        app.world.send_event(ElementalApplyEvent { target: e, element: Element::Fire });
        app.update();
        assert!(app.world.get::<ElementalStatus>(e).unwrap().active.contains_key(&Element::Fire));
    }

    #[test]
    fn 융해반응은_15피해를_주고_두_원소를_모두_제거한다() {
        let mut app = apply_app();
        let e = app.world.spawn((status_with(&[(Element::Fire, 3)]), stats(50))).id();
        app.world.send_event(ElementalApplyEvent { target: e, element: Element::Ice });
        app.update();
        assert!(app.world.get::<ElementalStatus>(e).unwrap().active.is_empty());
        assert_eq!(app.world.get::<CombatStats>(e).unwrap().hp, 35);
    }

    #[test]
    fn 독기반응은_10피해를_준다() {
        let mut app = apply_app();
        let e = app.world.spawn((status_with(&[(Element::Fire, 3)]), stats(50))).id();
        app.world.send_event(ElementalApplyEvent { target: e, element: Element::Poison });
        app.update();
        assert_eq!(app.world.get::<CombatStats>(e).unwrap().hp, 40);
    }

    #[test]
    fn 파쇄반응은_20피해를_준다() {
        let mut app = apply_app();
        let e = app.world.spawn((status_with(&[(Element::Ice, 4)]), stats(50))).id();
        app.world.send_event(ElementalApplyEvent { target: e, element: Element::Lightning });
        app.update();
        assert_eq!(app.world.get::<CombatStats>(e).unwrap().hp, 30);
    }

    #[test]
    fn 동상반응은_비플레이어를_기절시킨다() {
        let mut app = apply_app();
        let e = app.world.spawn((status_with(&[(Element::Ice, 4)]), stats(50))).id();
        app.world.send_event(ElementalApplyEvent { target: e, element: Element::Poison });
        app.update();
        assert_eq!(app.world.get::<CombatStats>(e).unwrap().hp, 45);
        assert!(app.world.entity(e).contains::<Stunned>());
    }

    #[test]
    fn 동상반응은_플레이어를_기절시키지_않는다() {
        // is_player == true → if !is_player 거짓 경로
        let mut app = apply_app();
        let e = app.world.spawn((status_with(&[(Element::Ice, 4)]), stats(50), Player)).id();
        app.world.send_event(ElementalApplyEvent { target: e, element: Element::Poison });
        app.update();
        assert!(!app.world.entity(e).contains::<Stunned>());
    }

    #[test]
    fn 플라즈마반응은_비플레이어를_기절시킨다() {
        let mut app = apply_app();
        let e = app.world.spawn((status_with(&[(Element::Fire, 3)]), stats(50))).id();
        app.world.send_event(ElementalApplyEvent { target: e, element: Element::Lightning });
        app.update();
        assert_eq!(app.world.get::<CombatStats>(e).unwrap().hp, 42);
        assert!(app.world.entity(e).contains::<Stunned>());
    }

    #[test]
    fn 전기독반응은_강화된_독을_추가한다() {
        let mut app = apply_app();
        let e = app.world.spawn((status_with(&[(Element::Poison, 5)]), stats(50))).id();
        app.world.send_event(ElementalApplyEvent { target: e, element: Element::Lightning });
        app.update();
        assert_eq!(app.world.get::<CombatStats>(e).unwrap().hp, 42);
        assert_eq!(app.world.get::<ElementalStatus>(e).unwrap().active.get(&Element::Poison), Some(&6));
    }

    #[test]
    fn 원소반응으로_체력이_0이되면_플레이어가_사망한다() {
        // is_player && hp <= 0 → Defeated
        let mut app = apply_app();
        let e = app.world.spawn((status_with(&[(Element::Fire, 3)]), stats(10), Player)).id();
        app.world.send_event(ElementalApplyEvent { target: e, element: Element::Ice });
        app.update();
        assert!(app.world.entity(e).contains::<Defeated>());
    }

    #[test]
    fn 얼음은_속도를_가진_대상을_둔화시킨다() {
        let mut app = apply_app();
        let e = app.world.spawn((ElementalStatus::default(), stats(50), Speed::new(1.0))).id();
        app.world.send_event(ElementalApplyEvent { target: e, element: Element::Ice });
        app.update();
        assert_eq!(app.world.get::<Speed>(e).unwrap().value, 0.5);
        assert!(app.world.entity(e).contains::<Slowed>());
    }

    #[test]
    fn 얼음_둔화속도는_최소값_이하로_내려가지_않는다() {
        // (0.4 * 0.5).max(0.25) == 0.25
        let mut app = apply_app();
        let e = app.world.spawn((ElementalStatus::default(), stats(50), Speed::new(0.4))).id();
        app.world.send_event(ElementalApplyEvent { target: e, element: Element::Ice });
        app.update();
        assert_eq!(app.world.get::<Speed>(e).unwrap().value, 0.25);
    }

    #[test]
    fn 속도가_없는_대상에_얼음을_걸어도_정상_동작한다() {
        // speed_opt == None 경로
        let mut app = apply_app();
        let e = app.world.spawn((ElementalStatus::default(), stats(50))).id();
        app.world.send_event(ElementalApplyEvent { target: e, element: Element::Ice });
        app.update();
        assert!(!app.world.entity(e).contains::<Slowed>());
    }

    #[test]
    fn 이미_둔화된_대상은_얼음으로_다시_둔화되지_않는다() {
        // slowed_opt.is_none() == false 경로
        let mut app = apply_app();
        let e = app.world.spawn((
            ElementalStatus::default(), stats(50), Speed::new(0.5),
            Slowed { original_speed: 1.0 },
        )).id();
        app.world.send_event(ElementalApplyEvent { target: e, element: Element::Ice });
        app.update();
        assert_eq!(app.world.get::<Speed>(e).unwrap().value, 0.5, "이미 둔화 상태면 추가 둔화 없음");
    }

    #[test]
    fn 이미_가진_얼음을_재적용하면_둔화가_트리거되지_않는다() {
        // was_new == false 경로 (이미 Ice 보유, Speed 있으나 둔화 미적용)
        let mut app = apply_app();
        let e = app.world.spawn((status_with(&[(Element::Ice, 2)]), stats(50), Speed::new(1.0))).id();
        app.world.send_event(ElementalApplyEvent { target: e, element: Element::Ice });
        app.update();
        assert_eq!(app.world.get::<Speed>(e).unwrap().value, 1.0, "재적용은 둔화 트리거 안 함");
        assert!(!app.world.entity(e).contains::<Slowed>());
    }

    // ── process_elemental_turns ──

    fn turn_app() -> App {
        let mut app = App::new();
        app.add_event::<PlayerActedEvent>()
            .add_event::<LogMessage>()
            .add_systems(Update, process_elemental_turns);
        app
    }

    #[test]
    fn 턴_이벤트가_없으면_원소_지속처리가_일어나지_않는다() {
        let mut app = turn_app();
        let e = app.world.spawn((status_with(&[(Element::Fire, 3)]), stats(50))).id();
        app.update();
        assert_eq!(app.world.get::<CombatStats>(e).unwrap().hp, 50);
        assert_eq!(app.world.get::<ElementalStatus>(e).unwrap().active.get(&Element::Fire), Some(&3));
    }

    #[test]
    fn 화염은_매턴_플레이어에게_지속피해를_준다() {
        let mut app = turn_app();
        let e = app.world.spawn((status_with(&[(Element::Fire, 3)]), stats(50), Player)).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<CombatStats>(e).unwrap().hp, 48);
        assert_eq!(app.world.get::<ElementalStatus>(e).unwrap().active.get(&Element::Fire), Some(&2));
    }

    #[test]
    fn 독은_매턴_플레이어에게_지속피해를_준다() {
        let mut app = turn_app();
        let e = app.world.spawn((status_with(&[(Element::Poison, 5)]), stats(50), Player)).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<CombatStats>(e).unwrap().hp, 49);
    }

    #[test]
    fn 화염과_독이_함께_있으면_지속피해가_합산된다() {
        let mut app = turn_app();
        let e = app.world.spawn((status_with(&[(Element::Fire, 3), (Element::Poison, 5)]), stats(50), Player)).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<CombatStats>(e).unwrap().hp, 47);
    }

    #[test]
    fn 비플레이어도_지속피해는_받지만_사망처리는_되지_않는다() {
        // is_player == false 경로 (로그/사망처리 스킵하지만 피해는 적용)
        let mut app = turn_app();
        let e = app.world.spawn((status_with(&[(Element::Fire, 3)]), stats(1))).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<CombatStats>(e).unwrap().hp, -1);
        assert!(!app.world.entity(e).contains::<Defeated>(), "비-플레이어는 원소피해 사망마커 미부여");
    }

    #[test]
    fn 지속피해로_체력이_0이되면_플레이어가_사망한다() {
        let mut app = turn_app();
        let e = app.world.spawn((status_with(&[(Element::Fire, 3)]), stats(2), Player)).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!(app.world.entity(e).contains::<Defeated>());
    }

    #[test]
    fn 번개는_피해없이_지속시간만_소모하고_만료된다() {
        // total_dot == 0 경로 + retain `_ => {}` arm (Lightning)
        let mut app = turn_app();
        let e = app.world.spawn((status_with(&[(Element::Lightning, 1)]), stats(50), Player)).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<CombatStats>(e).unwrap().hp, 50);
        assert!(app.world.get::<ElementalStatus>(e).unwrap().active.is_empty());
    }

    #[test]
    fn 얼음이_만료되면_둔화된_속도가_원래대로_복원된다() {
        let mut app = turn_app();
        let e = app.world.spawn((
            status_with(&[(Element::Ice, 1)]), stats(50),
            Speed::new(0.5), Slowed { original_speed: 1.0 },
        )).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<Speed>(e).unwrap().value, 1.0);
        assert!(!app.world.entity(e).contains::<Slowed>());
    }

    #[test]
    fn 둔화상태가_아닌데_얼음이_만료되어도_아무일도_없다() {
        // ice_expired == true 이지만 slowed_opt == None 경로
        let mut app = turn_app();
        let e = app.world.spawn((status_with(&[(Element::Ice, 1)]), stats(50))).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!(app.world.get::<ElementalStatus>(e).unwrap().active.is_empty());
    }

    // ── tick_stunned ──

    fn stun_app() -> App {
        let mut app = App::new();
        app.add_event::<PlayerActedEvent>()
            .add_systems(Update, tick_stunned);
        app
    }

    #[test]
    fn 턴_이벤트가_없으면_기절_시간이_감소하지_않는다() {
        let mut app = stun_app();
        let e = app.world.spawn(Stunned { turns: 2 }).id();
        app.update();
        assert_eq!(app.world.get::<Stunned>(e).unwrap().turns, 2);
    }

    #[test]
    fn 기절은_매턴_감소하며_남아있으면_유지된다() {
        let mut app = stun_app();
        let e = app.world.spawn(Stunned { turns: 2 }).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<Stunned>(e).unwrap().turns, 1);
    }

    #[test]
    fn 기절이_0이되면_기절_컴포넌트가_제거된다() {
        let mut app = stun_app();
        let e = app.world.spawn(Stunned { turns: 1 }).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!(!app.world.entity(e).contains::<Stunned>());
    }

    #[test]
    fn 원소플러그인이_정상적으로_빌드된다() {
        let mut app = App::new();
        app.add_plugins(ElementalPlugin);
    }
}
