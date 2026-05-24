use rand::prelude::*;
use crate::modules::map::{Map, TileKind, MapType, Rect};
use super::super::MapGenerator;
use super::add_rooms_from_floor;

/// Voronoi 구역 분할 도시 생성기.
///
/// 무작위 시드점으로 도시를 여러 구역(Voronoi 셀)으로 나눈다.
/// 서로 다른 셀이 맞닿는 경계 타일은 도로(Floor)로 두어 구역 간 연결을 만들고,
/// 각 셀 내부에는 외벽을 가진 건물(방)을 하나씩 세운다.
/// `map_type` 은 Village 로 설정된다.
pub struct VoronoiDistrictsGenerator;

impl MapGenerator for VoronoiDistrictsGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;
        map.algorithm = self.name().to_string();
        map.map_type = MapType::Village;

        // 시드점(구역 중심) 개수.
        let site_count = ((width * height) / 130).max(4);
        let sites: Vec<(i32, i32)> = (0..site_count)
            .map(|_| {
                (
                    rng.gen_range(2..width as i32 - 2),
                    rng.gen_range(2..height as i32 - 2),
                )
            })
            .collect();

        // 내부 타일 owner 라벨링.
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

        // 1) 셀 내부는 일단 바닥으로, 셀 경계(이웃 owner 가 다름)는 도로(Floor)로.
        //    (둘 다 Floor 지만 경계는 건물이 침범하지 못하도록 별도 표시.)
        let mut is_road = vec![false; width * height];
        for y in 1..height - 1 {
            for x in 1..width - 1 {
                map.set_tile(x, y, TileKind::Floor);
                let o = owner[y * width + x];
                let boundary = owner[y * width + (x - 1)] != o
                    || owner[y * width + (x + 1)] != o
                    || owner[(y - 1) * width + x] != o
                    || owner[(y + 1) * width + x] != o;
                if boundary {
                    is_road[y * width + x] = true;
                }
            }
        }

        // 2) 각 셀의 바운딩박스를 구하고 내부에 건물을 세운다.
        let mut bounds: Vec<Option<(usize, usize, usize, usize)>> = vec![None; sites.len()];
        for y in 1..height - 1 {
            for x in 1..width - 1 {
                if is_road[y * width + x] { continue; }
                let o = owner[y * width + x];
                let e = bounds[o].get_or_insert((x, y, x, y));
                e.0 = e.0.min(x);
                e.1 = e.1.min(y);
                e.2 = e.2.max(x);
                e.3 = e.3.max(y);
            }
        }

        let mut rooms: Vec<Rect> = Vec::new();
        for b in bounds.iter().flatten() {
            let (minx, miny, maxx, maxy) = *b;
            // 건물은 셀 내부에서 한 칸 여백을 둔다 (도로 보존).
            let bx = minx + 1;
            let by = miny + 1;
            // 셀 바운딩박스가 건물(최소 3×3)을 담을 만큼 크지 않으면 건너뛴다.
            if maxx <= bx + 2 || maxy <= by + 1 { continue; }
            // 위 가드 통과 시 maxx-bx ≥ 3, maxy-by ≥ 2 보장 — bw/bh 는 최소 3×3.
            let bw = (maxx - bx).max(3);
            let bh = (maxy - by).max(3);
            let building = Rect::new(bx, by, bw, bh);
            carve_building(&mut map, &building, &mut is_road, width, &mut rng);
            rooms.push(building);
        }

        map.rooms = rooms;
        add_rooms_from_floor(&mut map);
        map
    }

    fn name(&self) -> &str { "voronoi_districts" }
}

/// 셀 내부에 건물 외벽을 두르되, 도로 타일은 침범하지 않는다.
fn carve_building(
    map: &mut Map,
    b: &Rect,
    is_road: &[bool],
    width: usize,
    rng: &mut impl Rng,
) {
    // 건물 바운딩박스는 owner 라벨(내부 1..W-1) 에서 파생되므로 항상 맵 내부다.
    for wy in b.y1..b.y2 {
        for wx in b.x1..b.x2 {
            if is_road[wy * width + wx] { continue; } // 도로 보존
            if wy == b.y1 || wy == b.y2 - 1 || wx == b.x1 || wx == b.x2 - 1 {
                map.set_tile(wx, wy, TileKind::Wall);
            }
        }
    }
    // 문 한 칸 (남쪽 또는 동쪽) — 좌표는 항상 맵 내부.
    if rng.gen_bool(0.5) {
        map.set_tile((b.x1 + b.x2) / 2, b.y2 - 1, TileKind::Floor);
    } else {
        map.set_tile(b.x2 - 1, (b.y1 + b.y2) / 2, TileKind::Floor);
    }
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;
    use crate::modules::map::{MapGenerator, MapType};

    #[test]
    fn 같은_시드는_같은_맵을_만든다() {
        let gen = VoronoiDistrictsGenerator;
        let a = gen.generate(40, 30, 42);
        let b = gen.generate(40, 30, 42);
        assert_eq!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 다른_시드는_다른_맵을_만든다() {
        let gen = VoronoiDistrictsGenerator;
        let a = gen.generate(40, 30, 1);
        let b = gen.generate(40, 30, 2);
        assert_ne!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn 마을_유형으로_설정된다() {
        let gen = VoronoiDistrictsGenerator;
        let map = gen.generate(40, 30, 3);
        assert_eq!(map.map_type, MapType::Village);
    }

    #[test]
    fn 건물_외벽이_생긴다() {
        // 셀 내부에 건물 외벽(내부 벽)이 만들어져야 한다.
        let gen = VoronoiDistrictsGenerator;
        let map = gen.generate(40, 30, 19);
        let interior_walls = (2..30 - 2)
            .flat_map(|y| (2..40 - 2).map(move |x| (x, y)))
            .filter(|&(x, y)| map.get_tile(x, y) == TileKind::Wall)
            .count();
        assert!(interior_walls > 0, "건물 외벽이 내부에 생겨야 한다");
    }

    #[test]
    fn 구역_여러개가_방으로_등록된다() {
        let gen = VoronoiDistrictsGenerator;
        let map = gen.generate(40, 30, 11);
        assert!(map.rooms.len() >= 2, "구역별 건물이 방으로 등록돼야 한다");
    }
}
