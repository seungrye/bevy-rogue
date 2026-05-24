use rand::prelude::*;
use crate::modules::map::{Map, TileKind};
use super::super::MapGenerator;
use super::{mark_beaches, force_water_border, add_rooms_from_water_land};

/// 대양 생성기.
///
/// 맵 대부분을 바다(`Water`)로 채우고, 드문드문 작은 섬/암초만 흩뿌린다.
/// 각 암초는 무작위 중심에 작은 원형 땅(`Floor`)으로 카브되고, 땅-물 경계
/// 한 칸은 모래(`Sand`) 가 된다. 통과타일 비율이 낮아 "바다가 대부분" 인
/// 맵이지만 스폰 가능한 땅은 반드시 존재한다.
pub struct OceanGenerator;

impl MapGenerator for OceanGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;
        map.algorithm = self.name().to_string();

        // 전부 바다로 시작.
        for tile in map.tiles.iter_mut() {
            tile.kind = TileKind::Water;
        }

        // 맵 크기에 비례한 암초 개수(최소 3개) — 작아도 땅이 충분히 생기게.
        let reef_count = ((width * height) / 220).max(3);
        for _ in 0..reef_count {
            let rcx = rng.gen_range(3..width as i32 - 3);
            let rcy = rng.gen_range(3..height as i32 - 3);
            let radius = rng.gen_range(2..=3);
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    // 원형 + 약간의 노이즈로 들쭉날쭉한 암초.
                    let within = dx * dx + dy * dy <= radius * radius;
                    if !within || rng.gen_bool(0.15) {
                        continue;
                    }
                    let nx = (rcx + dx).clamp(1, width as i32 - 2) as usize;
                    let ny = (rcy + dy).clamp(1, height as i32 - 2) as usize;
                    map.set_tile(nx, ny, TileKind::Floor);
                }
            }
        }

        mark_beaches(&mut map);
        force_water_border(&mut map);
        add_rooms_from_water_land(&mut map);
        map
    }

    fn name(&self) -> &str { "ocean" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::map::MapGenerator;

    #[test]
    fn 같은_시드는_같은_맵을_만든다() {
        let gen = OceanGenerator;
        let a = gen.generate(40, 30, 42);
        let b = gen.generate(40, 30, 42);
        assert_eq!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 다른_시드는_다른_맵을_만든다() {
        let gen = OceanGenerator;
        let a = gen.generate(40, 30, 1);
        let b = gen.generate(40, 30, 2);
        assert_ne!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 물_비율이_매우_높다() {
        // 대양: 타일의 압도적 다수가 물이어야 한다.
        let gen = OceanGenerator;
        let map = gen.generate(40, 30, 5);
        let water = map.tiles.iter().filter(|t| t.kind == TileKind::Water).count();
        let ratio = water as f32 / map.tiles.len() as f32;
        assert!(ratio > 0.80, "대양은 물 비율이 80% 초과여야 하는데 {:.1}%", ratio * 100.0);
    }

    #[test]
    fn 작은_섬과_모래가_존재한다() {
        let gen = OceanGenerator;
        let map = gen.generate(40, 30, 9);
        let land = map.tiles.iter().filter(|t| t.kind.is_walkable()).count();
        let sand = map.tiles.iter().filter(|t| t.kind == TileKind::Sand).count();
        assert!(land > 0, "스폰 가능한 땅(암초)이 있어야 한다");
        assert!(sand > 0, "암초 둘레에 모래가 있어야 한다");
    }
}
