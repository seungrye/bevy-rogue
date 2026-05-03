use bevy::prelude::*;
use rand::Rng;
use crate::modules::{
    map::{tile_to_world_coords, world_to_tile_coords, TILE_SIZE, PlayerActedEvent},
    player::{Player, MovingTo, PlayerSystemSet},
    combat::CombatStats,
    ui::LogMessage,
};

pub const POTION_HEAL: i32 = 8;
const Z_ITEM: f32 = 0.3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemType {
    HealthPotion,
}

impl ItemType {
    pub fn glyph(self) -> &'static str {
        match self {
            ItemType::HealthPotion => "!",
        }
    }

    pub fn color(self) -> Color {
        match self {
            ItemType::HealthPotion => Color::rgb(0.2, 0.9, 0.2),
        }
    }

    pub fn pickup_message(self) -> &'static str {
        match self {
            ItemType::HealthPotion => "체력 물약을 획득했다!",
        }
    }
}

#[derive(Component)]
pub struct Item {
    pub item_type: ItemType,
    pub tile_x: usize,
    pub tile_y: usize,
}

/// 몬스터 처치 시 아이템 드롭을 요청하는 이벤트
#[derive(Event)]
pub struct ItemDropEvent {
    pub tile_x: usize,
    pub tile_y: usize,
    pub monster_name: String,
}

pub struct ItemPlugin;

impl Plugin for ItemPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<ItemDropEvent>()
            .add_systems(Update, (
                spawn_dropped_items,
                pickup_items.after(PlayerSystemSet::Movement),
            ));
    }
}

/// 몬스터 이름별 드롭률 (0.0–1.0)
pub fn drop_rate(monster_name: &str) -> f32 {
    match monster_name {
        "고블린" => 0.30,
        "오크"   => 0.40,
        "트롤"   => 0.50,
        _        => 0.25,
    }
}

/// 아이템 타입별 HP 회복량
pub fn heal_amount(item_type: ItemType) -> i32 {
    match item_type {
        ItemType::HealthPotion => POTION_HEAL,
    }
}

fn spawn_dropped_items(
    mut events: EventReader<ItemDropEvent>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    let mut rng = rand::thread_rng();
    for event in events.read() {
        if rng.gen::<f32>() >= drop_rate(&event.monster_name) { continue; }
        let item_type = ItemType::HealthPotion;
        let pos = tile_to_world_coords(event.tile_x, event.tile_y);
        commands.spawn((
            Text2dBundle {
                text: Text::from_section(item_type.glyph(), TextStyle {
                    font: asset_server.load("fonts/FiraMono-Medium.ttf"),
                    font_size: TILE_SIZE,
                    color: item_type.color(),
                }),
                transform: Transform::from_xyz(pos.x, pos.y, Z_ITEM),
                ..default()
            },
            Item { item_type, tile_x: event.tile_x, tile_y: event.tile_y },
        ));
    }
}

fn pickup_items(
    mut commands: Commands,
    mut turn_events: EventReader<PlayerActedEvent>,
    mut player_query: Query<(Option<&MovingTo>, &Transform, &mut CombatStats), With<Player>>,
    item_query: Query<(Entity, &Item)>,
    mut log: EventWriter<LogMessage>,
) {
    if turn_events.read().next().is_none() { return; }
    let Ok((moving_to, transform, mut stats)) = player_query.get_single_mut() else { return };
    let (px, py) = moving_to
        .map(|m| world_to_tile_coords(m.target))
        .unwrap_or_else(|| world_to_tile_coords(transform.translation));

    for (entity, item) in item_query.iter() {
        if item.tile_x != px || item.tile_y != py { continue; }
        let heal = heal_amount(item.item_type);
        stats.hp = (stats.hp + heal).min(stats.max_hp);
        log.send(LogMessage(format!(
            "{} (HP +{}, {}/{})",
            item.item_type.pickup_message(), heal, stats.hp, stats.max_hp
        )));
        commands.entity(entity).despawn();
        break;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_rate_goblin_is_thirty_percent() {
        assert!((drop_rate("고블린") - 0.30).abs() < f32::EPSILON);
    }

    #[test]
    fn drop_rate_orc_is_forty_percent() {
        assert!((drop_rate("오크") - 0.40).abs() < f32::EPSILON);
    }

    #[test]
    fn drop_rate_troll_is_fifty_percent() {
        assert!((drop_rate("트롤") - 0.50).abs() < f32::EPSILON);
    }

    #[test]
    fn drop_rate_unknown_monster_has_default() {
        assert!(drop_rate("알수없는몬스터") > 0.0);
    }

    #[test]
    fn heal_amount_potion_equals_constant() {
        assert_eq!(heal_amount(ItemType::HealthPotion), POTION_HEAL);
    }

    #[test]
    fn potion_heal_does_not_exceed_max_hp() {
        let max_hp = 10;
        let current_hp = 8;
        let healed = (current_hp + heal_amount(ItemType::HealthPotion)).min(max_hp);
        assert_eq!(healed, max_hp);
    }

    #[test]
    fn potion_heal_adds_to_current_hp() {
        let max_hp = 30;
        let current_hp = 15;
        let healed = (current_hp + heal_amount(ItemType::HealthPotion)).min(max_hp);
        assert_eq!(healed, 15 + POTION_HEAL);
    }
}
