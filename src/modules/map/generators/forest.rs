use rand::prelude::*;
use crate::modules::map::{Map, MapTile};
use super::super::MapGenerator;
use super::{count_wall_neighbors, ensure_connectivity, add_rooms_from_floor};

pub struct ForestGenerator;

impl MapGenerator for ForestGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;

        // 나무 밀도 65%
        for y in 1..height - 1 {
            for x in 1..width - 1 {
                if rng.gen_bool(0.35) {
                    map.set_tile(x, y, MapTile::Floor);
                }
            }
        }

        // CA — 벽(나무) 우세 규칙
        for _ in 0..6 {
            let old = map.tiles.clone();
            for y in 1..height - 1 {
                for x in 1..width - 1 {
                    let walls = count_wall_neighbors(&old, width, height, x, y);
                    if walls >= 4 {
                        map.set_tile(x, y, MapTile::Wall);
                    } else {
                        map.set_tile(x, y, MapTile::Floor);
                    }
                }
            }
        }

        // 중앙 빈터 + 방향별 경로 개통
        let cx = width / 2;
        let cy = height / 2;
        carve_clearing(&mut map, cx, cy, 4);

        let edges = [
            (cx, 4usize),
            (cx, height - 4),
            (4usize, cy),
            (width - 4, cy),
        ];
        for &(ex, ey) in &edges {
            carve_path(&mut map, cx, cy, ex, ey, &mut rng);
        }

        ensure_connectivity(&mut map);
        add_rooms_from_floor(&mut map);
        map
    }
    fn name(&self) -> &str { "forest" }
}

fn carve_clearing(map: &mut Map, cx: usize, cy: usize, radius: i32) {
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let nx = (cx as i32 + dx).clamp(1, map.width as i32 - 2) as usize;
            let ny = (cy as i32 + dy).clamp(1, map.height as i32 - 2) as usize;
            map.set_tile(nx, ny, MapTile::Floor);
        }
    }
}

fn carve_path(map: &mut Map, x1: usize, y1: usize, x2: usize, y2: usize, rng: &mut impl Rng) {
    let mut x = x1 as i32;
    let mut y = y1 as i32;
    let tx = x2 as i32;
    let ty = y2 as i32;
    while (x - tx).abs() + (y - ty).abs() > 1 {
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let nx = (x + dx).clamp(1, map.width as i32 - 2) as usize;
                let ny = (y + dy).clamp(1, map.height as i32 - 2) as usize;
                map.set_tile(nx, ny, MapTile::Floor);
            }
        }
        if rng.gen_bool(0.15) {
            match rng.gen_range(0..4) {
                0 => x = (x - 1).max(1),
                1 => x = (x + 1).min(map.width as i32 - 2),
                2 => y = (y - 1).max(1),
                _ => y = (y + 1).min(map.height as i32 - 2),
            }
        } else if (x - tx).abs() >= (y - ty).abs() {
            x += (tx - x).signum();
        } else {
            y += (ty - y).signum();
        }
    }
}
