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

pub fn weapon_element(kind: WeaponKind) -> Option<Element> {
    match kind {
        WeaponKind::Sword => Some(Element::Fire),
        WeaponKind::Spear => Some(Element::Ice),
        WeaponKind::Bow   => Some(Element::Lightning),
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
    fn reaction_fire_ice_is_melt() {
        assert_eq!(Reaction::from_pair(Element::Fire, Element::Ice), Some(Reaction::Melt));
        assert_eq!(Reaction::from_pair(Element::Ice, Element::Fire), Some(Reaction::Melt));
    }

    #[test]
    fn reaction_shatter_both_orders() {
        assert_eq!(Reaction::from_pair(Element::Ice, Element::Lightning), Some(Reaction::Shatter));
        assert_eq!(Reaction::from_pair(Element::Lightning, Element::Ice), Some(Reaction::Shatter));
    }

    #[test]
    fn reaction_same_element_is_none() {
        assert_eq!(Reaction::from_pair(Element::Fire, Element::Fire), None);
    }

    #[test]
    fn weapon_sword_gives_fire() {
        assert_eq!(weapon_element(WeaponKind::Sword), Some(Element::Fire));
    }

    #[test]
    fn weapon_bow_gives_lightning() {
        assert_eq!(weapon_element(WeaponKind::Bow), Some(Element::Lightning));
    }

    #[test]
    fn weapon_spear_gives_ice() {
        assert_eq!(weapon_element(WeaponKind::Spear), Some(Element::Ice));
    }

    #[test]
    fn monster_goblin_gives_poison() {
        assert_eq!(monster_element("고블린"), Some(Element::Poison));
    }

    #[test]
    fn monster_orc_gives_fire() {
        assert_eq!(monster_element("오크"), Some(Element::Fire));
    }

    #[test]
    fn monster_troll_gives_ice() {
        assert_eq!(monster_element("트롤"), Some(Element::Ice));
    }

    #[test]
    fn element_duration_is_positive() {
        for el in [Element::Fire, Element::Ice, Element::Poison, Element::Lightning] {
            assert!(el.default_duration() > 0);
        }
    }
}
