//! 인게임 피처 도감/가이드 패널 (§F).
//!
//! 조작키만 안내하는 Help(`ui/help.rs`)와 달리, 이 패널은 게임의 **실제 기능**을
//! 카테고리/항목으로 나눠 설명하는 읽기 전용 도감이다. Help 의 전체화면 오버레이·
//! 토글 패턴과 퀘스트 저널(`ui/quest_panel.rs`)의 "데이터 → 텍스트 빌더 분리"
//! 패턴을 그대로 재사용한다.
//!
//! - 토글 키: `F2` (Help `?`/`H`, 맵생성 `F1`, 퀘스트 `J`/`Q` 등과 충돌 없음).
//! - 탐색: 패널이 열린 동안 `↑`/`↓` 로 항목 선택(경계에서 wrap).
//! - 읽기 전용: 항목을 고르면 그 설명만 강조해 보여 준다.
//!
//! 도감 내용은 코드/specs 에 구현된 실제 동작(키·수치·메커니즘)을 근거로 한다.
//! 항목 목록은 순수 함수 `guide_entries()` 로, 패널 텍스트는 `build_guide_sections()`
//! 로 분리해 단위 테스트가 같은 데이터를 공유하게 한다.

use bevy::prelude::*;

const OVERLAY_Z: i32 = 460;
const PANEL_WIDTH: f32 = 720.0;
const FONT_SIZE: f32 = 14.0;

const C_HEADER:   Color = Color::rgb(0.4, 1.0, 0.6);
const C_HINT:     Color = Color::rgb(0.7, 0.85, 0.8);
const C_CATEGORY: Color = Color::rgb(1.0, 0.85, 0.3);
const C_TITLE:    Color = Color::rgb(0.95, 0.95, 0.95);
const C_TITLE_SEL: Color = Color::rgb(0.3, 1.0, 0.4);
const C_DESC:     Color = Color::rgb(0.8, 0.85, 0.85);
const C_VISUAL:   Color = Color::rgb(0.55, 0.78, 1.0);

/// 도감 가이드 한 항목: 카테고리, 제목, 설명, (선택) 간단 텍스트 시각화.
///
/// 데이터 주도 — 패널 렌더와 단위 테스트가 같은 목록을 쓴다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuideEntry {
    pub category: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// 간단 텍스트 시각화(없으면 빈 문자열).
    pub visual: &'static str,
}

#[derive(Component)] struct GuideOverlay;
#[derive(Component)] struct GuideText;

/// 도감 패널 열림 상태.
#[derive(Resource, Default)]
pub struct GuidePanelOpen(pub bool);

/// 도감에서 현재 선택된 항목 인덱스(`guide_entries()` 기준).
#[derive(Resource, Default)]
pub struct GuideSelection(pub usize);

/// 도감 패널 토글 키 — Help(`?`/`H`)·맵생성(`F1`)·퀘스트(`J`/`Q`) 등과 충돌 없는 `F2`.
pub const KEY_TOGGLE_GUIDE: KeyCode = KeyCode::F2;

