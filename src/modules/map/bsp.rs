use rand::prelude::*;
use rand::thread_rng;
use super::{Map, TileKind, Rect};

/// 이진 공간 분할(BSP) 알고리즘을 사용하여 맵을 생성합니다.
///
/// 맵을 반복적으로 분할하여 방을 만들고, 생성된 방들을 복도로 연결합니다.
///
/// # 인수
/// * `width` - 생성할 맵의 너비
/// * `height` - 생성할 맵의 높이
///
/// # 반환값
/// BSP 알고리즘으로 생성된 Map 인스턴스
pub fn generate_bsp_map(width: usize, height: usize) -> Map {
    let mut map = Map::new(width, height);
    let mut rooms = Vec::new();
    // 맵 전체 영역에서 시작 (테두리 1칸 제외)
    let root = Rect::new(1, 1, width - 2, height - 2);
    // 재귀적으로 영역 분할
    split_rect(root, &mut rooms, 5);

    // 생성된 영역들을 실제 방(바닥 타일)으로 변환
    for room in rooms.iter() {
        for y in room.y1..room.y2 {
            for x in room.x1..room.x2 {
                map.set_tile(x, y, TileKind::Floor);
            }
        }
    }

    // 방들을 복도로 연결
    for i in 0..rooms.len() - 1 {
        let (x1, y1) = rooms[i].center();
        let (x2, y2) = rooms[i+1].center();
        draw_corridor(&mut map, x1, y1, x2, y2);
    }
    map.rooms = rooms;
    map
}

/// 사각형 영역을 재귀적으로 분할합니다.
///
/// # 인수
/// * `rect` - 분할할 대상 영역
/// * `rooms` - 최종 분할된 영역들을 저장할 벡터
/// * `depth` - 재귀 분할 깊이
fn split_rect(rect: Rect, rooms: &mut Vec<Rect>, depth: usize) {
    // 깊이가 0이거나 영역이 충분히 작으면 분할 중단
    if depth == 0 || (rect.width() < 10 && rect.height() < 10) {
        rooms.push(rect);
        return;
    }
    let mut rng = thread_rng();
    // 가로 또는 세로 분할 방향 결정
    let split_horizontal = if rect.width() > rect.height() { 
        false 
    } else if rect.height() > rect.width() { 
        true 
    } else { 
        rng.gen_bool(0.5) 
    };

    if split_horizontal {
        // 세로로 분할 (수평선 긋기)
        let split_y = rng.gen_range(rect.y1 + 3 .. rect.y2 - 3);
        split_rect(Rect { x1: rect.x1, y1: rect.y1, x2: rect.x2, y2: split_y }, rooms, depth - 1);
        split_rect(Rect { x1: rect.x1, y1: split_y, x2: rect.x2, y2: rect.y2 }, rooms, depth - 1);
    } else {
        // 가로로 분할 (수직선 긋기)
        let split_x = rng.gen_range(rect.x1 + 3 .. rect.x2 - 3);
        split_rect(Rect { x1: rect.x1, y1: rect.y1, x2: split_x, y2: rect.y2 }, rooms, depth - 1);
        split_rect(Rect { x1: split_x, y1: rect.y1, x2: rect.x2, y2: rect.y2 }, rooms, depth - 1);
    }
}

/// 두 점 사이를 잇는 'L'자 모양의 복도를 생성합니다.
///
/// # 인수
/// * `map` - 타일을 수정할 맵 참조
/// * `x1`, `y1` - 시작 좌표
/// * `x2`, `y2` - 끝 좌표
fn draw_corridor(map: &mut Map, x1: usize, y1: usize, x2: usize, y2: usize) {
    let mut x = x1; let mut y = y1;
    // 수평 이동
    while x != x2 { 
        map.set_tile(x, y, TileKind::Floor); 
        if x < x2 { x += 1; } else { x -= 1; } 
    }
    // 수직 이동
    while y != y2 { 
        map.set_tile(x, y, TileKind::Floor); 
        if y < y2 { y += 1; } else { y -= 1; } 
    }
}
