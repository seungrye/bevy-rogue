//! 플레이어가 상자(Chest)를 열었을 때의 UI 표시 로직을 담당하는 모듈입니다.
//!
//! `spawn_chest_on_event` 함수를 통해 화면 중앙에 상자 이미지를 팝업으로 띄웁니다.

use bevy::prelude::*;
use super::{Z_CHEST_UI, TriggerEffect};

/// 상자 열기 이벤트가 발생했을 때 화면에 상자 이미지를 스폰하는 시스템입니다.
pub fn spawn_chest_on_event(
    commands: &mut Commands,
    asset_server: &Res<AssetServer>,
    // 다른 UI가 이미 화면에 표시되어 있는지 확인하기 위한 쿼리입니다.
    q_trigger_effect: &Query<Entity, With<TriggerEffect>>,
) {
    // 이미 다른 UI(상자, 메시지 등)가 화면에 표시되어 있다면, 중복으로 띄우지 않도록 조기 종료합니다.
    if !q_trigger_effect.is_empty() {
        info!("Chest is already displayed.");
        return;
    }

    info!("Spawning chest image on screen.");

    let chest_image = asset_server.load("scene/open-chest.png");

    // 화면 전체를 덮는 부모 노드를 생성하여 자식인 이미지를 중앙에 배치합니다.
    // 이 노드는 투명한 배경을 가지며, 다른 UI 요소들보다 위에 표시됩니다.
    commands
        .spawn((
            NodeBundle {
                style: Style {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    // Flexbox를 사용하여 자식 요소를 수평/수직 중앙에 정렬합니다.
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    // 다른 UI와 관계없이 화면 전체를 기준으로 위치를 잡습니다.
                    position_type: PositionType::Absolute,
                    ..default()
                },
                // 다른 UI 요소들 위에 렌더링되도록 z_index를 설정합니다.
                z_index: ZIndex::Global(Z_CHEST_UI),
                // 컨테이너 자체의 배경은 보이지 않도록 투명하게 설정합니다.
                background_color: Color::NONE.into(),
                ..default()
            },
            // 이 UI가 '상자 열기' 효과로 인해 생성되었음을 나타내는 컴포넌트를 추가합니다.
            // 이 컴포넌트는 `close_ui_on_input` 시스템에서 UI를 닫을 때 사용됩니다.
            TriggerEffect::OpenChest,
        ))
        .with_children(|parent| {
            // 부모 노드(컨테이너)의 자식으로 실제 상자 이미지를 스폰합니다.
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
