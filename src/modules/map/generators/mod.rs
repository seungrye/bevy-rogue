pub mod bsp;
pub mod rooms;
pub mod drunkard;
pub mod cellular_automata;
pub mod dla;
pub mod bsp_indoor;
pub mod prefab;
pub mod organic_village;
pub mod grid_village;
pub mod forest;
pub mod perlin;

use super::{Map, MapTile, Rect};

/// 연결되지 않은 바닥 타일을 벽으로 채워 맵의 접근 가능 영역을 단일 연결 요소로 만든다.
/// 중앙이 Floor면 거기서 시작해 중앙 구역이 보존되도록 한다.
pub fn ensure_connectivity(map: &mut Map) {
    let width = map.width;
    let height = map.height;
    let (cx, cy) = (width / 2, height / 2);

    let start = if map.get_tile(cx, cy) == MapTile::Floor {
        Some((cx, cy))
    } else {
        (1..height - 1)
            .flat_map(|y| (1..width - 1).map(move |x| (x, y)))
            .find(|&(x, y)| map.get_tile(x, y) == MapTile::Floor)
    };

    let Some((sx, sy)) = start else { return };

    let mut visited = vec![false; width * height];
    let mut stack = vec![(sx, sy)];
    visited[map.index(sx, sy)] = true;

    while let Some((cx, cy)) = stack.pop() {
        for (dx, dy) in [(-1i32, 0), (1, 0), (0, -1i32), (0, 1)] {
            let nx = cx as i32 + dx;
            let ny = cy as i32 + dy;
            if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                continue;
            }
            let (nx, ny) = (nx as usize, ny as usize);
            let idx = map.index(nx, ny);
            if !visited[idx] && map.tiles[idx] == MapTile::Floor {
                visited[idx] = true;
                stack.push((nx, ny));
            }
        }
    }

    for y in 0..height {
        for x in 0..width {
            let idx = map.index(x, y);
            if map.tiles[idx] == MapTile::Floor && !visited[idx] {
                map.tiles[idx] = MapTile::Wall;
            }
        }
    }
}

/// rooms 가 비어있으면 바닥 타일 분포에서 스폰용 방을 2개 추가한다.
pub fn add_rooms_from_floor(map: &mut Map) {
    if map.rooms.len() >= 2 {
        return;
    }

    let floor_tiles: Vec<(usize, usize)> = (1..map.height - 1)
        .flat_map(|y| (1..map.width - 1).map(move |x| (x, y)))
        .filter(|&(x, y)| map.get_tile(x, y) == MapTile::Floor)
        .collect();

    if floor_tiles.is_empty() {
        return;
    }

    let cx = map.width / 2;
    let cy = map.height / 2;

    if map.rooms.is_empty() {
        let (sx, sy) = floor_tiles
            .iter()
            .min_by_key(|&&(x, y)| {
                let dx = x as i32 - cx as i32;
                let dy = y as i32 - cy as i32;
                dx * dx + dy * dy
            })
            .copied()
            .unwrap_or((cx, cy));
        map.rooms.push(Rect::new(sx.saturating_sub(1), sy.saturating_sub(1), 3, 3));
    }

    if map.rooms.len() < 2 {
        let (rx, ry) = map.rooms[0].center();
        let (ex, ey) = floor_tiles
            .iter()
            .max_by_key(|&&(x, y)| {
                let dx = x as i32 - rx as i32;
                let dy = y as i32 - ry as i32;
                dx * dx + dy * dy
            })
            .copied()
            .unwrap_or((cx, cy));
        map.rooms.push(Rect::new(ex.saturating_sub(1), ey.saturating_sub(1), 3, 3));
    }
}

/// 8방향 이웃 중 벽 타일 개수를 센다. 맵 경계는 벽으로 간주한다.
pub fn count_wall_neighbors(tiles: &[MapTile], width: usize, height: usize, x: usize, y: usize) -> usize {
    let mut count = 0;
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            if dx == 0 && dy == 0 { continue; }
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                count += 1;
            } else {
                let idx = ny as usize * width + nx as usize;
                if tiles[idx] == MapTile::Wall { count += 1; }
            }
        }
    }
    count
}

/// 두 좌표 사이를 L자형 복도로 이어준다.
pub fn carve_corridor(map: &mut Map, x1: usize, y1: usize, x2: usize, y2: usize) {
    let mut x = x1 as i32;
    let mut y = y1 as i32;
    let tx = x2 as i32;
    let ty = y2 as i32;
    while x != tx {
        map.set_tile(x as usize, y as usize, MapTile::Floor);
        x += if x < tx { 1 } else { -1 };
    }
    while y != ty {
        map.set_tile(x as usize, y as usize, MapTile::Floor);
        y += if y < ty { 1 } else { -1 };
    }
    map.set_tile(x as usize, y as usize, MapTile::Floor);
}


#[cfg(test)]
mod tests {
    use super::super::MapGenerator;
    use super::super::MapTile;
    use super::{bsp, rooms, drunkard, cellular_automata, dla, bsp_indoor, prefab, organic_village, grid_village, forest, perlin};

    const W: usize = 40;
    const H: usize = 30;

