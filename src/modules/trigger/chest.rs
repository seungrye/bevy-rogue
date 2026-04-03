use bevy::prelude::*;
use super::{Z_CHEST_UI, TriggerEffect};

/// 상자 열기 이벤트가 발생했을 때 화면 중앙에 상자 이미지를 팝업으로 표시하는 함수입니다.
///
/// # Arguments
/// * `commands` - Bevy 명령 시스템 참조
/// * `asset_server` - 에셋 로더 참조 (상자 이미지 로드용)
/// * `is_displayed` - 이미 다른 UI가 화면에 떠 있는지 여부 (중복 방지용)
pub fn spawn_chest_on_event(
    commands: &mut Commands,
    asset_server: &AssetServer,
    // 이미 화면에 다른 UI가 떠 있는지 상위 시스템에서 판단하여 불값으로 넘겨줍니다.
    is_displayed: bool,
) {
    // 중복 UI 방지
    if is_displayed {
        info!("Chest is already displayed.");
        return;
    }

    info!("Spawning chest image on screen.");
    let chest_image = asset_server.load("scene/open-chest.png");

    // 화면 전체를 덮는 투명한 컨테이너 노드 생성 (자식 중앙 정렬용)
    commands
        .spawn((
            NodeBundle {
                style: Style {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    position_type: PositionType::Absolute,
                    ..default()
                },
                z_index: ZIndex::Global(Z_CHEST_UI),
                background_color: Color::NONE.into(),
                ..default()
            },
            // UI를 닫기 위한 태그로 TriggerEffect 사용
            TriggerEffect::OpenChest,
        ))
        .with_children(|parent| {
            // 실제 상자 이미지 스폰
            parent.spawn(ImageBundle {
                image: chest_image.into(),
                style: Style {
                    width: Val::Percent(50.0),
                    height: Val::Auto,
                    ..default()
                },
                ..default()
            });
        });
}
