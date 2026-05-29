use rand::prelude::*;
use noise::{NoiseFn, Perlin};
use crate::modules::map::{Map, TileKind};
use super::super::MapGenerator;
use super::{ensure_connectivity, add_rooms_from_floor};

/// 자연스러운 숲 — Perlin fBm 으로 밀도 변화를 만든 뒤 임계값으로 나무/빈터를
/// 가른다. 결정론적인 십자 통로 + 중앙 빈터 알고리즘(이전 버전)이 모든 시드에서
/// 동일한 십자가 모양을 만들던 문제를 해결.
///
/// 디자인:
/// - **fBm Perlin** (4 옥타브 합성): 큰 밀림/빈터 흐름 + 작은 디테일.
/// - **임계값 분기**: `< THRESHOLD` 는 floor(공터), 그 외는 wall(나무).
/// - **RNG 빈터 1~3 개**: 위치·반경 랜덤. 원형 carve.
/// - **0~1 개의 구불구불 경로**: 50% 확률로 두 랜덤 점 사이를 약하게 잇는다 — 항상
///   잇진 않아 시드에 따라 막힌 숲 / 트인 숲 / 길 있는 숲의 분포가 달라짐.
/// - **ensure_connectivity**: 마지막에 분리된 floor 영역을 연결.
pub struct ForestGenerator;

/// fBm 결과 임계값 — `< 임계값` 인 셀이 floor 가 된다. 작을수록 나무 우세.
/// Perlin 출력은 약 [-1, 1] 분포라 0.2 면 약 60% 가 floor — 트인 숲.
/// 사용자 디자인 튠 포인트.
const FOREST_FLOOR_THRESHOLD: f64 = 0.2;

impl MapGenerator for ForestGenerator {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = StdRng::seed_from_u64(seed);
        map.seed = seed;

        // fBm 4 옥타브 — 한 Perlin 을 4 frequency 에서 샘플링해 합성.
        // 큰 패턴(저주파)과 작은 디테일(고주파)이 자연 균형을 이룬다.
        let perlin = Perlin::new(rng.gen::<u32>());
        let base_freq = 0.06;
        let lacunarity = 2.0;
        let persistence = 0.5;

        for y in 1..height - 1 {
            for x in 1..width - 1 {
                let v = fbm_2d(&perlin, x as f64, y as f64, 4, base_freq, lacunarity, persistence);
                if v < FOREST_FLOOR_THRESHOLD {
                    map.set_tile(x, y, TileKind::Floor);
                }
                // else: 기본 Wall(나무) 유지.
            }
        }

        // 빈터 1~3 개 — 랜덤 위치·반경. 원형으로 carve 해 직각을 피한다.
        let clearing_count = rng.gen_range(1..=3);
        for _ in 0..clearing_count {
            let cx = rng.gen_range(6..width - 6);
            let cy = rng.gen_range(6..height - 6);
            let radius = rng.gen_range(2..=4);
            carve_circular_clearing(&mut map, cx, cy, radius);
        }

        // 50% 확률로 구불구불 경로 한 줄기 — '숲길' 분위기. 항상 안 그려서
        // 시드에 따라 어떤 맵은 길이 없는 깊은 숲, 어떤 맵은 트인 길.
        if rng.gen_bool(0.5) {
            let fx = rng.gen_range(2..width - 2);
            let fy = rng.gen_range(2..height - 2);
            let tx = rng.gen_range(2..width - 2);
            let ty = rng.gen_range(2..height - 2);
            carve_winding_path(&mut map, fx, fy, tx, ty, &mut rng);
        }

        ensure_connectivity(&mut map);
        add_rooms_from_floor(&mut map);
        map
    }
    fn name(&self) -> &str { "forest" }
}

/// **fractional Brownian motion** — Perlin 의 여러 옥타브를 가중 합성.
/// 옥타브가 늘수록 주파수가 `lacunarity` 배로 커지고 진폭이 `persistence` 배로 작아진다.
/// 결과는 약 [-1, 1] 정규화.
fn fbm_2d(
    perlin: &Perlin,
    x: f64,
    y: f64,
    octaves: u32,
    base_freq: f64,
    lacunarity: f64,
    persistence: f64,
) -> f64 {
    let mut total = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = base_freq;
    let mut max_amp = 0.0;
    for _ in 0..octaves {
        total += perlin.get([x * frequency, y * frequency]) * amplitude;
        max_amp += amplitude;
        amplitude *= persistence;
        frequency *= lacunarity;
    }
    total / max_amp
}

/// 원형 빈터 carve — `dx² + dy² <= radius²` 인 셀을 Floor 로. 직각 사각 빈터보다
/// 자연스러운 형태.
fn carve_circular_clearing(map: &mut Map, cx: usize, cy: usize, radius: i32) {
    let r2 = radius * radius;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy > r2 { continue; }
            let nx = (cx as i32 + dx).clamp(1, map.width as i32 - 2) as usize;
            let ny = (cy as i32 + dy).clamp(1, map.height as i32 - 2) as usize;
            map.set_tile(nx, ny, TileKind::Floor);
        }
    }
}

/// 두 점 사이 구불구불한 1~2 칸 폭 경로. 40% 확률로 랜덤 walk(구불), 그 외엔
/// goal 방향. 추가로 매 step 40% 확률로 인접 한 칸을 더 carve 해 자연 폭 변화.
/// 이전 십자 carve(3×3 직선) 대비 훨씬 자연스러움.
fn carve_winding_path(map: &mut Map, x1: usize, y1: usize, x2: usize, y2: usize, rng: &mut impl Rng) {
    let mut x = x1 as i32;
    let mut y = y1 as i32;
    let tx = x2 as i32;
    let ty = y2 as i32;
    // 무한 루프 방어 — 최대 step 은 맵 둘레 정도.
    let max_steps = (map.width + map.height) * 2;
    for _ in 0..max_steps {
        if (x - tx).abs() + (y - ty).abs() <= 1 { break; }
        let nx = x.clamp(1, map.width as i32 - 2) as usize;
        let ny = y.clamp(1, map.height as i32 - 2) as usize;
        map.set_tile(nx, ny, TileKind::Floor);
        // 자연 폭 변화 — 가끔 인접 한 칸 carve.
        if rng.gen_bool(0.4) {
            let dx = rng.gen_range(-1..=1);
            let dy = rng.gen_range(-1..=1);
            let nx2 = (x + dx).clamp(1, map.width as i32 - 2) as usize;
            let ny2 = (y + dy).clamp(1, map.height as i32 - 2) as usize;
            map.set_tile(nx2, ny2, TileKind::Floor);
        }
        // 40% 랜덤 walk, 60% goal 방향.
        if rng.gen_bool(0.4) {
            match rng.gen_range(0..4) {
                0 => x = (x - 1).max(1),
                1 => x = (x + 1).min(map.width as i32 - 2),
                2 => y = (y - 1).max(1),
                _ => y = (y + 1).min(map.height as i32 - 2),
            }
        } else if (x - tx).abs() >= (y - ty).abs() {
            x += (tx - x).signum();
        } else {
            y += (ty - y).signum();
        }
    }
}