/// 모든 핵심 기능을 카테고리/항목으로 정리한 도감 목록(순수 함수).
///
/// 각 설명은 코드/specs 에 구현된 실제 동작(키·수치·메커니즘)을 근거로 한다.
/// 새 기능이 생기면 여기에만 추가하면 패널·테스트가 함께 따라온다.
pub fn guide_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            category: "시야 / 잠입",
            title: "방향 시야 (Directional FOV)",
            description: "바라보는 방향(정면)으로는 8타일까지 멀리, 등 뒤로는 3타일까지 \
                가깝게만 보인다. 정지 직후처럼 방향이 없으면 전방향 8타일 원형으로 본다.",
            visual: "정면 ▶ 8타일   ◀ 등 뒤 3타일",
        },
        GuideEntry {
            category: "시야 / 잠입",
            title: "위험 타일 표시 (정찰 도구)",
            description: "정찰 도구(scout_lens)를 인벤토리에 지니면 가드 시야가 닿는 \
                '위험 타일'에 반투명 붉은 오버레이가 깔린다. 어두운 타일은 가드가 더 \
                가까워야만 위험으로 표시돼 탐지 로직과 같은 광량을 공유한다.",
            visual: "위험 타일 = 붉은 틴트",
        },
        GuideEntry {
            category: "조명 / 그림자",
            title: "어둠 은신 보너스",
            description: "타일은 밝음/어둠 2단계다. 플레이어 시야광 반경은 6타일. \
                플레이어가 어둠에 서 있으면 가드의 탐지 반경이 4만큼 줄어(은신 보너스), \
                밝은 곳에서는 기본 반경 그대로 노출된다. 어두운 타일은 렌더도 절반 디밍된다.",
            visual: "어둠: 탐지반경 -4   밝음: 그대로",
        },
        GuideEntry {
            category: "원소 반응",
            title: "원소 부여와 반응",
            description: "불·얼음·독·번개 4원소를 대상에 부여하면 서로 다른 두 원소가 \
                겹칠 때 반응이 터진다: 불+얼음=융해(15), 불+독=독기(10), 얼음+번개=파쇄(20), \
                얼음+독=동상(5+행동불능), 불+번개=플라즈마(8+행동불능), 독+번개=전기독(8+강화독).",
            visual: "불+얼음→융해  얼음+번개→파쇄",
        },
        GuideEntry {
            category: "아이템",
            title: "티어와 레어도",
            description: "무기·방어구는 티어 1~5로 나뉘고, 드롭 시 티어 범위 안에서 스탯을 \
                롤한다. 롤값 백분위로 레어도가 정해진다: 일반·고급·희귀·영웅·전설. \
                레어도는 인벤토리/드롭 글리프 색으로 구분된다.",
            visual: "일반→고급→희귀→영웅→전설",
        },
        GuideEntry {
            category: "파밍 / 드롭",
            title: "랜덤 스탯 · 레벨 스케일 드롭",
            description: "처치한 적은 장비를 떨굴 수 있고, 같은 종류라도 스탯이 매번 다르게 \
                롤된다. 드롭 티어 분포는 플레이어 레벨을 따라간다(약 3레벨마다 티어 밴드 +1, \
                티어 1~5 범위). 높은 레벨일수록 상위 티어 장비가 더 자주 나온다.",
            visual: "≈3레벨마다 티어 밴드 +1",
        },
        GuideEntry {
            category: "액티브 스킬",
            title: "파이어볼 / 점멸 / 치유 (MP)",
            description: "1=파이어볼(MP 8): 조준 타일에 폭발(지형 파괴+범위 피해 8)+화염. \
                2=점멸(MP 5): 사거리·시야 안의 빈 타일로 순간이동. 3=치유(MP 6): 즉시 HP 12 회복. \
                조준 스킬은 방향키로 커서 이동·Enter 시전·Esc 취소. MP 부족 시 불가, 성공 시 턴 소비.",
            visual: "1 파이어볼  2 점멸  3 치유",
        },
        GuideEntry {
            category: "함정",
            title: "함정 종류 (Spike/Poison/Alarm/Teleport)",
            description: "가시(^): 밟으면 8 피해. 독(*): 독 상태 부여. 경보(!): 잠입 발각 + \
                주변 가드 경계. 전이(&): 무작위 통과 타일로 이동. 숨은 함정은 인접·탐지 시 노출된다. \
                가시·전이는 1회성, 독·경보는 지속(여러 번 발동).",
            visual: "^가시  *독  !경보  &전이",
        },
        GuideEntry {
            category: "함정",
            title: "함정 설치 / 해제",
            description: "T: 함정 키트(trap_kit)로 정면의 빈 타일에 플레이어 함정 설치(키트 1 소모). \
                이 함정은 몬스터 진입 시에만 발동하고 플레이어는 안 밟는다. Y: 인접·현재 타일의 \
                노출된 함정 해제 — 해제 도구(disarm_tool)가 있으면 확정, 없으면 50% 확률(성공 시 회수형 키트 회수).",
            visual: "T 설치   Y 해제",
        },
        GuideEntry {
            category: "지형",
            title: "파괴 가능 지형 · 폭발",
            description: "파괴 가능한 벽(집/건물 구조물)은 폭발 반경 안에 들면 부서져 잔해(통과 가능)로 \
                바뀐다. 맵 테두리·자연 암벽 같은 일반 벽은 파괴되지 않는다. 폭발은 지형 파괴와 \
                범위 피해를 함께 처리한다(파이어볼 등이 발행).",
            visual: "파괴 가능 벽 → 폭발 → 잔해",
        },
        GuideEntry {
            category: "상점",
            title: "고정 상점 (가판대)",
            description: "상인($)은 마을 가판대(카운터 =) 뒤에 고정되어 돌아다니지 않는다. \
                카운터는 통행 불가다. 카운터 앞 타일에서 카운터(또는 그 너머 상인)를 향해 이동하면 \
                상점이 열린다. 상점에서 Tab 으로 구매/판매 탭 전환, ↑↓ 선택, Enter 거래, Esc 닫기.",
            visual: "[당신] → [= 카운터] → [$ 상인]",
        },
        GuideEntry {
            category: "전투",
            title: "근접 · 원격 전투",
            description: "몬스터가 있는 칸으로 이동하면 자동 근접 공격한다. 활을 장착하면 \
                F 로 원격 조준을 시작하고 Tab/Shift+Tab 으로 타겟 전환, Enter·좌클릭으로 발사, \
                Esc·우클릭으로 취소한다. 무기에 원소가 붙어 있으면 명중 시 원소가 부여된다.",
            visual: "이동=근접   F=원격 조준",
        },
        GuideEntry {
            category: "퀘스트",
            title: "퀘스트 / 저널",
            description: "J 또는 Q 로 퀘스트 패널을 연다. 상단 저널 요약은 진행 중(수락했고 \
                아직 완료되지 않은) 퀘스트의 현재 목표만 읽기 전용으로 나열한다. 목표 존·진행도·\
                다음 행동 힌트와 미니맵 마커로 길을 안내한다.",
            visual: "J / Q = 퀘스트 · 저널",
        },
    ]
}

