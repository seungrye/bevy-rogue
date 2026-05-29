use bevy::prelude::*;

use crate::modules::{
    combat::CombatStats,
    item::{PlayerEquipment, PlayerInventory},
    map::{GlobalTurn, MapResource, MAP_WIDTH, TILE_SIZE},
    player::{Player, PlayerProgress},
    zone::WorldState,
};

const HUD_HEIGHT: f32 = 28.0;
const HUD_FONT_SIZE: f32 = 13.0;
const HUD_Z: i32 = 40;

#[derive(Component)]
struct StatusHud;

#[derive(Component)]
struct StatusHudText;

/// 항상 보이는 상단 상태 요약 HUD를 담당하는 플러그인이다.
pub struct StatusHudPlugin;

impl Plugin for StatusHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_status_hud)
            .add_systems(Update, update_status_hud);
    }
}

/// 시작 시 상단 HUD 컨테이너와 텍스트 엔티티를 생성한다.
///
/// 장비/퀘스트/상점 패널은 z-index 100 이상을 사용하므로, HUD는 그보다 낮은 z-index를 둬
/// 패널이 열렸을 때 자연스럽게 뒤로 깔리도록 한다.
fn setup_status_hud(mut commands: Commands, asset_server: Res<AssetServer>) {
    let font = asset_server.load("fonts/NanumSquareNeo-bRg.ttf");
    commands
        .spawn((
            NodeBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(MAP_WIDTH as f32 * TILE_SIZE),
                    height: Val::Px(HUD_HEIGHT),
                    padding: UiRect::horizontal(Val::Px(8.0)),
                    align_items: AlignItems::Center,
                    ..default()
                },
                background_color: Color::rgba(0.0, 0.0, 0.0, 0.62).into(),
                z_index: ZIndex::Global(HUD_Z),
                ..default()
            },
            StatusHud,
        ))
        .with_children(|parent| {
            parent.spawn((
                TextBundle::from_section(
                    "",
                    TextStyle {
                        font,
                        font_size: HUD_FONT_SIZE,
                        color: Color::rgb(0.86, 0.95, 0.86),
                    },
                ),
                StatusHudText,
            ));
        });
}

/// 현재 플레이 상태가 바뀌면 HUD 문자열을 갱신한다.
///
/// HUD는 플레이 판단에 필요한 요약 정보만 다룬다. 상세 인벤토리나 퀘스트 내용은 기존 패널에 맡기고,
/// 이 시스템은 현재 존/턴/전투 스탯/골드/장비/맵 생성기처럼 매 턴 자주 확인하는 정보만 압축한다.
fn update_status_hud(
    world: Res<WorldState>,
    turn: Res<GlobalTurn>,
    map_res: Res<MapResource>,
    inventory: Res<PlayerInventory>,
    equipment: Res<PlayerEquipment>,
    progress: Res<PlayerProgress>,
    items: Res<crate::modules::item::ItemRegistry>,
    player_q: Query<Ref<CombatStats>, With<Player>>,
    mut text_q: Query<&mut Text, With<StatusHudText>>,
) {
    let Ok(stats) = player_q.get_single() else { return; };
    if !world.is_changed()
        && !turn.is_changed()
        && !map_res.is_changed()
        && !inventory.is_changed()
        && !equipment.is_changed()
        && !progress.is_changed()
        && !stats.is_changed()
    {
        return;
    }

    let Ok(mut text) = text_q.get_single_mut() else { return; };
    text.sections[0].value = status_hud_text(&world, &turn, map_res.map(), &inventory, &equipment, &progress, &stats, &items);
}

