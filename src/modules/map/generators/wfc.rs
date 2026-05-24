use rand::prelude::*;
use crate::modules::map::{Map, TileKind};
use super::super::MapGenerator;
use super::{ensure_connectivity, add_rooms_from_floor};

/// Wave Function Collapse(타일드 모델) 지상 맵 생성기.
///
/// ## 타일셋 설계 (소켓 매칭 3×3 블록)
/// 타일 하나는 3×3 의 벽/바닥 패턴 블록이다. 최종 맵은 이 블록들을 격자로
/// 이어 붙여 만든다. 소규모 큐레이션 타일셋(9종)을 쓴다:
///   - `SOLID`  : 전부 벽. 벽 덩어리/구획 분리.
///   - `OPEN`   : 전부 바닥. 방 내부.
///   - `CROSS`  : 십자 통로(상하좌우 가운데 열림). 교차로.
///   - `H_HALL` : 가로 통로(가운데 가로줄만 바닥).
///   - `V_HALL` : 세로 통로(가운데 세로줄만 바닥).
///   - `ROOM_L/R/U/D` : 방-통로 연결 타일. 한 변은 방 모서리([O,O,O]),
///     반대편·옆 변은 통로 모서리([X,O,X])라서 OPEN 방과 통로를 이어준다.
///
/// ## 인접 제약 (소켓 매칭)
/// 두 타일이 가로 이웃이면 왼쪽 타일의 **오른쪽 열**과 오른쪽 타일의 **왼쪽 열**이
/// 정확히 같아야 한다(세로도 동일). 맞닿는 모서리의 벽/바닥 패턴이 일치해야
/// 이어 붙는다. 모든 모서리 소켓은 [X,X,X](벽), [O,O,O](방), [X,O,X](통로)
/// 셋 중 하나라서, ROOM_* 가 방([O,O,O])과 통로([X,O,X])를 다리처럼 잇는다.
/// 이 소켓 매칭이 방·통로·벽이 끊김 없이 연결되도록 강제한다.
///
/// ## 모순 처리
/// 엔트로피(가능 타일 수) 최소 셀을 가중 무작위로 붕괴(collapse)시키고 제약을
/// 전파(propagate)한다(변한 이웃으로 연쇄). 가능 타일이 0이 되면(모순) 시드를
/// 변형해 최대 `MAX_RESTARTS` 회 재시작한다. 모두 실패하면 안전한 폴백(빈 방)
/// 으로 맵을 만든다. 성공해도 Floor 비율이 부족하면 폴백으로 대체한다.
pub struct WfcGenerator;

/// 블록 한 변의 셀 수(3×3 타일).
const B: usize = 3;

/// 최대 재시작 횟수.
const MAX_RESTARTS: u32 = 20;

/// 3×3 블록 패턴 하나. `true` = 바닥, `false` = 벽. 행 우선(row-major) 9칸.
#[derive(Copy, Clone)]
struct Pattern {
    cells: [bool; B * B],
    weight: u32,
}

const X: bool = false; // 벽
const O: bool = true; // 바닥