/// 도감 항목들에서 등장 순서를 유지하며 중복 없이 카테고리 목록을 뽑는다(순수 함수).
pub fn guide_categories(entries: &[GuideEntry]) -> Vec<&'static str> {
    let mut cats: Vec<&'static str> = Vec::new();
    for e in entries {
        if !cats.contains(&e.category) {
            cats.push(e.category);
        }
    }
    cats
}

/// 선택 인덱스를 항목 수 안에서 한 칸 위/아래로 이동시킨다(경계에서 wrap, 순수 함수).
///
/// `delta` 가 음수면 위로, 양수면 아래로 이동한다. 항목이 없으면 0을 돌려준다.
pub fn move_selection(current: usize, len: usize, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    let len_i = len as i32;
    let next = (current as i32 + delta).rem_euclid(len_i);
    next as usize
}

pub struct GuidePanelPlugin;

impl Plugin for GuidePanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GuidePanelOpen>()
            .init_resource::<GuideSelection>()
            .add_systems(Startup, setup_guide_overlay)
            .add_systems(
                Update,
                (toggle_guide_overlay, navigate_guide, update_guide_overlay).chain(),
            );
    }
}

/// 시작 시 숨겨진 도감 오버레이 UI를 만든다. 내용은 선택이 바뀔 때만 다시 그린다.
fn setup_guide_overlay(mut commands: Commands, asset_server: Res<AssetServer>) {
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
                background_color: Color::rgba(0.0, 0.0, 0.0, 0.7).into(),
                z_index: ZIndex::Global(OVERLAY_Z),
                visibility: Visibility::Hidden,
                ..default()
            },
            GuideOverlay,
        ))
        .with_children(|parent| {
            parent
                .spawn(NodeBundle {
                    style: Style {
                        width: Val::Px(PANEL_WIDTH),
                        padding: UiRect::all(Val::Px(20.0)),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                    background_color: Color::rgba(0.0, 0.05, 0.04, 0.97).into(),
                    ..default()
                })
                .with_children(|panel| {
                    panel.spawn((
                        TextBundle::from_sections(build_guide_sections(
                            &guide_entries(),
                            0,
                            &font,
                        )),
                        GuideText,
                    ));
                });
        });
}

