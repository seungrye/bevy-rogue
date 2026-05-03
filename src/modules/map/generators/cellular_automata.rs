use rand::prelude::*;
use crate::modules::map::{Map, MapTile};
use super::super::MapGenerator;
use super::{count_wall_neighbors, ensure_connectivity, add_rooms_from_floor};

pub struct CellularAutomataGenerator;

impl MapGenerator for CellularAutomataGenerator {
    fn generate(&self, width: usize, height: usize) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = thread_rng();

        for y in 1..height - 1 {
            for x in 1..width - 1 {
                if rng.gen_bool(0.55) {
                    map.set_tile(x, y, MapTile::Floor);
                }
            }
        }

        for _ in 0..5 {
            let old = map.tiles.clone();
            for y in 1..height - 1 {
                for x in 1..width - 1 {
                    let walls = count_wall_neighbors(&old, width, height, x, y);
                    if walls >= 5 {
                        map.set_tile(x, y, MapTile::Wall);
                    } else if walls <= 3 {
                        map.set_tile(x, y, MapTile::Floor);
                    }
                }
            }
        }

        ensure_connectivity(&mut map);
        add_rooms_from_floor(&mut map);
        map
    }
    fn name(&self) -> &str { "동굴 - 셀룰러 오토마타" }
}
