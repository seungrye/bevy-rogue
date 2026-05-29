use rand::prelude::*;
use crate::modules::map::{Map, TileKind, Rect};
use super::super::MapGenerator;
use super::{carve_corridor, ensure_connectivity, add_rooms_from_floor};

/// TinyKeep 스타일 던전 생성기.
///
/// 절차:
/// 1) 중심 원형 영역에 무작위 크기/위치로 방 후보를 다수 뿌린다.
/// 2) Separation force 로 겹친 방들을 바깥쪽으로 밀어 분리한다(반복 수렴).
/// 3) 평균 크기 이상인 방만 "메인" 방으로 채택, 나머지는 폐기한다.
/// 4) 메인 방 중심점들로 들로네 삼각분할(Bowyer-Watson) 수행.
/// 5) 들로네 간선으로 Kruskal MST 를 구해 모든 방을 사이클 없이 연결.
/// 6) MST 에 들로네 나머지 간선 일부(15%)를 다시 추가해 루프 형성.
/// 7) 각 간선마다 두 방 중심을 L자형 복도로 잇는다.
pub struct TinyKeepGenerator;

impl MapGenerator for TinyKeepGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;
        map.algorithm = self.name().to_string();

        // 1) 방 후보 생성
        let rooms = scatter_rooms(width, height, &mut rng, TRY_COUNT);

        // 2) Separation
        let rooms = separate_rooms(rooms, SEPARATION_ITERS);

        // 맵 경계 안쪽으로 잘라낸다(테두리 보존, 여백 1).
        let clipped = clip_rooms_to_map(&rooms, width, height);

        // 3) 메인 방 선택(평균 면적 * MAIN_THRESHOLD 이상).
        let main_idx = select_main_rooms(&clipped, MAIN_THRESHOLD);

        // 메인 방이 2개 미만이면 던전을 형성할 수 없어 폴백:
        // 모든 사각방 중 큰 순으로 최소 2개라도 메인으로 선언.
        let main_idx = if main_idx.len() < 2 {
            ensure_min_two_main(&clipped)
        } else {
            main_idx
        };

        // 4) 메인 방 중심점 → 들로네 삼각분할
        let centers: Vec<(f64, f64)> = main_idx
            .iter()
            .map(|&i| {
                let r = &clipped[i];
                let (cx, cy) = r.center();
                (cx as f64, cy as f64)
            })
            .collect();

        let mut tris = delaunay_triangulate(&centers);
        // 점이 3개 미만이거나 모두 동일선상이면 tris 가 비어버릴 수 있다.
        // 그래도 MST 단계에서 모든 점쌍을 후보로 추가하므로 안전.
        let mut edges = triangles_to_edges(&mut tris);

        // 5) MST(Kruskal). centers 거리로 가중치.
        let weighted: Vec<(usize, usize, f64)> = edges
            .iter()
            .map(|&(a, b)| (a, b, dist_sq(centers[a], centers[b])))
            .collect();
        let mst = kruskal_mst(centers.len(), &weighted);

        // 6) 사이클 일부 재도입 — MST 에 없는 들로네 간선 15% 추가.
        let mst_set: std::collections::HashSet<(usize, usize)> = mst
            .iter()
            .map(|&(a, b)| if a < b { (a, b) } else { (b, a) })
            .collect();
        // edges 정규화(작은 인덱스 먼저)
        for e in edges.iter_mut() {
            if e.0 > e.1 { std::mem::swap(&mut e.0, &mut e.1); }
        }
        let mut chosen = mst.clone();
        for (a, b) in edges.iter().copied() {
            if mst_set.contains(&(a, b)) { continue; }
            if rng.gen_bool(CYCLE_RATIO) {
                chosen.push((a, b));
            }
        }

        // 7) 메인 방 카브 + 복도
        let main_rooms: Vec<Rect> = main_idx.iter().map(|&i| clipped[i]).collect();
        for r in &main_rooms {
            carve_rect(&mut map, r);
        }
        // 폴백: 메인이 2개로 강제됐고 다른 작은 방이 복도 경로상에 있으면
        // 그 작은 방도 같이 카브해 매끄럽게 만든다. — 단순화: 카브는 메인만.
        for (a, b) in chosen.iter().copied() {
            let (ax, ay) = main_rooms[a].center();
            let (bx, by) = main_rooms[b].center();
            carve_corridor(&mut map, ax, ay, bx, by);
        }

        map.rooms = main_rooms;

        // 안전망: 후처리 — 연결성 보장 & 방 수 보강.
        ensure_connectivity(&mut map);
        add_rooms_from_floor(&mut map);

        map
    }

    fn name(&self) -> &str { "tinykeep" }
}

