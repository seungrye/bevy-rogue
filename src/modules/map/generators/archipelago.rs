use rand::prelude::*;
use noise::Perlin;
use crate::modules::map::{Map, TileKind};
use super::super::MapGenerator;
use super::{octave_noise, mark_beaches, force_water_border, add_rooms_from_water_land};

/// 다도해(군도) 생성기.
///
/// 약한 방사형 falloff(가장자리만 살짝 가라앉힘) 와 두 개의 펄린 노이즈를
/// 합성한 고도장으로 **흩어진 여러 섬** 을 만든다. 임계값 위는 땅(`Floor`),
/// 섬마다 땅-물 경계 한 칸은 모래 해변(`Sand`), 나머지는 바다(`Water`).
/// 단일 연결을 강요하지 않아 섬들이 바다로 분리된 채 남는다.
pub struct ArchipelagoGenerator;

impl MapGenerator for ArchipelagoGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;
        map.algorithm = self.name().to_string();

        let perlin_a = Perlin::new(rng.gen::<u32>());
        let perlin_b = Perlin::new(rng.gen::<u32>());
        let cx = (width - 1) as f64 / 2.0;
        let cy = (height - 1) as f64 / 2.0;
        let max_d = (cx * cx + cy * cy).sqrt();

        for y in 1..height - 1 {
            for x in 1..width - 1 {
                let dx = x as f64 - cx;
                let dy = y as f64 - cy;
                let dist = (dx * dx + dy * dy).sqrt() / max_d;
                // 약한 falloff: 중심부 거의 1, 가장자리에서만 감쇠.
                let falloff = (1.2 - dist * 0.9).clamp(0.0, 1.0);
                // 두 노이즈를 합성 → 흩어진 블롭(섬).
                let na = (octave_noise(&perlin_a, x as f64, y as f64, 0.11, 3) + 1.0) / 2.0;
                let nb = (octave_noise(&perlin_b, x as f64, y as f64, 0.06, 2) + 1.0) / 2.0;
                let elevation = falloff * (na * 0.6 + nb * 0.4);
                if elevation > 0.52 {
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

        // 군도이므로 연결성 강제 없음(여러 섬 유지).
        mark_beaches(&mut map);
        force_water_border(&mut map);
        add_rooms_from_water_land(&mut map);
        map
    }

    fn name(&self) -> &str { "archipelago" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::map::MapGenerator;

    fn 통과타일_연결요소_수(map: &Map) -> usize {
        let mut visited = vec![false; map.width * map.height];
        let mut count = 0;
        for sy in 0..map.height {
            for sx in 0..map.width {
                let sidx = map.index(sx, sy);
                if visited[sidx] || !map.get_tile(sx, sy).is_walkable() {
                    continue;
                }
                count += 1;
                let mut stack = vec![(sx, sy)];
                visited[sidx] = true;
                // 테두리가 모두 Water 라 통과타일의 4이웃은 항상 맵 내부 — 경계 검사 불필요.
                while let Some((x, y)) = stack.pop() {
                    for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                        let idx = map.index(nx, ny);
                        if !visited[idx] && map.get_tile(nx, ny).is_walkable() {
                            visited[idx] = true;
                            stack.push((nx, ny));
                        }
                    }
                }
            }
        }
        count
    }

    #[test]
    fn 같은_시드는_같은_맵을_만든다() {
        let gen = ArchipelagoGenerator;
        let a = gen.generate(40, 30, 42);
        let b = gen.generate(40, 30, 42);
        assert_eq!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 다른_시드는_다른_맵을_만든다() {
        let gen = ArchipelagoGenerator;
        let a = gen.generate(40, 30, 1);
        let b = gen.generate(40, 30, 2);
        assert_ne!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 흩어진_여러_섬이_생긴다() {
        // 군도는 단일 섬이 아니라 분리된 여러 섬(연결요소)을 가진다.
        let gen = ArchipelagoGenerator;
        let map = gen.generate(60, 45, 7);
        assert!(
            통과타일_연결요소_수(&map) >= 2,
            "군도는 분리된 섬이 둘 이상이어야 한다"
        );
    }

    #[test]
    fn 섬마다_모래_해변이_있다() {
        let gen = ArchipelagoGenerator;
        let map = gen.generate(40, 30, 13);
        let sand = map.tiles.iter().filter(|t| t.kind == TileKind::Sand).count();
        assert!(sand > 0, "섬의 땅-물 경계에 모래 해변이 있어야 한다");
    }
}
