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
    fn speed_energy_accumulates_per_turn() {
        let mut s = Speed::new(0.5);
        s.energy += s.value;
        assert!(s.energy < 1.0, "0.5 속도는 첫 턴에 행동 불가");
        s.energy += s.value;
        assert!(s.energy >= 1.0, "0.5 속도는 두 번째 턴에 행동 가능");
    }

    #[test]
    fn speed_fast_acts_twice_per_turn() {
        let mut s = Speed::new(2.0);
        s.energy += s.value;
        let mut actions = 0;
        while s.energy >= 1.0 { s.energy -= 1.0; actions += 1; }
        assert_eq!(actions, 2, "속도 2.0은 한 턴에 2회 행동해야 한다");
    }

    #[test]
    fn speed_energy_carries_over() {
        let mut s = Speed::new(1.5);
        s.energy += s.value; // 1.5
        let mut actions = 0;
        while s.energy >= 1.0 { s.energy -= 1.0; actions += 1; }
        assert_eq!(actions, 1);
        assert!((s.energy - 0.5).abs() < 1e-6, "잔여 에너지 0.5가 다음 턴으로 이월돼야 한다");
    }

    #[test]
    fn calc_damage_exceeds_defense() {
        assert_eq!(calc_damage(5, 2), 3);
    }

    #[test]
    fn calc_damage_equals_defense() {
        assert_eq!(calc_damage(3, 3), 1);
    }

    #[test]
    fn calc_damage_minimum_is_one() {
        assert_eq!(calc_damage(1, 10), 1);
    }

    #[test]
    fn calc_damage_zero_defense() {
        assert_eq!(calc_damage(5, 0), 5);
    }
}