/// 큐레이션 타일셋. 모서리 소켓이 {벽[X,X,X], 방[O,O,O], 통로[X,O,X]} 셋으로만
/// 구성돼 서로 맞물린다. ROOM_* 타일이 방([O,O,O])과 통로([X,O,X]) 사이를 이어
/// 바닥이 단일 연결을 이루기 쉽게 한다.
const TILES: [Pattern; 9] = [
    // 0 SOLID — 전부 벽. 열린 공간을 구획으로 분리하되 바닥이 끊길 만큼 과하지 않게.
    Pattern { cells: [X, X, X,
                      X, X, X,
                      X, X, X], weight: 9 },
    // 1 OPEN — 전부 바닥(방 내부). 적당히 뭉쳐 방을 이룬다.
    Pattern { cells: [O, O, O,
                      O, O, O,
                      O, O, O], weight: 10 },
    // 2 CROSS — 십자 통로(상하좌우 [X,O,X] 통로 소켓, 교차로)
    Pattern { cells: [X, O, X,
                      O, O, O,
                      X, O, X], weight: 2 },
    // 3 H_HALL — 가로 통로(좌우 [X,O,X], 상하 [X,X,X])
    Pattern { cells: [X, X, X,
                      O, O, O,
                      X, X, X], weight: 3 },
    // 4 V_HALL — 세로 통로(상하 [X,O,X], 좌우 [X,X,X])
    Pattern { cells: [X, O, X,
                      X, O, X,
                      X, O, X], weight: 3 },
    // 5 ROOM_R — 왼쪽이 방 소켓[O,O,O], 오른쪽이 통로 소켓[X,O,X] (방→오른쪽 통로)
    Pattern { cells: [O, O, X,
                      O, O, O,
                      O, O, X], weight: 3 },
    // 6 ROOM_L — 오른쪽이 방 소켓[O,O,O], 왼쪽이 통로 소켓[X,O,X] (방→왼쪽 통로)
    Pattern { cells: [X, O, O,
                      O, O, O,
                      X, O, O], weight: 3 },
    // 7 ROOM_D — 위가 방 소켓[O,O,O], 아래가 통로 소켓[X,O,X] (방→아래 통로)
    Pattern { cells: [O, O, O,
                      O, O, O,
                      X, O, X], weight: 3 },
    // 8 ROOM_U — 아래가 방 소켓[O,O,O], 위가 통로 소켓[X,O,X] (방→위 통로)
    Pattern { cells: [X, O, X,
                      O, O, O,
                      O, O, O], weight: 3 },
];

const N_TILES: usize = TILES.len();

impl Pattern {
    /// 오른쪽 모서리 열(x=2) 의 세 셀.
    fn right_col(&self) -> [bool; B] { [self.cells[2], self.cells[5], self.cells[8]] }
    /// 왼쪽 모서리 열(x=0).
    fn left_col(&self) -> [bool; B] { [self.cells[0], self.cells[3], self.cells[6]] }
    /// 위쪽 모서리 행(y=0).
    fn top_row(&self) -> [bool; B] { [self.cells[0], self.cells[1], self.cells[2]] }
    /// 아래쪽 모서리 행(y=2).
    fn bottom_row(&self) -> [bool; B] { [self.cells[6], self.cells[7], self.cells[8]] }
}

/// 방향 인덱스: 0=좌, 1=우, 2=상, 3=하.
const DIRS: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

/// 타일 `a` 의 `dir` 쪽에 타일 `b` 가 올 수 있는가(소켓 일치).
fn compatible(a: usize, b: usize, dir: usize) -> bool {
    let pa = &TILES[a];
    let pb = &TILES[b];
    match dir {
        0 => pa.left_col() == pb.right_col(),   // b 가 a 의 왼쪽
        1 => pa.right_col() == pb.left_col(),   // b 가 a 의 오른쪽
        2 => pa.top_row() == pb.bottom_row(),   // b 가 a 의 위
        3 => pa.bottom_row() == pb.top_row(),   // b 가 a 의 아래
        _ => unreachable!(), // 도달 불가 방어코드: dir 은 0..4 만 쓰인다.
    }
}

/// 타일 `t` 의 `dir` 쪽에 올 수 있는 모든 타일의 비트마스크.
fn allowed_mask(t: usize, dir: usize) -> u16 {
    let mut m = 0u16;
    for b in 0..N_TILES {
        if compatible(t, b, dir) {
            m |= 1 << b;
        }
    }
    m
}

impl MapGenerator for WfcGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        map.seed = seed;
        map.algorithm = self.name().to_string();

        // 블록 격자 크기. 테두리(한 칸)는 후처리에서 Wall 강제하므로
        // 내부 영역 [1, w-1) × [1, h-1) 을 B 로 나눈 만큼만 블록을 깐다.
        let iw = width.saturating_sub(2) / B;
        let ih = height.saturating_sub(2) / B;

        // 블록 격자가 너무 작으면 WFC 가 의미 없다 — 폴백.
        if iw < 2 || ih < 2 {
            // 도달 불가 방어코드: 실제 맵(40×30 이상)에선 항상 iw/ih ≥ 2.
            apply_fallback(&mut map);
            finalize(&mut map);
            return map;
        }

        // 시드를 변형해가며 모순이면 재시작(production 제약 함수 주입).
        let weights: Vec<u32> = TILES.iter().map(|t| t.weight).collect();
        let result = solve(iw, ih, seed, N_TILES, &weights, &allowed_mask);

        match result {
            Some(grid) => {
                paint_grid(&mut map, &grid, iw, ih);
                finalize(&mut map);
                // 연결성 보장 후 Floor 비율이 부족하면 폴백으로 대체.
                let floor = map.tiles.iter().filter(|t| t.kind == TileKind::Floor).count();
                if (floor as f32) / ((width * height) as f32) < 0.10 {
                    apply_fallback(&mut map);
                    finalize(&mut map);
                }
            }
            None => {
                // 모든 재시작 실패 → 안전한 폴백.
                // 도달 불가 방어코드: production 타일셋(`allowed_mask`)은 SOLID/OPEN 이
                // 항상 유효 해라서 solve 가 None 을 내지 않는다. 모순→실패 경로는
                // 테스트 seam(`impossible_allowed`)으로 solve/run_wfc 단위에서 검증한다.
                apply_fallback(&mut map);
                finalize(&mut map);
            }
        }

        map
    }

    fn name(&self) -> &str { "wfc" }
}

