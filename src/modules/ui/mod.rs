use bevy::prelude::*;
pub mod equipment;
pub mod game_over;
pub mod help;
pub mod hud;
pub mod minimap;
pub mod quest_panel;
pub mod shop;
use crate::modules::map::{MAP_WIDTH, TILE_SIZE};

const DIALOG_PANEL_BG_COLOR: Color = Color::rgba(0.1, 0.1, 0.1, 0.8);

pub const DIALOG_PANEL_HEIGHT_PX: f32 = 96.0;
const MAX_LOG_LINES: usize = 5;

pub struct GameUiPlugin;

impl Plugin for GameUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(minimap::MinimapPlugin)
            .add_plugins(equipment::EquipmentPlugin)
            .add_plugins(game_over::GameOverPlugin)
            .add_plugins(help::HelpPlugin)
            .add_plugins(hud::StatusHudPlugin)
            .add_plugins(quest_panel::QuestPanelPlugin)
            .add_plugins(shop::ShopPlugin)
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

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;

    /// AssetServer 가 필요한 UI 렌더 시스템용 App 하네스.
    fn 렌더_하네스() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app.init_asset::<Image>();
        app
    }

    /// 대화창 텍스트 갱신 시스템용 App — DialogText 엔티티 포함.
    fn 대화창_하네스() -> App {
        let mut app = App::new();
        app.add_event::<LogMessage>();
        app.init_resource::<MessageLog>();
        app.world.spawn((
            TextBundle::from_section("", TextStyle::default()),
            DialogText,
        ));
        app.add_systems(Update, update_dialog_box);
        app
    }

    fn 대화창_텍스트(app: &mut App) -> String {
        let mut q = app.world.query_filtered::<&Text, With<DialogText>>();
        q.single(&app.world).sections[0].value.clone()
    }

    #[test]
    fn 하단_대화창_셋업은_대화텍스트_노드를_생성한다() {
        let mut app = 렌더_하네스();
        app.add_systems(Startup, setup_ui);
        app.update();
        assert_eq!(
            app.world.query::<&DialogText>().iter(&app.world).count(),
            1,
            "대화창 텍스트 엔티티가 하나 있어야 한다"
        );
    }

    #[test]
    fn 로그메시지가_오면_대화창에_표시된다() {
        let mut app = 대화창_하네스();
        app.world.send_event(LogMessage("첫 메시지".into()));
        app.update();
        assert_eq!(대화창_텍스트(&mut app), "첫 메시지");
        assert_eq!(app.world.resource::<MessageLog>().0.len(), 1);
    }

    #[test]
    fn 로그이벤트가_없으면_대화창을_갱신하지_않는다() {
        let mut app = 대화창_하네스();
        // 미리 표식값을 넣어두고, 이벤트 없이 update → 시스템이 일찍 return.
        {
            let mut q = app.world.query_filtered::<&mut Text, With<DialogText>>();
            q.single_mut(&mut app.world).sections[0].value = "표식".into();
        }
        app.update();
        assert_eq!(대화창_텍스트(&mut app), "표식", "이벤트가 없으면 텍스트가 유지돼야 한다");
    }

    #[test]
    fn 로그가_최대줄수를_넘으면_가장_오래된_줄부터_버려진다() {
        let mut app = 대화창_하네스();
        // MAX_LOG_LINES(5) 보다 많은 메시지를 보낸다.
        for i in 0..(MAX_LOG_LINES + 2) {
            app.world.send_event(LogMessage(format!("줄{i}")));
        }
        app.update();
        let joined = {
            let log = &app.world.resource::<MessageLog>().0;
            assert_eq!(log.len(), MAX_LOG_LINES, "로그는 최대 줄 수로 잘려야 한다");
            assert_eq!(log.first().unwrap(), "줄2", "가장 오래된 두 줄이 제거돼야 한다");
            assert_eq!(log.last().unwrap(), &format!("줄{}", MAX_LOG_LINES + 1));
            log.join("\n")
        };
        assert_eq!(대화창_텍스트(&mut app), joined);
    }

    #[test]
    fn 게임UI_플러그인이_정상적으로_빌드된다() {
        // bare App 에 GameUiPlugin 을 등록만 한다 (update 하지 않음).
        // 하위 UI 플러그인들이 등록 단계에서 의존 리소스를 즉시 접근하지 않으므로
        // 패닉 없이 시스템·리소스·이벤트가 등록돼야 한다.
        let mut app = App::new();
        app.add_plugins(GameUiPlugin);
        assert!(app.world.get_resource::<MessageLog>().is_some());
    }
}

