use bevy::prelude::*;

use crate::modules::{
    combat::CombatStats,
    item::{PlayerEquipment, PlayerInventory},
    map::{GlobalTurn, MapResource, MAP_WIDTH, TILE_SIZE},
    player::Player,
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
    player_q: Query<Ref<CombatStats>, With<Player>>,
    mut text_q: Query<&mut Text, With<StatusHudText>>,
) {
    let Ok(stats) = player_q.get_single() else { return; };
    if !world.is_changed()
        && !turn.is_changed()
        && !map_res.is_changed()
        && !inventory.is_changed()
        && !equipment.is_changed()
        && !stats.is_changed()
    {
        return;
    }

    let Ok(mut text) = text_q.get_single_mut() else { return; };
    text.sections[0].value = status_hud_text(&world, &turn, map_res.map(), &inventory, &equipment, &stats);
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
    stats: &CombatStats,
) -> String {
    let weapon = equipment.weapon.map(|w| w.display_name()).unwrap_or("맨손");
    let armor = equipment.armor.map(|a| a.display_name()).unwrap_or("방어구 없음");
    let algorithm = if map.algorithm.is_empty() { "unknown" } else { &map.algorithm };
    format!(
        "{} | Turn {} | HP {}/{} MP {}/{} | ATK {} DEF {} | {}G | {} / {} | {}",
        world.current.display_name(),
        turn.0,
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
    use super::*;
    use crate::modules::{
        item::{ArmorKind, PlayerEquipment, PlayerInventory, WeaponKind},
        map::{GlobalTurn, Map},
        zone::{WorldState, ZoneId},
    };

    #[test]
    fn status_hud_text_contains_core_summary() {
        let mut world = WorldState::default();
        world.current = ZoneId::Dungeon(2);
        let mut map = Map::new(10, 10);
        map.algorithm = "bsp".to_string();
        let inventory = PlayerInventory { gold: 75, ..Default::default() };
        let equipment = PlayerEquipment {
            weapon: Some(WeaponKind::Sword),
            armor: Some(ArmorKind::LeatherArmor),
        };
        let stats = CombatStats { hp: 12, max_hp: 30, mp: 4, max_mp: 20, attack: 7, defense: 3 };

        let text = status_hud_text(&world, &GlobalTurn(42), &map, &inventory, &equipment, &stats);

        assert!(text.contains("던전 2층"));
        assert!(text.contains("Turn 42"));
        assert!(text.contains("HP 12/30"));
        assert!(text.contains("75G"));
        assert!(text.contains("검"));
        assert!(text.contains("bsp"));
    }
}
