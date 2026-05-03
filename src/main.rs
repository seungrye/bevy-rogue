use bevy::prelude::*;
use crate::modules::ui::DIALOG_PANEL_HEIGHT_PX;
mod modules;

const HELP_TEXT: &str = "\
사용법: bevy-rogue [OPTIONS]

옵션:
  -a, --algorithm <name>  시작 시 사용할 맵 생성기를 지정합니다
  -h, --help              이 도움말을 출력합니다

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

실행 중 Tab 키로 생성기를 순환할 수 있습니다.";

enum ParseResult {
    Run(Option<String>),
    Help,
    Error(String),
}

fn parse_args(args: &[String]) -> ParseResult {
    let mut algorithm: Option<String> = None;
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
            other => return ParseResult::Error(format!("알 수 없는 인수: {}", other)),
        }
        i += 1;
    }
    ParseResult::Run(algorithm)
}

fn main() {
    let raw_args: Vec<String> = std::env::args().collect();
    let initial_algorithm = match parse_args(&raw_args) {
        ParseResult::Help => {
            println!("{}", HELP_TEXT);
            return;
        }
        ParseResult::Error(msg) => {
            eprintln!("오류: {}\n\n{}", msg, HELP_TEXT);
            std::process::exit(1);
        }
        ParseResult::Run(alg) => Some(alg.unwrap_or_else(|| "grid_village".to_string())),
    };

    let tile_size = modules::map::TILE_SIZE;

    App::new()
        .add_systems(Startup, modules::core::systems::spawn_2d_camera)
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Bevy Rogue Map".into(),
                resolution: (
                    40_f32 * tile_size,
                    25_f32 * tile_size + DIALOG_PANEL_HEIGHT_PX,
                ).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(modules::map::MapPlugin { initial_algorithm })
        .add_plugins(modules::player::PlayerPlugin)
        .add_plugins(modules::monster::MonsterPlugin)
        .add_plugins(modules::combat_feedback::CombatFeedbackPlugin)
        .add_plugins(modules::trigger::TriggerPlugin)
        .add_plugins(modules::ui::GameUiPlugin)
        .add_plugins(modules::villager::VillagerPlugin)
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        std::iter::once("bevy-rogue")
            .chain(v.iter().copied())
            .map(String::from)
            .collect()
    }

    #[test]
    fn no_args_runs_with_none() {
        let result = parse_args(&args(&[]));
        assert!(matches!(result, ParseResult::Run(None)));
    }

    #[test]
    fn algorithm_long_flag_parsed() {
        let result = parse_args(&args(&["--algorithm", "bsp"]));
        assert!(matches!(result, ParseResult::Run(Some(ref s)) if s == "bsp"));
    }

    #[test]
    fn algorithm_short_flag_parsed() {
        let result = parse_args(&args(&["-a", "perlin"]));
        assert!(matches!(result, ParseResult::Run(Some(ref s)) if s == "perlin"));
    }

    #[test]
    fn help_long_flag_returns_help() {
        let result = parse_args(&args(&["--help"]));
        assert!(matches!(result, ParseResult::Help));
    }

    #[test]
    fn help_short_flag_returns_help() {
        let result = parse_args(&args(&["-h"]));
        assert!(matches!(result, ParseResult::Help));
    }

    #[test]
    fn algorithm_missing_value_returns_error() {
        let result = parse_args(&args(&["--algorithm"]));
        assert!(matches!(result, ParseResult::Error(_)));
    }

    #[test]
    fn unknown_flag_returns_error() {
        let result = parse_args(&args(&["--unknown"]));
        assert!(matches!(result, ParseResult::Error(_)));
    }
}
