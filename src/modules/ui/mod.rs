use bevy::prelude::*;
pub mod minimap;
use crate::modules::map::{MAP_WIDTH, TILE_SIZE};

/// UI 패널의 기본 배경색 (반투명 검정)
const UI_PANEL_BG_COLOR: Color = Color::rgba(0.1, 0.1, 0.1, 1.0);
const DIALOG_PANEL_BG_COLOR: Color = Color::rgba(0.1, 0.1, 0.1, 0.8);

/// 오른쪽 능력치 패널의 너비
pub const STATS_PANEL_WIDTH_PX: f32 = 200.0;
/// 하단 대화창 패널의 높이
pub const DIALOG_PANEL_HEIGHT_PX: f32 = 96.0;
/// 로그 메시지 최대 보관 개수
const MAX_LOG_LINES: usize = 5;

/// 게임의 전체 UI(스탯창, 대화창, 미니맵)를 관리하는 플러그인입니다.
pub struct GameUiPlugin;

impl Plugin for GameUiPlugin {
    /// UI 에셋을 로드하고 레이아웃을 초기화하며 미니맵 플러그인을 포함시킵니다.
    fn build(&self, app: &mut App) {
        app.add_plugins(minimap::MinimapPlugin)
            .add_systems(Startup, setup_ui.after(minimap::setup_minimap))
            .add_event::<LogMessage>()
            .init_resource::<MessageLog>()
            .add_systems(Update, update_dialog_box);
    }
}

/// 새로운 로그 메시지 발생 시 전파되는 이벤트 형태입니다.
#[derive(Event)] pub struct LogMessage(pub String);

/// 게임 내 발생한 과거 로그 메시지들을 저장하는 리소스입니다.
#[derive(Resource, Default)] pub struct MessageLog(pub Vec<String>);

/// 하단 대화창의 텍스트 엔티티를 식별하기 위한 컴포넌트입니다.
#[derive(Component)] struct DialogText;

/// UI의 레이아웃을 구성하고 스탯창, 대화창 및 미니맵 컨테이너를 스폰합니다.
fn setup_ui(mut commands: Commands, asset_server: Res<AssetServer>, minimap_image: Res<minimap::MinimapImage>) {
    let stat_font = asset_server.load("fonts/FiraMono-Medium.ttf");
    let dialog_font = asset_server.load("fonts/NanumSquareNeo-bRg.ttf");
    let map_width = MAP_WIDTH;
    let tile_size = TILE_SIZE;

    // --- 오른쪽 스탯 패널 구성 ---
    commands.spawn(NodeBundle { 
        style: Style { 
            width: Val::Px(STATS_PANEL_WIDTH_PX), height: Val::Percent(100.0), 
            position_type: PositionType::Absolute, right: Val::Px(0.0), top: Val::Px(0.0), 
            flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(10.0)), ..default() 
        }, 
        background_color: UI_PANEL_BG_COLOR.into(), 
        ..default() 
    }).with_children(|parent| {
        // 미니맵 이미지 컨테이너
        parent.spawn(ImageBundle { 
            style: Style { width: Val::Px(160.0), height: Val::Px(160.0), margin: UiRect::all(Val::Px(10.0)), border: UiRect::all(Val::Px(2.0)), ..default() }, 
            image: minimap_image.0.clone().into(), 
            background_color: Color::rgb(0.1, 0.1, 0.1).into(), ..default() 
        });
        
        // 스탯 텍스트
        parent.spawn(TextBundle::from_section("Player Stats", TextStyle { font: stat_font.clone(), font_size: 24.0, color: Color::WHITE }));
        parent.spawn(TextBundle::from_section("HP: 100/100", TextStyle { font: stat_font.clone(), font_size: 18.0, color: Color::GREEN }).with_style(Style { margin: UiRect::top(Val::Px(10.0)), ..default() }));
    });

    // --- 하단 대화창 패널 구성 ---
    commands.spawn(NodeBundle { 
        style: Style { 
            width: Val::Px(map_width as f32 * tile_size), height: Val::Px(DIALOG_PANEL_HEIGHT_PX), 
            position_type: PositionType::Absolute, left: Val::Px(0.0), bottom: Val::Px(0.0), 
            padding: UiRect::all(Val::Px(10.0)), overflow: Overflow::clip_y(), flex_wrap: FlexWrap::Wrap, ..default() 
        }, 
        background_color: DIALOG_PANEL_BG_COLOR.into(), 
        ..default() 
    }).with_children(|parent| {
        parent.spawn((TextBundle::from_section("", TextStyle { font: dialog_font.clone(), font_size: 16.0, color: Color::WHITE }), DialogText));
    });
}

/// LogMessage 이벤트를 감시하여 대화창 UI의 텍스트 내용을 갱신합니다.
fn update_dialog_box(mut log_events: EventReader<LogMessage>, mut message_log: ResMut<MessageLog>, mut q_dialog_text: Query<&mut Text, With<DialogText>>) {
    if !log_events.is_empty() {
        let mut text = q_dialog_text.single_mut();
        // 새 메시지 추가 및 개수 제한
        for LogMessage(message) in log_events.read() { message_log.0.push(message.clone()); }
        while message_log.0.len() > MAX_LOG_LINES { message_log.0.remove(0); }
        // 텍스트 출력
        text.sections[0].value = message_log.0.join("\n");
    }
}
