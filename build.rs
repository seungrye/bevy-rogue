//! 빌드 스크립트 — wasm 분기가 임베드해야 할 RON 에셋을 자동 enumerate 한다.
//!
//! 이전에는 `src/modules/quest/mod.rs` 등에서 `include_str!` 을 손으로 늘어놓아
//! 새 .ron 추가 시 wasm 분기에 누락되는 위험이 있었다. 이제 빌드 시점에 디렉터리를
//! 스캔해 `$OUT_DIR/embedded_assets.rs` 를 생성하고, 모듈은 그 슬라이스를
//! `include!` 한다. 새 .ron 을 디렉터리에 추가하기만 하면 wasm 도 자동으로 잡힌다.
//!
//! `cargo::rerun-if-changed=` 로 해당 디렉터리 변경 시 build.rs 가 재실행되도록
//! 캐시 무효화 신호를 명시한다.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    // assets 루트 경로 (CARGO_MANIFEST_DIR 기준).
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR not set");
    let assets_root = PathBuf::from(&manifest_dir).join("assets");

    // 재빌드 트리거 — 디렉터리에 .ron 이 추가/삭제되면 다시 실행.
    println!("cargo::rerun-if-changed=build.rs");
    for sub in &["quests", "items", "monsters", "villagers"] {
        let dir = assets_root.join(sub);
        println!("cargo::rerun-if-changed={}", dir.display());
    }

    // 그룹별 슬라이스 생성.
    let mut out = String::new();
    out.push_str("// 자동 생성 파일 — build.rs 가 빌드 시점에 디렉터리 스캔으로 생성한다.\n");
    out.push_str("// 직접 수정 금지.\n\n");

    write_dir_slice(&mut out, &assets_root, "quests", "EMBEDDED_QUESTS", "ron");
    write_named_files(&mut out, &assets_root, "EMBEDDED_ITEMS", &[
        ("items", "quest_items.ron"),
        ("items", "weapons.ron"),
        ("items", "armors.ron"),
        ("items", "consumables.ron"),
        ("items", "start_loadout.ron"),
    ]);
    write_named_files(&mut out, &assets_root, "EMBEDDED_MONSTERS", &[
        ("monsters", "monsters.ron"),
    ]);
    write_named_files(&mut out, &assets_root, "EMBEDDED_VILLAGERS", &[
        ("villagers", "villagers.ron"),
    ]);

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let out_path = Path::new(&out_dir).join("embedded_assets.rs");
    fs::write(&out_path, out).expect("embedded_assets.rs 쓰기 실패");
}

/// `assets/<sub>/*.<ext>` 를 스캔해 `(파일명, 내용)` 슬라이스를 생성한다.
fn write_dir_slice(out: &mut String, assets_root: &Path, sub: &str, const_name: &str, ext: &str) {
    let dir = assets_root.join(sub);
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    if let Ok(rd) = fs::read_dir(&dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some(ext) {
                let name = path.file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string());
                if let Some(name) = name {
                    entries.push((name, path));
                }
            }
        }
    }
    // 안정적인 정렬(파일명) — 결정적 빌드 산출물.
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    out.push_str(&format!(
        "/// {} 디렉터리의 모든 .{} 파일을 빌드 시점에 임베드.\n",
        sub, ext
    ));
    out.push_str(&format!(
        "pub static {}: &[(&str, &str)] = &[\n", const_name));
    for (name, path) in &entries {
        out.push_str(&format!(
            "    (\"{}\", include_str!(r\"{}\")),\n",
            name,
            path.display()
        ));
    }
    out.push_str("];\n\n");
}

/// 명시된 (sub, filename) 목록을 임베드한다. 파일 이름이 key 다.
fn write_named_files(out: &mut String, assets_root: &Path, const_name: &str, files: &[(&str, &str)]) {
    out.push_str(&format!(
        "pub static {}: &[(&str, &str)] = &[\n", const_name));
    for (sub, name) in files {
        let path = assets_root.join(sub).join(name);
        out.push_str(&format!(
            "    (\"{}\", include_str!(r\"{}\")),\n",
            name,
            path.display()
        ));
    }
    out.push_str("];\n\n");
}