/// `F2` 로 도감을 열고 닫는다. 열린 상태에서는 `Esc` 로도 닫는다.
///
/// 다른 모달 패널과의 충돌은 이동/스킬 쪽 가드가 막으므로 여기서는 자체 토글만 한다
/// (Help 토글과 같은 흐름).
fn toggle_guide_overlay(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut open: ResMut<GuidePanelOpen>,
    defeated_q: Query<(), With<crate::modules::combat::Defeated>>,
) {
    if !defeated_q.is_empty() {
        return;
    }
    if keyboard.just_pressed(KEY_TOGGLE_GUIDE) {
        open.0 = !open.0;
    } else if open.0 && keyboard.just_pressed(KeyCode::Escape) {
        open.0 = false;
    }
}

/// 도감이 열린 동안 `↑`/`↓` 로 항목 선택을 옮긴다(경계에서 wrap). 읽기 전용.
fn navigate_guide(
    keyboard: Res<ButtonInput<KeyCode>>,
    open: Res<GuidePanelOpen>,
    mut selection: ResMut<GuideSelection>,
) {
    if !open.0 {
        return;
    }
    let len = guide_entries().len();
    if keyboard.just_pressed(KeyCode::ArrowUp) {
        selection.0 = move_selection(selection.0, len, -1);
    } else if keyboard.just_pressed(KeyCode::ArrowDown) {
        selection.0 = move_selection(selection.0, len, 1);
    }
}

/// 열림 상태/선택이 바뀌면 오버레이 visibility 와 도감 텍스트를 갱신한다.
fn update_guide_overlay(
    open: Res<GuidePanelOpen>,
    selection: Res<GuideSelection>,
    asset_server: Res<AssetServer>,
    mut overlay_q: Query<&mut Visibility, With<GuideOverlay>>,
    mut text_q: Query<&mut Text, With<GuideText>>,
) {
    if !open.is_changed() && !selection.is_changed() {
        return;
    }
    if let Ok(mut visibility) = overlay_q.get_single_mut() {
        *visibility = if open.0 { Visibility::Inherited } else { Visibility::Hidden };
    }
    if !open.0 {
        return;
    }
    let font = asset_server.load("fonts/NanumSquareNeo-bRg.ttf");
    if let Ok(mut text) = text_q.get_single_mut() {
        text.sections = build_guide_sections(&guide_entries(), selection.0, &font);
    }
}

/// 도감 패널 텍스트 섹션을 만든다(순수 함수).
///
/// 카테고리별로 묶어 항목 제목을 나열하고, 현재 선택된 항목은 색을 강조하면서
/// 그 설명·간단 시각화를 펼쳐 보여 준다. 실제 패널과 테스트가 같은 빌더를 쓴다.
fn build_guide_sections(
    entries: &[GuideEntry],
    selected: usize,
    font: &Handle<Font>,
) -> Vec<TextSection> {
    let mut s = Vec::new();
    push(&mut s, "/ 피처 도감 (GUIDE) /\n", font, 22.0, C_HEADER);
    push(
        &mut s,
        "F2: 열기/닫기    ↑↓: 항목 선택    Esc: 닫기\n\n",
        font,
        FONT_SIZE,
        C_HINT,
    );

    // 카테고리 순서(중복 제거)는 순수 함수로 뽑아 항목 그룹화에 그대로 쓴다.
    for category in guide_categories(entries) {
        push(&mut s, &format!("\n[{}]\n", category), font, FONT_SIZE, C_CATEGORY);
        for (i, entry) in entries.iter().enumerate() {
            if entry.category != category {
                continue;
            }
            let is_sel = i == selected;
            let marker = if is_sel { "▶ " } else { "  " };
            let title_color = if is_sel { C_TITLE_SEL } else { C_TITLE };
            push(&mut s, &format!("{}{}\n", marker, entry.title), font, FONT_SIZE, title_color);

            // 선택된 항목만 설명·시각화를 펼친다(읽기 전용 상세).
            if is_sel {
                push(&mut s, &format!("    {}\n", entry.description), font, FONT_SIZE, C_DESC);
                if !entry.visual.is_empty() {
                    push(&mut s, &format!("    {}\n", entry.visual), font, FONT_SIZE, C_VISUAL);
                }
            }
        }
    }
    s
}

