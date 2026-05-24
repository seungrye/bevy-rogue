use rand::prelude::*;
use crate::modules::map::{Map, TileKind};
use super::super::MapGenerator;
use super::add_rooms_from_floor;

/// Prim's 알고리즘 기반 미로 생성기.
///
/// 격자 셀(2칸 간격)을 노드로 보고, 방문 셀 집합의 경계에 있는 미방문 이웃들을
/// "프런티어"로 모은다. 매 단계 프런티어 중 하나를 **무작위로** 골라
/// 이미 방문한 이웃 셀과 벽을 뚫어 연결한다. 무작위 프런티어 선택이라
/// recursive backtracker 보다 분기(짧은 막다른 길)가 더 많은 미로가 나온다.
pub struct MazePrimGenerator;

impl MapGenerator for MazePrimGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;
        map.algorithm = self.name().to_string();

        let cols = (width.saturating_sub(1)) / 2;
        let rows = (height.saturating_sub(1)) / 2;
        if cols < 2 || rows < 2 {
            // 도달 불가 방어코드: 실제 맵 크기에선 항상 셀 ≥ 2.
            for y in 1..height.saturating_sub(1) {
                for x in 1..width.saturating_sub(1) {
                    map.set_tile(x, y, TileKind::Floor);
                }
            }
            add_rooms_from_floor(&mut map);
            return map;
        }

        let cell_to_tile = |cx: usize, cy: usize| (1 + cx * 2, 1 + cy * 2);
        let idx = |cx: usize, cy: usize| cy * cols + cx;
        let neighbors = |cx: usize, cy: usize| -> Vec<(usize, usize)> {
            let mut v = Vec::with_capacity(4);
            if cx > 0 { v.push((cx - 1, cy)); }
            if cx + 1 < cols { v.push((cx + 1, cy)); }
            if cy > 0 { v.push((cx, cy - 1)); }
            if cy + 1 < rows { v.push((cx, cy + 1)); }
            v
        };

        let mut in_maze = vec![false; cols * rows];

        // 시작 셀 선택
        let start = (rng.gen_range(0..cols), rng.gen_range(0..rows));
        in_maze[idx(start.0, start.1)] = true;
        {
            let (tx, ty) = cell_to_tile(start.0, start.1);
            map.set_tile(tx, ty, TileKind::Floor);
        }

        // 프런티어: 미로에 인접한 미방문 셀들
        let mut frontier: Vec<(usize, usize)> = neighbors(start.0, start.1);

        while !frontier.is_empty() {
            // 무작위 프런티어 선택 (swap_remove 로 O(1))
            let pick = rng.gen_range(0..frontier.len());
            let (fx, fy) = frontier.swap_remove(pick);

            if in_maze[idx(fx, fy)] {
                continue;
            }

            // 이 프런티어 셀의 이웃 중 이미 미로에 속한 셀을 무작위로 골라 연결
            let in_neighbors: Vec<(usize, usize)> = neighbors(fx, fy)
                .into_iter()
                .filter(|&(nx, ny)| in_maze[idx(nx, ny)])
                .collect();

            // 프런티어 셀은 정의상 항상 미로에 속한 이웃이 ≥1 개 있으므로 비지 않는다.
            let &(nx, ny) = in_neighbors
                .choose(&mut rng)
                .expect("프런티어 셀에는 미로에 속한 이웃이 반드시 있다");
            in_maze[idx(fx, fy)] = true;
            let (ftx, fty) = cell_to_tile(fx, fy);
            let (ntx, nty) = cell_to_tile(nx, ny);
            map.set_tile(ftx, fty, TileKind::Floor);
            map.set_tile((ftx + ntx) / 2, (fty + nty) / 2, TileKind::Floor);

            // 새로 편입된 셀의 미방문 이웃을 프런티어에 추가
            for (gx, gy) in neighbors(fx, fy) {
                if !in_maze[idx(gx, gy)] {
                    frontier.push((gx, gy));
                }
            }
        }

        add_rooms_from_floor(&mut map);
        map
    }

    fn name(&self) -> &str { "maze_prim" }
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;
    use crate::modules::map::MapGenerator;

    #[test]
    fn 같은_시드는_같은_미로를_만든다() {
        let gen = MazePrimGenerator;
        let a = gen.generate(40, 30, 42);
        let b = gen.generate(40, 30, 42);
        assert_eq!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 다른_시드는_다른_미로를_만든다() {
        let gen = MazePrimGenerator;
        let a = gen.generate(40, 30, 1);
        let b = gen.generate(40, 30, 2);
        assert_ne!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 모든_격자셀이_미로에_편입된다() {
        // Prim's 는 프런티어가 빌 때까지 도는 spanning tree — 모든 셀이 바닥.
        let gen = MazePrimGenerator;
        let map = gen.generate(40, 30, 5);
        let cols = (40 - 1) / 2;
        let rows = (30 - 1) / 2;
        for cy in 0..rows {
            for cx in 0..cols {
                let (tx, ty) = (1 + cx * 2, 1 + cy * 2);
                assert_eq!(map.get_tile(tx, ty), TileKind::Floor,
                    "격자 셀 ({},{}) 이 미로에 편입돼야 한다", tx, ty);
            }
        }
    }

    #[test]
    fn 짝수_교차점에는_벽이_남는다() {
        let gen = MazePrimGenerator;
        let map = gen.generate(40, 30, 9);
        for y in (2..30 - 1).step_by(2) {
            for x in (2..40 - 1).step_by(2) {
                assert_eq!(map.get_tile(x, y), TileKind::Wall,
                    "격자 모서리 ({},{}) 는 벽이어야 한다", x, y);
            }
        }
    }

    #[test]
    fn 가로로_좁은_맵은_폴백으로_바닥을_만든다() {
        // 도달 불가 방어코드 검증: cols≥2, rows<2 (10×4) → 폴백.
        let gen = MazePrimGenerator;
        let map = gen.generate(10, 4, 3);
        let floor = map.tiles.iter().filter(|t| t.kind == TileKind::Floor).count();
        assert!(floor > 0);
        assert!(map.rooms.len() >= 2);
    }

    #[test]
    fn 세로로_좁은_맵도_폴백으로_바닥을_만든다() {
        // cols<2 단축평가 분기 — 4×30 맵.
        let gen = MazePrimGenerator;
        let map = gen.generate(4, 30, 3);
        let floor = map.tiles.iter().filter(|t| t.kind == TileKind::Floor).count();
        assert!(floor > 0);
        assert!(map.rooms.len() >= 2);
    }
}
