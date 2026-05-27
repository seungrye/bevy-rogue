//! bevy-rogue 라이브러리 진입점.
//!
//! - 네이티브: `src/main.rs` 가 `run_cli()` 를 호출.
//! - wasm32: `#[wasm_bindgen(start)]` 진입점 `start()` 가 `run_default()` 를 호출.
//!
//! 게임 본체 구성(플러그인 배선)은 `build_app()` 한 곳으로 모아 두 진입점이 공유한다.

pub mod modules;

use bevy::prelude::*;
use bevy_rapier2d::prelude::*;
use crate::modules::ui::DIALOG_PANEL_HEIGHT_PX;
use crate::modules::item::{GlyphStyle, ItemPlugin};

pub const HELP_TEXT: &str = "\
사용법: bevy-rogue [OPTIONS]

옵션:
  -a, --algorithm <name>    시작 시 사용할 맵 생성기를 지정합니다
  -g, --glyph-style <style> 아이템 글리프 스타일 (ascii|unicode|icon, 기본: ascii)
  -h, --help                이 도움말을 출력합니다

사용 가능한 생성기:
  bsp               던전 - 규칙적인 방 분할, 깔끔한 복도
  simple_rooms      던전 - 크기 다양한 방들이 랜덤 배치
  drunkard          동굴 - 취한 듯 굴곡진 유기적 통로
  cellular_automata 동굴 - 자연 침식된 느낌의 불규칙 동굴
  dla               동굴 - 중심에서 뻗어나가는 침식 구조
  bsp_indoor        실내 - BSP를 소규모에 적용한 건물 평면도
  prefab            실내 - 손제작 방 청사진 조합
  organic_village   마을 - 유기적 배치의 건물군
  grid_village      마을 - 격자 도로망 + 블록 건물
  forest            숲   - 나무 군집 사이 좁은 길
  perlin            숲   - 펄린 노이즈 기반 자연 지형

실행 중 F1 키로 생성기를 순환, G 키로 글리프 스타일을 순환할 수 있습니다.";

/// 인수가 주어지지 않았을 때 사용할 기본 맵 생성기.
pub const DEFAULT_ALGORITHM: &str = "organic_village";

pub enum ParseResult {
    Run { algorithm: Option<String>, glyph_style: GlyphStyle },
    Help,
    Error(String),
}

/// `--algorithm` 옵션 파싱 결과를 실제 시작 생성기 이름으로 변환한다.
///
/// 인수가 없으면 기본 생성기(`organic_village`)를 적용한다.
pub fn resolve_initial_algorithm(algorithm: Option<String>) -> Option<String> {
    Some(algorithm.unwrap_or_else(|| DEFAULT_ALGORITHM.to_string()))
}

pub fn parse_args(args: &[String]) -> ParseResult {
    let mut algorithm: Option<String> = None;
    let mut glyph_style = GlyphStyle::Ascii;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => return ParseResult::Help,
            "--algorithm" | "-a" => {
                i += 1;
                if i >= args.len() {
                    return ParseResult::Error("--algorithm 에 값이 필요합니다".to_string());
                }
                algorithm = Some(args[i].clone());
            }
            "--glyph-style" | "-g" => {
                i += 1;
                if i >= args.len() {
                    return ParseResult::Error("--glyph-style 에 값이 필요합니다".to_string());
                }
                match GlyphStyle::from_str(&args[i]) {
                    Some(s) => glyph_style = s,
                    None    => return ParseResult::Error(
                        format!("알 수 없는 글리프 스타일: {} (ascii|unicode|icon)", args[i])
                    ),
                }
            }
            other => return ParseResult::Error(format!("알 수 없는 인수: {}", other)),
        }
        i += 1;
    }
    ParseResult::Run { algorithm, glyph_style }
}

/// 네이티브 CLI 진입점: argv 파싱 후 `run()` 호출.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_cli() {
    let raw_args: Vec<String> = std::env::args().collect();
    let (initial_algorithm, initial_glyph_style) = match parse_args(&raw_args) {
        ParseResult::Help => {
            println!("{}", HELP_TEXT);
            return;
        }
        ParseResult::Error(msg) => {
            eprintln!("오류: {}\n\n{}", msg, HELP_TEXT);
            std::process::exit(1);
        }
        ParseResult::Run { algorithm, glyph_style } => (
            resolve_initial_algorithm(algorithm),
            glyph_style,
        ),
    };
    run(initial_algorithm, initial_glyph_style);
}

