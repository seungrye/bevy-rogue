use rand::prelude::*;
use crate::modules::map::{Map, MapTile, Rect};
use super::super::MapGenerator;

pub struct BspIndoorGenerator;

impl MapGenerator for BspIndoorGenerator {
    fn generate(&self, width: usize, height: usize) -> Map {
        let mut map = Map::new(width, height);
        let mut rooms = Vec::new();
        split_rect(Rect::new(1, 1, width - 2, height - 2), &mut rooms, 7);

        for room in &rooms {
            // 방 내부를 바닥으로, 경계를 벽으로 그린다
            for y in room.y1..room.y2 {
                for x in room.x1..room.x2 {
                    if y == room.y1 || y == room.y2 - 1 || x == room.x1 || x == room.x2 - 1 {
                        map.set_tile(x, y, MapTile::Wall);
                    } else {
                        map.set_tile(x, y, MapTile::Floor);
                    }
                }
            }
        }

        // 인접 방 쌍 사이에 문(바닥 1칸) 개통
        let mut rng = thread_rng();
        for i in 0..rooms.len().saturating_sub(1) {
            connect_rooms(&mut map, &rooms[i], &rooms[i + 1], &mut rng);
        }

        map.rooms = rooms;
        map
    }
    fn name(&self) -> &str { "bsp_indoor" }
}

fn split_rect(rect: Rect, rooms: &mut Vec<Rect>, depth: usize) {
    if depth == 0 || (rect.width() < 6 && rect.height() < 6) {
        if rect.width() >= 5 && rect.height() >= 4 {
            rooms.push(rect);
        }
        return;
    }
    let mut rng = thread_rng();
    let split_h = if rect.width() > rect.height() { false }
                  else if rect.height() > rect.width() { true }
                  else { rng.gen_bool(0.5) };

    if split_h {
        if rect.y2.saturating_sub(rect.y1) < 9 { if rect.width() >= 5 { rooms.push(rect); } return; }
        let sy = rng.gen_range(rect.y1 + 4..rect.y2 - 4);
        split_rect(Rect { y2: sy, ..rect }, rooms, depth - 1);
        split_rect(Rect { y1: sy, ..rect }, rooms, depth - 1);
    } else {
        if rect.x2.saturating_sub(rect.x1) < 9 { if rect.height() >= 4 { rooms.push(rect); } return; }
        let sx = rng.gen_range(rect.x1 + 4..rect.x2 - 4);
        split_rect(Rect { x2: sx, ..rect }, rooms, depth - 1);
        split_rect(Rect { x1: sx, ..rect }, rooms, depth - 1);
    }
}

fn connect_rooms(map: &mut Map, a: &Rect, b: &Rect, rng: &mut ThreadRng) {
    // 공유 벽 또는 가장 가까운 벽에 문 1개 개통
    let (ax, ay) = a.center();
    let (bx, by) = b.center();

    // 두 방 사이 벽 타일 중 하나를 Floor로 전환
    let mx = (ax + bx) / 2;
    let my = (ay + by) / 2;

    // 중간 지점부터 양 방향으로 Floor 타일을 찾아 벽을 뚫는다
    for step in 0..5i32 {
        for &sign in &[-1i32, 1] {
            let tx = (mx as i32 + sign * step * (bx as i32 - ax as i32).signum())
                .clamp(1, map.width as i32 - 2) as usize;
            let ty = (my as i32 + sign * step * (by as i32 - ay as i32).signum())
                .clamp(1, map.height as i32 - 2) as usize;
            if map.get_tile(tx, ty) == MapTile::Wall {
                // 양쪽이 각기 다른 Floor 구역과 닿아 있으면 문 개통
                let neighbors_floor = [(0i32, 1), (0, -1), (1, 0), (-1, 0)]
                    .iter()
                    .filter(|&&(dx, dy)| {
                        let nx = (tx as i32 + dx).clamp(0, map.width as i32 - 1) as usize;
                        let ny = (ty as i32 + dy).clamp(0, map.height as i32 - 1) as usize;
                        map.get_tile(nx, ny) == MapTile::Floor
                    })
                    .count();
                if neighbors_floor >= 2 {
                    map.set_tile(tx, ty, MapTile::Floor);
                    return;
                }
            }
        }
    }
    // 폴백: L자 복도
    let _ = rng;
    super::carve_corridor(map, ax, ay, bx, by);
}
