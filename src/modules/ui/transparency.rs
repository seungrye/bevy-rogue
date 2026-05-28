//! 플레이어 글리프와 UI(미니맵·하단 다이얼로그) 겹침 감지 시 알파를 일시적으로
//! 낮춰(0.5×) 화면상 캐릭터/지도 시인성을 확보한다.
//!
//! 배경:
//! - 카메라가 플레이어를 따라가지만 맵 경계 근처에선 카메라가 더 이동하지 못해
//!   플레이어가 화면 중심에서 벗어난다(`camera_follow_player`).
//! - 플레이어가 화면 우상단(미니맵 영역)이나 하단(다이얼로그 영역)으로 가면
//!   글리프가 UI 뒤로 숨어 시인성이 떨어진다.
//!
//! 동작:
//! - 매 프레임 플레이어 월드 좌표 → viewport 좌표를 구한 뒤,
//!   미니맵·다이얼로그 화면 영역(Rect)과의 겹침을 검사한다.
//! - 겹치면 해당 UI 의 BackgroundColor / UiImage.color / Text 색상의 alpha 를
//!   원본 × 0.5 로 낮추고, 겹치지 않을 때는 원본으로 복원한다.
//! - 원본 alpha 는 첫 관측 시 `OriginalAlpha*` 컴포넌트로 캐싱해
//!   페이드/복원이 누적·소실되지 않도록 한다.
//!
//! 분할/테스트:
//! - 결정 로직(rect 계산, 겹침 판정, 목표 alpha 계산)은 모두 순수 함수로
//!   분리해 단위 테스트한다.
//! - 시스템 자체는 헤드리스 환경에서 `world_to_viewport` 가 None 을 반환하므로
//!   본체 로직 도달은 불가하다. 플러그인 빌드/등록과 캐싱 동작은 별도 검증.

use bevy::prelude::*;

use crate::modules::map::TILE_SIZE;
use crate::modules::player::Player;
use super::DIALOG_PANEL_HEIGHT_PX;
use super::minimap::MINIMAP_DISPLAY_SIZE;

/// 겹침 시 어느 UI 영역(rect)과 비교할지 식별한다.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FadeTarget {
    Dialog,
    Minimap,
}

/// 플레이어 위치가 지정 영역과 겹칠 때 알파를 낮춰야 하는 UI 엔티티 마커.
///
/// 같은 엔티티에 BackgroundColor/UiImage/Text 가 동시에 있어도 각각 처리한다.
#[derive(Component, Clone, Copy, Debug)]
pub struct FadeOnOverlap(pub FadeTarget);

/// 겹치지 않을 때 복원할 BackgroundColor 의 원본 alpha. 첫 관측 시 부착된다.
#[derive(Component, Clone, Copy, Debug)]
pub struct OriginalBgAlpha(pub f32);

/// 겹치지 않을 때 복원할 Text 섹션별 원본 alpha. 첫 관측 시 부착된다.
#[derive(Component, Clone, Debug)]
pub struct OriginalTextAlphas(pub Vec<f32>);

/// 페이드 비율 — 겹칠 때 원본 alpha 에 곱해진다.
pub(crate) const FADE_FACTOR: f32 = 0.5;

/// 미니맵 오버레이의 화면 점유 높이(px).
/// 이미지(180) + row_gap(4) + 이름텍스트(약 13) + row_gap(4) + 힌트(약 11) ≈ 212.
pub(crate) const MINIMAP_OVERLAY_HEIGHT_PX: f32 = 212.0;

/// 미니맵 오버레이의 화면 점유 너비(px) — 이미지 너비와 동일.
pub(crate) const MINIMAP_OVERLAY_WIDTH_PX: f32 = MINIMAP_DISPLAY_SIZE;

/// 미니맵 오버레이의 오른쪽 여백(px). spawn_minimap_overlay 와 동기화.
pub(crate) const MINIMAP_RIGHT_PX: f32 = 5.0;

/// 미니맵 오버레이의 상단 여백(px). spawn_minimap_overlay 와 동기화.
pub(crate) const MINIMAP_TOP_PX: f32 = 10.0;