/// 붕괴된 블록 격자를 맵 타일로 그린다(테두리 안쪽부터 B 칸 단위 블록 배치).
fn paint_grid(map: &mut Map, grid: &[usize], iw: usize, ih: usize) {
    for by in 0..ih {
        for bx in 0..iw {
            let tile = &TILES[grid[by * iw + bx]];
            for ly in 0..B {
                for lx in 0..B {
                    let kind = if tile.cells[ly * B + lx] { TileKind::Floor } else { TileKind::Wall };
                    let x = 1 + bx * B + lx;
                    let y = 1 + by * B + ly;
                    map.set_tile(x, y, kind);
                }
            }
        }
    }
}

/// 시드를 변형해가며 WFC 를 최대 `MAX_RESTARTS`+1 회 시도한다.
/// 한 시도가 모순이면 시드를 바꿔 재시작하고, 모두 실패하면 None.
///
/// `allowed(t, dir)` 는 타일 `t` 의 `dir` 방향에 올 수 있는 타일 비트마스크.
/// production 은 `allowed_mask` 를, 테스트는 일부러 모순을 내는 함수를 주입해
/// 재시작/실패 경로를 검증한다(테스트 seam).
fn solve(
    iw: usize,
    ih: usize,
    seed: u64,
    n_tiles: usize,
    weights: &[u32],
    allowed: &dyn Fn(usize, usize) -> u16,
) -> Option<Vec<usize>> {
    for attempt in 0..=MAX_RESTARTS {
        let attempt_seed = seed.wrapping_add(attempt as u64).wrapping_mul(0x9e3779b97f4a7c15);
        let mut rng = StdRng::seed_from_u64(attempt_seed);
        if let Some(grid) = run_wfc(iw, ih, n_tiles, weights, allowed, &mut rng) {
            return Some(grid);
        }
        // 모순 → 다음 시드로 재시작.
    }
    None
}

/// 한 번의 WFC 실행. 성공 시 iw×ih 블록 인덱스 격자를, 모순 시 None.
fn run_wfc(
    iw: usize,
    ih: usize,
    n_tiles: usize,
    weights: &[u32],
    allowed: &dyn Fn(usize, usize) -> u16,
    rng: &mut impl Rng,
) -> Option<Vec<usize>> {
    let n = iw * ih;
    let all_mask: u16 = (1u16 << n_tiles) - 1;
    // 각 셀의 가능상태 비트셋(초기: 모든 타일 가능).
    let mut cells = vec![all_mask; n];

    loop {
        // 1) 미붕괴(가능>1) 셀 중 엔트로피(가능 수) 최소 셀 선택.
        // 가능 0(모순)은 collapse 직후 propagate 가 항상 잡아 None 을 내므로
        // 이 시점의 mask 는 언제나 ≥1 비트 — 별도 0 검사는 두지 않는다.
        let mut best: Option<usize> = None;
        let mut best_count = u32::MAX;
        for (i, &mask) in cells.iter().enumerate() {
            let cnt = mask.count_ones();
            if cnt > 1 && cnt < best_count {
                best_count = cnt;
                best = Some(i);
            }
        }

        let Some(cell) = best else {
            break; // 모두 단일 상태로 붕괴 — 완료.
        };

        // 2) 가중 무작위로 하나 선택해 collapse.
        let chosen = weighted_pick(cells[cell], n_tiles, weights, rng);
        cells[cell] = 1 << chosen;

        // 3) 제약 전파.
        if !propagate(&mut cells, iw, ih, n_tiles, allowed, cell) {
            return None; // 전파 중 모순.
        }
    }

    // 각 셀의 단일 타일 인덱스 확정.
    let grid: Vec<usize> = cells.iter().map(|&mask| mask.trailing_zeros() as usize).collect();
    Some(grid)
}

