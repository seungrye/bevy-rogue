use bevy::prelude::*;
use crate::modules::{map::PlayerActedEvent, player::Player};

/// 매 턴 생존한 엔티티가 회복하는 HP 량.
pub const REGEN_PER_TURN: i32 = 1;

#[derive(Component)]
pub struct Speed {
    pub value: f32,
    pub energy: f32,
}

impl Speed {
    pub fn new(value: f32) -> Self { Speed { value, energy: 0.0 } }
}

#[derive(Component)]
pub struct CombatStats {
    pub hp: i32,
    pub max_hp: i32,
    pub mp: i32,
    pub max_mp: i32,
    pub attack: i32,
    pub defense: i32,
}

/// 플레이어 사망 마커 — 이 컴포넌트가 붙으면 이동이 차단된다
#[derive(Component)]
pub struct Defeated;

pub fn calc_damage(attack: i32, defense: i32) -> i32 {
    (attack - defense).max(1)
}

/// `hp` 에 `amount` 만큼 회복하되 `max_hp` 를 넘지 않게 클램프한다.
/// 순수 함수라 경계(이미 최대치/초과 회복)를 단독 테스트한다.
pub fn regen_hp(hp: i32, max_hp: i32, amount: i32) -> i32 {
    (hp + amount).min(max_hp)
}

