use bevy::prelude::*;
pub mod minimap;
use crate::modules::map::{MAP_WIDTH, TILE_SIZE, MapGeneratorRegistry};

const UI_PANEL_BG_COLOR: Color = Color::rgba(0.1, 0.1, 0.1, 1.0);
const DIALOG_PANEL_BG_COLOR: Color = Color::rgba(0.1, 0.1, 0.1, 0.8);

pub const STATS_PANEL_WIDTH_PX: f32 = 200.0;
pub const DIALOG_PANEL_HEIGHT_PX: f32 = 96.0;
const MAX_LOG_LINES: usize = 5;

pub struct GameUiPlugin;

impl Plugin for GameUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(minimap::MinimapPlugin)
            .add_systems(Startup, setup_ui.after(minimap::setup_minimap))
            .add_event::<LogMessage>()
            .init_resource::<MessageLog>()
            .add_systems(Update, (update_dialog_box, update_generator_name));
    }
}

#[derive(Event)] pub struct LogMessage(pub String);
#[derive(Resource, Default)] pub struct MessageLog(pub Vec<String>);
#[derive(Component)] struct DialogText;
#[derive(Component)] struct GeneratorNameText;

fn setup_ui(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    minimap_image: Res<minimap::MinimapImage>,
    registry: Res<MapGeneratorRegistry>,
) {
    let stat_font = asset_server.load("fonts/FiraMono-Medium.ttf");
    let dialog_font = asset_server.load("fonts/NanumSquareNeo-bRg.ttf");

    // 오른쪽 스탯 패널
    commands.spawn(NodeBundle {
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
    }).with_children(|parent| {
        parent.spawn(ImageBundle {
            style: Style {
                width: Val::Px(160.0),
                height: Val::Px(160.0),
                margin: UiRect::all(Val::Px(10.0)),
                ..default()
            },
            image: minimap_image.0.clone().into(),
            background_color: Color::rgb(0.1, 0.1, 0.1).into(),
            ..default()
        });
        parent.spawn(TextBundle::from_section(
            "Player Stats",
            TextStyle { font: stat_font.clone(), font_size: 24.0, color: Color::WHITE },
        ));
        parent.spawn(TextBundle::from_section(
            "HP: 100/100",
            TextStyle { font: stat_font.clone(), font_size: 18.0, color: Color::GREEN },
        ).with_style(Style { margin: UiRect::top(Val::Px(10.0)), ..default() }));

        // 현재 맵 생성기 이름
        parent.spawn((
            TextBundle::from_section(
                registry.current_name(),
                TextStyle { font: dialog_font.clone(), font_size: 13.0, color: Color::CYAN },
            ).with_style(Style { margin: UiRect::top(Val::Px(8.0)), ..default() }),
            GeneratorNameText,
        ));
        parent.spawn(TextBundle::from_section(
            "[Tab] 맵 전환",
            TextStyle { font: dialog_font.clone(), font_size: 11.0, color: Color::GRAY },
        ).with_style(Style { margin: UiRect::top(Val::Px(2.0)), ..default() }));
    });

    // 하단 대화창
    commands.spawn(NodeBundle {
        style: Style {
            width: Val::Px(MAP_WIDTH as f32 * TILE_SIZE),
            height: Val::Px(DIALOG_PANEL_HEIGHT_PX),
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            bottom: Val::Px(0.0),
            padding: UiRect::all(Val::Px(10.0)),
            overflow: Overflow::clip_y(),
            flex_wrap: FlexWrap::Wrap,
            ..default()
        },
        background_color: DIALOG_PANEL_BG_COLOR.into(),
        ..default()
    }).with_children(|parent| {
        parent.spawn((
            TextBundle::from_section(
                "",
                TextStyle { font: dialog_font, font_size: 16.0, color: Color::WHITE },
            ),
            DialogText,
        ));
    });
}

fn update_dialog_box(
    mut log_events: EventReader<LogMessage>,
    mut message_log: ResMut<MessageLog>,
    mut q: Query<&mut Text, With<DialogText>>,
) {
    if log_events.is_empty() { return; }
    let mut text = q.single_mut();
    for LogMessage(msg) in log_events.read() { message_log.0.push(msg.clone()); }
    while message_log.0.len() > MAX_LOG_LINES { message_log.0.remove(0); }
    text.sections[0].value = message_log.0.join("\n");
}

fn update_generator_name(
    registry: Res<MapGeneratorRegistry>,
    mut q: Query<&mut Text, With<GeneratorNameText>>,
) {
    if registry.is_changed() {
        if let Ok(mut text) = q.get_single_mut() {
            text.sections[0].value = registry.current_name().to_string();
        }
    }
}
