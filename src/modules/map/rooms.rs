use rand::prelude::*;
use rand::thread_rng;
use super::{Map, TileKind, Rect};

/// 무작위 방 배치 알고리즘을 사용하여 맵을 생성합니다.
///
/// 맵의 빈 공간에 사각형 모양의 방을 겹치지 않게 배치한 뒤, 각 방을 복도로 연결합니다.
///
/// # 인수
/// * `width` - 생성할 맵의 너비
/// * `height` - 생성할 맵의 높이
///
/// # 반환값
/// 무작위 방 배치 알고리즘으로 생성된 Map 인스턴스
pub fn generate_rooms_map(width: usize, height: usize) -> Map {
    let mut map = Map::new(width, height);
    let mut rooms = Vec::new();
    let max_rooms = 10;
    for _ in 0..max_rooms {
        let w = thread_rng().gen_range(4..8);
        let h = thread_rng().gen_range(4..8);
        // 맵 테두리 1칸 제외
        let x = thread_rng().gen_range(1..width - w - 1);
        let y = thread_rng().gen_range(1..height - h - 1);
        let new_room = Rect::new(x, y, w, h);

        // 방 겹침 확인
        let mut ok = true;
        for other_room in rooms.iter() {
            if intersect(&new_room, other_room) { ok = false; break; }
        }

        if ok {
            // 바닥 타일로 변환
            for ry in new_room.y1..new_room.y2 {
                for rx in new_room.x1..new_room.x2 {
                    map.set_tile(rx, ry, TileKind::Floor);
                }
            }

            // 이전 방과 연결
            if !rooms.is_empty() {
                let (new_x, new_y) = new_room.center();
                let (prev_x, prev_y) = rooms[rooms.len()-1].center();
                // 50% 확률로 수평 또는 수직 이동 순서 결정
                if thread_rng().gen_range(0..2) == 1 {
                    apply_horizontal_tunnel(&mut map, prev_x, new_x, prev_y);
                    apply_vertical_tunnel(&mut map, prev_y, new_y, new_x);
                } else {
                    apply_vertical_tunnel(&mut map, prev_y, new_y, prev_x);
                    apply_horizontal_tunnel(&mut map, prev_x, new_x, new_y);
                }
            }
            rooms.push(new_room);
        }
    }
    map.rooms = rooms;
    map
}

/// 두 사각형 영역이 겹치는지 확인합니다.
///
/// # 인수
/// * `r1`, `r2` - 비교할 두 Rect
///
/// # 반환값
/// 겹치면 true, 아니면 false
fn intersect(r1: &Rect, r2: &Rect) -> bool {
    r1.x1 <= r2.x2 && r1.x2 >= r2.x1 && r1.y1 <= r2.y2 && r1.y2 >= r2.y1
}

/// 특정 y 좌표에서 두 x 좌표 사이를 잇는 수평 복도를 생성합니다.
///
/// # 인수
/// * `map` - 타일을 수정할 맵 참조
/// * `x1`, `x2` - 시작 및 끝 x 좌표
/// * `y` - 고정된 y 좌표
fn apply_horizontal_tunnel(map: &mut Map, x1: usize, x2: usize, y: usize) {
    use std::cmp::{min, max};
    for x in min(x1, x2)..=max(x1, x2) { map.set_tile(x, y, TileKind::Floor); }
}

/// 특정 x 좌표에서 두 y 좌표 사이를 잇는 수직 복도를 생성합니다.
///
/// # 인수
/// * `map` - 타일을 수정할 맵 참조
/// * `y1`, `y2` - 시작 및 끝 y 좌표
/// * `x` - 고정된 x 좌표
fn apply_vertical_tunnel(map: &mut Map, y1: usize, y2: usize, x: usize) {
    use std::cmp::{min, max};
    for y in min(y1, y2)..=max(y1, y2) { map.set_tile(x, y, TileKind::Floor); }
}
