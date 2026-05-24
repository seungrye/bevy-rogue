use rand::prelude::*;
use crate::modules::map::{Map, TileKind, MapType, Rect};
use super::super::MapGenerator;
use super::grid_village::carve_shop_counter;

pub struct OrganicVillageGenerator;

impl MapGenerator for OrganicVillageGenerator {
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
                        map.set_tile(bx, by, TileKind::Wall);
                    }
                }
            }

            // 남쪽 벽 중앙에 문 개통
            let door_x = (building.x1 + building.x2) / 2;
            map.set_tile(door_x, building.y2 - 1, TileKind::Floor);

            // 두 번째 이후 건물 중 충분히 큰 것 하나를 상점으로 — 카운터 + 그 뒤 상인 자리.
            // (rooms[0] 은 플레이어 스폰 방이므로 제외.)
            if map.shop_vendor.is_none() && !rooms.is_empty() {
                if let Some(vendor) = carve_shop_counter(&mut map, &building) {
                    map.shop_vendor = Some(vendor);
                }
            }

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
                map.set_tile(x, y, TileKind::Wall);
            }
        }

        map.rooms = rooms;
        map.map_type = MapType::Village;
        map
    }
    fn name(&self) -> &str { "organic_village" }
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;
    use crate::modules::map::MapGenerator;

    #[test]
    fn 유기마을은_마을타입이고_상점건물을_둔다() {
        let gen = OrganicVillageGenerator;
        // 여러 시드를 시도해 충분히 큰 상점 건물이 생기는 시드를 찾는다(건물 크기 랜덤).
        let mut found = false;
        for seed in 0..30u64 {
            let map = gen.generate(60, 60, seed);
            assert_eq!(map.map_type, MapType::Village);
            if let Some(vendor) = map.shop_vendor {
                assert_eq!(map.get_tile(vendor.0, vendor.1), TileKind::Floor, "상인 자리는 바닥");
                assert!(map.tiles.iter().any(|t| t.kind == TileKind::Counter), "카운터 존재");
                found = true;
                break;
            }
        }
        assert!(found, "여러 시드 중 하나는 상점 건물을 둬야 한다");
    }
}
