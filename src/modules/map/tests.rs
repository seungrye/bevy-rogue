#[test]
fn test_coordinate_conversion_round_trip() {
    // Given: 특정 타일 좌표가 주어졌을 때
    let original_tile_x = 25;
    let original_tile_y = 15;

    // When: 타일 좌표를 월드 좌표로 변환하고, 다시 타일 좌표로 변환하면
    let world_pos_vec2 = crate::modules::map::tile_to_world_coords(original_tile_x, original_tile_y);
    let world_pos_vec3 = bevy::math::Vec3::new(world_pos_vec2.x, world_pos_vec2.y, 0.0);
    let (converted_x, converted_y) = crate::modules::map::world_to_tile_coords(world_pos_vec3);

    // Then: 결과는 원래 타일 좌표와 같아야 합니다.
    assert_eq!(
        (original_tile_x, original_tile_y),
        (converted_x, converted_y),
        "Coordinate conversion round-trip failed: tile -> world -> tile."
    );
}

#[test]
fn test_world_to_tile_snapping() {
    // Given: 특정 타일의 중심 월드 좌표가 주어졌을 때
    let center_world_pos = crate::modules::map::tile_to_world_coords(10, 10);

    // When: 해당 타일 경계 내에서 약간 벗어난 월드 좌표를 사용하면
    let offset_world_pos = bevy::math::Vec3::new(center_world_pos.x + 4.0, center_world_pos.y - 3.0, 0.0);
    let (snapped_x, snapped_y) = crate::modules::map::world_to_tile_coords(offset_world_pos);

    // Then: 좌표는 원래 타일로 정확하게 스냅되어야 합니다.
    assert_eq!((10, 10), (snapped_x, snapped_y), "World position did not snap to the correct tile center.");
}