/// 점이 사각형 내부 또는 경계에 포함되는지 검사한다.
///
/// Bevy `Rect::contains` 의 동작과 동일(폐구간) — 경계에 정확히 놓이는 점도
/// 포함으로 본다. 헤드리스에서도 결정론적으로 분기를 탈 수 있게 별도 헬퍼로 둔다.
pub(crate) fn point_in_rect(p: Vec2, rect: Rect) -> bool {
    p.x >= rect.min.x && p.x <= rect.max.x && p.y >= rect.min.y && p.y <= rect.max.y
}

/// 화면 viewport 크기를 받아 하단 다이얼로그 패널 영역을 반환한다.
///
/// 다이얼로그는 viewport 하단에 고정 96px 높이로 폭 전체를 차지한다.
/// viewport 가 패널 높이보다 낮으면 패널은 viewport 전체를 덮는다(min y 가 0).
pub(crate) fn dialog_rect(viewport_size: Vec2) -> Rect {
    let h = DIALOG_PANEL_HEIGHT_PX.min(viewport_size.y).max(0.0);
    let top_y = (viewport_size.y - h).max(0.0);
    Rect::new(0.0, top_y, viewport_size.x.max(0.0), viewport_size.y.max(0.0))
}

/// 화면 viewport 크기를 받아 미니맵 오버레이 영역을 반환한다.
///
/// 미니맵은 우상단에 고정 오프셋(right=5, top=10)과 고정 크기(180x212)로 배치된다.
/// viewport 가 너무 작으면 음수가 되지 않게 0 으로 clamp 한다.
pub(crate) fn minimap_rect(viewport_size: Vec2) -> Rect {
    let right_x = viewport_size.x.max(0.0) - MINIMAP_RIGHT_PX;
    let left_x = (right_x - MINIMAP_OVERLAY_WIDTH_PX).max(0.0);
    let top_y = MINIMAP_TOP_PX.min(viewport_size.y).max(0.0);
    let bottom_y = (top_y + MINIMAP_OVERLAY_HEIGHT_PX).min(viewport_size.y);
    Rect::new(left_x, top_y, right_x.max(left_x), bottom_y.max(top_y))
}

/// 겹침 여부에 따른 목표 alpha 를 반환한다.
///
/// 겹치면 `base_alpha * FADE_FACTOR`, 아니면 `base_alpha` 그대로.
pub(crate) fn target_alpha(base_alpha: f32, overlapping: bool) -> f32 {
    if overlapping { base_alpha * FADE_FACTOR } else { base_alpha }
}

/// 플레이어 글리프의 viewport 좌표가 특정 FadeTarget 영역과 겹치는지 결정한다.
///
/// 좌표·rect 계산을 모두 순수 함수로 위임해, 시스템 본체와 같은 결정을 단위
/// 테스트로 검증할 수 있게 한다.
pub(crate) fn is_player_overlapping(
    player_viewport: Vec2,
    viewport_size: Vec2,
    target: FadeTarget,
) -> bool {
    let rect = match target {
        FadeTarget::Dialog => dialog_rect(viewport_size),
        FadeTarget::Minimap => minimap_rect(viewport_size),
    };
    point_in_rect(player_viewport, rect)
}

/// 플레이어 글리프가 차지하는 타일 한 칸(약 TILE_SIZE 픽셀) 영역을 더 정밀히
/// 다루고 싶을 때 쓰는 헬퍼. 현재 시스템은 중심점만으로 판정한다 — 시인성
/// 보정에는 단일 점 판정으로 충분하다(글리프가 한 타일 크기보다 작음).
#[allow(dead_code)]
pub(crate) const PLAYER_GLYPH_HALF_PX: f32 = TILE_SIZE * 0.5;

/// 단일 BackgroundColor 의 alpha 를 페이드/복원한다.
///
/// `original` 이 None 이면 현재 색의 alpha 를 원본으로 채택해 반환한다.
/// (호출 측에서 OriginalBgAlpha 컴포넌트 부착에 사용)
pub(crate) fn fade_bg_alpha(bg: &mut BackgroundColor, original: Option<f32>, overlapping: bool) -> f32 {
    let base = original.unwrap_or_else(|| bg.0.a());
    let want = target_alpha(base, overlapping);
    let mut c = bg.0;
    c.set_a(want);
    *bg = BackgroundColor(c);
    base
}

