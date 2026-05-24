use rand::prelude::*;
use crate::modules::map::{Map, TileKind};
use super::super::MapGenerator;
use super::add_rooms_from_floor;

/// recursive backtracker(DFS+스택) 완전미로 생성기.
///
/// 홀수 좌표(2칸 간격)를 격자 셀로 보고 셀 사이를 벽으로 분리한 뒤,
/// 임의의 미방문 이웃 셀로 벽을 한 칸씩 뚫으며 깊이우선으로 진행한다.
/// 막다른 곳에서는 스택을 되감아(backtrack) 미방문 이웃이 있는 셀로 돌아간다.
/// 결과는 루프가 없는 완전미로(모든 셀이 정확히 하나의 경로로 연결).
pub struct MazeGenerator;

impl MapGenerator for MazeGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;
        map.algorithm = self.name().to_string();

        // 격자 셀 개수 — 셀은 (1,1) 부터 2칸 간격으로 배치된다.
        let cols = (width.saturating_sub(1)) / 2;
        let rows = (height.saturating_sub(1)) / 2;
        // 셀이 2개 미만이면 미로를 만들 수 없다 — 작은 빈 방으로 폴백.
        if cols < 2 || rows < 2 {
            // 도달 불가 방어코드: 실제 맵 크기(40×30 이상)에선 항상 cols/rows ≥ 2.
            carve_fallback(&mut map);
            add_rooms_from_floor(&mut map);
            return map;
        }

        let cell_to_tile = |cx: usize, cy: usize| (1 + cx * 2, 1 + cy * 2);

        let mut visited = vec![false; cols * rows];
        let idx = |cx: usize, cy: usize| cy * cols + cx;

        // 시작 셀에서 DFS
        let (mut cx, mut cy) = (0usize, 0usize);
        visited[idx(cx, cy)] = true;
        {
            let (tx, ty) = cell_to_tile(cx, cy);
            map.set_tile(tx, ty, TileKind::Floor);
        }
        let mut stack: Vec<(usize, usize)> = vec![(cx, cy)];

        while let Some(&(scx, scy)) = stack.last() {
            cx = scx;
            cy = scy;
            // 미방문 이웃 수집
            let mut neighbors: Vec<(usize, usize)> = Vec::with_capacity(4);
            if cx > 0 && !visited[idx(cx - 1, cy)] { neighbors.push((cx - 1, cy)); }
            if cx + 1 < cols && !visited[idx(cx + 1, cy)] { neighbors.push((cx + 1, cy)); }
            if cy > 0 && !visited[idx(cx, cy - 1)] { neighbors.push((cx, cy - 1)); }
            if cy + 1 < rows && !visited[idx(cx, cy + 1)] { neighbors.push((cx, cy + 1)); }

            if neighbors.is_empty() {
                // 막다른 곳 — 되감기
                stack.pop();
                continue;
            }

            let &(nx, ny) = neighbors.choose(&mut rng).unwrap();
            visited[idx(nx, ny)] = true;

            // 현재 셀과 이웃 셀, 그 사이 벽을 모두 뚫는다.
            let (ctx, cty) = cell_to_tile(cx, cy);
            let (ntx, nty) = cell_to_tile(nx, ny);
            let wall_x = (ctx + ntx) / 2;
            let wall_y = (cty + nty) / 2;
            map.set_tile(ntx, nty, TileKind::Floor);
            map.set_tile(wall_x, wall_y, TileKind::Floor);

            stack.push((nx, ny));
        }

        // 미로는 단일 연결이라 ensure_connectivity 불필요.
        // 방 개념이 없으므로 바닥에서 스폰용 방 2개 생성.
        add_rooms_from_floor(&mut map);
        map
    }

    fn name(&self) -> &str { "maze" }
}

/// 셀이 부족한 극소형 맵용 폴백 — 중앙 영역을 바닥으로 카브.
fn carve_fallback(map: &mut Map) {
    for y in 1..map.height.saturating_sub(1) {
        for x in 1..map.width.saturating_sub(1) {
            map.set_tile(x, y, TileKind::Floor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::map::MapGenerator;

    #[test]
    fn 같은_시드는_같은_미로를_만든다() {
        let gen = MazeGenerator;
        let a = gen.generate(40, 30, 42);
        let b = gen.generate(40, 30, 42);
        assert_eq!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            "같은 시드는 동일한 미로를 생성해야 한다"
        );
    }

    #[test]
    fn 다른_시드는_다른_미로를_만든다() {
        let gen = MazeGenerator;
        let a = gen.generate(40, 30, 1);
        let b = gen.generate(40, 30, 2);
        assert_ne!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            "다른 시드는 다른 미로를 생성해야 한다"
        );
    }

    #[test]
    fn 격자셀_위치는_모두_바닥이다() {
        // recursive backtracker 는 모든 격자 셀(홀수 좌표)을 방문하므로 전부 바닥.
        let gen = MazeGenerator;
        let map = gen.generate(40, 30, 7);
        let cols = (40 - 1) / 2;
        let rows = (30 - 1) / 2;
        for cy in 0..rows {
            for cx in 0..cols {
                let (tx, ty) = (1 + cx * 2, 1 + cy * 2);
                assert_eq!(map.get_tile(tx, ty), TileKind::Floor,
                    "격자 셀 ({},{}) 은 미로 통로로 바닥이어야 한다", tx, ty);
            }
        }
    }

    #[test]
    fn 짝수_교차점에는_벽이_남는다() {
        // 완전미로의 격자 구조 — 짝수×짝수 좌표(셀 사이 모서리)는 절대 뚫리지 않는다.
        let gen = MazeGenerator;
        let map = gen.generate(40, 30, 11);
        for y in (2..30 - 1).step_by(2) {
            for x in (2..40 - 1).step_by(2) {
                assert_eq!(map.get_tile(x, y), TileKind::Wall,
                    "격자 모서리 ({},{}) 는 미로 패턴상 벽이어야 한다", x, y);
            }
        }
    }

    #[test]
    fn 가로로_좁은_맵은_폴백으로_바닥을_만든다() {
        // 도달 불가 방어코드 검증: cols≥2 이지만 rows<2 (10×4) → 폴백 경로.
        let gen = MazeGenerator;
        let map = gen.generate(10, 4, 3);
        let floor = map.tiles.iter().filter(|t| t.kind == TileKind::Floor).count();
        assert!(floor > 0, "극소맵 폴백도 바닥을 만들어야 한다");
        assert!(map.rooms.len() >= 2, "폴백에서도 스폰용 방 2개가 있어야 한다");
    }

    #[test]
    fn 세로로_좁은_맵도_폴백으로_바닥을_만든다() {
        // cols<2 단축평가 분기 — 4×30 맵.
        let gen = MazeGenerator;
        let map = gen.generate(4, 30, 3);
        let floor = map.tiles.iter().filter(|t| t.kind == TileKind::Floor).count();
        assert!(floor > 0);
        assert!(map.rooms.len() >= 2);
    }
}
