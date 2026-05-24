use rand::prelude::*;
use noise::Perlin;
use crate::modules::map::{Map, TileKind};
use super::super::MapGenerator;
use super::{octave_noise, force_water_border, add_rooms_from_water_land};

/// 바이옴 월드 생성기.
///
/// 고도 노이즈(약한 가장자리 falloff 포함)로 고도 밴드를 나눈다:
/// 저지대 = 바다(`Water`), 해안 = 모래(`Sand`), 평원 = 땅(`Floor`),
/// 고지대 = 산(`Wall`). 습도 노이즈가 평원 일부를 다시 모래(건조 평원)로
/// 바꿔 변화를 준다. 네 종류 타일이 모두 등장하는 대륙형 맵.
pub struct BiomeWorldGenerator;

impl MapGenerator for BiomeWorldGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;
        map.algorithm = self.name().to_string();

        let elevation = Perlin::new(rng.gen::<u32>());
        let moisture = Perlin::new(rng.gen::<u32>());
        let cx = (width - 1) as f64 / 2.0;
        let cy = (height - 1) as f64 / 2.0;
        let max_d = (cx * cx + cy * cy).sqrt();

        for y in 1..height - 1 {
            for x in 1..width - 1 {
                let dx = x as f64 - cx;
                let dy = y as f64 - cy;
                let dist = (dx * dx + dy * dy).sqrt() / max_d;
                // 가장자리를 살짝 가라앉혀 대륙이 바다에 둘러싸이게.
                let falloff = (1.15 - dist * 0.9).clamp(0.0, 1.0);
                let e = ((octave_noise(&elevation, x as f64, y as f64, 0.06, 4) + 1.0) / 2.0) * falloff;
                let m = (octave_noise(&moisture, x as f64, y as f64, 0.09, 2) + 1.0) / 2.0;

                let kind = if e < 0.30 {
                    TileKind::Water
                } else if e < 0.38 {
                    TileKind::Sand
                } else if e < 0.70 {
                    // 평원: 습도가 매우 낮으면 건조 평원(모래)로 변형.
                    if m < 0.30 { TileKind::Sand } else { TileKind::Floor }
                } else {
                    TileKind::Wall // 산
                };
                map.set_tile(x, y, kind);
            }
        }

        // 테두리는 항상 물.
        force_water_border(&mut map);
        add_rooms_from_water_land(&mut map);
        map
    }

    fn name(&self) -> &str { "biome_world" }
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;
    use crate::modules::map::MapGenerator;

    #[test]
    fn 같은_시드는_같은_맵을_만든다() {
        let gen = BiomeWorldGenerator;
        let a = gen.generate(40, 30, 42);
        let b = gen.generate(40, 30, 42);
        assert_eq!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 다른_시드는_다른_맵을_만든다() {
        let gen = BiomeWorldGenerator;
        let a = gen.generate(40, 30, 1);
        let b = gen.generate(40, 30, 2);
        assert_ne!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 네가지_타일이_모두_등장한다() {
        // 바이옴 월드: 물/모래/평원/산이 모두 나타나야 한다.
        let gen = BiomeWorldGenerator;
        let map = gen.generate(60, 45, 5);
        let has = |k: TileKind| map.tiles.iter().any(|t| t.kind == k);
        assert!(has(TileKind::Water), "물(저지대)이 있어야 한다");
        assert!(has(TileKind::Sand), "모래(해안/건조)가 있어야 한다");
        assert!(has(TileKind::Floor), "평원(땅)이 있어야 한다");
        assert!(has(TileKind::Wall), "산(고지대)이 있어야 한다");
    }

    #[test]
    fn 건조_평원_변형_분기가_실행된다() {
        // 습도 분기 양쪽(건조→Sand, 습윤→Floor)이 한 맵 안에 모두 나타나도록.
        let gen = BiomeWorldGenerator;
        let map = gen.generate(80, 60, 21);
        let sand = map.tiles.iter().filter(|t| t.kind == TileKind::Sand).count();
        let floor = map.tiles.iter().filter(|t| t.kind == TileKind::Floor).count();
        assert!(sand > 0, "건조 평원 변형으로 모래가 존재해야 한다");
        assert!(floor > 0, "습윤 평원으로 땅이 존재해야 한다");
    }
}
