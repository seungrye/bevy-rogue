//! Generator 카탈로그 샘플 prebuild — 각 generator 의 결과를 N 개 시드로 생성해
//! 80×50 grid 의 JSON 파일들로 출력한다. Site (Next.js) 가 이를 정적 자산으로
//! 로드해 카탈로그 페이지에 미리보기를 표시한다.
//!
//! 사용:
//!   cargo run --release --bin sample_generators -- <output_dir>
//!
//! 기본 output: `generator-samples/` (CWD 기준). site 의 public 폴더에 복사 또는
//! 직접 그 경로 지정.
//!
//! 출력 파일 포맷 (`<output>/<name>.json`):
//! ```json
//! {
//!   "name": "forest",
//!   "width": 80,
//!   "height": 50,
//!   "samples": [
//!     { "seed": 42, "grid": ["#####...", "#...##..", ...] },
//!     ...
//!   ]
//! }
//! ```
//! 각 grid 행은 문자열 — site 가 monospace / SVG / Canvas 로 자유 렌더링.

use bevy_rogue::modules::map::{MapGeneratorRegistry, TileKind};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

/// 결정론적 시드 8 개 — 시각적 다양성 확인 + 십자가 같은 고정 패턴 회귀 감지.
const SAMPLE_SEEDS: &[u64] = &[42, 7, 1234, 9999, 55555, 31337, 808, 1729];

/// 카탈로그용 맵 크기. 게임 실제 크기(80×50) 와 동일하게 — 카탈로그가 실제 게임의
/// 모습을 그대로 보여준다.
const SAMPLE_WIDTH: usize = 80;
const SAMPLE_HEIGHT: usize = 50;

fn tile_char(k: TileKind) -> char {
    match k {
        TileKind::Wall => '#',
        TileKind::Floor => '.',
        TileKind::Water => '~',
        TileKind::Sand => 's',
        TileKind::DestructibleWall => 'd',
        TileKind::Rubble => 'r',
        TileKind::Counter => 'c',
    }
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let output_dir = args.get(1).cloned()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("generator-samples"));
    std::fs::create_dir_all(&output_dir)?;

    let registry = MapGeneratorRegistry::default_registry();
    let names = registry.names();
    println!("Generating samples for {} generators → {}", names.len(), output_dir.display());

    // 인덱스 파일 — site 가 처음에 어떤 generator 들이 있는지 알도록.
    let mut index_entries: Vec<serde_json::Value> = Vec::new();

    for name in &names {
        let mut samples: Vec<serde_json::Value> = Vec::with_capacity(SAMPLE_SEEDS.len());
        for &seed in SAMPLE_SEEDS {
            let map = registry.generate_with(name, SAMPLE_WIDTH, SAMPLE_HEIGHT, seed)
                .expect("default_registry 에 등록된 generator");
            let mut grid: Vec<String> = Vec::with_capacity(SAMPLE_HEIGHT);
            for y in 0..SAMPLE_HEIGHT {
                let mut row = String::with_capacity(SAMPLE_WIDTH);
                for x in 0..SAMPLE_WIDTH {
                    row.push(tile_char(map.get_tile(x, y)));
                }
                grid.push(row);
            }
            samples.push(serde_json::json!({
                "seed": seed,
                "grid": grid,
            }));
        }

        let out = serde_json::json!({
            "name": *name,
            "width": SAMPLE_WIDTH,
            "height": SAMPLE_HEIGHT,
            "samples": samples,
        });
        let path = output_dir.join(format!("{}.json", name));
        let mut f = File::create(&path)?;
        f.write_all(serde_json::to_string_pretty(&out)?.as_bytes())?;
        println!("  ✓ {}", path.display());

        index_entries.push(serde_json::json!({
            "name": *name,
            "file": format!("{}.json", name),
        }));
    }

    let index = serde_json::json!({
        "generators": index_entries,
        "width": SAMPLE_WIDTH,
        "height": SAMPLE_HEIGHT,
        "seeds": SAMPLE_SEEDS,
    });
    let index_path = output_dir.join("index.json");
    let mut f = File::create(&index_path)?;
    f.write_all(serde_json::to_string_pretty(&index)?.as_bytes())?;
    println!("  ✓ {}", index_path.display());

    println!("Done — {} generators × {} seeds.", names.len(), SAMPLE_SEEDS.len());
    Ok(())
}