// === 매개변수 ===
const TRY_COUNT: usize = 30;
const ROOM_MIN: i32 = 5;
const ROOM_MAX: i32 = 12;
const SCATTER_RADIUS: f64 = 30.0;
const SEPARATION_ITERS: usize = 80;
const MAIN_THRESHOLD: f64 = 1.25;
const CYCLE_RATIO: f64 = 0.15;

// === 방 후보(부동소수 중심·크기) ===
#[derive(Clone, Copy, Debug)]
struct Room {
    cx: f64,
    cy: f64,
    w: i32,
    h: i32,
}

impl Room {
    fn half_w(&self) -> f64 { self.w as f64 * 0.5 }
    fn half_h(&self) -> f64 { self.h as f64 * 0.5 }
}

/// 중심점 (map_w/2, map_h/2) 의 원형 영역에 랜덤 크기 방들을 흩뿌린다.
fn scatter_rooms(map_w: usize, map_h: usize, rng: &mut impl Rng, count: usize) -> Vec<Room> {
    let cx0 = map_w as f64 * 0.5;
    let cy0 = map_h as f64 * 0.5;
    let mut rooms = Vec::with_capacity(count);
    for _ in 0..count {
        // 단위원 균등 샘플(거부 샘플링) — 결정적, RNG 사용.
        let (rx, ry) = loop {
            let x = rng.gen_range(-1.0f64..=1.0);
            let y = rng.gen_range(-1.0f64..=1.0);
            if x * x + y * y <= 1.0 {
                break (x, y);
            }
        };
        let w = rng.gen_range(ROOM_MIN..=ROOM_MAX);
        let h = rng.gen_range(ROOM_MIN..=ROOM_MAX);
        rooms.push(Room {
            cx: cx0 + rx * SCATTER_RADIUS,
            cy: cy0 + ry * SCATTER_RADIUS,
            w,
            h,
        });
    }
    rooms
}

/// Separation steering — 겹친 방들을 서로 멀어지게 반복 이동.
/// 결정론(시드만이 입력이고 RNG 미사용).
fn separate_rooms(mut rooms: Vec<Room>, max_iters: usize) -> Vec<Room> {
    for _ in 0..max_iters {
        let mut moved = false;
        let n = rooms.len();
        let mut deltas = vec![(0.0f64, 0.0f64); n];
        for i in 0..n {
            for j in (i + 1)..n {
                let a = rooms[i];
                let b = rooms[j];
                let dx = a.cx - b.cx;
                let dy = a.cy - b.cy;
                // AABB 겹침 + 한 칸 패딩(복도 침투 방지).
                let overlap_x = (a.half_w() + b.half_w() + 1.0) - dx.abs();
                let overlap_y = (a.half_h() + b.half_h() + 1.0) - dy.abs();
                if overlap_x > 0.0 && overlap_y > 0.0 {
                    // 더 작게 겹친 축으로 밀어낸다 — 안정적 분리.
                    if overlap_x < overlap_y {
                        let push = overlap_x * 0.5 * if dx >= 0.0 { 1.0 } else { -1.0 };
                        deltas[i].0 += push;
                        deltas[j].0 -= push;
                    } else {
                        let push = overlap_y * 0.5 * if dy >= 0.0 { 1.0 } else { -1.0 };
                        deltas[i].1 += push;
                        deltas[j].1 -= push;
                    }
                    moved = true;
                }
            }
        }
        for i in 0..n {
            rooms[i].cx += deltas[i].0;
            rooms[i].cy += deltas[i].1;
        }
        if !moved { break; }
    }
    rooms
}

