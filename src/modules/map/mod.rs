use bevy::math::{Vec2, Vec3};

mod tests;

pub mod bsp;

// --- Constants ---
/// 맵의 너비 (타일 단위)
pub const MAP_WIDTH: usize = 40 * 4;
/// 맵의 높이 (타일 단위)
pub const MAP_HEIGHT: usize = 25 * 4;
/// 각 타일의 픽셀 크기. 폰트 크기 및 타일 간격에 사용됩니다.
pub const TILE_SIZE: f32 = 16.0;

/// 맵 타일 좌표를 월드 좌표(화면 위치)로 변환합니다.
pub fn tile_to_world_coords(x: usize, y: usize) -> Vec2 {
    // Bevy의 기본 2D 카메라는 월드의 (0,0) 좌표를 화면 중앙으로 인식합니다.
    // 생성된 맵 전체가 화면 중앙에 오도록 하려면, 맵의 월드 좌표를 조정해야 합니다.
    // 맵의 전체 너비와 높이의 절반만큼 모든 타일과 플레이어의 좌표를 왼쪽 아래로 이동시켜
    // 맵의 중앙이 월드의 (0,0)에 위치하도록 만듭니다.
    let screen_width_offset = (MAP_WIDTH as f32 * TILE_SIZE) / 2.0 - TILE_SIZE / 2.0;
    let screen_height_offset = (MAP_HEIGHT as f32 * TILE_SIZE) / 2.0 - TILE_SIZE / 2.0;

    Vec2::new(
        x as f32 * TILE_SIZE - screen_width_offset,
        y as f32 * TILE_SIZE - screen_height_offset,
    )
}

/// 월드 좌표(화면 위치)를 가장 가까운 맵 타일 좌표로 변환합니다.
pub fn world_to_tile_coords(world_pos: Vec3) -> (usize, usize) {
    let screen_width_offset = (MAP_WIDTH as f32 * TILE_SIZE) / 2.0 - TILE_SIZE / 2.0;
    let screen_height_offset = (MAP_HEIGHT as f32 * TILE_SIZE) / 2.0 - TILE_SIZE / 2.0;

    let x = ((world_pos.x + screen_width_offset + TILE_SIZE / 2.0) / TILE_SIZE).floor() as usize;
    let y = ((world_pos.y + screen_height_offset + TILE_SIZE / 2.0) / TILE_SIZE).floor() as usize;

    (x.clamp(0, MAP_WIDTH - 1), y.clamp(0, MAP_HEIGHT - 1))
}
