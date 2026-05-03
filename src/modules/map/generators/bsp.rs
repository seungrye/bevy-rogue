use rand::prelude::*;
use crate::modules::map::{Map, MapTile, Rect};
use super::super::MapGenerator;

pub struct BspGenerator;

impl MapGenerator for BspGenerator {
    fn generate(&self, width: usize, height: usize) -> Map {
        let mut map = Map::new(width, height);
        let mut rooms = Vec::new();
        split_rect(Rect::new(1, 1, width - 2, height - 2), &mut rooms, 5);
        for room in &rooms {
            for y in room.y1..room.y2 {
                for x in room.x1..room.x2 {
                    map.set_tile(x, y, MapTile::Floor);
                }
            }
        }
        for i in 0..rooms.len().saturating_sub(1) {
            let (x1, y1) = rooms[i].center();
            let (x2, y2) = rooms[i + 1].center();
            super::carve_corridor(&mut map, x1, y1, x2, y2);
        }
        map.rooms = rooms;
        map
    }
    fn name(&self) -> &str { "던전 - BSP" }
}

fn split_rect(rect: Rect, rooms: &mut Vec<Rect>, depth: usize) {
    if depth == 0 || (rect.width() < 10 && rect.height() < 10) {
        rooms.push(rect);
        return;
    }
    let mut rng = thread_rng();
    let split_h = if rect.width() > rect.height() { false }
                  else if rect.height() > rect.width() { true }
                  else { rng.gen_bool(0.5) };
    if split_h {
        if rect.y2.saturating_sub(rect.y1) < 7 { rooms.push(rect); return; }
        let sy = rng.gen_range(rect.y1 + 3..rect.y2 - 3);
        split_rect(Rect { x1: rect.x1, y1: rect.y1, x2: rect.x2, y2: sy }, rooms, depth - 1);
        split_rect(Rect { x1: rect.x1, y1: sy,      x2: rect.x2, y2: rect.y2 }, rooms, depth - 1);
    } else {
        if rect.x2.saturating_sub(rect.x1) < 7 { rooms.push(rect); return; }
        let sx = rng.gen_range(rect.x1 + 3..rect.x2 - 3);
        split_rect(Rect { x1: rect.x1, y1: rect.y1, x2: sx,      y2: rect.y2 }, rooms, depth - 1);
        split_rect(Rect { x1: sx,      y1: rect.y1, x2: rect.x2, y2: rect.y2 }, rooms, depth - 1);
    }
}
