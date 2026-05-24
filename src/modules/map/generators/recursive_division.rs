use rand::prelude::*;
use crate::modules::map::{Map, TileKind};
use super::super::MapGenerator;
use super::add_rooms_from_floor;

/// recursive division 미로 생성기.
///
/// 먼저 테두리를 제외한 전 영역을 빈 방(바닥)으로 만든 뒤,
/// 그 영역을 수평 또는 수직 벽으로 가르고 벽에 통로 한 칸을 남긴다.
/// 분할로 생긴 두 하위 영역을 같은 방식으로 재귀 분할한다.
/// 영역이 충분히 작아지면(폭/높이 < 임계값) 분할을 멈춰 작은 방으로 남긴다.
pub struct RecursiveDivisionGenerator;

/// 영역이 이보다 작으면 더 나누지 않는다.
const MIN_SPLIT: usize = 4;

impl MapGenerator for RecursiveDivisionGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;
        map.algorithm = self.name().to_string();

        // 테두리 안쪽 전체를 바닥으로
        for y in 1..height.saturating_sub(1) {
            for x in 1..width.saturating_sub(1) {
                map.set_tile(x, y, TileKind::Floor);
            }
        }

        // 내부 영역 [x1,x2) × [y1,y2) 재귀 분할
        divide(&mut map, 1, 1, width.saturating_sub(1), height.saturating_sub(1), &mut rng);

        add_rooms_from_floor(&mut map);
        map
    }

    fn name(&self) -> &str { "recursive_division" }
}

/// 영역 [x1,x2) × [y1,y2) 을 벽으로 분할한다.
fn divide(map: &mut Map, x1: usize, y1: usize, x2: usize, y2: usize, rng: &mut impl Rng) {
    let w = x2.saturating_sub(x1);
    let h = y2.saturating_sub(y1);
    if w < MIN_SPLIT || h < MIN_SPLIT {
        return;
    }

    // 더 긴 축을 가로지르는 벽을 친다 (정사각이면 무작위).
    let horizontal = if w < h {
        true
    } else if h < w {
        false
    } else {
        rng.gen_bool(0.5)
    };

    if horizontal {
        // 수평 벽 — y 후보는 (y1+1 .. y2-1), 통로는 x 한 칸
        let wy = rng.gen_range(y1 + 1..y2 - 1);
        let passage = rng.gen_range(x1..x2);
        for x in x1..x2 {
            if x != passage {
                map.set_tile(x, wy, TileKind::Wall);
            }
        }
        divide(map, x1, y1, x2, wy, rng);
        divide(map, x1, wy + 1, x2, y2, rng);
    } else {
        // 수직 벽
        let wx = rng.gen_range(x1 + 1..x2 - 1);
        let passage = rng.gen_range(y1..y2);
        for y in y1..y2 {
            if y != passage {
                map.set_tile(wx, y, TileKind::Wall);
            }
        }
        divide(map, x1, y1, wx, y2, rng);
        divide(map, wx + 1, y1, x2, y2, rng);
    }
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;
    use crate::modules::map::MapGenerator;

    #[test]
    fn 같은_시드는_같은_맵을_만든다() {
        let gen = RecursiveDivisionGenerator;
        let a = gen.generate(40, 30, 42);
        let b = gen.generate(40, 30, 42);
        assert_eq!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 다른_시드는_다른_맵을_만든다() {
        let gen = RecursiveDivisionGenerator;
        let a = gen.generate(40, 30, 1);
        let b = gen.generate(40, 30, 2);
        assert_ne!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 내부에_분할벽이_생긴다() {
        // 빈 방에서 시작하지만 재귀 분할로 내부에 벽이 추가돼야 한다.
        let gen = RecursiveDivisionGenerator;
        let map = gen.generate(40, 30, 13);
        let mut interior_walls = 0;
        for y in 2..30 - 2 {
            for x in 2..40 - 2 {
                if map.get_tile(x, y) == TileKind::Wall {
                    interior_walls += 1;
                }
            }
        }
        assert!(interior_walls > 0, "재귀 분할은 내부 벽을 만들어야 한다");
    }

    #[test]
    fn 정사각영역도_분할된다() {
        // w == h 인 정사각 맵에서도 분할(무작위 방향 분기)이 동작해야 한다.
        let gen = RecursiveDivisionGenerator;
        let map = gen.generate(32, 32, 21);
        let interior_walls = (2..30)
            .flat_map(|y| (2..30).map(move |x| (x, y)))
            .filter(|&(x, y)| map.get_tile(x, y) == TileKind::Wall)
            .count();
        assert!(interior_walls > 0, "정사각 맵도 분할돼야 한다");
    }

    #[test]
    fn 분할영역이_작으면_더_나누지_않는다() {
        // MIN_SPLIT 미만 영역은 분할 종료 — 작은 맵은 통째로 바닥.
        let gen = RecursiveDivisionGenerator;
        // 내부 영역이 (MIN_SPLIT) 미만이 되도록 아주 좁은 맵.
        let map = gen.generate(5, 5, 1);
        // 테두리 안쪽 3×3 은 분할되지 않고 모두 바닥.
        for y in 1..4 {
            for x in 1..4 {
                assert_eq!(map.get_tile(x, y), TileKind::Floor,
                    "작은 영역 ({},{}) 은 분할되지 않아 바닥이어야 한다", x, y);
            }
        }
    }
}
