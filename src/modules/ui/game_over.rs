use bevy::{app::AppExit, ecs::system::SystemParam, prelude::*};

use crate::modules::{
    combat::{CombatStats, Defeated},
    combat_feedback::BloodStain,
    item::{EquipmentPanelOpen, Item, PlayerEquipment, PlayerInventory},
    map::{ApplyMapEvent, GlobalSeed, GlobalTurn, Map, MapGeneratorRegistry, MAP_HEIGHT, MAP_WIDTH},
    monster::Monster,
    player::{
        MoveHoldState, MovingTo, Player, PlayerPath, PlayerProgress, PLAYER_ATK, PLAYER_DEF, PLAYER_HP, PLAYER_MP,
    },
    quest::QuestState,
    save::delete_save,
    ui::{
        equipment::EquipmentUiState,
        quest_panel::QuestPanelOpen,
        shop::{ShopPanelOpen, ShopUiState},
        MessageLog,
    },
    villager::Villager,
    zone::{zone_seed, NamedZoneConfig, WorldState, ZoneId, ZonePersistence, ZonePortal},
};

const OVERLAY_Z: i32 = 500;
const PANEL_WIDTH: f32 = 520.0;

#[derive(Component)]
struct GameOverOverlay;

#[derive(Component)]
struct GameOverText;

/// Game Over 화면과 사망 후 새 게임 입력을 담당하는 UI 플러그인이다.
pub struct GameOverPlugin;

impl Plugin for GameOverPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_game_over_overlay)
            .add_systems(
                Update,
                (
                    update_game_over_overlay,
                    handle_game_over_exit,
                    handle_new_game_input,
                ),
            );
    }
}

/// 시작 시 숨겨진 Game Over 오버레이 UI를 생성한다.
///
/// 플레이어가 사망하기 전까지는 `Visibility::Hidden`으로 유지하고,
/// 사망 이후에는 같은 엔티티를 재사용해 현재 존과 생존 턴만 갱신한다.
fn setup_game_over_overlay(mut commands: Commands, asset_server: Res<AssetServer>) {
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
                background_color: Color::rgba(0.0, 0.0, 0.0, 0.72).into(),
                z_index: ZIndex::Global(OVERLAY_Z),
                visibility: Visibility::Hidden,
                ..default()
            },
            GameOverOverlay,
        ))
        .with_children(|parent| {
            parent
                .spawn((NodeBundle {
                    style: Style {
                        width: Val::Px(PANEL_WIDTH),
                        padding: UiRect::all(Val::Px(24.0)),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    background_color: Color::rgba(0.02, 0.02, 0.02, 0.94).into(),
                    border_color: Color::rgba(0.85, 0.15, 0.15, 1.0).into(),
                    ..default()
                },))
                .with_children(|panel| {
                    panel.spawn((
                        TextBundle::from_sections(game_over_sections(0, "마을", &font)),
                        GameOverText,
                    ));
                });
        });
}

