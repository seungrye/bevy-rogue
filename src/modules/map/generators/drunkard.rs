use rand::prelude::*;
use crate::modules::map::{Map, TileKind, Rect};
use super::super::MapGenerator;

pub struct DrunkardWalkGenerator;

impl MapGenerator for DrunkardWalkGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;
        let (mut x, mut y) = (width / 2, height / 2);
        map.set_tile(x, y, TileKind::Floor);
        let target = ((width * height) as f32 * 0.4) as usize;
        let mut count = 1;
        while count < target {
            match rng.gen_range(0..4) {
                0 => { if x > 1 { x -= 1; } }
                1 => { if x < width - 2 { x += 1; } }
                2 => { if y > 1 { y -= 1; } }
                _ => { if y < height - 2 { y += 1; } }
            }
            if map.get_tile(x, y) == TileKind::Wall {
                map.set_tile(x, y, TileKind::Floor);
                count += 1;
            }
        }
        let cx = width / 2;
        let cy = height / 2;
        map.rooms.push(Rect::new(cx.saturating_sub(2), cy.saturating_sub(2), 4, 4));
        super::add_rooms_from_floor(&mut map);
        map
    }
    fn name(&self) -> &str { "drunkard" }
}
