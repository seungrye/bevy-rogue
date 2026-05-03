use rand::prelude::*;
use crate::modules::map::{Map, MapTile, Rect};
use super::super::MapGenerator;

pub struct SimpleRoomsGenerator;

impl MapGenerator for SimpleRoomsGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rooms: Vec<Rect> = Vec::new();
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;

        for _ in 0..200 {
            if rooms.len() >= 10 { break; }
            let w = rng.gen_range(4..8);
            let h = rng.gen_range(4..8);
            let x = rng.gen_range(1..width - w - 1);
            let y = rng.gen_range(1..height - h - 1);
            let new_room = Rect::new(x, y, w, h);
            if rooms.iter().any(|r| intersect(&new_room, r)) { continue; }
            for ry in new_room.y1..new_room.y2 {
                for rx in new_room.x1..new_room.x2 {
                    map.set_tile(rx, ry, MapTile::Floor);
                }
            }
            if let Some(prev) = rooms.last() {
                let (nx, ny) = new_room.center();
                let (px, py) = prev.center();
                if rng.gen_bool(0.5) {
                    h_tunnel(&mut map, px, nx, py);
                    v_tunnel(&mut map, py, ny, nx);
                } else {
                    v_tunnel(&mut map, py, ny, px);
                    h_tunnel(&mut map, px, nx, ny);
                }
            }
            rooms.push(new_room);
        }
        map.rooms = rooms;
        map
    }
    fn name(&self) -> &str { "simple_rooms" }
}

fn intersect(a: &Rect, b: &Rect) -> bool {
    a.x1 <= b.x2 && a.x2 >= b.x1 && a.y1 <= b.y2 && a.y2 >= b.y1
}

fn h_tunnel(map: &mut Map, x1: usize, x2: usize, y: usize) {
    let (lo, hi) = (x1.min(x2), x1.max(x2));
    for x in lo..=hi { map.set_tile(x, y, MapTile::Floor); }
}

fn v_tunnel(map: &mut Map, y1: usize, y2: usize, x: usize) {
    let (lo, hi) = (y1.min(y2), y1.max(y2));
    for y in lo..=hi { map.set_tile(x, y, MapTile::Floor); }
}