/// 플레이어의 `Defeated` 상태에 맞춰 Game Over 오버레이를 표시하고 내용을 갱신한다.
///
/// 사망 직후뿐 아니라 오버레이가 떠 있는 동안에도 현재 `WorldState`와 `GlobalTurn`을
/// 읽어 화면의 run summary가 리소스 상태와 어긋나지 않게 한다.
fn update_game_over_overlay(
    defeated_q: Query<(), With<Defeated>>,
    world: Res<WorldState>,
    turn: Res<GlobalTurn>,
    asset_server: Res<AssetServer>,
    mut overlay_q: Query<&mut Visibility, With<GameOverOverlay>>,
    mut text_q: Query<&mut Text, With<GameOverText>>,
) {
    let defeated = !defeated_q.is_empty();
    if let Ok(mut visibility) = overlay_q.get_single_mut() {
        *visibility = if defeated {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if defeated {
        let font = asset_server.load("fonts/NanumSquareNeo-bRg.ttf");
        if let Ok(mut text) = text_q.get_single_mut() {
            text.sections = game_over_sections(turn.0, &world.current.display_name(), &font);
        }
    }
}

/// 새 게임 시작에 필요한 리소스와 쿼리를 하나로 묶는다.
///
/// Bevy 시스템 함수가 직접 받기에는 리소스 수가 많기 때문에 `SystemParam`으로 묶어
/// `handle_new_game_input`의 시스템 파라미터 수를 줄이고, 초기화 범위를 한눈에 보이게 한다.
#[derive(SystemParam)]
struct NewGameParams<'w, 's> {
    commands: Commands<'w, 's>,
    registry: Res<'w, MapGeneratorRegistry>,
    apply_ev: EventWriter<'w, ApplyMapEvent>,
    global_seed: ResMut<'w, GlobalSeed>,
    global_turn: ResMut<'w, GlobalTurn>,
    world: ResMut<'w, WorldState>,
    persistence: ResMut<'w, ZonePersistence>,
    named_zones: ResMut<'w, NamedZoneConfig>,
    quest_state: ResMut<'w, QuestState>,
    inventory: ResMut<'w, PlayerInventory>,
    equipment: ResMut<'w, PlayerEquipment>,
    progress: ResMut<'w, PlayerProgress>,
    message_log: ResMut<'w, MessageLog>,
    equipment_open: ResMut<'w, EquipmentPanelOpen>,
    equipment_ui: ResMut<'w, EquipmentUiState>,
    quest_panel_open: ResMut<'w, QuestPanelOpen>,
    shop_open: ResMut<'w, ShopPanelOpen>,
    shop_ui: ResMut<'w, ShopUiState>,
    move_hold: ResMut<'w, MoveHoldState>,
    player_path: ResMut<'w, PlayerPath>,
    player_q: Query<'w, 's, (Entity, &'static mut CombatStats), With<Player>>,
    item_q: Query<'w, 's, Entity, With<Item>>,
    monster_q: Query<'w, 's, Entity, With<Monster>>,
    villager_q: Query<'w, 's, Entity, With<Villager>>,
    portal_q: Query<'w, 's, Entity, With<ZonePortal>>,
    blood_q: Query<'w, 's, Entity, With<BloodStain>>,
    item_registry: Res<'w, crate::modules::item::ItemRegistry>,
    start_loadout: Res<'w, crate::modules::item::StartLoadoutRegistry>,
    ranged: ResMut<'w, crate::modules::ranged::RangedTargeting>,
    ranged_cursor_q: Query<'w, 's, Entity, With<crate::modules::ranged::RangedCursor>>,
}

/// Game Over 상태에서 `Esc` 입력을 게임 종료 이벤트로 변환한다.
///
/// 사망하지 않은 상태에서는 일반 플레이 중 `Esc` 입력과 충돌하지 않도록 아무 작업도 하지 않는다.
fn handle_game_over_exit(
    keyboard: Res<ButtonInput<KeyCode>>,
    defeated_q: Query<(), With<Defeated>>,
    mut exit: EventWriter<AppExit>,
) {
    if defeated_q.is_empty() {
        return;
    }
    if keyboard.just_pressed(KeyCode::Escape) {
        exit.send(AppExit);
    }
}

/// Game Over 상태에서 새 게임 시작 입력을 처리한다.
///
/// `R`과 `N`은 현재 동일하게 동작한다. 둘 다 기존 세이브를 삭제하고 모든 런타임 상태를
/// 초기화한 뒤 새 Town 맵을 적용한다.
fn handle_new_game_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    defeated_q: Query<(), With<Defeated>>,
    mut params: NewGameParams,
) {
    if defeated_q.is_empty() {
        return;
    }
    if keyboard.just_pressed(KeyCode::KeyR) || keyboard.just_pressed(KeyCode::KeyN) {
        start_new_run(&mut params);
    }
}

/// 기존 세이브와 런타임 상태를 지우고 새 run을 시작한다.
///
/// 플레이어 컴포넌트는 재사용하되 `Defeated`/`MovingTo`를 제거하고 기본 스탯으로 되돌린다.
/// 아이템, 몬스터, 주민, 포털, 핏자국 엔티티는 맵 교체 이벤트가 다시 생성할 수 있도록 제거한다.
fn start_new_run(params: &mut NewGameParams) {
    delete_save();

    for entity in params
        .item_q
        .iter()
        .chain(params.monster_q.iter())
        .chain(params.villager_q.iter())
        .chain(params.portal_q.iter())
        .chain(params.blood_q.iter())
    {
        params.commands.entity(entity).despawn_recursive();
    }

    let seed: u64 = rand::random();
    params.global_seed.0 = seed;
    params.global_turn.0 = 0;
    *params.world = WorldState::default();
    *params.persistence = ZonePersistence::default();
    *params.named_zones = NamedZoneConfig::default();
    *params.quest_state = QuestState::default();
    *params.inventory = PlayerInventory::default();
    *params.equipment = PlayerEquipment::default();
    crate::modules::item::apply_start_loadout(
        &mut params.inventory,
        &mut params.equipment,
        &params.start_loadout.0,
        &params.item_registry,
    );
    *params.progress = PlayerProgress::default();
    params.message_log.0.clear();
    params.equipment_open.0 = false;
    params.equipment_ui.cursor = 0;
    params.quest_panel_open.0 = false;
    params.shop_open.0 = false;
    *params.shop_ui = ShopUiState::default();
    *params.move_hold = MoveHoldState::default();
    params.player_path.0.clear();
    params.ranged.active = false;
    for e in params.ranged_cursor_q.iter() {
        params.commands.entity(e).despawn();
    }

    if let Ok((player_entity, mut stats)) = params.player_q.get_single_mut() {
        stats.hp = PLAYER_HP;
        stats.max_hp = PLAYER_HP;
        stats.mp = PLAYER_MP;
        stats.max_mp = PLAYER_MP;
        stats.attack = PLAYER_ATK;
        stats.defense = PLAYER_DEF;
        params.commands.entity(player_entity).remove::<Defeated>();
        params.commands.entity(player_entity).remove::<MovingTo>();
    }

    let zone = ZoneId::Town;
    let map_seed = zone_seed(seed, &zone);
    let algo = zone.algorithm().to_string();
    let mut map = params
        .registry
        .generate_with(&algo, MAP_WIDTH, MAP_HEIGHT, map_seed)
        .unwrap_or_else(|| {
            warn!(
                "알 수 없는 맵 생성기 {} - 새 게임에 빈 맵을 생성합니다",
                algo
            );
            Map::new(MAP_WIDTH, MAP_HEIGHT)
        });
    map.seed = map_seed;
    map.algorithm = algo;

    params.apply_ev.send(ApplyMapEvent {
        map,
        spawn_pos: None,
    });
}

/// Game Over 오버레이에 표시할 텍스트 섹션을 구성한다.
///
/// UI 렌더링 없이도 테스트할 수 있도록 순수 함수로 분리해, 표시해야 할 핵심 문구와
/// 조작 안내가 빠지지 않는지 단위 테스트에서 검증한다.
fn game_over_sections(turn: u64, zone_name: &str, font: &Handle<Font>) -> Vec<TextSection> {
    vec![
        section("GAME OVER\n", font, 38.0, Color::rgb(1.0, 0.18, 0.18)),
        section("당신은 쓰러졌습니다.\n\n", font, 18.0, Color::WHITE),
        section(
            format!("마지막 위치: {}\n", zone_name),
            font,
            16.0,
            Color::rgb(0.85, 0.85, 0.85),
        ),
        section(
            format!("생존 턴: {}\n\n", turn),
            font,
            16.0,
            Color::rgb(0.85, 0.85, 0.85),
        ),
        section(
            "R / N  새 게임 시작\n",
            font,
            16.0,
            Color::rgb(0.5, 1.0, 0.5),
        ),
        section("Esc    종료", font, 16.0, Color::rgb(0.8, 0.8, 0.8)),
    ]
}

/// 동일한 폰트 핸들을 공유하는 `TextSection`을 만든다.
fn section(value: impl Into<String>, font: &Handle<Font>, size: f32, color: Color) -> TextSection {
    TextSection::new(
        value.into(),
        TextStyle {
            font: font.clone(),
            font_size: size,
            color,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_over_text_includes_actions_and_summary() {
        let sections = game_over_sections(42, "던전 1층", &Handle::default());
        let text: String = sections.iter().map(|s| s.value.as_str()).collect();
        assert!(text.contains("GAME OVER"));
        assert!(text.contains("던전 1층"));
        assert!(text.contains("42"));
        assert!(text.contains("R / N"));
        assert!(text.contains("Esc"));
    }
}
