//! 게임의 주요 UI(사용자 정보, 메시지 로그 등)를 담당하는 모듈입니다.

use bevy::prelude::*;

use crate::modules;

pub mod minimap;

// UI 레이아웃을 위한 상수들
const UI_PANEL_BG_COLOR: Color = Color::rgba(0.1, 0.1, 0.1, 1.0);
const DIALOG_PANEL_BG_COLOR: Color = Color::rgba(0.1, 0.1, 0.1, 0.8);

pub const STATS_PANEL_WIDTH_PX: f32 = 200.0;
pub const DIALOG_PANEL_HEIGHT_PX: f32 = 96.0;
const MAX_LOG_LINES: usize = 5;

/// UI 관련 컴포넌트와 시스템을 등록하는 플러그인입니다.
pub struct GameUiPlugin;

impl Plugin for GameUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(minimap::MinimapPlugin)
            .add_systems(Startup, setup_ui.after(minimap::setup_minimap))
            .add_event::<LogMessage>()
            .init_resource::<MessageLog>()
            .add_systems(Update, update_dialog_box);
    }
}

/// 메시지 로그에 새로운 메시지를 추가하기 위한 이벤트입니다.
#[derive(Event)]
pub struct LogMessage(pub String);

/// 화면에 표시될 메시지들을 저장하는 리소스입니다.
#[derive(Resource, Default)]
pub struct MessageLog(pub Vec<String>);

/// 다이얼로그 박스의 텍스트를 식별하기 위한 컴포넌트입니다.
#[derive(Component)]
struct DialogText;

/// 게임 시작 시 기본 UI 레이아웃을 생성하는 시스템입니다.
fn setup_ui(mut commands: Commands, asset_server: Res<AssetServer>, minimap_image: Res<minimap::MinimapImage>) {
    let stat_font = load_font_asset(&asset_server, "fonts/FiraMono-Medium.ttf");
    let dialog_font = load_font_asset(&asset_server, "fonts/NanumSquareNeo-bRg.ttf");

    let map_width = modules::map::MAP_WIDTH;
    let tile_size = modules::map::TILE_SIZE;

    // --- 우측 스탯 패널 ---
    commands
        .spawn(NodeBundle {
            style: Style {
                width: Val::Px(STATS_PANEL_WIDTH_PX),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                ..default()
            },
            background_color: UI_PANEL_BG_COLOR.into(),
            ..default()
        })
        .with_children(|parent| {
            // --- 미니맵 표시 ---
            parent.spawn(ImageBundle {
                style: Style {
                    width: Val::Px(160.0),
                    height: Val::Px(100.0),
                    margin: UiRect::all(Val::Px(10.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                image: minimap_image.0.clone().into(),
                background_color: Color::rgb(0.1, 0.1, 0.1).into(), // 테두리 색상 역할
                ..default()
            });

            parent.spawn(TextBundle::from_section(
                "Player Stats",
                TextStyle {
                    font: stat_font.clone(),
                    font_size: 24.0,
                    color: Color::WHITE,
                },
            ));
            // TODO: 여기에 실제 플레이어 스탯을 표시할 TextBundle들을 추가하고, 업데이트하는 시스템을 만드세요.
            // 예: 체력, 공격력 등
            parent.spawn(
                TextBundle::from_section(
                    "HP: 100/100",
                    TextStyle {
                        font: stat_font.clone(),
                        font_size: 18.0,
                        color: Color::GREEN,
                    },
                )
                .with_style(Style {
                    margin: UiRect::top(Val::Px(10.0)),
                    ..default()
                }),
            );
        });

    // --- 하단 다이얼로그 패널 ---
    commands
        .spawn(NodeBundle {
            style: Style {
                width: Val::Px(map_width as f32 * tile_size),
                height: Val::Px(DIALOG_PANEL_HEIGHT_PX),
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                bottom: Val::Px(0.0),
                padding: UiRect::all(Val::Px(10.0)),
                overflow: Overflow::clip_y(), // Y축을 벗어나는 콘텐츠를 숨깁니다.
                flex_wrap: FlexWrap::Wrap, // 텍스트가 길어지면 다음 줄로 넘어가도록 설정
                ..default()
            },
            background_color: DIALOG_PANEL_BG_COLOR.into(),
            ..default()
        })
        .with_children(|parent| {
            parent.spawn((
                TextBundle::from_section(
                    "", // 처음에는 비어있음
                    TextStyle {
                        font: dialog_font.clone(),
                        font_size: 16.0,
                        color: Color::WHITE,
                    },
                ),
                DialogText,
            ));
        });

    info!("Game UI initialized.");
}

fn load_font_asset(asset_server: &Res<'_, AssetServer>, ttf:&'static str) -> Handle<Font> {
    std::path::Path::new(ttf).try_exists().unwrap_or_else(|_| panic!("{} 파일이 존재하지 않습니다.", ttf));
    asset_server.load(ttf)
}

/// `LogMessage` 이벤트를 수신하여 다이얼로그 박스를 업데이트하는 시스템입니다.
fn update_dialog_box(
    mut log_events: EventReader<LogMessage>,
    mut message_log: ResMut<MessageLog>,
    mut q_dialog_text: Query<&mut Text, With<DialogText>>,
) {
    // 이벤트가 있을 때만 실행하여 불필요한 작업을 줄입니다.
    if !log_events.is_empty() {
        let mut text = q_dialog_text.single_mut();
        for LogMessage(message) in log_events.read() {
            message_log.0.push(message.clone());
        }

        // 로그가 최대 줄 수를 초과하면 오래된 로그를 제거합니다.
        while message_log.0.len() > MAX_LOG_LINES {
            message_log.0.remove(0);
        }

        // Text 컴포넌트를 업데이트합니다.
        text.sections[0].value = message_log.0.join("\n");
    }
}