/// 상단 HUD에 표시할 한 줄 상태 문자열을 만든다.
///
/// UI 시스템 없이도 테스트할 수 있게 순수 함수로 분리한다. 숫자와 장비 이름이 한 줄에 모이므로,
/// 게임을 플레이하지 않아도 기본 표시 내용 누락을 단위 테스트에서 잡을 수 있다.
fn status_hud_text(
    world: &WorldState,
    turn: &GlobalTurn,
    map: &crate::modules::map::Map,
    inventory: &PlayerInventory,
    equipment: &PlayerEquipment,
    progress: &PlayerProgress,
    stats: &CombatStats,
    items: &crate::modules::item::ItemRegistry,
) -> String {
    let weapon = equipment.weapon.map(|w| w.display_name(items)).unwrap_or("맨손");
    let armor = equipment.armor.map(|a| a.display_name(items)).unwrap_or("방어구 없음");
    let algorithm = if map.algorithm.is_empty() { "unknown" } else { &map.algorithm };
    format!(
        "{} | Turn {} | Lv.{} XP {}/{} | HP {}/{} MP {}/{} | ATK {} DEF {} | {}G | {} / {} | {}",
        world.current.display_name(),
        turn.0,
        progress.level,
        progress.xp,
        progress.next_level_xp,
        stats.hp,
        stats.max_hp,
        stats.mp,
        stats.max_mp,
        stats.attack,
        stats.defense,
        inventory.gold,
        weapon,
        armor,
        algorithm,
    )
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;
    use crate::modules::{
        item::{ArmorKind, PlayerEquipment, PlayerInventory, WeaponKind},
        map::{GlobalTurn, Map, MapResource},
        zone::{WorldState, ZoneId},
    };

    // ── 순수 문자열 빌더: status_hud_text ──────────────────────────────────

    #[test]
    fn HUD_문자열은_존_턴_레벨_스탯_골드_장비를_한_줄에_담는다() {
        let mut world = WorldState::default();
        world.current = ZoneId::dungeon(2);
        let mut map = Map::new(10, 10);
        map.algorithm = "bsp".to_string();
        let inventory = PlayerInventory { gold: 75, ..Default::default() };
        let equipment = PlayerEquipment {
            weapon: Some(WeaponKind::SWORD),
            armor: Some(ArmorKind::LEATHER_ARMOR),
            ..Default::default()
        };
        let stats = CombatStats { hp: 12, max_hp: 30, mp: 4, max_mp: 20, attack: 7, defense: 3 };

        let progress = PlayerProgress { level: 2, xp: 9, next_level_xp: 35, kills: 3 };

        let items = crate::modules::item::build_test_registry();
        let text = status_hud_text(&world, &GlobalTurn(42), &map, &inventory, &equipment, &progress, &stats, &items);

        assert!(text.contains("던전 2층"));
        assert!(text.contains("Turn 42"));
        assert!(text.contains("Lv.2 XP 9/35"));
        assert!(text.contains("HP 12/30"));
        assert!(text.contains("75G"));
        assert!(text.contains("검"));
        assert!(text.contains("bsp"));
    }

    #[test]
    fn 무기와_방어구를_장착하지_않으면_HUD에_맨손과_방어구없음이_표시된다() {
        let world = WorldState::default();
        let map = Map::new(10, 10);
        let inventory = PlayerInventory::default();
        let equipment = PlayerEquipment { weapon: None, armor: None, ..Default::default() };
        let stats = CombatStats { hp: 1, max_hp: 1, mp: 0, max_mp: 0, attack: 1, defense: 0 };
        let progress = PlayerProgress::default();
        let items = crate::modules::item::build_test_registry();

        let text = status_hud_text(&world, &GlobalTurn(0), &map, &inventory, &equipment, &progress, &stats, &items);

        assert!(text.contains("맨손"));
        assert!(text.contains("방어구 없음"));
    }

    #[test]
    fn 맵_알고리즘이_비어있으면_HUD에_unknown으로_표시된다() {
        let world = WorldState::default();
        let map = Map::new(10, 10); // algorithm 기본값은 빈 문자열
        let inventory = PlayerInventory::default();
        let equipment = PlayerEquipment::default();
        let stats = CombatStats { hp: 1, max_hp: 1, mp: 0, max_mp: 0, attack: 1, defense: 0 };
        let progress = PlayerProgress::default();
        let items = crate::modules::item::build_test_registry();

        assert!(map.algorithm.is_empty());
        let text = status_hud_text(&world, &GlobalTurn(0), &map, &inventory, &equipment, &progress, &stats, &items);

        assert!(text.contains("unknown"));
    }

    // ── App 하네스 ─────────────────────────────────────────────────────────

    /// AssetServer(폰트 로드)가 필요한 HUD 시스템용 App 하네스.
    fn 하네스() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app
    }

    /// HUD update 시스템이 읽는 모든 리소스를 기본값으로 채운다.
    fn HUD_리소스_삽입(app: &mut App) {
        app.insert_resource(WorldState::default());
        app.insert_resource(GlobalTurn(0));
        app.insert_resource(MapResource(Map::new(10, 10)));
        app.insert_resource(PlayerInventory::default());
        app.insert_resource(PlayerEquipment::default());
        app.insert_resource(PlayerProgress::default());
        app.insert_resource(crate::modules::item::build_test_registry());
    }

    fn 기본_플레이어_스탯() -> CombatStats {
        CombatStats { hp: 10, max_hp: 10, mp: 5, max_mp: 5, attack: 3, defense: 1 }
    }

    #[test]
    fn 플러그인을_추가하면_시작시_HUD_컨테이너와_텍스트가_생성된다() {
        let mut app = 하네스();
        HUD_리소스_삽입(&mut app);
        app.world.spawn((Player, 기본_플레이어_스탯()));
        app.add_plugins(StatusHudPlugin);
        app.update();

        assert_eq!(app.world.query::<&StatusHud>().iter(&app.world).count(), 1);
        assert_eq!(app.world.query::<&StatusHudText>().iter(&app.world).count(), 1);
    }

    #[test]
    fn 리소스가_바뀌면_HUD_텍스트가_현재_상태로_갱신된다() {
        let mut app = 하네스();
        HUD_리소스_삽입(&mut app);
        app.world.spawn((Player, 기본_플레이어_스탯()));
        app.add_systems(Startup, setup_status_hud);
        app.add_systems(Update, update_status_hud);
        // 첫 update: setup 이후 모든 리소스가 막 추가되어 is_changed 가 참 → 갱신된다.
        app.update();

        let mut q = app.world.query_filtered::<&Text, With<StatusHudText>>();
        let text = q.single(&app.world);
        assert!(text.sections[0].value.contains("Turn 0"));
    }

    #[test]
    fn 플레이어가_없으면_HUD_텍스트는_빈_채로_유지된다() {
        let mut app = 하네스();
        HUD_리소스_삽입(&mut app);
        // 플레이어 엔티티 없음 → get_single() 실패로 early return.
        app.add_systems(Startup, setup_status_hud);
        app.add_systems(Update, update_status_hud);
        app.update();

        let mut q = app.world.query_filtered::<&Text, With<StatusHudText>>();
        let text = q.single(&app.world);
        assert_eq!(text.sections[0].value, "");
    }

    #[test]
    fn 아무_리소스도_바뀌지_않은_프레임에는_HUD를_다시_갱신하지_않는다() {
        let mut app = 하네스();
        HUD_리소스_삽입(&mut app);
        app.world.spawn((Player, 기본_플레이어_스탯()));
        app.add_systems(Startup, setup_status_hud);
        app.add_systems(Update, update_status_hud);
        // 1프레임: 변경 감지로 텍스트가 채워진다.
        app.update();
        let mut q = app.world.query_filtered::<&Text, With<StatusHudText>>();
        assert!(q.single(&app.world).sections[0].value.contains("Turn 0"));

        // 텍스트를 일부러 비우고 한 프레임 더 돌린다.
        // 이번 프레임에는 어떤 리소스/스탯도 바뀌지 않아 변경 없음 분기로 빠져 그대로 비어 있어야 한다.
        {
            let mut q = app.world.query_filtered::<&mut Text, With<StatusHudText>>();
            q.single_mut(&mut app.world).sections[0].value.clear();
        }
        app.update();
        let mut q = app.world.query_filtered::<&Text, With<StatusHudText>>();
        assert_eq!(q.single(&app.world).sections[0].value, "");
    }

    #[test]
    fn 존이_안바뀌어도_뒤따르는_리소스나_스탯이_바뀌면_HUD가_갱신된다() {
        // update_status_hud 의 단락 평가(&& 체인)에서, world 는 그대로지만
        // 그 뒤의 turn/map/inventory/equipment/progress/stats 가 각각 바뀌는 프레임을 만들어
        // 체인 중간 조건들의 거짓(단락) 분기를 모두 탄다.
        let mut app = 하네스();
        HUD_리소스_삽입(&mut app);
        let player = app.world.spawn((Player, 기본_플레이어_스탯())).id();
        app.add_systems(Startup, setup_status_hud);
        app.add_systems(Update, update_status_hud);

        // 첫 프레임: setup + 모든 리소스가 막 추가되어 한 번 갱신된다.
        app.update();
        // 두 번째 프레임: 아무것도 건드리지 않아 변경 감지가 모두 가라앉는다(전부 미변경).
        app.update();

        // 이제 world 는 건드리지 않은 채, 체인 뒤쪽 항목을 하나씩만 바꿔가며 단락 분기를 탄다.
        // turn 만 변경 (line 88 의 거짓 분기)
        app.world.resource_mut::<GlobalTurn>().0 += 1;
        app.update();
        // map 만 변경 (line 89)
        app.world.resource_mut::<MapResource>().0.algorithm = "bsp".into();
        app.update();
        // inventory 만 변경 (line 90)
        app.world.resource_mut::<PlayerInventory>().gold += 1;
        app.update();
        // equipment 만 변경 (line 91) — DerefMut 가 일어나야 변경으로 표시되므로 실제로 필드를 건드린다.
        app.world.resource_mut::<PlayerEquipment>().weapon = None;
        app.update();
        // progress 만 변경 (line 92)
        app.world.resource_mut::<PlayerProgress>().xp += 1;
        app.update();
        // stats 만 변경 (line 93)
        app.world.get_mut::<CombatStats>(player).unwrap().hp -= 1;
        app.update();

        let mut q = app.world.query_filtered::<&Text, With<StatusHudText>>();
        let value = &q.single(&app.world).sections[0].value;
        // 마지막으로 stats 가 바뀌었으니 갱신된 HP 가 반영돼 있어야 한다.
        assert!(value.contains("Turn"));
    }

    #[test]
    fn HUD_텍스트_엔티티가_없으면_갱신은_조용히_넘어간다() {
        let mut app = 하네스();
        HUD_리소스_삽입(&mut app);
        app.world.spawn((Player, 기본_플레이어_스탯()));
        // setup 없이 update 시스템만 등록 → StatusHudText 엔티티가 존재하지 않는다.
        app.add_systems(Update, update_status_hud);
        app.update(); // 패닉 없이 통과해야 한다.

        assert_eq!(app.world.query::<&StatusHudText>().iter(&app.world).count(), 0);
    }
}