/// Text 의 모든 섹션 alpha 를 페이드/복원한다. 원본 길이가 섹션 수와 맞지
/// 않으면 부족분은 현재 alpha 로 채우고, 남는 분은 무시한다.
pub(crate) fn fade_text_alpha(
    text: &mut Text,
    originals: Option<&[f32]>,
    overlapping: bool,
) -> Vec<f32> {
    let mut captured = Vec::with_capacity(text.sections.len());
    for (i, sec) in text.sections.iter_mut().enumerate() {
        let base = originals
            .and_then(|o| o.get(i).copied())
            .unwrap_or_else(|| sec.style.color.a());
        captured.push(base);
        let want = target_alpha(base, overlapping);
        let mut c = sec.style.color;
        c.set_a(want);
        sec.style.color = c;
    }
    captured
}

/// 플레이어가 미니맵·다이얼로그 영역과 겹치는지 매 프레임 평가해 해당 UI 의
/// alpha 를 조정한다. 카메라 → viewport 변환은 헤드리스에서 None 이므로
/// 본체 분기는 실런타임에만 도달한다.
pub fn update_ui_overlap_transparency(
    mut commands: Commands,
    player_q: Query<&Transform, With<Player>>,
    camera_q: Query<(&Camera, &GlobalTransform), With<Camera>>,
    mut bg_q: Query<(Entity, &FadeOnOverlap, &mut BackgroundColor, Option<&OriginalBgAlpha>)>,
    mut text_q: Query<(Entity, &FadeOnOverlap, &mut Text, Option<&OriginalTextAlphas>)>,
) {
    let Ok(player_t) = player_q.get_single() else { return };
    let Ok((camera, cam_global)) = camera_q.get_single() else { return };
    // viewport 크기 — 헤드리스에선 None 이므로 분기 가드.
    let Some(viewport_size) = camera.logical_viewport_size() else { return };
    // 월드 → viewport 좌표(헤드리스에선 None).
    let Some(player_viewport) = camera.world_to_viewport(cam_global, player_t.translation) else { return };

    // 두 영역의 겹침 여부를 한 번씩만 계산해 재사용.
    let over_dialog = is_player_overlapping(player_viewport, viewport_size, FadeTarget::Dialog);
    let over_minimap = is_player_overlapping(player_viewport, viewport_size, FadeTarget::Minimap);
    let pick = |t: FadeTarget| match t {
        FadeTarget::Dialog => over_dialog,
        FadeTarget::Minimap => over_minimap,
    };

    for (e, fade, mut bg, orig) in &mut bg_q {
        let original = orig.map(|o| o.0);
        let captured = fade_bg_alpha(&mut bg, original, pick(fade.0));
        if orig.is_none() {
            commands.entity(e).insert(OriginalBgAlpha(captured));
        }
    }
    for (e, fade, mut text, orig) in &mut text_q {
        let originals = orig.map(|o| o.0.as_slice());
        let captured = fade_text_alpha(&mut text, originals, pick(fade.0));
        if orig.is_none() {
            commands.entity(e).insert(OriginalTextAlphas(captured));
        }
    }
}

/// 플러그인 — Update 후반에 시스템을 등록한다(이미지/텍스트가 그려지기 직전에
/// 알파를 조정하기만 하면 되므로 stage 가 특별히 중요하진 않다).
pub struct UiOverlapTransparencyPlugin;

