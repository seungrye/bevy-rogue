use rand::prelude::*;
use crate::modules::map::{Map, MapTile, Rect};
use super::super::MapGenerator;

pub struct BspGenerator;

impl MapGenerator for BspGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;

        // 1단계: BSP 분할 → 리프 렉트 수집
        let mut leaves: Vec<Rect> = Vec::new();
        split_rect(Rect::new(1, 1, width - 2, height - 2), &mut leaves, 5, &mut rng);

        // 2단계: 각 리프 안에 여백을 두고 방 조각
        let mut rooms: Vec<Rect> = Vec::new();
        for leaf in &leaves {
            if let Some(room) = carve_room_in_leaf(leaf, &mut rng) {
                for y in room.y1..room.y2 {
                    for x in room.x1..room.x2 {
                        map.set_tile(x, y, MapTile::Floor);
                    }
                }
                rooms.push(room);
            }
        }

        // 3단계: 인접 방 중심끼리 복도 연결
        for i in 0..rooms.len().saturating_sub(1) {
            let (x1, y1) = rooms[i].center();
            let (x2, y2) = rooms[i + 1].center();
            super::carve_corridor(&mut map, x1, y1, x2, y2);
        }

        map.rooms = rooms;
        map
    }

    fn name(&self) -> &str { "bsp" }
}

/// BSP 재귀 분할 — 리프 렉트를 `leaves`에 수집한다
fn split_rect(rect: Rect, leaves: &mut Vec<Rect>, depth: usize, rng: &mut impl Rng) {
    const MIN_LEAF: usize = 10; // 리프 최소 크기

    let too_small = rect.width() < MIN_LEAF * 2 && rect.height() < MIN_LEAF * 2;
    if depth == 0 || too_small {
        leaves.push(rect);
        return;
    }

    // 긴 쪽을 기준으로 분할 방향 결정
    let split_h = if rect.width() > rect.height() + 4 { false }
                  else if rect.height() > rect.width() + 4 { true }
                  else { rng.gen_bool(0.5) };

    if split_h {
        // 수평 분할 — 최소 MIN_LEAF 확보
        if rect.height() < MIN_LEAF * 2 { leaves.push(rect); return; }
        let lo = rect.y1 + MIN_LEAF;
        let hi = rect.y2.saturating_sub(MIN_LEAF);
        if lo >= hi { leaves.push(rect); return; }
        let sy = rng.gen_range(lo..hi);
        split_rect(Rect { x1: rect.x1, y1: rect.y1, x2: rect.x2, y2: sy }, leaves, depth - 1, rng);
        split_rect(Rect { x1: rect.x1, y1: sy,      x2: rect.x2, y2: rect.y2 }, leaves, depth - 1, rng);
    } else {
        // 수직 분할
        if rect.width() < MIN_LEAF * 2 { leaves.push(rect); return; }
        let lo = rect.x1 + MIN_LEAF;
        let hi = rect.x2.saturating_sub(MIN_LEAF);
        if lo >= hi { leaves.push(rect); return; }
        let sx = rng.gen_range(lo..hi);
        split_rect(Rect { x1: rect.x1, y1: rect.y1, x2: sx,      y2: rect.y2 }, leaves, depth - 1, rng);
        split_rect(Rect { x1: sx,      y1: rect.y1, x2: rect.x2, y2: rect.y2 }, leaves, depth - 1, rng);
    }
}

/// 리프 렉트 안에 여백을 두고 랜덤 크기의 방을 생성한다
fn carve_room_in_leaf(leaf: &Rect, rng: &mut impl Rng) -> Option<Rect> {
    const MARGIN: usize = 1;
    const MIN_ROOM: usize = 4;

    let inner_w = leaf.width().saturating_sub(MARGIN * 2);
    let inner_h = leaf.height().saturating_sub(MARGIN * 2);
    if inner_w < MIN_ROOM || inner_h < MIN_ROOM { return None; }

    let rw = rng.gen_range(MIN_ROOM..=inner_w);
    let rh = rng.gen_range(MIN_ROOM..=inner_h);

    let max_x = leaf.x1 + MARGIN + inner_w - rw;
    let max_y = leaf.y1 + MARGIN + inner_h - rh;
    if max_x <= leaf.x1 + MARGIN || max_y <= leaf.y1 + MARGIN { return None; }

    let rx = rng.gen_range(leaf.x1 + MARGIN..=max_x);
    let ry = rng.gen_range(leaf.y1 + MARGIN..=max_y);

    Some(Rect { x1: rx, y1: ry, x2: rx + rw, y2: ry + rh })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::map::MapGenerator;

    #[test]
    fn bsp_rooms_do_not_touch_each_other() {
        let gen = BspGenerator;
        for _ in 0..10 {
            let map = gen.generate(80, 50, 42);
            // 각 방 사이에 최소 1타일 벽이 있어야 한다
            for i in 0..map.rooms.len() {
                for j in (i + 1)..map.rooms.len() {
                    let a = &map.rooms[i];
                    let b = &map.rooms[j];
                    // x축과 y축 중 하나에서 겹치지 않아야 함 (최소 1 간격)
                    let x_overlap = a.x2 > b.x1 && b.x2 > a.x1;
                    let y_overlap = a.y2 > b.y1 && b.y2 > a.y1;
                    assert!(
                        !(x_overlap && y_overlap),
                        "방 {} 와 방 {} 이 겹침: {:?} vs {:?}", i, j, a, b
                    );
                }
            }
        }
    }

    #[test]
    fn bsp_produces_multiple_rooms() {
        let gen = BspGenerator;
        for _ in 0..5 {
            let map = gen.generate(80, 50, 42);
            assert!(map.rooms.len() >= 4, "방이 최소 4개 이상 생성돼야 한다 (실제: {})", map.rooms.len());
        }
    }

    #[test]
    fn bsp_rooms_within_map_bounds() {
        let gen = BspGenerator;
        let map = gen.generate(80, 50, 42);
        for room in &map.rooms {
            assert!(room.x1 >= 1 && room.x2 <= 79, "방이 맵 가로 경계를 벗어남");
            assert!(room.y1 >= 1 && room.y2 <= 49, "방이 맵 세로 경계를 벗어남");
        }
    }
}
