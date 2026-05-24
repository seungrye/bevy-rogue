use rand::prelude::*;
use crate::modules::map::{Map, TileKind, MapType, Rect};
use super::super::MapGenerator;
use super::add_rooms_from_floor;

/// 성벽으로 둘러싸인 마을 생성기.
///
/// 테두리 바로 안쪽에 둘레 성벽을 한 겹 두르고 성문을 1~2개 뚫는다.
/// 성벽 안쪽은 바닥으로 채운 뒤 일정 간격의 격자 도로망을 깔고,
/// 도로로 나뉜 블록마다 외벽을 가진 건물(방)을 배치한다.
/// `map_type` 은 Village 로 설정된다.
pub struct WalledTownGenerator;

impl MapGenerator for WalledTownGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;
        map.algorithm = self.name().to_string();
        map.map_type = MapType::Village;

        // 1) 테두리 안쪽 전체를 바닥(마을 내부)으로
        for y in 1..height - 1 {
            for x in 1..width - 1 {
                map.set_tile(x, y, TileKind::Floor);
            }
        }

        // 2) 둘레 성벽 (테두리 한 칸 안쪽 링)
        for x in 1..width - 1 {
            map.set_tile(x, 1, TileKind::Wall);
            map.set_tile(x, height - 2, TileKind::Wall);
        }
        for y in 1..height - 1 {
            map.set_tile(1, y, TileKind::Wall);
            map.set_tile(width - 2, y, TileKind::Wall);
        }

        // 3) 성문 1~2개 — 네 변 중 무작위로 골라 한 칸씩 뚫는다.
        let gate_count = rng.gen_range(1..=2);
        for _ in 0..gate_count {
            match rng.gen_range(0..4) {
                0 => { let gx = rng.gen_range(2..width - 2);  map.set_tile(gx, 1, TileKind::Floor); }
                1 => { let gx = rng.gen_range(2..width - 2);  map.set_tile(gx, height - 2, TileKind::Floor); }
                2 => { let gy = rng.gen_range(2..height - 2); map.set_tile(1, gy, TileKind::Floor); }
                _ => { let gy = rng.gen_range(2..height - 2); map.set_tile(width - 2, gy, TileKind::Floor); }
            }
        }

        // 4) 내부 격자 도로망 — 성벽 안쪽 영역 [2 .. W-2) × [2 .. H-2)
        let block = 8usize; // 도로 간격
        // 도로는 이미 바닥이므로 별도 카브 불필요. 도로 사이 블록에 건물 배치.
        let mut rooms: Vec<Rect> = Vec::new();
        let mut by = 3usize;
        while by + 4 < height - 2 {
            let mut bx = 3usize;
            while bx + 4 < width - 2 {
                // 블록 내부에 건물 (도로 한 칸 여백 유지).
                // 루프 조건이 bx+4 < width-2, by+4 < height-2 를 보장하므로
                // bw≥4, bh≥3 이 항상 성립한다 → 별도 크기 가드 불필요.
                let bw = (block - 3).min(width - 2 - bx - 1);
                let bh = (block - 3).min(height - 2 - by - 1);
                let building = Rect::new(bx, by, bw, bh);
                carve_building(&mut map, &building, &mut rng);
                rooms.push(building);
                bx += block;
            }
            by += block;
        }

        map.rooms = rooms;
        add_rooms_from_floor(&mut map);
        map
    }

    fn name(&self) -> &str { "walled_town" }
}

