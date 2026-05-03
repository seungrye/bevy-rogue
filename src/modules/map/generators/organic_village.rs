use rand::prelude::*;
use crate::modules::map::{Map, MapTile, MapType, Rect};
use super::super::MapGenerator;

pub struct OrganicVillageGenerator;

impl MapGenerator for OrganicVillageGenerator {
    fn generate(&self, width: usize, height: usize) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = thread_rng();

        // 전체를 야외(바닥)로 채운다
        for y in 1..height - 1 {
            for x in 1..width - 1 {
                map.set_tile(x, y, MapTile::Floor);
            }
        }

        let mut rooms: Vec<Rect> = Vec::new();

        // 건물 배치
        for _ in 0..300 {
            if rooms.len() >= 20 { break; }
            let w = rng.gen_range(5..11);
            let h = rng.gen_range(4..9);
            let x = rng.gen_range(2..width.saturating_sub(w + 2));
            let y = rng.gen_range(2..height.saturating_sub(h + 2));
            let building = Rect::new(x, y, w, h);

            let pad = 2;
            if rooms.iter().any(|r: &Rect| {
                building.x1 < r.x2 + pad && building.x2 + pad > r.x1
                    && building.y1 < r.y2 + pad && building.y2 + pad > r.y1
            }) {
                continue;
            }

            // 외벽 그리기
            for by in building.y1..building.y2 {
                for bx in building.x1..building.x2 {
                    if by == building.y1 || by == building.y2 - 1
                        || bx == building.x1 || bx == building.x2 - 1
                    {
                        map.set_tile(bx, by, MapTile::Wall);
                    }
                }
            }

            // 남쪽 벽 중앙에 문 개통
            let door_x = (building.x1 + building.x2) / 2;
            map.set_tile(door_x, building.y2 - 1, MapTile::Floor);

            rooms.push(building);
        }

        // 나무 산포 (건물 바깥 빈 공간)
        for _ in 0..300 {
            let x = rng.gen_range(1..width - 1);
            let y = rng.gen_range(1..height - 1);
            let in_building = rooms.iter().any(|r| {
                x >= r.x1.saturating_sub(1) && x <= r.x2
                    && y >= r.y1.saturating_sub(1) && y <= r.y2
            });
            if !in_building {
                map.set_tile(x, y, MapTile::Wall);
            }
        }

        map.rooms = rooms;
        map.map_type = MapType::Village;
        map
    }
    fn name(&self) -> &str { "organic_village" }
}
