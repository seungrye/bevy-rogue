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

                if bw >= 5 && bh >= 4
                    && bx + bw < width - 1
                    && by + bh < height - 1
                {
                    let building = Rect::new(bx, by, bw, bh);

                    // 외벽
                    for wy in building.y1..building.y2 {
                        for wx in building.x1..building.x2 {
                            if wy == building.y1 || wy == building.y2 - 1
                                || wx == building.x1 || wx == building.x2 - 1
                            {
                                map.set_tile(wx, wy, TileKind::Wall);
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
