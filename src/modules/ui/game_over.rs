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
    use crate::modules::map::MapGenerator;

    // ── 순수 텍스트 빌더 ───────────────────────────────────────────────────

    #[test]
    fn 게임오버_텍스트는_제목과_생존요약과_조작안내를_담는다() {
        let sections = game_over_sections(42, "던전 1층", &Handle::default());
        let text: String = sections.iter().map(|s| s.value.as_str()).collect();
        assert!(text.contains("GAME OVER"));
        assert!(text.contains("던전 1층"));
        assert!(text.contains("42"));
        assert!(text.contains("R / N"));
        assert!(text.contains("Esc"));
    }

    #[test]
    fn 섹션_빌더는_주어진_값과_스타일로_텍스트섹션을_만든다() {
        let section = section("안녕", &Handle::default(), 21.0, Color::WHITE);
        assert_eq!(section.value, "안녕");
        assert_eq!(section.style.font_size, 21.0);
        assert_eq!(section.style.color, Color::WHITE);
    }

    // ── 테스트용 맵 생성기 ─────────────────────────────────────────────────

    /// Town 알고리즘 이름("organic_village")으로 등록해 generate_with 가 Some 을
    /// 반환하게 만드는 가벼운 더미 생성기.
    struct 더미생성기;
    impl MapGenerator for 더미생성기 {
        fn generate(&self, width: usize, height: usize, _seed: u64) -> Map {
            Map::new(width, height)
        }
        fn name(&self) -> &str {
            "organic_village"
        }
    }

    // ── 실제 세이브 파일 보호 가드 ─────────────────────────────────────────

    /// start_new_run 은 delete_save() 로 실제 SAVE_PATH 를 지우려 한다.
    /// 테스트가 실제 save/progress.ron 을 파괴하지 않도록, 호출 전에 임시 백업으로
    /// rename 해 두고 끝나면 되돌린다. (save 모듈 테스트와 동일한 패턴)
    ///
    /// rename 은 대상이 없으면 조용히 실패(`let _`)하므로 분기 없이 무조건 시도해도 안전하다.
    struct 세이브_가드 {
        backup: String,
    }
    impl 세이브_가드 {
        fn new() -> Self {
            let path = crate::modules::save::SAVE_PATH;
            let backup = format!("{path}.game_over_test_backup");
            let _ = std::fs::rename(path, &backup); // 실제 세이브가 있으면 잠시 치워 둔다
            Self { backup }
        }
    }
    impl Drop for 세이브_가드 {
        fn drop(&mut self) {
            // 백업이 없으면 no-op. 새로 만들어진 (없던) 세이브는 남기지 않도록 우선 삭제 후 복원.
            let _ = std::fs::remove_file(crate::modules::save::SAVE_PATH);
            let _ = std::fs::rename(&self.backup, crate::modules::save::SAVE_PATH);
        }
    }

    // ── App 하네스 ─────────────────────────────────────────────────────────

    /// AssetServer(폰트 로드)가 필요한 오버레이 렌더 시스템용 App 하네스.
    fn 렌더_하네스() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app
    }

    /// 키 입력으로 종료/새 게임을 다루는 시스템용 App 하네스.
    fn 입력_하네스() -> App {
        let mut app = App::new();
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app
    }

    /// handle_new_game_input / start_new_run 이 요구하는 모든 리소스를 기본값으로 채운다.
    fn 새게임_리소스_삽입(app: &mut App, registry: MapGeneratorRegistry) {
        app.add_event::<ApplyMapEvent>();
        app.insert_resource(registry);
        app.insert_resource(GlobalSeed(123));
        app.insert_resource(GlobalTurn(7));
        app.insert_resource(WorldState::default());
        app.insert_resource(ZonePersistence::default());
        app.insert_resource(NamedZoneConfig::default());
        app.insert_resource(QuestState::default());
        app.insert_resource(PlayerInventory::default());
        app.insert_resource(PlayerEquipment::default());
        app.insert_resource(PlayerProgress::default());
        app.insert_resource(MessageLog::default());
        app.insert_resource(EquipmentPanelOpen(false));
        app.insert_resource(EquipmentUiState::default());
        app.insert_resource(QuestPanelOpen(false));
        app.insert_resource(ShopPanelOpen(false));
        app.insert_resource(ShopUiState::default());
        app.insert_resource(MoveHoldState::default());
        app.insert_resource(PlayerPath::default());
        app.insert_resource(crate::modules::item::build_test_registry());
        app.insert_resource(crate::modules::item::StartLoadoutRegistry::default());
        app.insert_resource(crate::modules::ranged::RangedTargeting::default());
    }

    // ── setup_game_over_overlay ────────────────────────────────────────────

    #[test]
    fn 시작시_숨김상태의_게임오버_오버레이와_텍스트가_생성된다() {
        let mut app = 렌더_하네스();
        app.add_systems(Startup, setup_game_over_overlay);
        app.update();

        let mut q = app.world.query_filtered::<&Visibility, With<GameOverOverlay>>();
        assert_eq!(*q.single(&app.world), Visibility::Hidden);
        assert_eq!(app.world.query::<&GameOverText>().iter(&app.world).count(), 1);
    }

    // ── update_game_over_overlay ───────────────────────────────────────────

    #[test]
    fn 플레이어가_쓰러지면_오버레이가_보이고_텍스트가_현재상태로_갱신된다() {
        let mut app = 렌더_하네스();
        app.insert_resource(WorldState::default());
        app.insert_resource(GlobalTurn(99));
        app.add_systems(Startup, setup_game_over_overlay);
        app.add_systems(Update, update_game_over_overlay);
        app.update(); // setup

        app.world.spawn((Player, Defeated));
        app.update();

        let mut vq = app.world.query_filtered::<&Visibility, With<GameOverOverlay>>();
        assert_eq!(*vq.single(&app.world), Visibility::Inherited);
        let mut tq = app.world.query_filtered::<&Text, With<GameOverText>>();
        let text: String = tq.single(&app.world).sections.iter().map(|s| s.value.as_str()).collect();
        assert!(text.contains("GAME OVER"));
        assert!(text.contains("마을")); // 기본 WorldState 의 현재 존
        assert!(text.contains("99")); // 생존 턴
    }

    #[test]
    fn 오버레이와_텍스트_엔티티가_없으면_갱신은_조용히_넘어간다() {
        let mut app = 렌더_하네스();
        app.insert_resource(WorldState::default());
        app.insert_resource(GlobalTurn(0));
        // setup 을 돌리지 않아 GameOverOverlay / GameOverText 엔티티가 없다.
        app.world.spawn((Player, Defeated)); // defeated == true 로 두 get_single_mut 의 Err 분기를 모두 탄다.
        app.add_systems(Update, update_game_over_overlay);
        app.update(); // 패닉 없이 통과해야 한다.

        assert_eq!(app.world.query::<&GameOverOverlay>().iter(&app.world).count(), 0);
        assert_eq!(app.world.query::<&GameOverText>().iter(&app.world).count(), 0);
    }

    #[test]
    fn 쓰러지지_않은_상태면_오버레이는_숨김으로_유지된다() {
        let mut app = 렌더_하네스();
        app.insert_resource(WorldState::default());
        app.insert_resource(GlobalTurn(0));
        app.add_systems(Startup, setup_game_over_overlay);
        app.add_systems(Update, update_game_over_overlay);
        app.update(); // setup + 갱신: defeated 없음 → Hidden

        let mut vq = app.world.query_filtered::<&Visibility, With<GameOverOverlay>>();
        assert_eq!(*vq.single(&app.world), Visibility::Hidden);
    }

    // ── handle_game_over_exit ──────────────────────────────────────────────

    #[test]
    fn 쓰러진_상태에서_Esc를_누르면_종료이벤트가_발행된다() {
        let mut app = 입력_하네스();
        app.add_event::<AppExit>();
        app.world.spawn(Defeated);
        app.add_systems(Update, handle_game_over_exit);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
        app.update();

        let events = app.world.resource::<Events<AppExit>>();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn 쓰러진_상태라도_Esc를_누르지_않으면_종료되지_않는다() {
        let mut app = 입력_하네스();
        app.add_event::<AppExit>();
        app.world.spawn(Defeated);
        app.add_systems(Update, handle_game_over_exit);
        app.update();

        let events = app.world.resource::<Events<AppExit>>();
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn 쓰러지지_않은_상태에서는_Esc를_눌러도_종료되지_않는다() {
        let mut app = 입력_하네스();
        app.add_event::<AppExit>();
        // Defeated 엔티티 없음 → early return.
        app.add_systems(Update, handle_game_over_exit);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
        app.update();

        let events = app.world.resource::<Events<AppExit>>();
        assert_eq!(events.len(), 0);
    }

    // ── handle_new_game_input + start_new_run ──────────────────────────────

    #[test]
    fn 쓰러진_상태에서_R을_누르면_새_게임이_시작되어_상태가_초기화된다() {
        let _가드 = 세이브_가드::new();
        let mut app = 입력_하네스();
        let mut registry = MapGeneratorRegistry::new();
        registry.register(Box::new(더미생성기));
        새게임_리소스_삽입(&mut app, registry);
        // 초기화 대상 상태를 일부러 오염시킨다.
        app.world.resource_mut::<GlobalTurn>().0 = 50;
        app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        app.world.resource_mut::<MessageLog>().0.push("죽었다".into());
        let player = app
            .world
            .spawn((
                Player,
                Defeated,
                CombatStats { hp: 0, max_hp: 30, mp: 0, max_mp: 10, attack: 1, defense: 0 },
            ))
            .id();

        app.add_systems(Update, handle_new_game_input);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyR);
        app.update();

        // 상태 초기화 확인
        assert_eq!(app.world.resource::<GlobalTurn>().0, 0);
        assert!(!app.world.resource::<EquipmentPanelOpen>().0);
        assert!(app.world.resource::<MessageLog>().0.is_empty());
        // 플레이어 스탯 복구 + Defeated 제거
        let stats = app.world.get::<CombatStats>(player).unwrap();
        assert_eq!(stats.hp, PLAYER_HP);
        assert_eq!(stats.max_hp, PLAYER_HP);
        assert!(!app.world.entity(player).contains::<Defeated>());
        // 맵 적용 이벤트 발행 (생성기가 organic_village 를 알기 때문에 Some 경로)
        let events = app.world.resource::<Events<ApplyMapEvent>>();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn 쓰러진_상태에서_N을_눌러도_R과_동일하게_새_게임이_시작된다() {
        let _가드 = 세이브_가드::new();
        let mut app = 입력_하네스();
        let mut registry = MapGeneratorRegistry::new();
        registry.register(Box::new(더미생성기));
        새게임_리소스_삽입(&mut app, registry);
        app.world.spawn((
            Player,
            Defeated,
            CombatStats { hp: 0, max_hp: 30, mp: 0, max_mp: 10, attack: 1, defense: 0 },
        ));

        app.add_systems(Update, handle_new_game_input);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyN);
        app.update();

        let events = app.world.resource::<Events<ApplyMapEvent>>();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn 새_게임은_몬스터_핏자국_원격커서_등_월드_엔티티를_제거한다() {
        let _가드 = 세이브_가드::new();
        let mut app = 입력_하네스();
        let mut registry = MapGeneratorRegistry::new();
        registry.register(Box::new(더미생성기));
        새게임_리소스_삽입(&mut app, registry);
        app.world.spawn((Player, Defeated, CombatStats { hp: 0, max_hp: 1, mp: 0, max_mp: 1, attack: 1, defense: 0 }));
        // despawn 대상 마커가 붙은 엔티티들 (필드 의미는 무관, 마커 존재만 중요).
        let monster = app
            .world
            .spawn(Monster {
                name: "슬라임".into(),
                tile_x: 0,
                tile_y: 0,
                vision_radius: 0,
                alert_turns: 0,
                slot_idx: 0,
            })
            .id();
        let blood = app.world.spawn(BloodStain { alpha: 1.0, decay_per_turn: 0.1 }).id();
        let cursor = app.world.spawn(crate::modules::ranged::RangedCursor).id();
        // 원격 타게팅이 켜져 있던 상태를 끄는지 확인
        app.world.resource_mut::<crate::modules::ranged::RangedTargeting>().active = true;

        app.add_systems(Update, handle_new_game_input);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyR);
        app.update();

        for e in [monster, blood, cursor] {
            assert!(app.world.get_entity(e).is_none(), "엔티티 {e:?} 가 제거되지 않았다");
        }
        assert!(!app.world.resource::<crate::modules::ranged::RangedTargeting>().active);
    }

    #[test]
    fn 생성기가_없으면_새_게임은_빈_맵으로_폴백한다() {
        let _가드 = 세이브_가드::new();
        let mut app = 입력_하네스();
        // 빈 레지스트리 → generate_with 가 None → unwrap_or_else 의 폴백 분기.
        새게임_리소스_삽입(&mut app, MapGeneratorRegistry::new());
        app.world.spawn((Player, Defeated, CombatStats { hp: 0, max_hp: 1, mp: 0, max_mp: 1, attack: 1, defense: 0 }));

        app.add_systems(Update, handle_new_game_input);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyR);
        app.update();

        // 폴백이어도 맵 적용 이벤트는 발행된다.
        let events = app.world.resource::<Events<ApplyMapEvent>>();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn 플레이어_엔티티가_없어도_새_게임은_패닉없이_맵을_적용한다() {
        let _가드 = 세이브_가드::new();
        let mut app = 입력_하네스();
        let mut registry = MapGeneratorRegistry::new();
        registry.register(Box::new(더미생성기));
        새게임_리소스_삽입(&mut app, registry);
        // Player+CombatStats 엔티티 없이 Defeated 만 있는 엔티티로 진입 조건만 만족시킨다.
        app.world.spawn(Defeated);

        app.add_systems(Update, handle_new_game_input);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyR);
        app.update(); // get_single_mut 실패 분기 → 스탯 복구 건너뜀

        let events = app.world.resource::<Events<ApplyMapEvent>>();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn 쓰러진_상태라도_R이나_N이_아니면_새_게임은_시작되지_않는다() {
        let _가드 = 세이브_가드::new();
        let mut app = 입력_하네스();
        let mut registry = MapGeneratorRegistry::new();
        registry.register(Box::new(더미생성기));
        새게임_리소스_삽입(&mut app, registry);
        app.world.resource_mut::<GlobalTurn>().0 = 50;
        app.world.spawn((Player, Defeated, CombatStats { hp: 0, max_hp: 1, mp: 0, max_mp: 1, attack: 1, defense: 0 }));

        app.add_systems(Update, handle_new_game_input);
        // R/N 이 아닌 다른 키.
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Space);
        app.update();

        assert_eq!(app.world.resource::<GlobalTurn>().0, 50); // 초기화되지 않음
        let events = app.world.resource::<Events<ApplyMapEvent>>();
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn 쓰러지지_않은_상태에서는_R을_눌러도_새_게임이_시작되지_않는다() {
        let mut app = 입력_하네스();
        let mut registry = MapGeneratorRegistry::new();
        registry.register(Box::new(더미생성기));
        새게임_리소스_삽입(&mut app, registry);
        app.world.resource_mut::<GlobalTurn>().0 = 50;
        // Defeated 엔티티 없음 → early return (delete_save 호출 전이라 가드 불필요).

        app.add_systems(Update, handle_new_game_input);
        app.world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyR);
        app.update();

        assert_eq!(app.world.resource::<GlobalTurn>().0, 50);
        let events = app.world.resource::<Events<ApplyMapEvent>>();
        assert_eq!(events.len(), 0);
    }

    // ── 플러그인 build ─────────────────────────────────────────────────────

    #[test]
    fn 플러그인을_추가하면_게임오버_시스템들이_등록된다() {
        let mut app = 렌더_하네스();
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app.insert_resource(WorldState::default());
        app.insert_resource(GlobalTurn(0));
        app.add_event::<AppExit>();
        새게임_리소스_삽입(&mut app, MapGeneratorRegistry::new());
        app.add_plugins(GameOverPlugin);
        app.update(); // build() + 시작 시스템 실행

        assert_eq!(app.world.query::<&GameOverOverlay>().iter(&app.world).count(), 1);
    }
}