/// 비트셋에서 가중치에 따라 타일 인덱스 하나를 고른다(최소 1비트 있다고 가정).
fn weighted_pick(mask: u16, n_tiles: usize, weights: &[u32], rng: &mut impl Rng) -> usize {
    let candidates: Vec<usize> = (0..n_tiles).filter(|&t| mask & (1 << t) != 0).collect();
    let total: u32 = candidates.iter().map(|&t| weights[t]).sum();
    let mut roll = rng.gen_range(0..total);
    for &t in &candidates {
        let w = weights[t];
        if roll < w {
            return t;
        }
        roll -= w;
    }
    // 도달 불가 방어코드: roll 은 가중치 합 미만이라 반드시 위에서 반환됨.
    candidates[0]
}

/// `start` 셀에서 BFS 로 소켓 제약을 이웃에 전파한다. 어떤 이웃의 가능상태가
/// 변하면 다시 그 이웃으로 연쇄한다. 모순(가능 0) 발생 시 false.
fn propagate(
    cells: &mut [u16],
    iw: usize,
    ih: usize,
    n_tiles: usize,
    allowed: &dyn Fn(usize, usize) -> u16,
    start: usize,
) -> bool {
    let mut stack = vec![start];
    while let Some(idx) = stack.pop() {
        let cx = idx % iw;
        let cy = idx / iw;
        let cur = cells[idx];

        for (dir, &(dx, dy)) in DIRS.iter().enumerate() {
            let nx = cx as i32 + dx;
            let ny = cy as i32 + dy;
            if nx < 0 || ny < 0 || nx >= iw as i32 || ny >= ih as i32 {
                continue;
            }
            let nidx = ny as usize * iw + nx as usize;

            // 현재 셀의 가능 타일들이 dir 방향으로 허용하는 이웃 마스크 합집합.
            let mut allowed_neighbors = 0u16;
            for t in 0..n_tiles {
                if cur & (1 << t) != 0 {
                    allowed_neighbors |= allowed(t, dir);
                }
            }

            let before = cells[nidx];
            let after = before & allowed_neighbors;
            if after != before {
                if after == 0 {
                    return false; // 모순.
                }
                cells[nidx] = after;
                stack.push(nidx);
            }
        }
    }
    true
}

/// 테두리 강제·연결성 보장·스폰용 방 추가를 한 번에 처리한다.
fn finalize(map: &mut Map) {
    force_wall_border(map);
    ensure_connectivity(map);
    add_rooms_from_floor(map);
}

/// 맵 테두리를 전부 Wall 로 강제한다(플레이어가 맵 밖으로 못 나감).
fn force_wall_border(map: &mut Map) {
    let (w, h) = (map.width, map.height);
    for x in 0..w {
        map.set_tile(x, 0, TileKind::Wall);
        map.set_tile(x, h - 1, TileKind::Wall);
    }
    for y in 0..h {
        map.set_tile(0, y, TileKind::Wall);
        map.set_tile(w - 1, y, TileKind::Wall);
    }
}

