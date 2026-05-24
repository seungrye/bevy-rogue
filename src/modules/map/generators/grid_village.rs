use rand::prelude::*;
use crate::modules::map::{Map, TileKind, MapType, Rect};
use super::super::MapGenerator;

pub struct GridVillageGenerator;

/// 한 건물을 "상점"으로 만든다 — 내부에 통행 불가 카운터(가판대) 한 줄을 깔고,
/// 그 뒤(건물 안쪽)에 상인(vendor)이 설 바닥 타일을 비워둔다.
///
/// 카운터는 건물 내부의 한 행(가로 한 줄) 전체를 `Counter` 로 채운다.
/// 카운터 앞 행(문 쪽)은 손님(플레이어)이 서는 자리, 카운터 뒤 행은 상인 자리다.
/// 상인 타일은 항상 `Floor` 로 보장한다.
///
/// 반환값: 상인이 설 타일 `(x, y)`. 건물이 너무 작아 카운터를 둘 수 없으면 `None`.
///
/// 건물 내부(벽 제외)는 `[x1+1, x2-1) × [y1+1, y2-1)` 이다. 내부 높이가 3 이상이어야
/// (뒤·카운터·앞) 세 행을 둘 수 있다.
pub fn carve_shop_counter(map: &mut Map, building: &Rect) -> Option<(usize, usize)> {
    let in_x1 = building.x1 + 1;
    let in_x2 = building.x2 - 1; // exclusive
    let in_y1 = building.y1 + 1;
    let in_y2 = building.y2 - 1; // exclusive
    // 내부 너비/높이 검사: 카운터 한 줄 + 앞뒤 한 줄씩 = 높이 3 이상, 너비 1 이상.
    if in_x2 <= in_x1 || in_y2 < in_y1 + 3 {
        return None;
    }
    // 카운터 행: 내부 위에서 한 칸 아래(뒤=상인 자리 한 줄 확보). 문은 남쪽/동쪽이므로
    // 카운터 앞(아래쪽)에 손님 공간이 남는다.
    let counter_y = in_y1 + 1;
    let vendor_y = in_y1; // 카운터 뒤(건물 안쪽 위) — 상인 자리
    let vendor_x = (in_x1 + in_x2) / 2; // 내부 가로 중앙

    // 상인 자리·앞 손님 자리를 바닥으로 보장한 뒤 카운터 줄을 깐다.
    for x in in_x1..in_x2 {
        map.set_tile(x, vendor_y, TileKind::Floor);
        map.set_tile(x, counter_y, TileKind::Counter);
    }
    // 손님 통로(카운터 앞)도 바닥으로 — 좁은 건물에서 막히지 않게.
    for x in in_x1..in_x2 {
        map.set_tile(x, counter_y + 1, TileKind::Floor);
    }
    Some((vendor_x, vendor_y))
}

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

                    // 두 번째 건물을 상점으로 — 카운터 한 줄 + 그 뒤 상인 자리.
                    // (rooms[0] 은 플레이어 스폰 방이므로 상점은 그 다음 건물에 둔다.)
                    // 통행 불가 카운터가 건물을 막아도 카운터 앞 손님 통로가 남는다.
                    if map.shop_vendor.is_none() && !rooms.is_empty() {
                        if let Some(vendor) = carve_shop_counter(&mut map, &building) {
                            map.shop_vendor = Some(vendor);
                        }
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
    #![allow(non_snake_case)]
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

    // --- 상점 가판대(carve_shop_counter) ---

    #[test]
    fn 카운터를_깔면_상인_자리는_바닥이고_그_앞은_카운터다() {
        // 충분히 큰 건물(내부 높이 ≥ 3)에 카운터를 깐다.
        let mut map = Map::new(20, 20);
        let building = Rect::new(2, 2, 8, 8); // x1=2,y1=2,x2=10,y2=10 (내부 [3,9)×[3,9))
        let vendor = carve_shop_counter(&mut map, &building).expect("카운터를 깔 수 있어야 한다");
        // 상인 자리는 바닥(통행 가능, 상인이 선다).
        assert_eq!(map.get_tile(vendor.0, vendor.1), TileKind::Floor, "상인 자리는 바닥");
        // 상인 자리 바로 앞(y+1)은 카운터(통행 불가).
        assert_eq!(map.get_tile(vendor.0, vendor.1 + 1), TileKind::Counter, "상인 앞은 카운터");
        assert!(!map.get_tile(vendor.0, vendor.1 + 1).is_walkable(), "카운터는 통행 불가");
    }

    #[test]
    fn 카운터_뒤_상인자리에_인접해야_거래할_수_있다() {
        // 손님(플레이어)이 서는 카운터 앞 타일은 통행 가능해야 한다.
        let mut map = Map::new(20, 20);
        let building = Rect::new(2, 2, 8, 8);
        let vendor = carve_shop_counter(&mut map, &building).unwrap();
        let counter = (vendor.0, vendor.1 + 1);
        let customer = (vendor.0, vendor.1 + 2); // 카운터 앞 손님 자리
        assert!(map.get_tile(customer.0, customer.1).is_walkable(), "카운터 앞 손님 자리는 통행 가능");
        // 손님 자리는 카운터에 인접, 카운터는 상인에 인접 — 카운터 너머 거래 성립.
        assert_eq!(counter.1, customer.1 - 1);
        assert_eq!(counter.1, vendor.1 + 1);
    }

    #[test]
    fn 내부가_너무_낮은_건물은_카운터를_깔_수_없다() {
        // 내부 높이가 3 미만이면 (뒤·카운터·앞) 세 행을 둘 수 없어 None.
        let mut map = Map::new(20, 20);
        let small = Rect::new(2, 2, 6, 4); // 내부 높이 = (y2-1)-(y1+1) = 5-3 = 2 < 3
        assert!(carve_shop_counter(&mut map, &small).is_none(), "낮은 건물은 카운터 불가");
    }

    #[test]
    fn 내부가_너무_좁은_건물은_카운터를_깔_수_없다() {
        // 내부 너비가 1 미만(0)이면 None.
        let mut map = Map::new(20, 20);
        let narrow = Rect::new(2, 2, 2, 8); // x1=2,x2=4 → 내부 x [3,3) = 비어 있음
        assert!(carve_shop_counter(&mut map, &narrow).is_none(), "좁은 건물은 카운터 불가");
    }

    #[test]
    fn 그리드마을은_상점건물에_상인위치를_기록한다() {
        let gen = GridVillageGenerator;
        let map = gen.generate(60, 60, 7);
        let vendor = map.shop_vendor.expect("마을에는 상점 vendor 위치가 있어야 한다");
        // 상인 자리는 바닥이고, 그 앞 어딘가에 카운터가 있어야 한다.
        assert_eq!(map.get_tile(vendor.0, vendor.1), TileKind::Floor, "상인 자리는 바닥");
        let counter_count = map.tiles.iter().filter(|t| t.kind == TileKind::Counter).count();
        assert!(counter_count > 0, "상점에는 카운터 타일이 있어야 한다");
    }

    #[test]
    fn 상점은_플레이어_스폰방이_아닌_건물에_둔다() {
        // 상점은 rooms[1] 이후에 배치되어 rooms[0](플레이어 스폰방)과 겹치지 않는다.
        let gen = GridVillageGenerator;
        let map = gen.generate(60, 60, 7);
        let (sx, sy) = map.rooms[0].center();
        // 플레이어 스폰 타일은 카운터가 아니어야 한다(상점에 갇히지 않음).
        assert_ne!(map.get_tile(sx, sy), TileKind::Counter, "플레이어 스폰 타일은 카운터가 아니다");
    }
}

#[cfg(test)]
mod shop_reachability_tests {
    #![allow(non_snake_case)]
    use super::*;
    use crate::modules::map::MapGenerator;

    #[test]
    fn 상점_카운터_앞은_바닥이라_손님이_설_수_있다() {
        let gen = GridVillageGenerator;
        let map = gen.generate(60, 60, 7);
        let (vx, vy) = map.shop_vendor.unwrap();
        assert_eq!(map.get_tile(vx, vy + 1), TileKind::Counter, "상인 앞은 카운터");
        assert!(map.get_tile(vx, vy + 2).is_walkable(), "카운터 앞 손님 자리는 통행 가능");
    }
}
