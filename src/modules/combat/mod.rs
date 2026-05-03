use bevy::prelude::*;

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
