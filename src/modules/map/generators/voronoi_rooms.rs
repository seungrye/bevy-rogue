use rand::prelude::*;
use crate::modules::map::{Map, TileKind, Rect};
use super::super::MapGenerator;
use super::{carve_corridor, ensure_connectivity, add_rooms_from_floor};

/// Voronoi 셀 기반 방 생성기.
///
/// 내부에 무작위 시드점을 여러 개 뿌리고, 각 타일을 가장 가까운 시드점의
/// 셀에 귀속시킨다(Voronoi 분할). 그중 일부 셀의 "코어"(테두리에서 한 칸
/// 안쪽)를 방으로 카브하고, 카브된 방들의 시드점 중심끼리 L자 복도로
/// 연결한다. 셀 경계는 벽으로 남아 방마다 구분된 형태가 된다.
pub struct VoronoiRoomsGenerator;

impl MapGenerator for VoronoiRoomsGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;
        map.algorithm = self.name().to_string();

        // 맵 크기에 비례한 시드점 개수 (최소 4개).
        let seed_count = ((width * height) / 90).max(4);
        let sites: Vec<(i32, i32)> = (0..seed_count)
            .map(|_| {
                (
                    rng.gen_range(1..width as i32 - 1),
                    rng.gen_range(1..height as i32 - 1),
                )
            })
            .collect();

        // 각 내부 타일을 가장 가까운 시드점 인덱스로 라벨링.
        let mut owner = vec![usize::MAX; width * height];
        for y in 1..height - 1 {
            for x in 1..width - 1 {
                let mut best = 0usize;
                let mut best_d = i32::MAX;
                for (i, &(sx, sy)) in sites.iter().enumerate() {
                    let dx = x as i32 - sx;
                    let dy = y as i32 - sy;
                    let d = dx * dx + dy * dy;
                    if d < best_d {
                        best_d = d;
                        best = i;
                    }
                }
                owner[y * width + x] = best;
            }
        }

        // 셀 내부(같은 owner 인 타일이 사방 이웃 모두 같은 owner)를 바닥으로 카브.
        // 셀 경계 타일은 owner 가 다른 이웃이 있으므로 벽으로 남는다.
        let mut carved_cells = vec![false; sites.len()];
        for y in 1..height - 1 {
            for x in 1..width - 1 {
                let o = owner[y * width + x];
                let interior = owner[y * width + (x - 1)] == o
                    && owner[y * width + (x + 1)] == o
                    && owner[(y - 1) * width + x] == o
                    && owner[(y + 1) * width + x] == o;
                if interior {
                    map.set_tile(x, y, TileKind::Floor);
                    carved_cells[o] = true;
                }
            }
        }

        // 카브된 셀들의 시드점을 순서대로 복도로 잇는다 — 인접 방 연결.
        let carved_sites: Vec<(usize, usize)> = sites
            .iter()
            .enumerate()
            .filter(|&(i, _)| carved_cells[i])
            .map(|(_, &(sx, sy))| (sx as usize, sy as usize))
            .collect();

        for pair in carved_sites.windows(2) {
            let (x1, y1) = pair[0];
            let (x2, y2) = pair[1];
            carve_corridor(&mut map, x1, y1, x2, y2);
        }

        // 복도 연결 직후 시드점은 모두 바닥(복도 끝점) — 스폰용 방으로 등록.
        // (이후 ensure_connectivity 가 일부를 벽으로 되돌려도 스폰 헬퍼가 바닥만
        //  고르므로 무해하며, add_rooms_from_floor 가 최소 2개를 보장한다.)
        for &(sx, sy) in &carved_sites {
            map.rooms.push(Rect::new(
                sx.saturating_sub(1),
                sy.saturating_sub(1),
                3,
                3,
            ));
        }

        ensure_connectivity(&mut map);
        add_rooms_from_floor(&mut map);
        map
    }

    fn name(&self) -> &str { "voronoi_rooms" }
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;
    use crate::modules::map::MapGenerator;

    #[test]
    fn 같은_시드는_같은_맵을_만든다() {
        let gen = VoronoiRoomsGenerator;
        let a = gen.generate(40, 30, 42);
        let b = gen.generate(40, 30, 42);
        assert_eq!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 다른_시드는_다른_맵을_만든다() {
        let gen = VoronoiRoomsGenerator;
        let a = gen.generate(40, 30, 1);
        let b = gen.generate(40, 30, 2);
        assert_ne!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 셀_경계는_벽으로_남는다() {
        // Voronoi 분할 특성: 카브 후에도 내부에 셀 경계 벽이 존재한다.
        let gen = VoronoiRoomsGenerator;
        let map = gen.generate(40, 30, 17);
        let interior_walls = (2..30 - 2)
            .flat_map(|y| (2..40 - 2).map(move |x| (x, y)))
            .filter(|&(x, y)| map.get_tile(x, y) == TileKind::Wall)
            .count();
        assert!(interior_walls > 0, "셀 경계 벽이 내부에 남아야 한다");
    }

    #[test]
    fn 바닥은_단일_연결요소다() {
        // ensure_connectivity 적용으로 모든 바닥이 한 덩어리.
        let gen = VoronoiRoomsGenerator;
        let map = gen.generate(40, 30, 23);
        // BFS 로 첫 바닥에서 도달 가능한 바닥 수 == 전체 바닥 수
        let total = map.tiles.iter().filter(|t| t.kind.is_walkable()).count();
        let start = (1..29)
            .flat_map(|y| (1..39).map(move |x| (x, y)))
            .find(|&(x, y)| map.get_tile(x, y).is_walkable());
        let (sx, sy) = start.expect("바닥이 하나는 있어야 한다");
        let mut seen = std::collections::HashSet::new();
        let mut stack = vec![(sx, sy)];
        seen.insert((sx, sy));
        // 테두리가 모두 Wall이라 바닥 타일의 4이웃은 항상 맵 내부 — 경계 검사 불필요.
        while let Some((x, y)) = stack.pop() {
            for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                if map.get_tile(nx, ny).is_walkable() && seen.insert((nx, ny)) {
                    stack.push((nx, ny));
                }
            }
        }
        assert_eq!(seen.len(), total, "모든 바닥이 단일 연결요소여야 한다");
    }
}