/// 건물 외벽을 두르고 한쪽에 문을 낸다.
/// 건물 벽은 `DestructibleWall` 로 만들어 폭발로 부술 수 있게 한다(성벽·테두리는 일반 Wall 유지).
fn carve_building(map: &mut Map, b: &Rect, rng: &mut impl Rng) {
    for wy in b.y1..b.y2 {
        for wx in b.x1..b.x2 {
            if wy == b.y1 || wy == b.y2 - 1 || wx == b.x1 || wx == b.x2 - 1 {
                map.set_tile(wx, wy, TileKind::DestructibleWall);
            } else {
                map.set_tile(wx, wy, TileKind::Floor);
            }
        }
    }
    // 문: 남쪽 또는 동쪽
    if rng.gen_bool(0.5) {
        map.set_tile((b.x1 + b.x2) / 2, b.y2 - 1, TileKind::Floor);
    } else {
        map.set_tile(b.x2 - 1, (b.y1 + b.y2) / 2, TileKind::Floor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::map::{MapGenerator, MapType};

    #[test]
    fn 같은_시드는_같은_맵을_만든다() {
        let gen = WalledTownGenerator;
        let a = gen.generate(40, 30, 42);
        let b = gen.generate(40, 30, 42);
        assert_eq!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 다른_시드는_다른_맵을_만든다() {
        let gen = WalledTownGenerator;
        let a = gen.generate(40, 30, 1);
        let b = gen.generate(40, 30, 2);
        assert_ne!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 마을_유형으로_설정된다() {
        let gen = WalledTownGenerator;
        let map = gen.generate(40, 30, 3);
        assert_eq!(map.map_type, MapType::Village);
    }

    #[test]
    fn 테두리_안쪽에_성벽이_있다() {
        // 둘레 성벽: 테두리(0/W-1) 바로 안쪽 링(1/W-2)이 대부분 벽이어야 한다.
        let gen = WalledTownGenerator;
        let map = gen.generate(40, 30, 5);
        // 분기 없이 합산 — top/bottom 성벽 링의 벽 칸 수.
        let wall_on_ring: usize = (1..40 - 1)
            .map(|x| (map.get_tile(x, 1) == TileKind::Wall) as usize
                + (map.get_tile(x, 30 - 2) == TileKind::Wall) as usize)
            .sum();
        // 성문 몇 칸을 빼도 링 대부분은 벽.
        assert!(wall_on_ring > 40, "테두리 안쪽 성벽 링이 있어야 한다 (벽 {}칸)", wall_on_ring);
    }

    #[test]
    fn 성문이_최소_하나_뚫린다() {
        // 성벽 링에 바닥(성문)이 최소 하나 존재해야 한다.
        let gen = WalledTownGenerator;
        // 여러 시드로 gate_count 1·2, 네 변 선택 분기를 모두 거치게 한다.
        // 각 시드마다 성벽 링에 바닥(성문)이 반드시 하나 이상 존재해야 한다.
        for s in 0..20 {
            let map = gen.generate(40, 30, s);
            let gate = (2..40 - 2).any(|x| map.get_tile(x, 1) == TileKind::Floor
                || map.get_tile(x, 30 - 2) == TileKind::Floor)
                || (2..30 - 2).any(|y| map.get_tile(1, y) == TileKind::Floor
                    || map.get_tile(40 - 2, y) == TileKind::Floor);
            assert!(gate, "시드 {} 에서 성문이 적어도 하나 뚫려야 한다", s);
        }
    }

    #[test]
    fn 건물_블록이_방으로_등록된다() {
        let gen = WalledTownGenerator;
        let map = gen.generate(40, 30, 7);
        assert!(map.rooms.len() >= 2, "건물 블록이 방으로 등록돼야 한다");
    }

    #[test]
    fn 건물_벽은_파괴가능벽이고_맵_테두리는_일반벽으로_남는다() {
        let gen = WalledTownGenerator;
        let map = gen.generate(40, 30, 7);
        // 건물(방) 외벽 중 적어도 일부가 DestructibleWall 이어야 한다.
        let dwall = map.tiles.iter().filter(|t| t.kind == TileKind::DestructibleWall).count();
        assert!(dwall > 0, "건물 벽은 파괴가능벽으로 생성돼야 한다");
        // 맵 테두리는 일반 Wall 유지.
        for x in 0..40 {
            assert_eq!(map.get_tile(x, 0), TileKind::Wall, "상단 테두리는 일반 벽");
            assert_eq!(map.get_tile(x, 30 - 1), TileKind::Wall, "하단 테두리는 일반 벽");
        }
        for y in 0..30 {
            assert_eq!(map.get_tile(0, y), TileKind::Wall, "좌측 테두리는 일반 벽");
            assert_eq!(map.get_tile(40 - 1, y), TileKind::Wall, "우측 테두리는 일반 벽");
        }
    }
}