/// 매 턴(`PlayerActedEvent`) 생존한 플레이어·몬스터의 HP 가 최대치 미만이면
/// `REGEN_PER_TURN` 만큼 회복한다. 사망(hp<=0)·`Defeated` 엔티티는 회복하지 않는다.
pub fn regenerate_health(
    mut events: EventReader<PlayerActedEvent>,
    mut query: Query<&mut CombatStats, Without<Defeated>>,
    player_q: Query<(), (With<Player>, With<Defeated>)>,
) {
    // 플레이어가 패배 상태면 회복 흐름 자체를 멈춘다(게임 오버).
    if !player_q.is_empty() {
        return;
    }
    // 이번 프레임에 행동 이벤트가 있을 때만 한 턴치 회복.
    if events.read().next().is_none() {
        return;
    }
    for mut stats in query.iter_mut() {
        // 죽은(hp<=0) 엔티티나 이미 최대치인 엔티티는 회복하지 않는다.
        if stats.hp <= 0 || stats.hp >= stats.max_hp {
            continue;
        }
        stats.hp = regen_hp(stats.hp, stats.max_hp, REGEN_PER_TURN);
    }
}

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, regenerate_health);
    }
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;

    #[test]
    fn 속도_에너지는_매턴_누적된다() {
        let mut s = Speed::new(0.5);
        s.energy += s.value;
        assert!(s.energy < 1.0, "0.5 속도는 첫 턴에 행동 불가");
        s.energy += s.value;
        assert!(s.energy >= 1.0, "0.5 속도는 두 번째 턴에 행동 가능");
    }

    #[test]
    fn 속도가_빠르면_한턴에_두번_행동한다() {
        let mut s = Speed::new(2.0);
        s.energy += s.value;
        let mut actions = 0;
        while s.energy >= 1.0 { s.energy -= 1.0; actions += 1; }
        assert_eq!(actions, 2, "속도 2.0은 한 턴에 2회 행동해야 한다");
    }

    #[test]
    fn 남은_속도에너지는_다음턴으로_이월된다() {
        let mut s = Speed::new(1.5);
        s.energy += s.value; // 1.5
        let mut actions = 0;
        while s.energy >= 1.0 { s.energy -= 1.0; actions += 1; }
        assert_eq!(actions, 1);
        assert!((s.energy - 0.5).abs() < 1e-6, "잔여 에너지 0.5가 다음 턴으로 이월돼야 한다");
    }

    #[test]
    fn 공격력이_방어력보다_크면_차이만큼_피해를_준다() {
        assert_eq!(calc_damage(5, 2), 3);
    }

    #[test]
    fn 공격력과_방어력이_같으면_최소_1피해다() {
        assert_eq!(calc_damage(3, 3), 1);
    }

    #[test]
    fn 방어력이_더_높아도_최소_1피해는_보장된다() {
        assert_eq!(calc_damage(1, 10), 1);
    }

    #[test]
    fn 방어력이_0이면_공격력_전부가_피해다() {
        assert_eq!(calc_damage(5, 0), 5);
    }

    // --- regen_hp (순수 함수) ---

    #[test]
    fn 회복은_최대치_미만이면_지정량만큼_더한다() {
        assert_eq!(regen_hp(5, 10, 1), 6);
        assert_eq!(regen_hp(5, 10, 3), 8);
    }

    #[test]
    fn 회복은_최대치를_넘지_않게_클램프한다() {
        assert_eq!(regen_hp(9, 10, 5), 10, "최대치를 넘지 않아야 한다");
        assert_eq!(regen_hp(10, 10, 1), 10, "이미 최대치면 그대로");
    }

    // --- regenerate_health (시스템) ---

    use crate::modules::map::PlayerActedEvent;
    use crate::modules::player::Player;

    fn regen_app() -> App {
        let mut app = App::new();
        app.add_event::<PlayerActedEvent>();
        app.add_systems(Update, regenerate_health);
        app
    }

    fn stats(hp: i32, max: i32) -> CombatStats {
        CombatStats { hp, max_hp: max, mp: 0, max_mp: 0, attack: 1, defense: 0 }
    }

    #[test]
    fn 매턴_생존한_엔티티는_체력을_회복한다() {
        let mut app = regen_app();
        let player = app.world.spawn((Player, stats(20, 30))).id();
        let monster = app.world.spawn(stats(5, 10)).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<CombatStats>(player).unwrap().hp, 20 + REGEN_PER_TURN);
        assert_eq!(app.world.get::<CombatStats>(monster).unwrap().hp, 5 + REGEN_PER_TURN);
    }

    #[test]
    fn 행동_이벤트가_없으면_회복하지_않는다() {
        let mut app = regen_app();
        let e = app.world.spawn(stats(20, 30)).id();
        app.update(); // PlayerActedEvent 없음
        assert_eq!(app.world.get::<CombatStats>(e).unwrap().hp, 20, "이벤트 없으면 회복 없음");
    }

    #[test]
    fn 최대체력인_엔티티는_더_회복하지_않는다() {
        let mut app = regen_app();
        let e = app.world.spawn(stats(30, 30)).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<CombatStats>(e).unwrap().hp, 30, "최대치면 그대로");
    }

    #[test]
    fn 죽은_엔티티는_회복하지_않는다() {
        // hp<=0 인 엔티티(아직 cleanup 안 된)는 회복 흐름에서 제외돼야 한다.
        let mut app = regen_app();
        let dead = app.world.spawn(stats(0, 10)).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<CombatStats>(dead).unwrap().hp, 0, "죽은 엔티티는 회복 안 함");
    }

    #[test]
    fn Defeated_엔티티는_회복에서_제외된다() {
        // Defeated 마커가 붙은 엔티티는 Without<Defeated> 필터로 제외된다.
        let mut app = regen_app();
        let e = app.world.spawn((stats(5, 10), Defeated)).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<CombatStats>(e).unwrap().hp, 5, "Defeated 는 회복 안 함");
    }

    #[test]
    fn 플레이어가_패배하면_모든_회복이_멈춘다() {
        // 플레이어가 Defeated 면(게임 오버) 살아있는 몬스터도 회복하지 않는다.
        let mut app = regen_app();
        app.world.spawn((Player, stats(0, 30), Defeated));
        let monster = app.world.spawn(stats(5, 10)).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<CombatStats>(monster).unwrap().hp, 5,
            "플레이어 패배 시 회복 전체 중단");
    }

    #[test]
    fn 회복_플러그인을_추가하면_시스템이_등록된다() {
        let mut app = App::new();
        app.add_event::<PlayerActedEvent>();
        app.add_plugins(CombatPlugin);
        let e = app.world.spawn(stats(5, 10)).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<CombatStats>(e).unwrap().hp, 5 + REGEN_PER_TURN);
    }
}
