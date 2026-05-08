use rand::prelude::*;
use rand::thread_rng;
use super::{Map, TileKind};

/// Drunkard's Walk (취한 보행자) 알고리즘을 사용하여 맵을 생성합니다.
///
/// 맵의 중앙에서 시작하여 무작위 방향으로 이동하며 바닥을 파내려가는 유기적인 알고리즘입니다.
/// 동굴이나 자연스러운 지형을 생성할 때 주로 사용됩니다.
///
/// # Arguments
/// * `width` - 생성할 맵의 너비
/// * `height` - 생성할 맵의 높이
///
/// # Returns
/// 유기적인 구조를 가진 Map 인스턴스
pub fn generate_drunkard_map(width: usize, height: usize) -> Map {
    let mut map = Map::new(width, height);
    let mut rng = thread_rng();
    
    // 중앙에서 시작
    let mut x = width / 2; 
    let mut y = height / 2;
    map.set_tile(x, y, TileKind::Floor);

    // 전체의 40% 정도가 바닥이 될 때까지 파냄
    let target_floor_count = (width * height) as f32 * 0.4;
    let mut floor_count = 1;

    while floor_count < target_floor_count as usize {
        let dir = rng.gen_range(0..4);
        match dir {
            0 => if x > 1 { x -= 1; },          // 서쪽
            1 => if x < width - 2 { x += 1; },  // 동쪽
            2 => if y > 1 { y -= 1; },          // 남쪽
            3 => if y < height - 2 { y += 1; }, // 북쪽
            _ => {}
        }
        
        // 새로 파낸 곳이 벽인 경우에만 바닥으로 바꾸고 카운트 증가
        if map.get_tile(x, y) == TileKind::Wall {
            map.set_tile(x, y, TileKind::Floor);
            floor_count += 1;
        }
    }
    map
}
