use rand::prelude::*;
use crate::modules::map::{Map, TileKind};
use super::super::MapGenerator;
use super::{count_wall_neighbors, ensure_connectivity, add_rooms_from_floor};

pub struct CellularAutomataGenerator;

impl MapGenerator for CellularAutomataGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;

        for y in 1..height - 1 {
            for x in 1..width - 1 {
                if rng.gen_bool(0.55) {
                    map.set_tile(x, y, TileKind::Floor);
                }
            }
        }

        for _ in 0..5 {
            let old = map.tiles.clone();
            for y in 1..height - 1 {
                for x in 1..width - 1 {
                    let walls = count_wall_neighbors(&old, width, height, x, y);
                    if walls >= 5 {
                        map.set_tile(x, y, TileKind::Wall);
                    } else if walls <= 3 {
                        map.set_tile(x, y, TileKind::Floor);
                    }
                }
            }
        }

        ensure_connectivity(&mut map);
        add_rooms_from_floor(&mut map);
        map
    }
    fn name(&self) -> &str { "cellular_automata" }
}
