use rand::prelude::*;
use noise::{NoiseFn, Perlin};
use crate::modules::map::{Map, MapTile};
use super::super::MapGenerator;
use super::{ensure_connectivity, add_rooms_from_floor};

pub struct PerlinNoiseGenerator;

impl MapGenerator for PerlinNoiseGenerator {
    fn generate(&self, width: usize, height: usize) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = thread_rng();
        let perlin = Perlin::new(rng.gen::<u32>());
        let scale = 0.09;

        for y in 1..height - 1 {
            for x in 1..width - 1 {
                let v = perlin.get([x as f64 * scale, y as f64 * scale]);
                if v > 0.15 {
                    map.set_tile(x, y, MapTile::Wall);
                } else {
                    map.set_tile(x, y, MapTile::Floor);
                }
            }
        }

        // 중앙 빈터
        let cx = width / 2;
        let cy = height / 2;
        for dy in -5i32..=5 {
            for dx in -5i32..=5 {
                let nx = (cx as i32 + dx).clamp(1, width as i32 - 2) as usize;
                let ny = (cy as i32 + dy).clamp(1, height as i32 - 2) as usize;
                map.set_tile(nx, ny, MapTile::Floor);
            }
        }

        ensure_connectivity(&mut map);
        add_rooms_from_floor(&mut map);
        map
    }
    fn name(&self) -> &str { "perlin" }
}