/// `Room` 의 부동소수 중심·크기를 정수 `Rect` 로 변환하고 맵 경계 안쪽으로 잘라낸다.
/// 잘려서 너비/높이가 ROOM_MIN 미만이 된 방은 폐기한다.
fn clip_rooms_to_map(rooms: &[Room], map_w: usize, map_h: usize) -> Vec<Rect> {
    let mut out = Vec::new();
    for r in rooms {
        let x1f = r.cx - r.half_w();
        let y1f = r.cy - r.half_h();
        let mut x1 = x1f.floor() as i32;
        let mut y1 = y1f.floor() as i32;
        let mut x2 = x1 + r.w;
        let mut y2 = y1 + r.h;
        // 맵 경계(1..w-1, 1..h-1) 안쪽으로 클램프.
        if x1 < 1 { x1 = 1; }
        if y1 < 1 { y1 = 1; }
        if x2 > map_w as i32 - 1 { x2 = map_w as i32 - 1; }
        if y2 > map_h as i32 - 1 { y2 = map_h as i32 - 1; }
        let w = x2 - x1;
        let h = y2 - y1;
        if w < ROOM_MIN || h < ROOM_MIN { continue; }
        out.push(Rect::new(x1 as usize, y1 as usize, w as usize, h as usize));
    }
    out
}

/// 평균 면적 * threshold 이상인 방 인덱스만 골라 반환.
pub(crate) fn select_main_rooms(rooms: &[Rect], threshold: f64) -> Vec<usize> {
    if rooms.is_empty() { return Vec::new(); }
    let avg = rooms.iter().map(|r| (r.width() * r.height()) as f64).sum::<f64>()
        / rooms.len() as f64;
    let cutoff = avg * threshold;
    rooms.iter().enumerate()
        .filter(|(_, r)| (r.width() * r.height()) as f64 >= cutoff)
        .map(|(i, _)| i)
        .collect()
}

/// 메인 방이 부족할 때의 폴백: 면적 내림차순으로 가능한 만큼 메인으로.
/// 보통 너무 작은 영역/시드에서만 호출된다.
fn ensure_min_two_main(rooms: &[Rect]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..rooms.len()).collect();
    idx.sort_by(|&a, &b| {
        let aa = rooms[a].width() * rooms[a].height();
        let bb = rooms[b].width() * rooms[b].height();
        bb.cmp(&aa)
    });
    // 최대 두 개(또는 가능한 만큼)만 채택 — 폴백이 너무 많은 방을 카브하지 않도록.
    idx.into_iter().take(2.min(rooms.len())).collect()
}

fn carve_rect(map: &mut Map, r: &Rect) {
    for y in r.y1..r.y2 {
        for x in r.x1..r.x2 {
            map.set_tile(x, y, TileKind::Floor);
        }
    }
}

fn dist_sq(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    dx * dx + dy * dy
}

// === Bowyer-Watson 들로네 삼각분할 ===

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Tri(pub usize, pub usize, pub usize);

/// 점 집합에 대해 들로네 삼각형 인덱스 목록을 반환한다.
/// 점이 3개 미만이거나 모든 점이 동일선상에 있으면 빈 리스트.
pub(crate) fn delaunay_triangulate(points: &[(f64, f64)]) -> Vec<Tri> {
    let n = points.len();
    if n < 3 { return Vec::new(); }

    // 슈퍼 삼각형 — 모든 점을 포함하는 거대한 삼각형.
    let (mut minx, mut miny, mut maxx, mut maxy) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for &(x, y) in points {
        if x < minx { minx = x; }
        if y < miny { miny = y; }
        if x > maxx { maxx = x; }
        if y > maxy { maxy = y; }
    }
    let dx = (maxx - minx).max(1.0);
    let dy = (maxy - miny).max(1.0);
    let dmax = dx.max(dy) * 20.0;
    let midx = (minx + maxx) * 0.5;
    let midy = (miny + maxy) * 0.5;

    // 슈퍼 삼각형 정점은 points 끝에 임시 추가.
    let mut pts: Vec<(f64, f64)> = points.to_vec();
    let s0 = pts.len();
    pts.push((midx - dmax, midy - dmax));
    pts.push((midx + dmax, midy - dmax));
    pts.push((midx, midy + dmax));
    let s1 = s0 + 1;
    let s2 = s0 + 2;

    let mut tris: Vec<Tri> = vec![Tri(s0, s1, s2)];

    for i in 0..n {
        let p = pts[i];
        // 점 p 의 외접원 안에 들어가는 삼각형 → "bad" 로 표시 후 제거.
        let mut bad: Vec<usize> = Vec::new();
        for (ti, t) in tris.iter().enumerate() {
            if in_circumcircle(p, pts[t.0], pts[t.1], pts[t.2]) {
                bad.push(ti);
            }
        }
        // bad 삼각형들의 경계 다각형(중복되지 않은 간선) 추출.
        let mut edges: Vec<(usize, usize, u32)> = Vec::new(); // (a, b, count)
        for &bi in &bad {
            let t = tris[bi];
            for (a, b) in [(t.0, t.1), (t.1, t.2), (t.2, t.0)] {
                let key = if a < b { (a, b) } else { (b, a) };
                if let Some(e) = edges.iter_mut().find(|e| (e.0, e.1) == key) {
                    e.2 += 1;
                } else {
                    edges.push((key.0, key.1, 1));
                }
            }
        }
        // bad 제거(인덱스 뒤에서부터).
        bad.sort_unstable();
        for &bi in bad.iter().rev() {
            tris.swap_remove(bi);
        }
        // 경계(중복 1) 간선 각각으로 새 삼각형 추가.
        for (a, b, c) in edges {
            if c == 1 {
                tris.push(Tri(a, b, i));
            }
        }
    }

    // 슈퍼 삼각형 정점에 닿는 삼각형 제거.
    tris.retain(|t| t.0 < s0 && t.1 < s0 && t.2 < s0);
    tris
}

