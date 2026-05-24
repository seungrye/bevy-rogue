use rand::prelude::*;
use noise::Perlin;
use crate::modules::map::{Map, TileKind};
use super::super::MapGenerator;
use super::{octave_noise, mark_beaches, force_water_border, ensure_water_connectivity, add_rooms_from_water_land};

/// 해안 생성기.
///
/// 한 축(y)을 따라 위는 땅, 아래는 바다가 되는 **그라디언트** 에 노이즈를
/// 더해 구불구불한 해안선을 만든다. 절반은 땅(`Floor`), 절반은 바다(`Water`),
/// 그 사이 땅-물 경계 한 칸은 모래 해안(`Sand`).
pub struct CoastalGenerator;

impl MapGenerator for CoastalGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;
        map.algorithm = self.name().to_string();

        let perlin = Perlin::new(rng.gen::<u32>());

        for y in 1..height - 1 {
            for x in 1..width - 1 {
                // y=0(위) 에서 1, y=height-1(아래) 에서 0 으로 줄어드는 땅 점수.
                let gradient = 1.0 - y as f64 / (height - 1) as f64;
                // 노이즈로 해안선을 구불구불하게.
                let n = octave_noise(&perlin, x as f64, y as f64, 0.08, 3) * 0.25;
                let land_score = gradient + n;
                if land_score > 0.5 {
                    map.set_tile(x, y, TileKind::Floor);
                }
            }
        }

        // 땅이 아닌 모든 타일을 물로.
        for tile in map.tiles.iter_mut() {
            if tile.kind == TileKind::Wall {
                tile.kind = TileKind::Water;
            }
        }

        // 본토는 하나의 연결된 땅이어야 함.
        ensure_water_connectivity(&mut map);
        mark_beaches(&mut map);
        force_water_border(&mut map);
        add_rooms_from_water_land(&mut map);
        map
    }

    fn name(&self) -> &str { "coastal" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::map::MapGenerator;

    #[test]
    fn 같은_시드는_같은_맵을_만든다() {
        let gen = CoastalGenerator;
        let a = gen.generate(40, 30, 42);
        let b = gen.generate(40, 30, 42);
        assert_eq!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 다른_시드는_다른_맵을_만든다() {
        let gen = CoastalGenerator;
        let a = gen.generate(40, 30, 1);
        let b = gen.generate(40, 30, 2);
        assert_ne!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 위쪽은_땅이고_아래쪽은_바다다() {
        // 그라디언트 특성: 상단 영역에 땅이, 하단 영역에 바다가 우세하다.
        let gen = CoastalGenerator;
        let map = gen.generate(40, 30, 5);
        let band = |y0: usize, y1: usize, kind: TileKind| {
            (y0..y1)
                .flat_map(|y| (1..map.width - 1).map(move |x| (x, y)))
                .filter(|&(x, y)| map.get_tile(x, y) == kind)
                .count()
        };
        let top_land = band(1, map.height / 4, TileKind::Floor);
        let bottom_water = band(map.height * 3 / 4, map.height - 1, TileKind::Water);
        assert!(top_land > 0, "상단에 땅이 있어야 한다");
        assert!(bottom_water > 0, "하단에 바다가 있어야 한다");
    }

    #[test]
    fn 해안선에_모래가_생긴다() {
        let gen = CoastalGenerator;
        let map = gen.generate(40, 30, 9);
        let sand = map.tiles.iter().filter(|t| t.kind == TileKind::Sand).count();
        assert!(sand > 0, "해안선에 모래가 있어야 한다");
    }
}