/// 안전한 폴백 — 테두리 안쪽을 통째로 바닥으로 카브한 빈 방.
/// WFC 가 끝내 모순이거나 바닥이 너무 적을 때 계약을 보장하기 위해 쓴다.
fn apply_fallback(map: &mut Map) {
    let (w, h) = (map.width, map.height);
    for t in map.tiles.iter_mut() {
        t.kind = TileKind::Wall;
    }
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            map.set_tile(x, y, TileKind::Floor);
        }
    }
    map.rooms.clear();
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;
    use crate::modules::map::MapGenerator;

    const W: usize = 40;
    const H: usize = 30;
    /// 모든 타일 가능 비트셋(초기 셀 상태) — 테스트에서 전파 전후 비교에 쓴다.
    const ALL_MASK: u16 = (1u16 << N_TILES) - 1;

    /// production 가중치 슬라이스.
    fn weights() -> Vec<u32> {
        TILES.iter().map(|t| t.weight).collect()
    }

    #[test]
    fn 같은_시드는_같은_맵을_만든다() {
        let gen = WfcGenerator;
        let a = gen.generate(W, H, 42);
        let b = gen.generate(W, H, 42);
        assert_eq!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            "같은 시드는 동일한 맵을 생성해야 한다",
        );
        assert_eq!(a.rooms.len(), b.rooms.len(), "같은 시드는 같은 방 개수를 만든다");
    }

    #[test]
    fn 다른_시드는_다른_맵을_만든다() {
        let gen = WfcGenerator;
        let a = gen.generate(W, H, 1);
        let b = gen.generate(W, H, 2);
        assert_ne!(
            a.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            b.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            "다른 시드는 다른 맵을 생성해야 한다",
        );
    }

    #[test]
    fn 시드와_알고리즘이름이_맵에_기록된다() {
        let gen = WfcGenerator;
        let map = gen.generate(W, H, 77);
        assert_eq!(map.seed, 77, "맵 시드가 기록돼야 한다");
        assert_eq!(map.algorithm, "wfc", "알고리즘 이름이 기록돼야 한다");
    }

    #[test]
    fn 계약을_충족한다_테두리벽_바닥비율_방둘이상() {
        let gen = WfcGenerator;
        // 여러 시드에서 계약이 항상 성립해야 한다.
        for seed in 0..24u64 {
            let map = gen.generate(W, H, seed);

            // 테두리 전부 벽
            for x in 0..W {
                assert_eq!(map.get_tile(x, 0), TileKind::Wall, "seed {}: 상단 테두리", seed);
                assert_eq!(map.get_tile(x, H - 1), TileKind::Wall, "seed {}: 하단 테두리", seed);
            }
            for y in 0..H {
                assert_eq!(map.get_tile(0, y), TileKind::Wall, "seed {}: 좌측 테두리", seed);
                assert_eq!(map.get_tile(W - 1, y), TileKind::Wall, "seed {}: 우측 테두리", seed);
            }

            // 바닥 비율 ≥ 10%
            let floor = map.tiles.iter().filter(|t| t.kind == TileKind::Floor).count();
            let ratio = floor as f32 / (W * H) as f32;
            assert!(ratio >= 0.10, "seed {}: 바닥 비율 {:.1}% < 10%", seed, ratio * 100.0);

            // 방 ≥ 2개
            assert!(map.rooms.len() >= 2, "seed {}: 방 {} < 2", seed, map.rooms.len());
        }
    }

    #[test]
    fn 결과는_단일_연결요소다() {
        // ensure_connectivity 후 모든 바닥이 하나의 연결요소여야 한다.
        let gen = WfcGenerator;
        for seed in 0..8u64 {
            let map = gen.generate(W, H, seed);
            let floor_tiles: Vec<(usize, usize)> = (0..H)
                .flat_map(|y| (0..W).map(move |x| (x, y)))
                .filter(|&(x, y)| map.get_tile(x, y).is_walkable())
                .collect();
            assert!(!floor_tiles.is_empty(), "seed {}: 바닥이 있어야 한다", seed);

            // 테두리가 전부 Wall 이라 통과타일은 항상 맵 내부(1..W-1,1..H-1)에만
            // 있다 → 4이웃 좌표는 언제나 맵 안. 경계 검사 없이 BFS.
            let (sx, sy) = floor_tiles[0];
            let mut visited = std::collections::HashSet::new();
            let mut stack = vec![(sx, sy)];
            visited.insert((sx, sy));
            while let Some((x, y)) = stack.pop() {
                for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                    let walkable = map.get_tile(nx, ny).is_walkable();
                    let unseen = !visited.contains(&(nx, ny));
                    if walkable & unseen {
                        visited.insert((nx, ny));
                        stack.push((nx, ny));
                    }
                }
            }
            assert_eq!(visited.len(), floor_tiles.len(),
                "seed {}: 모든 바닥이 단일 연결요소여야 한다 (닿음 {} vs 전체 {})",
                seed, visited.len(), floor_tiles.len());
        }
    }

    #[test]
    fn 바닥과_벽이_둘다_생성된다() {
        // 텅 빈 방(전부 바닥)도, 꽉 찬 벽(전부 벽)도 아닌 구조여야 한다.
        let gen = WfcGenerator;
        let map = gen.generate(W, H, 9);
        let floor = map.tiles.iter().filter(|t| t.kind == TileKind::Floor).count();
        let wall = map.tiles.iter().filter(|t| t.kind == TileKind::Wall).count();
        // `&`(비단축)로 양쪽을 항상 평가해 단축평가 분기를 만들지 않는다.
        assert!((floor > 0) & (wall > 0), "벽과 바닥이 모두 있어야 한다");
        // 내부(테두리 외)에 벽이 존재 — 구조가 있다는 의미.
        let interior_wall = (1..H - 1)
            .flat_map(|y| (1..W - 1).map(move |x| (x, y)))
            .any(|(x, y)| map.get_tile(x, y) == TileKind::Wall);
        assert!(interior_wall, "내부에 벽 구조가 있어야 한다");
    }

    // --- 타일셋/소켓 제약 단위 테스트 ---

    #[test]
    fn 소켓제약은_맞물리는_모서리만_허용한다() {
        // SOLID(0) 의 오른쪽 열은 전부 벽 → SOLID(0) 와는 가로로 맞물리지만
        // OPEN(1, 전부 바닥) 과는 맞물리지 않는다.
        assert!(compatible(0, 0, 1), "SOLID-SOLID 가로 맞물림");
        assert!(!compatible(0, 1, 1), "SOLID-OPEN 가로 비맞물림");
        // OPEN 끼리는 사방으로 맞물린다. `&`(비단축)로 단축평가 분기를 피한다.
        assert!(compatible(1, 1, 0) & compatible(1, 1, 1)
            & compatible(1, 1, 2) & compatible(1, 1, 3), "OPEN 끼리 사방 맞물림");
        // ROOM_R(5) 의 왼쪽은 방 소켓 → OPEN(1) 이 왼쪽에 맞물린다(방 연결).
        assert!(compatible(5, 1, 0), "ROOM_R 왼쪽에 OPEN 방이 맞물림");
        // ROOM_R(5) 의 오른쪽은 통로 소켓 → V_HALL? 아니라 가로통로 H_HALL(3) 맞물림.
        assert!(compatible(5, 3, 1), "ROOM_R 오른쪽에 가로통로가 맞물림");
    }

    #[test]
    fn 소켓제약은_대칭이다() {
        // a 의 오른쪽에 b 가 가능하면, b 의 왼쪽에 a 도 가능해야 한다(무방향).
        for a in 0..N_TILES {
            for b in 0..N_TILES {
                assert_eq!(compatible(a, b, 1), compatible(b, a, 0),
                    "타일 {}↔{} 가로 제약이 비대칭", a, b);
                assert_eq!(compatible(a, b, 3), compatible(b, a, 2),
                    "타일 {}↔{} 세로 제약이 비대칭", a, b);
            }
        }
    }

    #[test]
    fn 균일타일은_자기자신과_사방으로_맞물려_해가_항상_존재한다() {
        // SOLID(0)·OPEN(1) 은 모서리가 균일해 자기 자신과 사방으로 맞물린다.
        // 따라서 "전부 SOLID" 또는 "전부 OPEN" 은 항상 유효한 해 → WFC 가
        // 운이 나빠도 보통 성공하며, 실패해도 재시작/폴백으로 맵을 낸다.
        for t in [0usize, 1] {
            for dir in 0..4 {
                assert!(compatible(t, t, dir), "균일타일 {} 자기 인접 dir {}", t, dir);
            }
        }
    }

    #[test]
    fn allowed_mask는_compatible과_일치한다() {
        for t in 0..N_TILES {
            for dir in 0..4 {
                let m = allowed_mask(t, dir);
                for b in 0..N_TILES {
                    assert_eq!((m & (1 << b)) != 0, compatible(t, b, dir),
                        "allowed_mask({},{}) 비트 {} 불일치", t, dir, b);
                }
            }
        }
    }

    #[test]
    fn 전파는_붕괴된_셀의_이웃을_좁힌다() {
        // 1×2 블록 격자: 왼쪽을 SOLID(0) 로 붕괴 후 전파하면 오른쪽은
        // SOLID 의 오른쪽 소켓과 맞물리는 타일만 남아야 한다(= allowed_mask(0,1)).
        let mut cells = vec![ALL_MASK; 2];
        cells[0] = 1 << 0; // SOLID
        let ok = propagate(&mut cells, 2, 1, N_TILES, &allowed_mask, 0);
        assert!(ok, "정상 전파는 모순 없이 끝나야 한다");
        assert_eq!(cells[1], allowed_mask(0, 1),
            "SOLID 옆 셀은 SOLID 의 오른쪽 소켓과 맞물리는 타일만 남는다");
        // 적어도 한 타일은 제거돼 마스크가 줄었어야 한다(전파가 실제로 좁힘).
        assert!(cells[1] != ALL_MASK, "전파가 이웃 마스크를 좁혀야 한다");
    }

    #[test]
    fn 전파는_연쇄적으로_먼_이웃까지_좁힌다() {
        // 1×3 격자: 왼쪽을 SOLID 로 붕괴 → 가운데를 좁히고, 가운데가 변했으니
        // 다시 오른쪽까지 연쇄 전파돼야 한다.
        let mut cells = vec![ALL_MASK; 3];
        cells[0] = 1 << 0; // SOLID
        let ok = propagate(&mut cells, 3, 1, N_TILES, &allowed_mask, 0);
        assert!(ok);
        // 오른쪽 끝 셀도 ALL_MASK 에서 줄어 있어야 한다(연쇄가 닿음).
        assert!(cells[2] != ALL_MASK, "연쇄 전파가 먼 이웃까지 닿아야 한다");
    }

    #[test]
    fn 전파는_모순이면_false를_반환한다() {
        // 왼쪽=SOLID(우측 전부 벽), 오른쪽=OPEN(좌측 전부 바닥) 고정 → 소켓 불일치 모순.
        let mut cells = vec![1u16 << 0, 1u16 << 1];
        let ok = propagate(&mut cells, 2, 1, N_TILES, &allowed_mask, 0);
        assert!(!ok, "맞물리지 않는 두 타일을 고정하면 모순이라 false 여야 한다");
    }

    #[test]
    fn 가중선택은_단일후보면_그것을_고른다() {
        let mut rng = StdRng::seed_from_u64(0);
        let w = weights();
        let only = 1u16 << 1; // OPEN 만
        for _ in 0..10 {
            assert_eq!(weighted_pick(only, N_TILES, &w, &mut rng), 1,
                "후보가 하나면 그것만 골라야 한다");
        }
    }

    #[test]
    fn 가중선택은_여러후보를_모두_뽑을수있다() {
        // 통계적으로 두 후보가 모두 한 번 이상 선택되는지.
        let mut rng = StdRng::seed_from_u64(123);
        let w = weights();
        let mask = (1u16 << 0) | (1u16 << 1); // SOLID | OPEN
        let mut saw0 = false;
        let mut saw1 = false;
        for _ in 0..200 {
            match weighted_pick(mask, N_TILES, &w, &mut rng) {
                0 => saw0 = true,
                1 => saw1 = true,
                other => panic!("마스크에 없는 타일 {} 선택됨", other),
            }
        }
        assert!(saw0 & saw1, "두 후보 모두 선택돼야 한다");
    }

    #[test]
    fn run_wfc는_정상적으로_격자를_채운다() {
        let mut rng = StdRng::seed_from_u64(3);
        let w = weights();
        let grid = run_wfc(6, 6, N_TILES, &w, &allowed_mask, &mut rng)
            .expect("작은 격자는 보통 성공한다");
        assert_eq!(grid.len(), 36, "격자 셀 수가 맞아야 한다");
        for &t in &grid {
            assert!(t < N_TILES, "확정된 타일 인덱스가 유효해야 한다");
        }
    }

    #[test]
    fn solve는_production_제약으로_성공한다() {
        let w = weights();
        let r = solve(10, 8, 42, N_TILES, &w, &allowed_mask);
        assert!(r.is_some(), "production 제약은 풀 수 있어야 한다");
    }

    // --- 모순 → 재시작 → 폴백 경로(테스트 seam) ---

    /// 어떤 타일도 어떤 방향으로도 이웃을 허용하지 않는 "불가능" 제약 함수.
    /// 2칸 이상 격자에서 즉시 모순을 일으켜 재시작/실패 경로를 강제한다.
    fn impossible_allowed(_t: usize, _dir: usize) -> u16 { 0 }

    #[test]
    fn 불가능한_제약에서_실행은_모순으로_실패한다() {
        let mut rng = StdRng::seed_from_u64(1);
        let w = weights();
        // 2×2 격자 + 모든 인접 금지 → 첫 붕괴의 전파에서 이웃이 0이 돼 모순.
        let r = run_wfc(2, 2, N_TILES, &w, &impossible_allowed, &mut rng);
        assert!(r.is_none(), "불가능한 제약은 모순으로 None 이어야 한다");
    }

    #[test]
    fn 모든_재시작이_실패하면_해결은_실패한다() {
        let w = weights();
        // 불가능 제약이라 MAX_RESTARTS+1 회 모두 모순 → 최종 None(재시작 루프 소진).
        let r = solve(3, 3, 7, N_TILES, &w, &impossible_allowed);
        assert!(r.is_none(), "모든 재시작 실패 시 None 이어야 한다");
    }

    #[test]
    fn run_wfc는_단일셀이면_전파없이_즉시_성공한다() {
        // 1×1 격자는 붕괴할 셀이 없거나 한 번 붕괴 후 이웃이 없어 전파가 비어 성공.
        let mut rng = StdRng::seed_from_u64(0);
        let w = weights();
        let r = run_wfc(1, 1, N_TILES, &w, &impossible_allowed, &mut rng);
        assert!(r.is_some(), "단일 셀은 이웃이 없어 불가능 제약과 무관하게 성공");
        assert_eq!(r.unwrap().len(), 1);
    }

    // --- 폴백 함수 단위 테스트 ---

    #[test]
    fn 폴백은_빈방을_만들고_계약을_지킨다() {
        let mut map = Map::new(W, H);
        map.set_tile(5, 5, TileKind::Floor);
        map.rooms.push(crate::modules::map::Rect::new(1, 1, 3, 3));
        apply_fallback(&mut map);
        finalize(&mut map);

        let floor = map.tiles.iter().filter(|t| t.kind == TileKind::Floor).count();
        assert!((floor as f32) / ((W * H) as f32) >= 0.10, "폴백도 바닥 ≥ 10%");
        assert!(map.rooms.len() >= 2, "폴백도 방 2개 이상");
        for x in 0..W {
            assert_eq!(map.get_tile(x, 0), TileKind::Wall);
        }
    }

    #[test]
    fn 폴백은_기존_방목록을_비운다() {
        let mut map = Map::new(W, H);
        map.rooms.push(crate::modules::map::Rect::new(2, 2, 3, 3));
        map.rooms.push(crate::modules::map::Rect::new(8, 8, 3, 3));
        map.rooms.push(crate::modules::map::Rect::new(12, 12, 3, 3));
        apply_fallback(&mut map);
        assert!(map.rooms.is_empty(), "폴백은 기존 방을 비운다");
    }

    #[test]
    fn 극소맵은_폴백경로로_계약을_지킨다() {
        // iw<2 (블록 격자 부족) → generate 의 early 폴백 분기.
        let gen = WfcGenerator;
        let map = gen.generate(7, 30, 1); // iw=(7-2)/3=1
        let floor = map.tiles.iter().filter(|t| t.kind == TileKind::Floor).count();
        assert!(floor > 0, "극소맵 폴백도 바닥을 만든다");
        assert!(map.rooms.len() >= 2, "극소맵 폴백도 방 2개");
        for x in 0..7 {
            assert_eq!(map.get_tile(x, 0), TileKind::Wall);
            assert_eq!(map.get_tile(x, 29), TileKind::Wall);
        }
    }

    #[test]
    fn 세로로_좁은_극소맵도_폴백경로다() {
        // ih<2 단축평가의 다른 쪽 분기.
        let gen = WfcGenerator;
        let map = gen.generate(40, 7, 1); // ih=(7-2)/3=1
        let floor = map.tiles.iter().filter(|t| t.kind == TileKind::Floor).count();
        assert!(floor > 0);
        assert!(map.rooms.len() >= 2);
    }
}
