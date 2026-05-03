use rand::prelude::*;
use crate::modules::map::{Map, MapTile};
use super::super::MapGenerator;
use super::{add_rooms_from_floor, random_floor_tile};

pub struct DlaGenerator;

impl MapGenerator for DlaGenerator {
    fn generate(&self, width: usize, height: usize) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = thread_rng();

        let cx = width / 2;
        let cy = height / 2;
        for dy in -3i32..=3 {
            for dx in -3i32..=3 {
                let nx = (cx as i32 + dx) as usize;
                let ny = (cy as i32 + dy) as usize;
                map.set_tile(nx, ny, MapTile::Floor);
            }
        }

        let target = ((width * height) as f32 * 0.35) as usize;
        let mut floor_count = 49;

        while floor_count < target {
            let (fx, fy) = random_floor_tile(&map, &mut rng);
            let angle = rng.gen::<f64>() * std::f64::consts::TAU;
            let radius = (width.min(height) / 5) as f64;
            let mut wx = (fx as f64 + angle.cos() * radius)
                .clamp(1.0, width as f64 - 2.0) as i32;
            let mut wy = (fy as f64 + angle.sin() * radius)
                .clamp(1.0, height as f64 - 2.0) as i32;

            for _ in 0..800 {
                let adjacent = [(-1i32, 0), (1, 0), (0, -1i32), (0, 1)].iter().any(|&(dx, dy)| {
                    let nx = wx + dx;
                    let ny = wy + dy;
                    nx >= 0 && ny >= 0 && nx < width as i32 && ny < height as i32
                        && map.get_tile(nx as usize, ny as usize) == MapTile::Floor
                });
                if adjacent {
                    if map.get_tile(wx as usize, wy as usize) == MapTile::Wall {
                        map.set_tile(wx as usize, wy as usize, MapTile::Floor);
                        floor_count += 1;
                    }
                    break;
                }
                match rng.gen_range(0..4) {
                    0 => wx = (wx - 1).max(1),
                    1 => wx = (wx + 1).min(width as i32 - 2),
                    2 => wy = (wy - 1).max(1),
                    _ => wy = (wy + 1).min(height as i32 - 2),
                }
            }
        }

        add_rooms_from_floor(&mut map);
        map
    }
    fn name(&self) -> &str { "동굴 - DLA" }
}
