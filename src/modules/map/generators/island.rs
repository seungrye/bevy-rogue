use rand::prelude::*;
use noise::Perlin;
use crate::modules::map::{Map, TileKind};
use super::super::MapGenerator;
use super::{octave_noise, mark_beaches, force_water_border, ensure_water_connectivity, add_rooms_from_water_land};

/// 단일 섬 생성기.
///
/// 중심에서 1, 가장자리에서 0 으로 떨어지는 **방사형 falloff** 에
/// **멀티옥타브 펄린 노이즈** 를 곱해 고도장을 만든다. 임계값 위는 땅(`Floor`),
/// 땅과 물의 경계 한 칸은 모래 해변(`Sand`), 나머지는 바다(`Water`) 가 되어
/// 바다로 둘러싸인 하나의 섬을 형성한다.
pub struct IslandGenerator;

impl MapGenerator for IslandGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;
        map.algorithm = self.name().to_string();

        let perlin = Perlin::new(rng.gen::<u32>());
        let cx = (width - 1) as f64 / 2.0;
        let cy = (height - 1) as f64 / 2.0;
        // 중심에서의 최대 거리(코너) — falloff 정규화에 사용.
        let max_d = (cx * cx + cy * cy).sqrt();

        for y in 1..height - 1 {
            for x in 1..width - 1 {
                let dx = x as f64 - cx;
                let dy = y as f64 - cy;
                let dist = (dx * dx + dy * dy).sqrt() / max_d;
                // 중심 1 → 가장자리 0 의 방사형 falloff.
                let falloff = (1.0 - dist).clamp(0.0, 1.0);
                // 멀티옥타브 노이즈를 [0,1] 로 정규화.
                let n = (octave_noise(&perlin, x as f64, y as f64, 0.07, 4) + 1.0) / 2.0;
                let elevation = falloff * n;
                if elevation > 0.28 {
                    map.set_tile(x, y, TileKind::Floor);
                }
                // 그 외는 Map::new 기본값(Wall) → 아래에서 물로 채운다.
            }
        }

        // 땅이 아닌 모든 타일을 물로 변환(테두리 포함).
        for tile in map.tiles.iter_mut() {
            if tile.kind == TileKind::Wall {
                tile.kind = TileKind::Water;
            }
        }

        // 가장 큰 섬만 남기고 나머지는 바다로(단일 섬 보장).
        ensure_water_connectivity(&mut map);
        mark_beaches(&mut map);
        force_water_border(&mut map);
        add_rooms_from_water_land(&mut map);
        map
    }

    fn name(&self) -> &str { "island" }
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;
    use crate::modules::map::MapGenerator;

    #[test]
    fn 같은_시드는_같은_맵을_만든다() {
        let gen = IslandGenerator;
        let a = gen.generate(40, 30, 42);
        let b = gen.generate(40, 30, 42);
        assert_eq!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 다른_시드는_다른_맵을_만든다() {
        let gen = IslandGenerator;
        let a = gen.generate(40, 30, 1);
        let b = gen.generate(40, 30, 2);
        assert_ne!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 중심_근처에는_통과가능한_땅이_있다() {
        let gen = IslandGenerator;
        let map = gen.generate(40, 30, 7);
        let (cx, cy) = (map.width / 2, map.height / 2);
        // 중심 5×5 안에 통과 가능한 땅이 최소 하나는 존재.
        let land_near_center = (-2i32..=2)
            .flat_map(|dy| (-2i32..=2).map(move |dx| (dx, dy)))
            .any(|(dx, dy)| {
                let x = (cx as i32 + dx) as usize;
                let y = (cy as i32 + dy) as usize;
                map.get_tile(x, y).is_walkable()
            });
        assert!(land_near_center, "섬의 중심 근처는 땅이어야 한다");
    }

    #[test]
    fn 섬은_바다로_둘러싸인다() {
        let gen = IslandGenerator;
        let map = gen.generate(40, 30, 13);
        // 테두리가 전부 물.
        for x in 0..map.width {
            assert_eq!(map.get_tile(x, 0), TileKind::Water);
            assert_eq!(map.get_tile(x, map.height - 1), TileKind::Water);
        }
        // 모래 해변이 실제로 생성됨(땅-물 경계).
        let sand = map.tiles.iter().filter(|t| t.kind == TileKind::Sand).count();
        assert!(sand > 0, "땅-물 경계에 모래 해변이 있어야 한다");
    }
}