/// 점 p 가 삼각형 (a,b,c) 외접원 내부(또는 경계)에 있는지.
fn in_circumcircle(p: (f64, f64), a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> bool {
    let ax = a.0 - p.0; let ay = a.1 - p.1;
    let bx = b.0 - p.0; let by = b.1 - p.1;
    let cx = c.0 - p.0; let cy = c.1 - p.1;
    let d = ax * (by * (cx * cx + cy * cy) - cy * (bx * bx + by * by))
          - ay * (bx * (cx * cx + cy * cy) - cx * (bx * bx + by * by))
          + (ax * ax + ay * ay) * (bx * cy - by * cx);
    // CCW(반시계) 가정 시 d > 0 면 내부. 입력 순서가 일정치 않으므로 부호 검사:
    // signed area s 와 같은 부호이면 내부.
    let s = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
    if s > 0.0 { d > 0.0 } else { d < 0.0 }
}

/// 삼각형 리스트 → 중복 제거된 간선(쌍).
/// 결정론적 순서 보장을 위해 정렬된 Vec 를 반환한다(HashSet 미사용).
pub(crate) fn triangles_to_edges(tris: &mut Vec<Tri>) -> Vec<(usize, usize)> {
    let mut v: Vec<(usize, usize)> = Vec::new();
    for t in tris.iter() {
        for (a, b) in [(t.0, t.1), (t.1, t.2), (t.2, t.0)] {
            let k = if a < b { (a, b) } else { (b, a) };
            v.push(k);
        }
    }
    v.sort_unstable();
    v.dedup();
    v
}

// === Kruskal MST ===

struct DisjointSet {
    parent: Vec<usize>,
    rank: Vec<usize>,
}

impl DisjointSet {
    fn new(n: usize) -> Self {
        Self { parent: (0..n).collect(), rank: vec![0; n] }
    }
    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }
    fn union(&mut self, a: usize, b: usize) -> bool {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb { return false; }
        if self.rank[ra] < self.rank[rb] {
            self.parent[ra] = rb;
        } else if self.rank[ra] > self.rank[rb] {
            self.parent[rb] = ra;
        } else {
            self.parent[rb] = ra;
            self.rank[ra] += 1;
        }
        true
    }
}