/// 인자 지정 진입점(테스트/wasm 공통).
pub fn run(initial_algorithm: Option<String>, initial_glyph_style: GlyphStyle) {
    let tile_size = modules::map::TILE_SIZE;

    // 윈도우 설정은 native/wasm 분기 (캔버스 셀렉터 등 wasm 전용 옵션).
    #[cfg(not(target_arch = "wasm32"))]
    let primary_window = Window {
        title: "Bevy Rogue Map".into(),
        resolution: (
            40_f32 * tile_size,
            25_f32 * tile_size + DIALOG_PANEL_HEIGHT_PX,
        ).into(),
        ..default()
    };
    #[cfg(target_arch = "wasm32")]
    let primary_window = Window {
        title: "Bevy Rogue Map".into(),
        canvas: Some("#bevy-canvas".to_string()),
        prevent_default_event_handling: true,
        resolution: (
            40_f32 * tile_size,
            25_f32 * tile_size + DIALOG_PANEL_HEIGHT_PX,
        ).into(),
        ..default()
    };

    App::new()
        .add_systems(Startup, modules::core::systems::spawn_2d_camera)
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(primary_window),
            ..default()
        }))
        .add_plugins(RapierPhysicsPlugin::<NoUserData>::pixels_per_meter(100.0))
        .add_plugins(modules::map::MapPlugin { initial_algorithm })
        .add_plugins(modules::player::PlayerPlugin)
        .add_plugins(modules::lighting::LightingPlugin)
        .add_plugins(modules::combat::CombatPlugin)
        .add_plugins(modules::monster::MonsterPlugin)
        .add_plugins(modules::combat_feedback::CombatFeedbackPlugin)
        .add_plugins(modules::elemental::ElementalPlugin)
        .add_plugins(modules::projectile::ProjectilePlugin)
        .add_plugins(modules::ranged::RangedPlugin)
        .add_plugins(modules::skill::SkillPlugin)
        .add_plugins(modules::trap::TrapPlugin)
        .add_plugins(modules::trap::PlayerTrapPlugin)
        .add_plugins(ItemPlugin { initial_glyph_style })
        .add_plugins(modules::ui::GameUiPlugin)
        .add_plugins(modules::villager::VillagerPlugin)
        .add_plugins(modules::zone::ZonePlugin)
        .add_plugins(modules::quest::QuestPlugin)
        .add_plugins(modules::save::SavePlugin)
        .run();
}

// ── wasm32 진입점 ────────────────────────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// 브라우저에서 wasm 모듈이 init 되면 호출되는 진입점.
/// PoC 라 CLI/쿼리 파싱 없이 기본 알고리즘으로 시작한다(stage 2).
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    run(Some(DEFAULT_ALGORITHM.to_string()), GlyphStyle::Ascii);
}