    fn check_contract(gen: &dyn MapGenerator) {
        let name = gen.name();
        let map = gen.generate(W, H);

        // 바닥 비율 ≥ 10%
        let floor_count = map.tiles.iter().filter(|&&t| t == MapTile::Floor).count();
        let floor_ratio = floor_count as f32 / (W * H) as f32;
        assert!(
            floor_ratio >= 0.10,
            "[{}] 바닥 비율 {:.1}% < 10%",
            name, floor_ratio * 100.0
        );

        // 경계는 모두 벽
        for x in 0..W {
            assert_eq!(map.get_tile(x, 0), MapTile::Wall, "[{}] 상단 경계 ({},{}) 가 벽이 아님", name, x, 0);
            assert_eq!(map.get_tile(x, H - 1), MapTile::Wall, "[{}] 하단 경계 ({},{}) 가 벽이 아님", name, x, H - 1);
        }
        for y in 0..H {
            assert_eq!(map.get_tile(0, y), MapTile::Wall, "[{}] 좌측 경계 ({},{}) 가 벽이 아님", name, 0, y);
            assert_eq!(map.get_tile(W - 1, y), MapTile::Wall, "[{}] 우측 경계 ({},{}) 가 벽이 아님", name, W - 1, y);
        }

        // 방 ≥ 2개
        assert!(
            map.rooms.len() >= 2,
            "[{}] 방 개수 {} < 2",
            name, map.rooms.len()
        );
    }

    // ensure_connectivity 단위 테스트
    #[test]
    fn ensure_connectivity_keeps_center_component_over_corner() {
        use super::super::Map;
        use super::ensure_connectivity;

        // 20×20 맵: 코너(1,1)에 고립된 작은 Floor 구역 + 중앙에 큰 Floor 구역
        let (w, h) = (20, 20);
        let mut map = Map::new(w, h);

        // 좌상단 고립 구역 (스캔 순서상 먼저 발견됨)
        map.set_tile(1, 1, MapTile::Floor);
        map.set_tile(2, 1, MapTile::Floor);
        map.set_tile(1, 2, MapTile::Floor);

        // 중앙 3×3 구역
        let (cx, cy) = (w / 2, h / 2);
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                map.set_tile((cx as i32 + dx) as usize, (cy as i32 + dy) as usize, MapTile::Floor);
            }
        }

        ensure_connectivity(&mut map);

        // 중앙 Floor 보존
        assert_eq!(map.get_tile(cx, cy), MapTile::Floor, "중앙 타일이 유지돼야 한다");
        // 코너 고립 구역 제거
        assert_eq!(map.get_tile(1, 1), MapTile::Wall, "고립된 코너 구역은 제거돼야 한다");
    }

    #[test]
    fn ensure_connectivity_falls_back_when_center_is_wall() {
        use super::super::Map;
        use super::ensure_connectivity;

        let (w, h) = (10, 10);
        let mut map = Map::new(w, h);
        // 중앙은 Wall, 코너에만 Floor
        map.set_tile(1, 1, MapTile::Floor);
        map.set_tile(2, 1, MapTile::Floor);

        ensure_connectivity(&mut map);

        // 중앙이 Wall이면 첫 번째 Floor 구역을 유지
        assert_eq!(map.get_tile(1, 1), MapTile::Floor);
    }

    #[test] fn bsp_contract()              { check_contract(&bsp::BspGenerator); }
    #[test] fn rooms_contract()            { check_contract(&rooms::SimpleRoomsGenerator); }
    #[test] fn drunkard_contract()         { check_contract(&drunkard::DrunkardWalkGenerator); }
    #[test] fn cellular_automata_contract(){ check_contract(&cellular_automata::CellularAutomataGenerator); }
    #[test] fn dla_contract()              { check_contract(&dla::DlaGenerator); }
    #[test] fn bsp_indoor_contract()       { check_contract(&bsp_indoor::BspIndoorGenerator); }
    #[test] fn prefab_contract()           { check_contract(&prefab::PrefabGenerator); }
    #[test] fn organic_village_contract()  { check_contract(&organic_village::OrganicVillageGenerator); }
    #[test] fn grid_village_contract()     { check_contract(&grid_village::GridVillageGenerator); }
    #[test] fn forest_contract()           { check_contract(&forest::ForestGenerator); }
    #[test] fn perlin_contract()           { check_contract(&perlin::PerlinNoiseGenerator); }

    // CLI --algorithm 인수와 select_by_name 이 사용하는 등록 이름 검증
    #[test]
    fn generator_names_match_spec() {
        assert_eq!(bsp::BspGenerator.name(),                                    "bsp");
        assert_eq!(rooms::SimpleRoomsGenerator.name(),                          "simple_rooms");
        assert_eq!(drunkard::DrunkardWalkGenerator.name(),                      "drunkard");
        assert_eq!(cellular_automata::CellularAutomataGenerator.name(),         "cellular_automata");
        assert_eq!(dla::DlaGenerator.name(),                                    "dla");
        assert_eq!(bsp_indoor::BspIndoorGenerator.name(),                       "bsp_indoor");
        assert_eq!(prefab::PrefabGenerator.name(),                              "prefab");
        assert_eq!(organic_village::OrganicVillageGenerator.name(),             "organic_village");
        assert_eq!(grid_village::GridVillageGenerator.name(),                   "grid_village");
        assert_eq!(forest::ForestGenerator.name(),                              "forest");
        assert_eq!(perlin::PerlinNoiseGenerator.name(),                         "perlin");
    }
}