/// Kruskal MST — 가중치(거리) 오름차순으로 union-find 로 사이클 회피.
/// 들로네 간선이 0 이거나 부족하면, n*(n-1)/2 모든 쌍을 후보로 보강해
/// 어떤 입력이든 spanning forest 를 반환한다(분리 그래프는 forest).
pub(crate) fn kruskal_mst(n: usize, edges: &[(usize, usize, f64)]) -> Vec<(usize, usize)> {
    if n == 0 { return Vec::new(); }
    let mut all: Vec<(usize, usize, f64)> = edges.to_vec();
    if all.is_empty() {
        // 모든 쌍을 후보로 — 거리 정보 없으니 가중치는 인덱스 차이로 결정론.
        for i in 0..n {
            for j in (i + 1)..n {
                let w = (j as f64 - i as f64).abs();
                all.push((i, j, w));
            }
        }
    }
    all.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
    let mut ds = DisjointSet::new(n);
    let mut mst = Vec::new();
    for (a, b, _) in all {
        if ds.union(a, b) {
            mst.push((a, b));
            if mst.len() == n - 1 { break; }
        }
    }
    mst
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;
    use crate::modules::map::MapGenerator;

    #[test]
    fn tinykeep_생성기는_시드가_같으면_동일한_맵을_낸다() {
        let g = TinyKeepGenerator;
        let a = g.generate(80, 50, 12345);
        let b = g.generate(80, 50, 12345);
        assert_eq!(a.tiles.len(), b.tiles.len());
        for i in 0..a.tiles.len() {
            assert_eq!(a.tiles[i].kind, b.tiles[i].kind, "tile {} differs", i);
        }
        assert_eq!(a.rooms.len(), b.rooms.len(), "방 개수도 동일해야 한다");
    }

    #[test]
    fn tinykeep_은_여러_시드에서_방이_하나_이상_나온다() {
        let g = TinyKeepGenerator;
        for seed in [1u64, 42, 100, 777, 9999] {
            let m = g.generate(80, 50, seed);
            assert!(!m.rooms.is_empty(), "seed {} 에서 방이 0", seed);
        }
    }

    #[test]
    fn tinykeep_은_모든_방이_walkable이며_BFS로_도달가능하다() {
        let g = TinyKeepGenerator;
        let m = g.generate(80, 50, 2026);
        // 모든 방 중심이 Floor.
        for r in &m.rooms {
            let (cx, cy) = r.center();
            assert!(m.get_tile(cx, cy).is_walkable(), "방 중심({},{}) 가 Floor 가 아님", cx, cy);
        }
        // 첫 방 중심에서 BFS — 모든 방 중심에 도달.
        let (sx, sy) = m.rooms[0].center();
        let w = m.width;
        let h = m.height;
        let mut visited = vec![false; w * h];
        let mut stack = vec![(sx, sy)];
        visited[sy * w + sx] = true;
        while let Some((x, y)) = stack.pop() {
            for (dx, dy) in [(-1i32, 0), (1, 0), (0, -1), (0, 1)] {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 { continue; }
                let (nx, ny) = (nx as usize, ny as usize);
                let idx = ny * w + nx;
                if !visited[idx] && m.get_tile(nx, ny).is_walkable() {
                    visited[idx] = true;
                    stack.push((nx, ny));
                }
            }
        }
        for r in &m.rooms {
            let (cx, cy) = r.center();
            assert!(visited[cy * w + cx], "방 중심({},{}) 미도달", cx, cy);
        }
    }

    // === 단위 테스트: 들로네 ===

    #[test]
    fn 들로네는_점이_3개미만이면_빈리스트를_낸다() {
        assert!(delaunay_triangulate(&[]).is_empty());
        assert!(delaunay_triangulate(&[(0.0, 0.0)]).is_empty());
        assert!(delaunay_triangulate(&[(0.0, 0.0), (1.0, 1.0)]).is_empty());
    }

    #[test]
    fn 들로네는_정사각형_네_점에_대해_두_삼각형을_낸다() {
        // 네 코너 → 들로네는 두 삼각형(어느 대각선이든) 4개 점 사용.
        let pts = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let tris = delaunay_triangulate(&pts);
        assert_eq!(tris.len(), 2, "네 점은 두 삼각형");
        // 모든 점 인덱스가 출현.
        let mut used = [false; 4];
        for t in &tris {
            used[t.0] = true;
            used[t.1] = true;
            used[t.2] = true;
        }
        assert!(used.iter().all(|&u| u), "모든 점이 어떤 삼각형에든 포함");
    }

    #[test]
    fn 들로네_법칙_각_삼각형_외접원에_다른점이_없다() {
        let pts: Vec<(f64, f64)> = vec![
            (0.0, 0.0), (10.0, 0.0), (5.0, 8.0), (5.0, 3.0),
            (12.0, 6.0), (-2.0, 5.0),
        ];
        let tris = delaunay_triangulate(&pts);
        assert!(!tris.is_empty(), "삼각형이 만들어져야 한다");
        for t in &tris {
            let a = pts[t.0]; let b = pts[t.1]; let c = pts[t.2];
            for (i, &p) in pts.iter().enumerate() {
                if i == t.0 || i == t.1 || i == t.2 { continue; }
                assert!(
                    !in_circumcircle_strict(p, a, b, c),
                    "삼각형 {:?} 의 외접원 내부에 점 {} 존재", t, i
                );
            }
        }
    }

    /// 테스트용: 경계는 제외하고 strict 내부만(epsilon tolerance).
    fn in_circumcircle_strict(p: (f64, f64), a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> bool {
        let ax = a.0 - p.0; let ay = a.1 - p.1;
        let bx = b.0 - p.0; let by = b.1 - p.1;
        let cx = c.0 - p.0; let cy = c.1 - p.1;
        let d = ax * (by * (cx * cx + cy * cy) - cy * (bx * bx + by * by))
              - ay * (bx * (cx * cx + cy * cy) - cx * (bx * bx + by * by))
              + (ax * ax + ay * ay) * (bx * cy - by * cx);
        let s = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
        if s > 0.0 { d > 1e-6 } else { d < -1e-6 }
    }

    // === 단위 테스트: MST ===

    #[test]
    fn MST는_N개_노드에_대해_N빼기1_간선과_모든_노드_연결을_보장한다() {
        // 5개 노드, 들로네/임의 간선.
        let edges = vec![
            (0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (3, 4, 1.0),
            (0, 4, 5.0), (1, 3, 3.0),
        ];
        let mst = kruskal_mst(5, &edges);
        assert_eq!(mst.len(), 4, "N-1 = 4 간선");
        // 모든 노드가 같은 컴포넌트.
        let mut ds = DisjointSet::new(5);
        for (a, b) in &mst { ds.union(*a, *b); }
        let root = ds.find(0);
        for i in 1..5 {
            assert_eq!(ds.find(i), root, "노드 {} 미연결", i);
        }
    }

    #[test]
    fn MST는_빈_간선_입력에서도_모든쌍_폴백으로_연결을_만든다() {
        // edges 가 비어있어도 n*(n-1)/2 폴백 후보로 N-1 트리를 만든다.
        let mst = kruskal_mst(4, &[]);
        assert_eq!(mst.len(), 3, "폴백으로도 N-1 트리");
    }

    #[test]
    fn MST는_노드가_없으면_빈리스트를_낸다() {
        assert!(kruskal_mst(0, &[]).is_empty());
    }

    // === 단위 테스트: Separation ===

    #[test]
    fn separation_후_방들이_AABB로_겹치지_않는다() {
        // 같은 중심에 다섯 방을 놓고 분리.
        let rooms = vec![
            Room { cx: 30.0, cy: 30.0, w: 6, h: 6 },
            Room { cx: 30.0, cy: 30.0, w: 7, h: 5 },
            Room { cx: 30.0, cy: 30.0, w: 5, h: 7 },
            Room { cx: 30.0, cy: 30.0, w: 8, h: 8 },
            Room { cx: 30.0, cy: 30.0, w: 5, h: 5 },
        ];
        let out = separate_rooms(rooms, 200);
        // 모든 쌍 검사.
        for i in 0..out.len() {
            for j in (i + 1)..out.len() {
                let a = out[i];
                let b = out[j];
                let ox = (a.half_w() + b.half_w()) - (a.cx - b.cx).abs();
                let oy = (a.half_h() + b.half_h()) - (a.cy - b.cy).abs();
                // 한 축이라도 분리되어 있으면 OK.
                assert!(
                    ox <= 0.5 || oy <= 0.5,
                    "방 {} 와 {} 가 분리되지 않음 (overlap_x={:.2}, overlap_y={:.2})",
                    i, j, ox, oy
                );
            }
        }
    }

    #[test]
    fn 메인방_선택은_평균보다_큰_방만_남긴다() {
        let rooms = vec![
            Rect::new(0, 0, 5, 5),    // 면적 25
            Rect::new(0, 0, 10, 10),  // 100
            Rect::new(0, 0, 6, 6),    // 36
            Rect::new(0, 0, 4, 4),    // 16
        ];
        // 평균 = (25+100+36+16)/4 = 44.25, threshold 1.0 → 100, 가능하면 36도 빠짐
        let main = select_main_rooms(&rooms, 1.0);
        assert!(main.contains(&1), "가장 큰 방이 포함돼야 한다");
        assert!(!main.contains(&3), "작은 방은 제외");
    }

    #[test]
    fn 메인방_선택은_빈입력에서_빈리스트를_낸다() {
        assert!(select_main_rooms(&[], 1.25).is_empty());
    }
}
