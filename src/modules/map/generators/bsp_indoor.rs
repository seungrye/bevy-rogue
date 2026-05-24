use rand::prelude::*;
use crate::modules::map::{Map, TileKind, Rect};
use super::super::MapGenerator;

pub struct BspIndoorGenerator;

impl MapGenerator for BspIndoorGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut map = Map::new(width, height);
        map.seed = seed;
        let mut rooms = Vec::new();
        split_rect(Rect::new(1, 1, width - 2, height - 2), &mut rooms, 7, &mut rng);

        for room in &rooms {
            for y in room.y1..room.y2 {
                for x in room.x1..room.x2 {
                    if y == room.y1 || y == room.y2 - 1 || x == room.x1 || x == room.x2 - 1 {
                        // 실내 건물 벽은 파괴 가능(DestructibleWall). 테두리는 손대지 않아 일반 Wall 유지.
                        map.set_tile(x, y, TileKind::DestructibleWall);
                    } else {
                        map.set_tile(x, y, TileKind::Floor);
                    }
                }
            }
        }

        for i in 0..rooms.len().saturating_sub(1) {
            connect_rooms(&mut map, &rooms[i], &rooms[i + 1], &mut rng);
        }

        map.rooms = rooms;
        map
    }
    fn name(&self) -> &str { "bsp_indoor" }
}

fn split_rect(rect: Rect, rooms: &mut Vec<Rect>, depth: usize, rng: &mut impl Rng) {
    if depth == 0 || (rect.width() < 6 && rect.height() < 6) {
        if rect.width() >= 5 && rect.height() >= 4 {
            rooms.push(rect);
        }
        return;
    }
    let split_h = if rect.width() > rect.height() { false }
                  else if rect.height() > rect.width() { true }
                  else { rng.gen_bool(0.5) };

    if split_h {
        if rect.y2.saturating_sub(rect.y1) < 9 { if rect.width() >= 5 { rooms.push(rect); } return; }
        let sy = rng.gen_range(rect.y1 + 4..rect.y2 - 4);
        split_rect(Rect { y2: sy, ..rect }, rooms, depth - 1, rng);
        split_rect(Rect { y1: sy, ..rect }, rooms, depth - 1, rng);
    } else {
        if rect.x2.saturating_sub(rect.x1) < 9 { if rect.height() >= 4 { rooms.push(rect); } return; }
        let sx = rng.gen_range(rect.x1 + 4..rect.x2 - 4);
        split_rect(Rect { x2: sx, ..rect }, rooms, depth - 1, rng);
        split_rect(Rect { x1: sx, ..rect }, rooms, depth - 1, rng);
    }
}

fn connect_rooms(map: &mut Map, a: &Rect, b: &Rect, rng: &mut impl Rng) {
    let (ax, ay) = a.center();
    let (bx, by) = b.center();

    let mx = (ax + bx) / 2;
    let my = (ay + by) / 2;

    for step in 0..5i32 {
        for &sign in &[-1i32, 1] {
            let tx = (mx as i32 + sign * step * (bx as i32 - ax as i32).signum())
                .clamp(1, map.width as i32 - 2) as usize;
            let ty = (my as i32 + sign * step * (by as i32 - ay as i32).signum())
                .clamp(1, map.height as i32 - 2) as usize;
            // 벽(일반/파괴가능 모두)을 도어웨이로 카브한다.
            if map.get_tile(tx, ty).blocks_sight() {
                let neighbors_floor = [(0i32, 1), (0, -1), (1, 0), (-1, 0)]
                    .iter()
                    .filter(|&&(dx, dy)| {
                        let nx = (tx as i32 + dx).clamp(0, map.width as i32 - 1) as usize;
                        let ny = (ty as i32 + dy).clamp(0, map.height as i32 - 1) as usize;
                        map.get_tile(nx, ny) == TileKind::Floor
                    })
                    .count();
                if neighbors_floor >= 2 {
                    map.set_tile(tx, ty, TileKind::Floor);
                    return;
                }
            }
        }
    }
    let _ = rng;
    super::carve_corridor(map, ax, ay, bx, by);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::map::MapGenerator;

    #[test]
    fn 실내_방벽은_파괴가능벽이고_맵_테두리는_일반벽으로_남는다() {
        let gen = BspIndoorGenerator;
        let map = gen.generate(50, 40, 7);
        // 방 외벽 일부가 DestructibleWall.
        let dwall = map.tiles.iter().filter(|t| t.kind == TileKind::DestructibleWall).count();
        assert!(dwall > 0, "실내 방벽은 파괴가능벽이어야 한다");
        // 테두리는 일반 Wall.
        for x in 0..50 {
            assert_eq!(map.get_tile(x, 0), TileKind::Wall, "상단 테두리는 일반 벽");
            assert_eq!(map.get_tile(x, 40 - 1), TileKind::Wall, "하단 테두리는 일반 벽");
        }
        for y in 0..40 {
            assert_eq!(map.get_tile(0, y), TileKind::Wall, "좌측 테두리는 일반 벽");
            assert_eq!(map.get_tile(50 - 1, y), TileKind::Wall, "우측 테두리는 일반 벽");
        }
    }

    #[test]
    fn 같은_시드는_같은_맵을_만든다() {
        let gen = BspIndoorGenerator;
        let a = gen.generate(50, 40, 3);
        let b = gen.generate(50, 40, 3);
        assert_eq!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }
}
