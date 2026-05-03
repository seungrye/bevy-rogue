use bevy::prelude::*;
pub mod minimap;
use crate::modules::map::{MAP_WIDTH, TILE_SIZE};

const DIALOG_PANEL_BG_COLOR: Color = Color::rgba(0.1, 0.1, 0.1, 0.8);

pub const DIALOG_PANEL_HEIGHT_PX: f32 = 96.0;
const MAX_LOG_LINES: usize = 5;

pub struct GameUiPlugin;

impl Plugin for GameUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(minimap::MinimapPlugin)
            .add_systems(Startup, setup_ui)
            .add_event::<LogMessage>()
            .init_resource::<MessageLog>()
            .add_systems(Update, update_dialog_box);
    }
}

#[derive(Event)] pub struct LogMessage(pub String);
#[derive(Resource, Default)] pub struct MessageLog(pub Vec<String>);
#[derive(Component)] struct DialogText;

fn setup_ui(mut commands: Commands, asset_server: Res<AssetServer>) {
    let dialog_font = asset_server.load("fonts/NanumSquareNeo-bRg.ttf");

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