// ── 테스트 ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        std::iter::once("bevy-rogue")
            .chain(v.iter().copied())
            .map(String::from)
            .collect()
    }

    #[test]
    fn 인수가_없으면_알고리즘은_미지정이고_글리프는_기본Ascii로_실행된다() {
        let result = parse_args(&args(&[]));
        assert!(matches!(result, ParseResult::Run { algorithm: None, glyph_style: GlyphStyle::Ascii }));
    }

    #[test]
    fn 알고리즘_긴플래그는_지정한_생성기이름으로_파싱된다() {
        let result = parse_args(&args(&["--algorithm", "bsp"]));
        assert!(matches!(result, ParseResult::Run { algorithm: Some(ref s), .. } if s == "bsp"));
    }

    #[test]
    fn 알고리즘_짧은플래그는_지정한_생성기이름으로_파싱된다() {
        let result = parse_args(&args(&["-a", "perlin"]));
        assert!(matches!(result, ParseResult::Run { algorithm: Some(ref s), .. } if s == "perlin"));
    }

    #[test]
    fn 글리프_긴플래그_icon은_GameIcon으로_파싱된다() {
        let result = parse_args(&args(&["--glyph-style", "icon"]));
        assert!(matches!(result, ParseResult::Run { glyph_style: GlyphStyle::GameIcon, .. }));
    }

    #[test]
    fn 글리프_짧은플래그_unicode는_Unicode로_파싱된다() {
        let result = parse_args(&args(&["-g", "unicode"]));
        assert!(matches!(result, ParseResult::Run { glyph_style: GlyphStyle::Unicode, .. }));
    }

    #[test]
    fn 알수없는_글리프스타일은_에러를_반환한다() {
        let result = parse_args(&args(&["--glyph-style", "nope"]));
        assert!(matches!(result, ParseResult::Error(_)));
    }

    #[test]
    fn 글리프스타일에_값이_없으면_에러를_반환한다() {
        let result = parse_args(&args(&["--glyph-style"]));
        assert!(matches!(result, ParseResult::Error(_)));
    }

    #[test]
    fn 도움말_긴플래그는_Help를_반환한다() {
        let result = parse_args(&args(&["--help"]));
        assert!(matches!(result, ParseResult::Help));
    }

    #[test]
    fn 도움말_짧은플래그는_Help를_반환한다() {
        let result = parse_args(&args(&["-h"]));
        assert!(matches!(result, ParseResult::Help));
    }

    #[test]
    fn 알고리즘에_값이_없으면_에러를_반환한다() {
        let result = parse_args(&args(&["--algorithm"]));
        assert!(matches!(result, ParseResult::Error(_)));
    }

    #[test]
    fn 알수없는_인수는_에러를_반환한다() {
        let result = parse_args(&args(&["--unknown"]));
        assert!(matches!(result, ParseResult::Error(_)));
    }

    // ── 결정 로직(순수 함수) : main() 의 파싱→설정 변환 분기 커버 ──────────────

    #[test]
    fn 알고리즘이_미지정이면_기본생성기_organic_village로_결정된다() {
        assert_eq!(resolve_initial_algorithm(None), Some(DEFAULT_ALGORITHM.to_string()));
        assert_eq!(resolve_initial_algorithm(None), Some("organic_village".to_string()));
    }

    #[test]
    fn 알고리즘이_지정되면_그_생성기이름이_그대로_결정된다() {
        assert_eq!(
            resolve_initial_algorithm(Some("bsp".to_string())),
            Some("bsp".to_string())
        );
    }

    /// 테스트 편의: Run 분기의 (initial_algorithm, glyph_style) 결정 결과만 뽑아낸다.
    /// Run 이 아니면 None — main() 의 Run 변환 분기와 동일한 결정 로직을 사용한다.
    fn run_config(args: &[String]) -> Option<(Option<String>, GlyphStyle)> {
        if let ParseResult::Run { algorithm, glyph_style } = parse_args(args) {
            Some((resolve_initial_algorithm(algorithm), glyph_style))
        } else {
            None
        }
    }

    #[test]
    fn 파싱부터_설정변환까지_Run분기는_기본알고리즘과_글리프를_적용한다() {
        // main() 의 Run 분기와 동일한 결정 흐름(파싱 → 변환)을 단위로 검증한다.
        assert_eq!(
            run_config(&args(&["-g", "icon"])),
            Some((Some("organic_village".to_string()), GlyphStyle::GameIcon)),
        );
    }

    #[test]
    fn Help입력은_Run설정으로_변환되지_않는다() {
        // run_config 의 else 분기(비-Run)도 커버한다.
        assert_eq!(run_config(&args(&["--help"])), None);
    }

    #[test]
    fn 도움말_텍스트는_사용법과_모든_생성기_목록을_담는다() {
        // Help 분기에서 출력되는 HELP_TEXT 의 핵심 항목들이 존재하는지 확인.
        assert!(HELP_TEXT.contains("사용법: bevy-rogue"));
        assert!(HELP_TEXT.contains("--algorithm"));
        assert!(HELP_TEXT.contains("--glyph-style"));
        assert!(HELP_TEXT.contains("--help"));
        for gen in [
            "bsp", "simple_rooms", "drunkard", "cellular_automata", "dla",
            "bsp_indoor", "prefab", "organic_village", "grid_village", "forest", "perlin",
        ] {
            assert!(HELP_TEXT.contains(gen), "도움말에 생성기 {gen} 가 없다");
        }
    }
}