/// 텍스트 섹션 하나를 지정 스타일로 추가한다.
fn push(sections: &mut Vec<TextSection>, value: &str, font: &Handle<Font>, size: f32, color: Color) {
    sections.push(TextSection::new(
        value.to_string(),
        TextStyle { font: font.clone(), font_size: size, color },
    ));
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;
    use crate::modules::combat::Defeated;

    fn all_text(sections: &[TextSection]) -> String {
        sections.iter().map(|s| s.value.as_str()).collect()
    }

    // ── 순수 함수: 도감 데이터 ───────────────────────────────────────────────

    #[test]
    fn 도감은_모든_핵심기능_카테고리를_포함한다() {
        let cats = guide_categories(&guide_entries());
        for expected in [
            "시야 / 잠입",
            "조명 / 그림자",
            "원소 반응",
            "아이템",
            "파밍 / 드롭",
            "액티브 스킬",
            "함정",
            "지형",
            "상점",
            "전투",
            "퀘스트",
        ] {
            assert!(cats.contains(&expected), "카테고리 '{expected}' 가 도감에 있어야 한다");
        }
    }

    #[test]
    fn 도감_항목은_제목과_설명을_모두_가진다() {
        for e in guide_entries() {
            assert!(!e.title.is_empty(), "제목이 비면 안 된다");
            assert!(!e.description.is_empty(), "설명이 비면 안 된다");
            assert!(!e.category.is_empty(), "카테고리가 비면 안 된다");
        }
    }

    #[test]
    fn 카테고리_목록은_중복없이_등장순서를_유지한다() {
        let entries = vec![
            GuideEntry { category: "A", title: "t1", description: "d", visual: "" },
            GuideEntry { category: "A", title: "t2", description: "d", visual: "" },
            GuideEntry { category: "B", title: "t3", description: "d", visual: "" },
            GuideEntry { category: "A", title: "t4", description: "d", visual: "" },
        ];
        assert_eq!(guide_categories(&entries), vec!["A", "B"]);
    }

    #[test]
    fn 도감_설명은_실제_구현된_키와_수치를_담는다() {
        // 내용 정확성: 코드 근거(스킬 키/MP, 함정 키, FOV 수치, scout_lens 등).
        let text = all_text(&build_guide_sections(&guide_entries(), 0, &Handle::default()));
        // 항목별 설명은 선택해야 펼쳐지므로 항목 전체를 순회하며 합친다.
        let mut full = String::new();
        for i in 0..guide_entries().len() {
            full.push_str(&all_text(&build_guide_sections(&guide_entries(), i, &Handle::default())));
        }
        assert!(full.contains("8타일"), "방향 FOV 정면 8타일 수치 포함");
        assert!(full.contains("scout_lens"), "정찰 도구 id 포함");
        assert!(full.contains("MP 8"), "파이어볼 MP 비용 포함");
        assert!(full.contains("trap_kit"), "함정 키트 id 포함");
        assert!(full.contains("disarm_tool"), "해제 도구 id 포함");
        // 첫 렌더 텍스트에도 머리말이 있다.
        assert!(text.contains("피처 도감"));
    }

    // ── 순수 함수: 선택 이동(경계 wrap) ──────────────────────────────────────

    #[test]
    fn 선택은_아래로_이동하면_다음_항목을_가리킨다() {
        assert_eq!(move_selection(0, 5, 1), 1);
    }

    #[test]
    fn 선택은_마지막에서_아래로_가면_처음으로_wrap된다() {
        assert_eq!(move_selection(4, 5, 1), 0);
    }

    #[test]
    fn 선택은_처음에서_위로_가면_마지막으로_wrap된다() {
        assert_eq!(move_selection(0, 5, -1), 4);
    }

    #[test]
    fn 항목이_없으면_선택은_0을_유지한다() {
        assert_eq!(move_selection(0, 0, 1), 0);
        assert_eq!(move_selection(3, 0, -1), 0);
    }

    // ── 순수 함수: 패널 텍스트 빌더 ─────────────────────────────────────────

    #[test]
    fn 패널은_카테고리_머리말과_모든_항목제목을_나열한다() {
        let entries = guide_entries();
        let text = all_text(&build_guide_sections(&entries, 0, &Handle::default()));
        // 첫 카테고리 머리말.
        assert!(text.contains("[시야 / 잠입]"));
        // 모든 항목 제목이 (선택 여부와 무관하게) 나열된다.
        for e in &entries {
            assert!(text.contains(e.title), "제목 '{}' 가 목록에 있어야 한다", e.title);
        }
    }

    #[test]
    fn 선택한_항목의_설명텍스트가_렌더된다() {
        let entries = guide_entries();
        // 1번 항목(인덱스 1)을 선택하면 그 설명·시각화가 펼쳐진다.
        let text = all_text(&build_guide_sections(&entries, 1, &Handle::default()));
        assert!(text.contains(entries[1].description));
        assert!(text.contains(entries[1].visual));
    }

    #[test]
    fn 선택되지_않은_항목의_설명은_펼쳐지지_않는다() {
        let entries = guide_entries();
        // 0번을 선택했을 때, 1번 항목의 설명은 펼쳐지지 않아야 한다.
        let text = all_text(&build_guide_sections(&entries, 0, &Handle::default()));
        assert!(!text.contains(entries[1].description));
    }

    #[test]
    fn 선택된_항목에는_화살표_표식이_붙는다() {
        let entries = guide_entries();
        let text = all_text(&build_guide_sections(&entries, 2, &Handle::default()));
        assert!(text.contains(&format!("▶ {}", entries[2].title)));
    }

    #[test]
    fn 시각화가_빈_항목은_시각화_줄을_렌더하지_않는다() {
        // visual: "" 분기 — 선택해도 시각화 줄이 추가되지 않아야 한다.
        let entries = vec![
            GuideEntry { category: "A", title: "제목", description: "설명", visual: "" },
        ];
        let sections = build_guide_sections(&entries, 0, &Handle::default());
        let text = all_text(&sections);
        assert!(text.contains("설명"));
        // 설명 줄 외에 시각화 들여쓰기 줄이 더 없는지: 들여쓰기 줄은 설명 하나뿐.
        let indented = sections.iter().filter(|s| s.value.starts_with("    ")).count();
        assert_eq!(indented, 1, "시각화가 비면 설명 한 줄만 들여쓰기된다");
    }

    // ── App 하네스: 플러그인 / 셋업 ──────────────────────────────────────────

    fn 렌더_하네스() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app
    }

    fn 토글_하네스() -> App {
        let mut app = App::new();
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app.init_resource::<GuidePanelOpen>();
        app.init_resource::<GuideSelection>();
        app.add_systems(Update, toggle_guide_overlay);
        app
    }

    fn 탐색_하네스() -> App {
        let mut app = App::new();
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app.init_resource::<GuidePanelOpen>();
        app.init_resource::<GuideSelection>();
        app.add_systems(Update, navigate_guide);
        app
    }

    #[test]
    fn 플러그인을_추가하면_시작시_숨김상태의_도감_오버레이가_생성된다() {
        let mut app = 렌더_하네스();
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app.add_plugins(GuidePanelPlugin);
        app.update();

        let mut q = app.world.query_filtered::<&Visibility, With<GuideOverlay>>();
        assert_eq!(*q.single(&app.world), Visibility::Hidden);
        assert_eq!(app.world.query::<&GuideText>().iter(&app.world).count(), 1);
        assert!(app.world.get_resource::<GuidePanelOpen>().is_some());
        assert!(app.world.get_resource::<GuideSelection>().is_some());
    }

    // ── 토글 ─────────────────────────────────────────────────────────────────

    #[test]
    fn F2를_누르면_도감_열림상태가_뒤집힌다() {
        let mut app = 토글_하네스();
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KEY_TOGGLE_GUIDE);
        app.update();
        assert!(app.world.resource::<GuidePanelOpen>().0);
    }

    #[test]
    fn 도감이_열린_상태에서_Esc를_누르면_닫힌다() {
        let mut app = 토글_하네스();
        app.world.resource_mut::<GuidePanelOpen>().0 = true;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
        app.update();
        assert!(!app.world.resource::<GuidePanelOpen>().0);
    }

    #[test]
    fn 도감이_닫힌_상태에서_Esc를_눌러도_상태는_그대로다() {
        let mut app = 토글_하네스();
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
        app.update();
        assert!(!app.world.resource::<GuidePanelOpen>().0);
    }

    #[test]
    fn 도감이_열린_상태에서_Esc가_아닌_키는_도감을_닫지_않는다() {
        let mut app = 토글_하네스();
        app.world.resource_mut::<GuidePanelOpen>().0 = true;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Space);
        app.update();
        assert!(app.world.resource::<GuidePanelOpen>().0);
    }

    #[test]
    fn 아무_키도_누르지_않으면_도감_상태는_변하지_않는다() {
        let mut app = 토글_하네스();
        app.update();
        assert!(!app.world.resource::<GuidePanelOpen>().0);
    }

    #[test]
    fn 플레이어가_쓰러진_상태면_F2를_눌러도_도감이_열리지_않는다() {
        let mut app = 토글_하네스();
        app.world.spawn(Defeated);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KEY_TOGGLE_GUIDE);
        app.update();
        assert!(!app.world.resource::<GuidePanelOpen>().0);
    }

    // ── 탐색(↑↓) ──────────────────────────────────────────────────────────────

    #[test]
    fn 도감이_닫혀_있으면_방향키_탐색은_무시된다() {
        let mut app = 탐색_하네스();
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowDown);
        app.update();
        assert_eq!(app.world.resource::<GuideSelection>().0, 0);
    }

    #[test]
    fn 도감이_열린_상태에서_아래키는_선택을_다음_항목으로_옮긴다() {
        let mut app = 탐색_하네스();
        app.world.resource_mut::<GuidePanelOpen>().0 = true;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowDown);
        app.update();
        assert_eq!(app.world.resource::<GuideSelection>().0, 1);
    }

    #[test]
    fn 도감이_열린_상태에서_위키는_처음에서_마지막으로_wrap한다() {
        let mut app = 탐색_하네스();
        app.world.resource_mut::<GuidePanelOpen>().0 = true;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowUp);
        app.update();
        assert_eq!(app.world.resource::<GuideSelection>().0, guide_entries().len() - 1);
    }

    #[test]
    fn 열린_도감에서_방향키가_아닌_키는_선택을_바꾸지_않는다() {
        let mut app = 탐색_하네스();
        app.world.resource_mut::<GuidePanelOpen>().0 = true;
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Space);
        app.update();
        assert_eq!(app.world.resource::<GuideSelection>().0, 0);
    }

    // ── 갱신(visibility / 텍스트) ────────────────────────────────────────────

    fn 갱신_하네스() -> App {
        let mut app = 렌더_하네스();
        app.init_resource::<GuidePanelOpen>();
        app.init_resource::<GuideSelection>();
        app.world.spawn((Visibility::Hidden, GuideOverlay));
        app.world.spawn((Text::default(), GuideText));
        app.add_systems(Update, update_guide_overlay);
        app
    }

    #[test]
    fn 도감이_열리면_오버레이가_보이고_텍스트가_채워진다() {
        let mut app = 갱신_하네스();
        app.world.resource_mut::<GuidePanelOpen>().0 = true;
        app.update();
        let vis = *app.world.query_filtered::<&Visibility, With<GuideOverlay>>().single(&app.world);
        assert!(matches!(vis, Visibility::Inherited));
        let text = app.world.query_filtered::<&Text, With<GuideText>>().single(&app.world);
        assert!(!text.sections.is_empty());
    }

    #[test]
    fn 도감이_닫히면_오버레이가_숨겨진다() {
        let mut app = 갱신_하네스();
        app.world.resource_mut::<GuidePanelOpen>().0 = true;
        app.update();
        app.world.resource_mut::<GuidePanelOpen>().0 = false;
        app.update();
        let vis = *app.world.query_filtered::<&Visibility, With<GuideOverlay>>().single(&app.world);
        assert!(matches!(vis, Visibility::Hidden));
    }

    #[test]
    fn 열린_도감에서_선택이_바뀌면_텍스트가_다시_채워진다() {
        let mut app = 갱신_하네스();
        app.world.resource_mut::<GuidePanelOpen>().0 = true;
        app.update();
        {
            let mut text = app.world.query_filtered::<&mut Text, With<GuideText>>().single_mut(&mut app.world);
            text.sections.clear();
        }
        // open 은 그대로, selection 만 변경 → selection.is_changed() 분기로 갱신.
        app.world.resource_mut::<GuideSelection>().0 = 2;
        app.update();
        let text = app.world.query_filtered::<&Text, With<GuideText>>().single(&app.world);
        assert!(!text.sections.is_empty());
    }

    #[test]
    fn 변경이_없으면_도감_텍스트를_다시_채우지_않는다() {
        let mut app = 갱신_하네스();
        app.world.resource_mut::<GuidePanelOpen>().0 = true;
        app.update();
        {
            let mut text = app.world.query_filtered::<&mut Text, With<GuideText>>().single_mut(&mut app.world);
            text.sections.clear();
        }
        app.update(); // open/selection 모두 변경 없음 → 조기 반환
        let text = app.world.query_filtered::<&Text, With<GuideText>>().single(&app.world);
        assert!(text.sections.is_empty());
    }

    #[test]
    fn 오버레이_엔티티가_없어도_갱신은_조용히_넘어간다() {
        let mut app = 렌더_하네스();
        app.init_resource::<GuidePanelOpen>();
        app.init_resource::<GuideSelection>();
        app.world.spawn((Text::default(), GuideText));
        app.add_systems(Update, update_guide_overlay);
        app.world.resource_mut::<GuidePanelOpen>().0 = true;
        app.update(); // 오버레이 없음 → get_single_mut Err 분기, 패닉 없음
        // 패널은 열림으로 간주돼 텍스트는 채워진다.
        let text = app.world.query_filtered::<&Text, With<GuideText>>().single(&app.world);
        assert!(!text.sections.is_empty());
    }

    #[test]
    fn 텍스트_엔티티가_없어도_갱신은_조용히_넘어간다() {
        let mut app = 렌더_하네스();
        app.init_resource::<GuidePanelOpen>();
        app.init_resource::<GuideSelection>();
        app.world.spawn((Visibility::Hidden, GuideOverlay));
        app.add_systems(Update, update_guide_overlay);
        app.world.resource_mut::<GuidePanelOpen>().0 = true;
        app.update(); // 텍스트 엔티티 없음 → text_q.get_single_mut Err 분기
        let vis = *app.world.query_filtered::<&Visibility, With<GuideOverlay>>().single(&app.world);
        assert!(matches!(vis, Visibility::Inherited));
    }

    #[test]
    fn 닫힌_도감은_갱신시_텍스트를_채우지_않는다() {
        // open 이 거짓이고 막 바뀐 첫 프레임: visibility 만 숨김으로 동기화하고 텍스트는 안 채움.
        let mut app = 갱신_하네스();
        app.world.resource_mut::<GuidePanelOpen>().set_changed();
        app.update();
        let text = app.world.query_filtered::<&Text, With<GuideText>>().single(&app.world);
        assert!(text.sections.is_empty());
        let vis = *app.world.query_filtered::<&Visibility, With<GuideOverlay>>().single(&app.world);
        assert!(matches!(vis, Visibility::Hidden));
    }
}
