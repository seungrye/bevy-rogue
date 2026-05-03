use rand::prelude::*;
use crate::modules::map::{Map, MapTile};
use super::super::MapGenerator;
use super::add_rooms_from_floor;

pub struct DlaGenerator;

impl MapGenerator for DlaGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;

        let cx = width / 2;
        let cy = height / 2;

        // 초기 씨앗: 중앙 7×7 패치를 Floor로 채우고 목록에 등록
        let mut floor_tiles: Vec<(usize, usize)> = Vec::with_capacity(512);
        for dy in -3i32..=3 {
            for dx in -3i32..=3 {
                let nx = (cx as i32 + dx) as usize;
                let ny = (cy as i32 + dy) as usize;
                map.set_tile(nx, ny, MapTile::Floor);
                floor_tiles.push((nx, ny));
            }
        }

        let target = ((width * height) as f32 * 0.35) as usize;
        let radius = (width.min(height) / 5) as f64;
        let w = width as i32;
        let h = height as i32;

        while floor_tiles.len() < target {
            // O(1): 목록에서 무작위 Floor 타일 선택
            let seed_idx = rng.gen_range(0..floor_tiles.len());
            let (fx, fy) = floor_tiles[seed_idx];

            // 씨앗 주위 반경에서 파티클 출발
            let angle = rng.gen::<f64>() * std::f64::consts::TAU;
            let mut wx = (fx as f64 + angle.cos() * radius).clamp(1.0, w as f64 - 2.0) as i32;
            let mut wy = (fy as f64 + angle.sin() * radius).clamp(1.0, h as f64 - 2.0) as i32;

            // 최대 400 스텝 랜덤 워크 (반경 ≈ 20, sqrt(400)=20 으로 도달 충분)
            for _ in 0..400 {
                let adjacent = [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)].iter().any(|&(dx, dy)| {
                    let nx = wx + dx;
                    let ny = wy + dy;
                    nx >= 0 && ny >= 0 && nx < w && ny < h
                        && map.get_tile(nx as usize, ny as usize) == MapTile::Floor
                });
                if adjacent {
                    if map.get_tile(wx as usize, wy as usize) == MapTile::Wall {
                        map.set_tile(wx as usize, wy as usize, MapTile::Floor);
                        floor_tiles.push((wx as usize, wy as usize)); // O(1) 목록 갱신
                    }
                    break;
                }
                match rng.gen_range(0u8..4) {
                    0 => wx = (wx - 1).max(1),
                    1 => wx = (wx + 1).min(w - 2),
                    2 => wy = (wy - 1).max(1),
                    _ => wy = (wy + 1).min(h - 2),
                }
            }
        }

        add_rooms_from_floor(&mut map);
        map
    }
    fn name(&self) -> &str { "dla" }
}
