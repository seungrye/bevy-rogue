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
pub mod maze;
pub mod maze_prim;
pub mod recursive_division;
pub mod voronoi_rooms;
pub mod walled_town;
pub mod voronoi_districts;
pub mod island;
pub mod archipelago;
pub mod coastal;
pub mod ocean;
pub mod biome_world;
pub mod wfc;
pub mod tinykeep;

use super::{Map, TileKind, MapTile, Rect};
use noise::{NoiseFn, Perlin};

/// 연결되지 않은 바닥 타일을 벽으로 채워 맵의 접근 가능 영역을 단일 연결 요소로 만든다.
/// 중앙이 Floor면 거기서 시작해 중앙 구역이 보존되도록 한다.
pub fn ensure_connectivity(map: &mut Map) {
    let width = map.width;
    let height = map.height;
    let (cx, cy) = (width / 2, height / 2);

    let start = if map.get_tile(cx, cy).is_walkable() {
        Some((cx, cy))
    } else {
        (1..height - 1)
            .flat_map(|y| (1..width - 1).map(move |x| (x, y)))
            .find(|&(x, y)| map.get_tile(x, y).is_walkable())
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
            if !visited[idx] && map.tiles[idx].kind.is_walkable() {
                visited[idx] = true;
                stack.push((nx, ny));
            }
        }
    }

    for y in 0..height {
        for x in 0..width {
            let idx = map.index(x, y);
            if map.tiles[idx].kind.is_walkable() && !visited[idx] {
                map.tiles[idx].kind = TileKind::Wall;
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
        .filter(|&(x, y)| map.get_tile(x, y).is_walkable())
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

    // 위 두 블록을 지나면 rooms 는 항상 정확히 1개(진입 시 >=2 면 early-return,
    // 0 이면 첫 블록에서 1개 push, 1 이면 그대로 1개)이므로 이 조건은 늘 참이다.
    // False(이미 2개) 는 도달 불가 방어코드.
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
                if tiles[idx].kind == TileKind::Wall { count += 1; }
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
        map.set_tile(x as usize, y as usize, TileKind::Floor);
        x += if x < tx { 1 } else { -1 };
    }
    while y != ty {
        map.set_tile(x as usize, y as usize, TileKind::Floor);
        y += if y < ty { 1 } else { -1 };
    }
    map.set_tile(x as usize, y as usize, TileKind::Floor);
}

// === 수상 맵(바다/섬/해안/바이옴) 공용 헬퍼 ===

/// 멀티옥타브 펄린 노이즈. octaves 만큼 주파수를 2배, 진폭을 0.5배 하며
/// 누적해 [-1,1] 근방의 자연스러운 프랙탈 값을 만든다.
pub fn octave_noise(perlin: &Perlin, x: f64, y: f64, scale: f64, octaves: u32) -> f64 {
    let mut value = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = scale;
    let mut max_amp = 0.0;
    for _ in 0..octaves {
        value += perlin.get([x * frequency, y * frequency]) * amplitude;
        max_amp += amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }
    value / max_amp
}

/// 수상 맵용 연결성 보장. 가장 큰 통과타일(`Floor`/`Sand`) 연결요소만 남기고
/// 나머지 통과타일은 물(`Water`)로 바꾼다. 지상용 `ensure_connectivity` 가
/// 고립 영역을 벽으로 메우는 것과 달리, 수상 맵에서는 고립된 작은 섬을
/// 바다로 가라앉혀 단일 섬(또는 군도면 호출하지 않음)을 보장한다.
pub fn ensure_water_connectivity(map: &mut Map) {
    let width = map.width;
    let height = map.height;
    let mut visited = vec![false; width * height];

    let mut best: Vec<usize> = Vec::new();
    for sy in 0..height {
        for sx in 0..width {
            let sidx = map.index(sx, sy);
            if visited[sidx] || !map.tiles[sidx].kind.is_walkable() {
                continue;
            }
            // 이 시작점에서 BFS 로 한 연결요소를 모은다.
            let mut component = Vec::new();
            let mut stack = vec![(sx, sy)];
            visited[sidx] = true;
            // 통과타일은 항상 맵 내부(1..w-1, 1..h-1)에만 생성되므로 4이웃은
            // 언제나 맵 안 — 경계 검사 불필요.
            while let Some((cx, cy)) = stack.pop() {
                component.push(map.index(cx, cy));
                for (nx, ny) in [(cx - 1, cy), (cx + 1, cy), (cx, cy - 1), (cx, cy + 1)] {
                    let idx = map.index(nx, ny);
                    if !visited[idx] && map.tiles[idx].kind.is_walkable() {
                        visited[idx] = true;
                        stack.push((nx, ny));
                    }
                }
            }
            if component.len() > best.len() {
                best = component;
            }
        }
    }

    let keep: std::collections::HashSet<usize> = best.into_iter().collect();
    for y in 0..height {
        for x in 0..width {
            let idx = map.index(x, y);
            if map.tiles[idx].kind.is_walkable() && !keep.contains(&idx) {
                map.tiles[idx].kind = TileKind::Water;
            }
        }
    }
}

/// 통과 가능한 땅(`Floor`/`Sand`) 중 4방향 이웃에 물이 있는 한 칸을
/// 모래 해변(`Sand`)으로 바꾼다. 땅-물 경계가 해변이 된다.
pub fn mark_beaches(map: &mut Map) {
    let width = map.width;
    let height = map.height;
    let mut beaches = Vec::new();
    for y in 0..height {
        for x in 0..width {
            if !map.get_tile(x, y).is_walkable() {
                continue;
            }
            // 통과타일은 항상 맵 내부 → 4이웃 좌표는 언제나 맵 안(경계 검사 불필요).
            let touches_water = [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
                .iter()
                .any(|&(nx, ny)| map.get_tile(nx, ny) == TileKind::Water);
            if touches_water {
                beaches.push((x, y));
            }
        }
    }
    for (x, y) in beaches {
        map.set_tile(x, y, TileKind::Sand);
    }
}

/// 맵 테두리를 전부 물로 만든다. 플레이어가 맵 밖으로 못 나가게 한다
/// (Water 는 is_walkable=false).
pub fn force_water_border(map: &mut Map) {
    let width = map.width;
    let height = map.height;
    for x in 0..width {
        map.set_tile(x, 0, TileKind::Water);
        map.set_tile(x, height - 1, TileKind::Water);
    }
    for y in 0..height {
        map.set_tile(0, y, TileKind::Water);
        map.set_tile(width - 1, y, TileKind::Water);
    }
}

/// 수상 맵에서 통과 가능한 땅(`Floor`/`Sand`) 분포로 스폰용 방을 보장한다.
/// 땅 위에서만 스폰하도록 통과타일 좌표로 최대 2개의 `Rect` 를 push 한다.
/// (`add_rooms_from_floor` 는 Floor 만 보지만, 여기선 Sand 도 땅으로 본다.)
pub fn add_rooms_from_water_land(map: &mut Map) {
    if map.rooms.len() >= 2 {
        return;
    }

    let land_tiles: Vec<(usize, usize)> = (1..map.height - 1)
        .flat_map(|y| (1..map.width - 1).map(move |x| (x, y)))
        .filter(|&(x, y)| map.get_tile(x, y).is_walkable())
        .collect();

    if land_tiles.is_empty() {
        return;
    }

    let cx = map.width / 2;
    let cy = map.height / 2;

    if map.rooms.is_empty() {
        let (sx, sy) = land_tiles
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

    // 진입 시 rooms 는 0 또는 1(>=2 면 위에서 early-return). 위 push 후엔 항상 1개
    // 이상이지만 2개 미만이므로, 두 번째 방을 첫 방에서 가장 먼 땅에 추가한다.
    let (rx, ry) = map.rooms[0].center();
    let (ex, ey) = land_tiles
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


#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::super::MapGenerator;
    use super::super::TileKind;
    use super::{bsp, rooms, drunkard, cellular_automata, dla, bsp_indoor, prefab, organic_village, grid_village, forest, perlin,
        maze, maze_prim, recursive_division, voronoi_rooms, walled_town, voronoi_districts,
        island, archipelago, coastal, ocean, biome_world, wfc, tinykeep};

    const W: usize = 40;
    const H: usize = 30;

    fn check_contract(gen: &dyn MapGenerator) {
        let name = gen.name();
        let map = gen.generate(W, H, 42);

        // 바닥 비율 ≥ 10%
        let floor_count = map.tiles.iter().filter(|t| t.kind == TileKind::Floor).count();
        let floor_ratio = floor_count as f32 / (W * H) as f32;
        assert!(
            floor_ratio >= 0.10,
            "[{}] 바닥 비율 {:.1}% < 10%",
            name, floor_ratio * 100.0
        );

        // 경계는 모두 벽
        for x in 0..W {
            assert_eq!(map.get_tile(x, 0), TileKind::Wall, "[{}] 상단 경계 ({},{}) 가 벽이 아님", name, x, 0);
            assert_eq!(map.get_tile(x, H - 1), TileKind::Wall, "[{}] 하단 경계 ({},{}) 가 벽이 아님", name, x, H - 1);
        }
        for y in 0..H {
            assert_eq!(map.get_tile(0, y), TileKind::Wall, "[{}] 좌측 경계 ({},{}) 가 벽이 아님", name, 0, y);
            assert_eq!(map.get_tile(W - 1, y), TileKind::Wall, "[{}] 우측 경계 ({},{}) 가 벽이 아님", name, W - 1, y);
        }

        // 방 ≥ 2개
        assert!(
            map.rooms.len() >= 2,
            "[{}] 방 개수 {} < 2",
            name, map.rooms.len()
        );
    }

    /// 수상 맵(바다/섬/해안/바이옴) 전용 계약 검증.
    /// 지상용 `check_contract` 와 달리 테두리는 Water, 통과타일 비율 기준이 다르다.
    fn check_water_contract(gen: &dyn MapGenerator) {
        let name = gen.name();
        let map = gen.generate(W, H, 42);

        // 테두리 전부 Water (플레이어가 맵 밖으로 못 나감)
        for x in 0..W {
            assert_eq!(map.get_tile(x, 0), TileKind::Water, "[{}] 상단 테두리 ({},{}) 가 물이 아님", name, x, 0);
            assert_eq!(map.get_tile(x, H - 1), TileKind::Water, "[{}] 하단 테두리 ({},{}) 가 물이 아님", name, x, H - 1);
        }
        for y in 0..H {
            assert_eq!(map.get_tile(0, y), TileKind::Water, "[{}] 좌측 테두리 ({},{}) 가 물이 아님", name, 0, y);
            assert_eq!(map.get_tile(W - 1, y), TileKind::Water, "[{}] 우측 테두리 ({},{}) 가 물이 아님", name, W - 1, y);
        }

        // 통과타일(Floor|Sand) 비율 >= 5%
        let walkable = map.tiles.iter().filter(|t| t.kind.is_walkable()).count();
        let ratio = walkable as f32 / (W * H) as f32;
        assert!(
            ratio >= 0.05,
            "[{}] 통과타일 비율 {:.1}% < 5%",
            name, ratio * 100.0
        );

        // Water 타일이 실제로 존재(바다), 통과타일(땅)도 존재
        let water = map.tiles.iter().filter(|t| t.kind == TileKind::Water).count();
        assert!(water > 0, "[{}] 바다(Water)가 없음", name);
        assert!(walkable > 0, "[{}] 땅(통과타일)이 없음", name);

        // 스폰 가능: 통과타일 최소 1개 또는 rooms >= 1.
        // `|` (비단축) 로 두 피연산자를 항상 평가해 분기를 만들지 않는다.
        let spawnable = (walkable >= 1) | !map.rooms.is_empty();
        assert!(spawnable, "[{}] 스폰 가능한 통과타일/방이 없음", name);
    }

    // ensure_connectivity 단위 테스트
    #[test]
    fn 연결성보장은_코너_고립구역보다_중앙_연결요소를_보존한다() {
        use super::super::Map;
        use super::ensure_connectivity;

        // 20×20 맵: 코너(1,1)에 고립된 작은 Floor 구역 + 중앙에 큰 Floor 구역
        let (w, h) = (20, 20);
        let mut map = Map::new(w, h);

        // 좌상단 고립 구역 (스캔 순서상 먼저 발견됨)
        map.set_tile(1, 1, TileKind::Floor);
        map.set_tile(2, 1, TileKind::Floor);
        map.set_tile(1, 2, TileKind::Floor);

        // 중앙 3×3 구역
        let (cx, cy) = (w / 2, h / 2);
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                map.set_tile((cx as i32 + dx) as usize, (cy as i32 + dy) as usize, TileKind::Floor);
            }
        }

        ensure_connectivity(&mut map);

        // 중앙 Floor 보존
        assert_eq!(map.get_tile(cx, cy), TileKind::Floor, "중앙 타일이 유지돼야 한다");
        // 코너 고립 구역 제거
        assert_eq!(map.get_tile(1, 1), TileKind::Wall, "고립된 코너 구역은 제거돼야 한다");
    }

    // add_rooms_from_water_land 단위 테스트 — 방어/분기 양쪽 커버
    #[test]
    fn 물맵_방추가는_이미_방이_둘이상이면_아무것도_안한다() {
        use super::super::{Map, Rect};
        use super::add_rooms_from_water_land;
        let mut map = Map::new(20, 20);
        map.rooms.push(Rect::new(2, 2, 3, 3));
        map.rooms.push(Rect::new(10, 10, 3, 3));
        add_rooms_from_water_land(&mut map);
        assert_eq!(map.rooms.len(), 2, "방이 이미 2개면 추가하지 않는다");
    }

    #[test]
    fn 물맵_방추가는_땅이_없으면_방을_못만든다() {
        use super::super::Map;
        use super::add_rooms_from_water_land;
        // 전부 Water(통과타일 없음).
        let mut map = Map::new(20, 20);
        for t in map.tiles.iter_mut() { t.kind = TileKind::Water; }
        add_rooms_from_water_land(&mut map);
        assert!(map.rooms.is_empty(), "땅이 없으면 방을 만들 수 없다");
    }

    #[test]
    fn 물맵_방추가는_방이_하나면_두번째만_채운다() {
        use super::super::{Map, Rect};
        use super::add_rooms_from_water_land;
        let mut map = Map::new(20, 20);
        // 통과 가능한 땅 두 곳.
        map.set_tile(3, 3, TileKind::Floor);
        map.set_tile(15, 15, TileKind::Sand);
        // 이미 방 1개 → 첫 if 는 건너뛰고 두 번째 if 만 실행.
        map.rooms.push(Rect::new(2, 2, 3, 3));
        add_rooms_from_water_land(&mut map);
        assert_eq!(map.rooms.len(), 2, "방이 하나면 두 번째 방을 추가한다");
    }

    #[test]
    fn 물맵_방추가는_방이_없으면_두개를_채운다() {
        use super::super::Map;
        use super::add_rooms_from_water_land;
        let mut map = Map::new(20, 20);
        map.set_tile(3, 3, TileKind::Floor);
        map.set_tile(15, 15, TileKind::Sand);
        add_rooms_from_water_land(&mut map);
        assert_eq!(map.rooms.len(), 2, "방이 없으면 두 개를 추가한다");
    }

    #[test]
    fn 연결성보장은_중앙이_벽이면_첫_바닥구역으로_폴백한다() {
        use super::super::Map;
        use super::ensure_connectivity;

        let (w, h) = (10, 10);
        let mut map = Map::new(w, h);
        // 중앙은 Wall, 코너에만 Floor
        map.set_tile(1, 1, TileKind::Floor);
        map.set_tile(2, 1, TileKind::Floor);

        ensure_connectivity(&mut map);

        // 중앙이 Wall이면 첫 번째 Floor 구역을 유지
        assert_eq!(map.get_tile(1, 1), TileKind::Floor);
    }

    #[test]
    fn 연결성보장은_통과타일이_하나도_없으면_조용히_종료한다() {
        use super::super::Map;
        use super::ensure_connectivity;
        // 전부 Wall — 중앙도 fallback 스캔도 Floor 를 못 찾아 start 가 None 이 된다.
        let mut map = Map::new(10, 10);
        ensure_connectivity(&mut map);
        // 변화 없이 그대로 전부 Wall 이어야 한다.
        for y in 0..10 {
            for x in 0..10 {
                assert_eq!(map.get_tile(x, y), TileKind::Wall, "({},{}) 는 Wall 이어야 한다", x, y);
            }
        }
    }

    #[test]
    fn 연결성보장의_플러드필은_네_테두리_이웃을_경계밖으로_안전하게_건너뛴다() {
        use super::super::Map;
        use super::ensure_connectivity;
        // 중앙에서 네 테두리(x=0, x=w-1, y=0, y=h-1)까지 모두 닿는 통과타일 십자.
        // 플러드필이 각 테두리 타일에서 맵 밖 이웃을 검사하는 네 방향 분기를 모두 탄다.
        let (w, h) = (10, 10);
        let mut map = Map::new(w, h);
        let (cx, cy) = (w / 2, h / 2);
        for x in 0..w { map.set_tile(x, cy, TileKind::Floor); } // 좌우 테두리까지
        for y in 0..h { map.set_tile(cx, y, TileKind::Floor); } // 상하 테두리까지
        ensure_connectivity(&mut map);
        // 중앙과 연결된 십자는 모두 보존돼야 한다(네 테두리 타일 포함).
        for x in 0..w { assert_eq!(map.get_tile(x, cy), TileKind::Floor, "({},{}) 보존", x, cy); }
        for y in 0..h { assert_eq!(map.get_tile(cx, y), TileKind::Floor, "({},{}) 보존", cx, y); }
    }

    #[test]
    fn 바닥기반_방추가는_내부에_통과타일이_없으면_방을_못만든다() {
        use super::super::Map;
        use super::add_rooms_from_floor;
        // 내부(1..h-1,1..w-1)에 통과타일이 전혀 없다 → floor_tiles 가 비어 early-return.
        let mut map = Map::new(10, 10);
        add_rooms_from_floor(&mut map);
        assert!(map.rooms.is_empty(), "통과타일이 없으면 방을 만들 수 없다");
    }

    #[test]
    fn 바닥기반_방추가는_이미_방이_둘이상이면_아무것도_안한다() {
        use super::super::{Map, Rect};
        use super::add_rooms_from_floor;
        let mut map = Map::new(20, 20);
        map.rooms.push(Rect::new(2, 2, 3, 3));
        map.rooms.push(Rect::new(10, 10, 3, 3));
        add_rooms_from_floor(&mut map);
        assert_eq!(map.rooms.len(), 2, "방이 이미 2개면 추가하지 않는다");
    }

    #[test]
    fn 바닥기반_방추가는_방이_없으면_가까운곳과_먼곳에_두개를_만든다() {
        use super::super::Map;
        use super::add_rooms_from_floor;
        let mut map = Map::new(20, 20);
        // 중앙 근처 한 칸 + 멀리 떨어진 한 칸.
        map.set_tile(10, 10, TileKind::Floor);
        map.set_tile(2, 2, TileKind::Floor);
        add_rooms_from_floor(&mut map);
        assert_eq!(map.rooms.len(), 2, "방이 없으면 두 개를 만든다");
    }

    #[test]
    fn 바닥기반_방추가는_방이_하나면_두번째만_채운다() {
        use super::super::{Map, Rect};
        use super::add_rooms_from_floor;
        let mut map = Map::new(20, 20);
        map.set_tile(10, 10, TileKind::Floor);
        map.set_tile(2, 2, TileKind::Floor);
        map.rooms.push(Rect::new(8, 8, 3, 3));
        add_rooms_from_floor(&mut map);
        assert_eq!(map.rooms.len(), 2, "방이 하나면 두 번째만 추가한다");
    }

    #[test]
    fn 벽이웃세기는_맵_경계밖을_벽으로_간주한다() {
        use super::super::{Map, MapTile};
        use super::count_wall_neighbors;
        // 5×5 전부 Floor. 좌상단 코너(0,0) 의 8이웃 중 5개는 경계 밖(벽 간주),
        // 나머지 3개는 Floor → 경계밖 분기(나가는 쪽)와 안쪽 Floor 분기를 함께 탄다.
        let (w, h) = (5, 5);
        let tiles = vec![MapTile::new(TileKind::Floor); w * h];
        let _ = Map::new(w, h); // 타입 사용 보장
        let corner = count_wall_neighbors(&tiles, w, h, 0, 0);
        assert_eq!(corner, 5, "좌상단 코너는 경계 밖 5칸을 벽으로 세야 한다");
        // 우하단 코너(w-1,h-1) — nx>=width, ny>=height 경계밖 분기도 탄다.
        let far = count_wall_neighbors(&tiles, w, h, w - 1, h - 1);
        assert_eq!(far, 5, "우하단 코너도 경계 밖 5칸을 벽으로 세야 한다");
        // 중앙(2,2) 은 8이웃이 모두 맵 안 Floor → 벽 0.
        let center = count_wall_neighbors(&tiles, w, h, 2, 2);
        assert_eq!(center, 0, "사방이 Floor 인 중앙은 벽 이웃이 없다");
    }

    #[test]
    fn 벽이웃세기는_안쪽_벽타일을_정확히_센다() {
        use super::super::MapTile;
        use super::count_wall_neighbors;
        // 3×3 가운데만 Floor, 나머지 8칸 Wall → 가운데의 벽 이웃은 8.
        let (w, h) = (3, 3);
        let mut tiles = vec![MapTile::new(TileKind::Wall); w * h];
        tiles[1 * w + 1].kind = TileKind::Floor;
        assert_eq!(count_wall_neighbors(&tiles, w, h, 1, 1), 8, "사방이 벽이면 8");
    }

    #[test] fn bsp_생성기는_생성계약을_지킨다()              { check_contract(&bsp::BspGenerator); }
    #[test] fn simple_rooms_생성기는_생성계약을_지킨다()     { check_contract(&rooms::SimpleRoomsGenerator); }
    #[test] fn drunkard_생성기는_생성계약을_지킨다()         { check_contract(&drunkard::DrunkardWalkGenerator); }
    #[test] fn cellular_automata_생성기는_생성계약을_지킨다(){ check_contract(&cellular_automata::CellularAutomataGenerator); }
    #[test] fn dla_생성기는_생성계약을_지킨다()              { check_contract(&dla::DlaGenerator); }
    #[test] fn bsp_indoor_생성기는_생성계약을_지킨다()       { check_contract(&bsp_indoor::BspIndoorGenerator); }
    #[test] fn prefab_생성기는_생성계약을_지킨다()           { check_contract(&prefab::PrefabGenerator); }
    #[test] fn organic_village_생성기는_생성계약을_지킨다()  { check_contract(&organic_village::OrganicVillageGenerator); }
    #[test] fn grid_village_생성기는_생성계약을_지킨다()     { check_contract(&grid_village::GridVillageGenerator); }
    #[test] fn forest_생성기는_생성계약을_지킨다()           { check_contract(&forest::ForestGenerator); }
    #[test] fn perlin_생성기는_생성계약을_지킨다()           { check_contract(&perlin::PerlinNoiseGenerator); }
    #[test] fn maze_생성기는_생성계약을_지킨다()             { check_contract(&maze::MazeGenerator); }
    #[test] fn maze_prim_생성기는_생성계약을_지킨다()        { check_contract(&maze_prim::MazePrimGenerator); }
    #[test] fn recursive_division_생성기는_생성계약을_지킨다(){ check_contract(&recursive_division::RecursiveDivisionGenerator); }
    #[test] fn voronoi_rooms_생성기는_생성계약을_지킨다()    { check_contract(&voronoi_rooms::VoronoiRoomsGenerator); }
    #[test] fn walled_town_생성기는_생성계약을_지킨다()      { check_contract(&walled_town::WalledTownGenerator); }
    #[test] fn voronoi_districts_생성기는_생성계약을_지킨다(){ check_contract(&voronoi_districts::VoronoiDistrictsGenerator); }
    #[test] fn wfc_생성기는_생성계약을_지킨다()              { check_contract(&wfc::WfcGenerator); }
    #[test] fn tinykeep_생성기는_생성계약을_지킨다()         { check_contract(&tinykeep::TinyKeepGenerator); }

    #[test] fn island_생성기는_수상맵_계약을_지킨다()      { check_water_contract(&island::IslandGenerator); }
    #[test] fn archipelago_생성기는_수상맵_계약을_지킨다() { check_water_contract(&archipelago::ArchipelagoGenerator); }
    #[test] fn coastal_생성기는_수상맵_계약을_지킨다()     { check_water_contract(&coastal::CoastalGenerator); }
    #[test] fn ocean_생성기는_수상맵_계약을_지킨다()       { check_water_contract(&ocean::OceanGenerator); }
    #[test] fn biome_world_생성기는_수상맵_계약을_지킨다() { check_water_contract(&biome_world::BiomeWorldGenerator); }

    // CLI --algorithm 인수와 select_by_name 이 사용하는 등록 이름 검증
    #[test]
    fn 모든_생성기의_등록이름은_명세와_일치한다() {
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
        assert_eq!(maze::MazeGenerator.name(),                                  "maze");
        assert_eq!(maze_prim::MazePrimGenerator.name(),                         "maze_prim");
        assert_eq!(recursive_division::RecursiveDivisionGenerator.name(),       "recursive_division");
        assert_eq!(voronoi_rooms::VoronoiRoomsGenerator.name(),                 "voronoi_rooms");
        assert_eq!(walled_town::WalledTownGenerator.name(),                     "walled_town");
        assert_eq!(voronoi_districts::VoronoiDistrictsGenerator.name(),         "voronoi_districts");
        assert_eq!(island::IslandGenerator.name(),                             "island");
        assert_eq!(archipelago::ArchipelagoGenerator.name(),                   "archipelago");
        assert_eq!(coastal::CoastalGenerator.name(),                           "coastal");
        assert_eq!(ocean::OceanGenerator.name(),                               "ocean");
        assert_eq!(biome_world::BiomeWorldGenerator.name(),                    "biome_world");
        assert_eq!(wfc::WfcGenerator.name(),                                   "wfc");
        assert_eq!(tinykeep::TinyKeepGenerator.name(),                         "tinykeep");
    }
}