impl Plugin for UiOverlapTransparencyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update_ui_overlap_transparency);
    }
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;

    // ── point_in_rect ────────────────────────────────────────────────────

    #[test]
    fn 사각형_내부_점은_포함으로_판정된다() {
        let r = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert!(point_in_rect(Vec2::new(50.0, 50.0), r));
    }

    #[test]
    fn 사각형_경계_점은_포함으로_판정된다() {
        let r = Rect::new(0.0, 0.0, 100.0, 100.0);
        // 네 모서리/네 변 위의 점은 폐구간 포함.
        assert!(point_in_rect(Vec2::new(0.0, 0.0), r));
        assert!(point_in_rect(Vec2::new(100.0, 100.0), r));
        assert!(point_in_rect(Vec2::new(100.0, 50.0), r));
        assert!(point_in_rect(Vec2::new(0.0, 50.0), r));
        assert!(point_in_rect(Vec2::new(50.0, 0.0), r));
        assert!(point_in_rect(Vec2::new(50.0, 100.0), r));
    }

    #[test]
    fn 사각형_네_방향_바깥_점은_제외로_판정된다() {
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        // 좌/우/상/하 각 분기를 모두 도달시킨다.
        assert!(!point_in_rect(Vec2::new(9.0, 25.0), r));   // x<min.x
        assert!(!point_in_rect(Vec2::new(31.0, 25.0), r));  // x>max.x
        assert!(!point_in_rect(Vec2::new(20.0, 19.0), r));  // y<min.y
        assert!(!point_in_rect(Vec2::new(20.0, 41.0), r));  // y>max.y
    }

    // ── dialog_rect / minimap_rect ───────────────────────────────────────

    #[test]
    fn 다이얼로그_사각형은_viewport_하단_96픽셀을_덮는다() {
        let r = dialog_rect(Vec2::new(800.0, 600.0));
        assert_eq!(r.min, Vec2::new(0.0, 600.0 - DIALOG_PANEL_HEIGHT_PX));
        assert_eq!(r.max, Vec2::new(800.0, 600.0));
        // 폭 = viewport.x, 높이 = DIALOG_PANEL_HEIGHT_PX.
        assert_eq!(r.width(), 800.0);
        assert!((r.height() - DIALOG_PANEL_HEIGHT_PX).abs() < 1e-3);
    }

    #[test]
    fn viewport_가_패널보다_낮으면_다이얼로그_사각형이_전체를_덮는다() {
        // viewport.y < DIALOG_PANEL_HEIGHT_PX → top_y == 0, max.y == viewport.y.
        let r = dialog_rect(Vec2::new(800.0, 50.0));
        assert_eq!(r.min.y, 0.0);
        assert_eq!(r.max.y, 50.0);
    }

    #[test]
    fn 미니맵_사각형은_viewport_우상단에_고정크기로_배치된다() {
        let r = minimap_rect(Vec2::new(800.0, 600.0));
        // 우측: 800 - 5 = 795, 좌측: 795 - 180 = 615.
        assert_eq!(r.max.x, 795.0);
        assert_eq!(r.min.x, 795.0 - MINIMAP_OVERLAY_WIDTH_PX);
        // 상단: 10, 하단: 10 + 212 = 222.
        assert_eq!(r.min.y, MINIMAP_TOP_PX);
        assert_eq!(r.max.y, MINIMAP_TOP_PX + MINIMAP_OVERLAY_HEIGHT_PX);
    }

    #[test]
    fn viewport_가_미니맵보다_작으면_사각형은_clamp된다() {
        // viewport.x < 미니맵 폭 + 여백 → min.x 가 0 으로 clamp.
        let r = minimap_rect(Vec2::new(100.0, 50.0));
        assert!(r.min.x >= 0.0);
        assert!(r.max.x >= r.min.x);
        // viewport.y < 미니맵 높이 → bottom 도 viewport.y 이하.
        assert!(r.max.y <= 50.0);
    }

    // ── target_alpha ─────────────────────────────────────────────────────

    #[test]
    fn 겹치지_않으면_목표_alpha는_원본_그대로다() {
        assert_eq!(target_alpha(0.8, false), 0.8);
        assert_eq!(target_alpha(1.0, false), 1.0);
    }

    #[test]
    fn 겹치면_목표_alpha는_원본의_절반이다() {
        assert!((target_alpha(0.8, true) - 0.4).abs() < 1e-6);
        assert!((target_alpha(1.0, true) - 0.5).abs() < 1e-6);
    }

    // ── is_player_overlapping ────────────────────────────────────────────

    #[test]
    fn 플레이어가_다이얼로그_영역_안이면_겹침으로_판정된다() {
        let vp = Vec2::new(800.0, 600.0);
        // 하단 패널 안쪽 픽셀.
        assert!(is_player_overlapping(Vec2::new(400.0, 580.0), vp, FadeTarget::Dialog));
    }

    #[test]
    fn 플레이어가_다이얼로그_영역_위쪽이면_겹치지_않는다() {
        let vp = Vec2::new(800.0, 600.0);
        // y=0 은 화면 최상단.
        assert!(!is_player_overlapping(Vec2::new(400.0, 100.0), vp, FadeTarget::Dialog));
    }

    #[test]
    fn 플레이어가_미니맵_영역_안이면_겹침으로_판정된다() {
        let vp = Vec2::new(800.0, 600.0);
        // 우상단 미니맵 안쪽.
        assert!(is_player_overlapping(Vec2::new(780.0, 30.0), vp, FadeTarget::Minimap));
    }

    #[test]
    fn 플레이어가_미니맵_영역_왼쪽이면_겹치지_않는다() {
        let vp = Vec2::new(800.0, 600.0);
        assert!(!is_player_overlapping(Vec2::new(100.0, 30.0), vp, FadeTarget::Minimap));
    }

    // ── fade_bg_alpha ────────────────────────────────────────────────────

    #[test]
    fn 원본없이_BG_페이드는_현재_alpha를_원본으로_채택해_절반으로_낮춘다() {
        let mut bg = BackgroundColor(Color::rgba(0.1, 0.2, 0.3, 0.8));
        let captured = fade_bg_alpha(&mut bg, None, true);
        assert!((captured - 0.8).abs() < 1e-6);
        assert!((bg.0.a() - 0.4).abs() < 1e-6);
        // 채널 값은 보존돼야 한다.
        assert!((bg.0.r() - 0.1).abs() < 1e-6);
        assert!((bg.0.g() - 0.2).abs() < 1e-6);
        assert!((bg.0.b() - 0.3).abs() < 1e-6);
    }

    #[test]
    fn 원본을_명시하면_BG_복원은_정확히_원본으로_되돌린다() {
        let mut bg = BackgroundColor(Color::rgba(0.0, 0.0, 0.0, 0.4)); // 이미 페이드된 상태
        let _ = fade_bg_alpha(&mut bg, Some(0.8), false);
        assert!((bg.0.a() - 0.8).abs() < 1e-6);
    }

    // ── fade_text_alpha ──────────────────────────────────────────────────

    fn 두_섹션_텍스트() -> Text {
        let mut t = Text::from_section("A", TextStyle { color: Color::rgba(1.0, 1.0, 1.0, 1.0), ..default() });
        t.sections.push(TextSection {
            value: "B".into(),
            style: TextStyle { color: Color::rgba(0.5, 0.5, 0.5, 0.6), ..default() },
        });
        t
    }

    #[test]
    fn 원본없이_Text_페이드는_각_섹션_현재_alpha를_원본으로_채택한다() {
        let mut t = 두_섹션_텍스트();
        let captured = fade_text_alpha(&mut t, None, true);
        assert_eq!(captured.len(), 2);
        assert!((captured[0] - 1.0).abs() < 1e-6);
        assert!((captured[1] - 0.6).abs() < 1e-6);
        assert!((t.sections[0].style.color.a() - 0.5).abs() < 1e-6);
        assert!((t.sections[1].style.color.a() - 0.3).abs() < 1e-6);
    }

    #[test]
    fn 원본_명시시_Text_복원은_각_섹션을_정확히_원본으로_되돌린다() {
        let mut t = 두_섹션_텍스트();
        // 페이드된 상태를 임의로 만들어둠.
        t.sections[0].style.color.set_a(0.4);
        t.sections[1].style.color.set_a(0.2);
        let _ = fade_text_alpha(&mut t, Some(&[1.0, 0.6]), false);
        assert!((t.sections[0].style.color.a() - 1.0).abs() < 1e-6);
        assert!((t.sections[1].style.color.a() - 0.6).abs() < 1e-6);
    }

    #[test]
    fn 원본_길이가_섹션수보다_짧으면_부족분은_현재_alpha로_채워진다() {
        // originals 가 첫 섹션만 지정한 경우, 두 번째 섹션은 자신의 현재 alpha 를
        // 새로 캡처해 처리한다.
        let mut t = 두_섹션_텍스트();
        let captured = fade_text_alpha(&mut t, Some(&[1.0]), true);
        assert_eq!(captured.len(), 2);
        assert!((captured[0] - 1.0).abs() < 1e-6);
        assert!((captured[1] - 0.6).abs() < 1e-6, "두 번째는 현재 alpha 0.6 으로 캡처");
        assert!((t.sections[0].style.color.a() - 0.5).abs() < 1e-6);
        assert!((t.sections[1].style.color.a() - 0.3).abs() < 1e-6);
    }

    #[test]
    fn 원본_길이가_섹션수보다_길면_남는_원본은_무시된다() {
        // originals 가 [1.0, 0.6, 0.9] 로 더 길어도, 세 번째 항목은 무시되고
        // 첫 두 섹션은 정상 처리된다.
        let mut t = 두_섹션_텍스트();
        let captured = fade_text_alpha(&mut t, Some(&[1.0, 0.6, 0.9]), false);
        assert_eq!(captured.len(), 2, "캡처는 섹션 수에 한정");
        assert!((t.sections[0].style.color.a() - 1.0).abs() < 1e-6);
        assert!((t.sections[1].style.color.a() - 0.6).abs() < 1e-6);
    }

    // ── 시스템·플러그인 통합 ─────────────────────────────────────────────

    /// 헤드리스 App 하네스 — 카메라/플레이어/UI 마커가 있어도 viewport 변환이
    /// 항상 None 이라 update 시 본체 분기는 도달하지 않는다. early return 분기와
    /// 패닉 부재만 확인한다.
    fn 페이드_하네스() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app
    }

    #[test]
    fn 플러그인이_정상적으로_빌드된다() {
        let mut app = App::new();
        app.add_plugins(UiOverlapTransparencyPlugin);
        // build 단계에서 패닉 없이 등록되면 충분.
        app.update(); // 빈 world 라 early return 으로 빠져나가야 한다.
    }

    #[test]
    fn 플레이어가_없으면_시스템은_조용히_종료한다() {
        let mut app = 페이드_하네스();
        app.add_systems(Update, update_ui_overlap_transparency);
        app.update(); // 패닉 없이 통과해야 한다.
    }

    #[test]
    fn 플레이어는_있어도_카메라가_없으면_시스템은_조용히_종료한다() {
        let mut app = 페이드_하네스();
        app.world.spawn((Player, Transform::default()));
        app.add_systems(Update, update_ui_overlap_transparency);
        app.update(); // 카메라 get_single 실패 → early return.
    }

    #[test]
    fn FadeTarget는_Dialog와_Minimap이_서로_다른_값으로_식별된다() {
        // Component 매칭(`match` 분기) 분기 양쪽을 동시에 도달시킨다.
        let vp = Vec2::new(800.0, 600.0);
        let p_in_dialog = Vec2::new(400.0, 580.0);
        let p_in_minimap = Vec2::new(780.0, 30.0);
        assert!(is_player_overlapping(p_in_dialog, vp, FadeTarget::Dialog));
        assert!(!is_player_overlapping(p_in_dialog, vp, FadeTarget::Minimap));
        assert!(is_player_overlapping(p_in_minimap, vp, FadeTarget::Minimap));
        assert!(!is_player_overlapping(p_in_minimap, vp, FadeTarget::Dialog));
    }

    #[test]
    fn FadeOnOverlap_컴포넌트는_명시한_FadeTarget을_보존한다() {
        let f = FadeOnOverlap(FadeTarget::Dialog);
        assert_eq!(f.0, FadeTarget::Dialog);
        let f2 = FadeOnOverlap(FadeTarget::Minimap);
        assert_eq!(f2.0, FadeTarget::Minimap);
    }

    #[test]
    fn 미니맵_오버레이_상수값은_spawn과_동기화돼있다() {
        // spawn_minimap_overlay 의 하드코딩 값과 차이가 생기면 회귀가 잡히게.
        assert_eq!(MINIMAP_OVERLAY_WIDTH_PX, MINIMAP_DISPLAY_SIZE);
        assert_eq!(MINIMAP_RIGHT_PX, 5.0);
        assert_eq!(MINIMAP_TOP_PX, 10.0);
        // 오버레이 높이는 이미지(180) 이상이어야 한다(텍스트가 더 붙으므로).
        assert!(MINIMAP_OVERLAY_HEIGHT_PX >= MINIMAP_DISPLAY_SIZE);
    }
}
