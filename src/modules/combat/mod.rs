use bevy::prelude::*;

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

#[cfg(test)]
mod tests {
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
}
