use bevy::prelude::*;

const OVERLAY_Z: i32 = 450;
const PANEL_WIDTH: f32 = 620.0;
const FONT_SIZE: f32 = 15.0;

#[derive(Resource, Default)]
/// 인게임 도움말 패널의 열림 상태를 저장한다.
///
/// 플레이어 이동 시스템이 이 값을 읽어 도움말이 열린 동안 턴 소비 입력을 차단한다.
pub struct HelpPanelOpen(pub bool);

#[derive(Component)]
struct HelpOverlay;

#[derive(Component)]
struct HelpText;

/// 키 바인딩 도움말 오버레이를 생성하고 입력으로 열고 닫는 UI 플러그인이다.
pub struct HelpPlugin;

impl Plugin for HelpPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HelpPanelOpen>()
            .add_systems(Startup, setup_help_overlay)
            .add_systems(Update, (toggle_help_overlay, update_help_overlay).chain());
    }
}

/// 시작 시 숨겨진 도움말 오버레이 UI를 생성한다.
///
/// 도움말 내용은 정적 조작 안내이므로 매번 다시 만들지 않고, visibility만 전환한다.
fn setup_help_overlay(mut commands: Commands, asset_server: Res<AssetServer>) {
    let font = asset_server.load("fonts/NanumSquareNeo-bRg.ttf");
    commands
        .spawn((
            NodeBundle {
                style: Style {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    position_type: PositionType::Absolute,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                background_color: Color::rgba(0.0, 0.0, 0.0, 0.68).into(),
                z_index: ZIndex::Global(OVERLAY_Z),
                visibility: Visibility::Hidden,
                ..default()
            },
            HelpOverlay,
        ))
        .with_children(|parent| {
            parent
                .spawn((NodeBundle {
                    style: Style {
                        width: Val::Px(PANEL_WIDTH),
                        padding: UiRect::all(Val::Px(20.0)),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                    background_color: Color::rgba(0.0, 0.05, 0.0, 0.96).into(),
                    ..default()
                },))
                .with_children(|panel| {
                    panel.spawn((
                        TextBundle::from_sections(help_sections(&font)),
                        HelpText,
                    ));
                });
        });
}

/// `H` 또는 `?` 입력으로 도움말을 열고 닫는다.
///
/// Bevy에서는 `?`가 별도 키가 아니라 `Shift + Slash` 조합으로 들어오기 때문에
/// 두 키 상태를 함께 확인한다. 열린 상태에서는 `Esc`로 닫을 수 있다.
fn toggle_help_overlay(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut open: ResMut<HelpPanelOpen>,
) {
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let question_mark = shift && keyboard.just_pressed(KeyCode::Slash);
    if keyboard.just_pressed(KeyCode::KeyH) || question_mark {
        open.0 = !open.0;
    } else if open.0 && keyboard.just_pressed(KeyCode::Escape) {
        open.0 = false;
    }
}

/// `HelpPanelOpen` 리소스 변경에 맞춰 도움말 오버레이 visibility를 갱신한다.
fn update_help_overlay(
    open: Res<HelpPanelOpen>,
    mut overlay_q: Query<&mut Visibility, With<HelpOverlay>>,
) {
    if !open.is_changed() {
        return;
    }
    let Ok(mut visibility) = overlay_q.get_single_mut() else { return; };
    *visibility = if open.0 { Visibility::Inherited } else { Visibility::Hidden };
}

/// 도움말 패널에 표시할 조작 안내 텍스트를 구성한다.
///
/// 실제 UI와 단위 테스트가 같은 데이터를 쓰도록 순수 함수로 유지한다.
fn help_sections(font: &Handle<Font>) -> Vec<TextSection> {
    let mut sections = Vec::new();
    push(&mut sections, "/ H E L P /\n", font, 24.0, Color::rgb(0.4, 1.0, 0.4));
    push(&mut sections, "? 또는 H: 도움말 열기/닫기    Esc: 닫기\n\n", font, FONT_SIZE, Color::rgb(0.75, 0.9, 0.75));

    push_group(&mut sections, font, "이동", &[
        ("← → ↑ ↓ / WASD", "한 칸 이동, 길게 누르면 연속 이동"),
        ("대각 입력", "대각선 이동"),
        ("Space", "제자리 대기"),
        ("마우스 왼쪽", "클릭 지점으로 자동 경로 이동"),
    ]);
    push_group(&mut sections, font, "전투와 탐험", &[
        ("몬스터 방향 이동", "근접 공격"),
        ("M", "전체화면 미니맵 토글"),
        ("Ctrl + +/-", "미니맵 줌 조절"),
    ]);
    push_group(&mut sections, font, "패널", &[
        ("E", "장비/인벤토리 패널"),
        ("Q", "퀘스트 패널"),
        ("상인에게 부딪힘", "상점 열기"),
        ("↑↓ / Tab / Enter", "목록 이동, 탭 전환, 확인"),
    ]);
    push_group(&mut sections, font, "기타", &[
        ("G", "아이템 글리프 스타일 전환"),
        ("F1", "맵 생성기 순환 및 재생성"),
        ("Game Over: R 또는 N", "세이브 삭제 후 새 게임"),
        ("Game Over: Esc", "게임 종료"),
    ]);
    sections
}

/// 도움말의 한 섹션 제목과 키/동작 목록을 텍스트 섹션으로 추가한다.
fn push_group(
    sections: &mut Vec<TextSection>,
    font: &Handle<Font>,
    title: &str,
    rows: &[(&str, &str)],
) {
    push(sections, &format!("\n[{}]\n", title), font, FONT_SIZE, Color::rgb(1.0, 0.9, 0.25));
    for (key, action) in rows {
        push(sections, &format!("  {:<18} {}\n", key, action), font, FONT_SIZE, Color::WHITE);
    }
}

/// 단일 도움말 텍스트 줄을 지정한 스타일로 추가한다.
fn push(
    sections: &mut Vec<TextSection>,
    value: &str,
    font: &Handle<Font>,
    size: f32,
    color: Color,
) {
    sections.push(TextSection::new(
        value.to_string(),
        TextStyle { font: font.clone(), font_size: size, color },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_text_lists_core_commands() {
        let text: String = help_sections(&Handle::default())
            .iter()
            .map(|s| s.value.as_str())
            .collect();
        assert!(text.contains("WASD"));
        assert!(text.contains("Space"));
        assert!(text.contains("미니맵"));
        assert!(text.contains("장비"));
        assert!(text.contains("Game Over"));
    }
}
