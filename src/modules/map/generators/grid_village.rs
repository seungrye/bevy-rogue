use rand::prelude::*;
use crate::modules::map::{Map, TileKind, MapType, Rect};
use super::super::MapGenerator;

pub struct GridVillageGenerator;

impl MapGenerator for GridVillageGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;

        // 전체를 야외(바닥)로 채운다
        for y in 1..height - 1 {
            for x in 1..width - 1 {
                map.set_tile(x, y, TileKind::Floor);
            }
        }

        let block = 14usize; // 블록 크기 (도로 포함)
        let road = 2usize;   // 도로 너비
        let margin = 2usize; // 건물-도로 간격

        let mut rooms: Vec<Rect> = Vec::new();

        let mut by_start = 1 + road;
        while by_start + block < height - 1 {
            let mut bx_start = 1 + road;
            while bx_start + block < width - 1 {
                let bx = bx_start + margin;
                let by = by_start + margin;
                let bw = block - margin * 2 - road;
                let bh = block - margin * 2 - road;

                // 도달 불가 방어코드: bw/bh 는 상수(block-margin*2-road=8)라 크기 조건은
                // 항상 참이고, while 루프 조건이 bx+bw/by+bh 범위도 이미 보장한다.
                // 가드는 상수 변경 시의 안전망으로만 둔다(False 분기 도달 불가).
                if bw >= 5 && bh >= 4
                    && bx + bw < width - 1
                    && by + bh < height - 1
                {
                    let building = Rect::new(bx, by, bw, bh);

                    // 외벽 — 건물 벽은 파괴 가능(DestructibleWall). 테두리는 일반 Wall 유지.
                    for wy in building.y1..building.y2 {
                        for wx in building.x1..building.x2 {
                            if wy == building.y1 || wy == building.y2 - 1
                                || wx == building.x1 || wx == building.x2 - 1
                            {
                                map.set_tile(wx, wy, TileKind::DestructibleWall);
                            }
                        }
                    }

                    // 문: 남쪽 또는 동쪽 선택
                    if rng.gen_bool(0.5) {
                        let dx = (building.x1 + building.x2) / 2;
                        map.set_tile(dx, building.y2 - 1, TileKind::Floor);
                    } else {
                        let dy = (building.y1 + building.y2) / 2;
                        map.set_tile(building.x2 - 1, dy, TileKind::Floor);
                    }

                    rooms.push(building);
                }

                bx_start += block + road;
            }
            by_start += block + road;
        }

        map.rooms = rooms;
        map.map_type = MapType::Village;
        map
    }
    fn name(&self) -> &str { "grid_village" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::map::MapGenerator;

    #[test]
    fn 건물_벽은_파괴가능벽이고_맵_테두리는_일반벽으로_남는다() {
        let gen = GridVillageGenerator;
        let map = gen.generate(60, 60, 7);
        // 건물 외벽 일부가 DestructibleWall.
        let dwall = map.tiles.iter().filter(|t| t.kind == TileKind::DestructibleWall).count();
        assert!(dwall > 0, "건물 벽은 파괴가능벽이어야 한다");
        // 테두리는 일반 Wall.
        for x in 0..60 {
            assert_eq!(map.get_tile(x, 0), TileKind::Wall, "상단 테두리는 일반 벽");
            assert_eq!(map.get_tile(x, 60 - 1), TileKind::Wall, "하단 테두리는 일반 벽");
        }
        for y in 0..60 {
            assert_eq!(map.get_tile(0, y), TileKind::Wall, "좌측 테두리는 일반 벽");
            assert_eq!(map.get_tile(60 - 1, y), TileKind::Wall, "우측 테두리는 일반 벽");
        }
    }
}
