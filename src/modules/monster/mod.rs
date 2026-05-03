use bevy::prelude::*;
use std::collections::HashSet;
use crate::modules::{
    map::{
        draw_map, Map, MapResource, MapTile, MapType, MonsterTiles,
        tile_to_world_coords, world_to_tile_coords,
        MAP_HEIGHT, MAP_WIDTH, TILE_SIZE,
        MapSystemSet, MonsterRespawnEvent, PlayerActedEvent, AttackMonsterEvent, Rect,
    },
    player::{Player, MovingTo, PlayerSystemSet},
    combat::{CombatStats, Defeated, calc_damage},
    ui::LogMessage,
    combat_feedback::CombatFeedbackEvent,
    item::ItemDropEvent,
};

const Z_MONSTER: f32 = 0.8;

static MONSTER_DATA: &[(&str, &str, [f32; 3], i32, i32, i32)] = &[
    // (이름, 글리프, 색상, hp, attack, defense)
    ("고블린", "g", [0.2, 0.8, 0.2],  6, 3, 0),
    ("오크",   "O", [0.9, 0.5, 0.1], 10, 5, 2),
    ("트롤",   "T", [0.3, 0.7, 0.5], 16, 8, 3),
];

#[derive(Component)]
pub struct Monster {
    pub name: String,
    pub tile_x: usize,
    pub tile_y: usize,
}

pub struct MonsterPlugin;

impl Plugin for MonsterPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_on_startup.after(draw_map))
            .add_systems(PreUpdate, sync_monster_tiles)
            .add_systems(Update, (
                respawn_on_regen.after(MapSystemSet::ExecuteRegen),
                (handle_player_attack, monster_turn, cleanup_dead)
                    .chain()
                    .after(PlayerSystemSet::Movement),
            ));
    }
}

fn sync_monster_tiles(
    monster_query: Query<&Monster>,
    mut monster_tiles: ResMut<MonsterTiles>,
) {
    monster_tiles.0.clear();
    for m in monster_query.iter() {
        monster_tiles.0.insert((m.tile_x, m.tile_y));
    }
}

fn spawn_on_startup(
    mut commands: Commands,
    map_res: Res<MapResource>,
    asset_server: Res<AssetServer>,
) {
    let map = map_res.map();
    if map.map_type == MapType::Dungeon {
        do_spawn(&mut commands, &map.rooms.clone(), &asset_server);
    }
}

fn respawn_on_regen(
    mut commands: Commands,
    mut events: EventReader<MonsterRespawnEvent>,
    monster_query: Query<Entity, With<Monster>>,
    asset_server: Res<AssetServer>,
) {
    for event in events.read() {
        for entity in monster_query.iter() {
            commands.entity(entity).despawn();
        }
        if event.map_type == MapType::Dungeon {
            do_spawn(&mut commands, &event.rooms, &asset_server);
        }
    }
}

fn do_spawn(commands: &mut Commands, rooms: &[Rect], asset_server: &AssetServer) {
    if rooms.is_empty() { return; }
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");

    for (i, room) in rooms.iter().skip(1).enumerate() {
        let data = MONSTER_DATA[i % MONSTER_DATA.len()];
        let (name, glyph, color, hp, atk, def) = (data.0, data.1, data.2, data.3, data.4, data.5);
        let (cx, cy) = room.center();
        let coord = tile_to_world_coords(cx, cy);

        commands.spawn((
            Text2dBundle {
                text: Text::from_section(glyph, TextStyle {
                    font: font.clone(),
                    font_size: TILE_SIZE,
                    color: Color::rgb(color[0], color[1], color[2]),
                }),
                transform: Transform::from_xyz(coord.x, coord.y, Z_MONSTER),
                ..default()
            },
            Monster { name: name.to_string(), tile_x: cx, tile_y: cy },
            CombatStats { hp, max_hp: hp, mp: 0, max_mp: 0, attack: atk, defense: def },
        ));
    }
}

fn handle_player_attack(
    mut events: EventReader<AttackMonsterEvent>,
    player_query: Query<&CombatStats, (With<Player>, Without<Monster>)>,
    mut monster_query: Query<(Entity, &Monster, &mut CombatStats), Without<Player>>,
    mut log_writer: EventWriter<LogMessage>,
    mut feedback_writer: EventWriter<CombatFeedbackEvent>,
    mut drop_writer: EventWriter<ItemDropEvent>,
) {
    for AttackMonsterEvent(tx, ty) in events.read() {
        let Ok(player_stats) = player_query.get_single() else { continue };
        for (monster_entity, monster, mut monster_stats) in monster_query.iter_mut() {
            if monster.tile_x != *tx || monster.tile_y != *ty { continue; }
            let dmg = calc_damage(player_stats.attack, monster_stats.defense);
            monster_stats.hp -= dmg;
            let original_color = MONSTER_DATA.iter()
                .find(|(n, ..)| *n == monster.name.as_str())
                .map(|(_, _, c, ..)| Color::rgb(c[0], c[1], c[2]))
                .unwrap_or(Color::WHITE);
            feedback_writer.send(CombatFeedbackEvent {
                tile_x: *tx,
                tile_y: *ty,
                hit_entity: monster_entity,
                original_color,
            });
            if monster_stats.hp <= 0 {
                log_writer.send(LogMessage(format!(
                    "{}을(를) 처치했다! ({} 데미지)", monster.name, dmg
                )));
                drop_writer.send(ItemDropEvent {
                    tile_x: *tx,
                    tile_y: *ty,
                    monster_name: monster.name.clone(),
                });
            } else {
                log_writer.send(LogMessage(format!(
                    "{}에게 {} 데미지! (HP: {}/{})",
                    monster.name, dmg, monster_stats.hp, monster_stats.max_hp
                )));
            }
            break;
        }
    }
}

fn monster_turn(
    mut commands: Commands,
    mut events: EventReader<PlayerActedEvent>,
    map_res: Res<MapResource>,
    mut monster_query: Query<(&mut Monster, &mut Transform, &CombatStats), Without<Player>>,
    mut player_query: Query<(Entity, &Transform, Option<&MovingTo>, &mut CombatStats), (With<Player>, Without<Monster>)>,
    mut log_writer: EventWriter<LogMessage>,
    mut feedback_writer: EventWriter<CombatFeedbackEvent>,
) {
    if events.read().next().is_none() { return; }

    let map = map_res.map();
    let Ok((player_entity, player_transform, player_moving, mut player_stats)) = player_query.get_single_mut() else { return };

    let (px, py) = player_moving
        .map(|m| world_to_tile_coords(m.target))
        .unwrap_or_else(|| world_to_tile_coords(player_transform.translation));

    let mut occupied: HashSet<(usize, usize)> = monster_query.iter()
        .filter(|(_, _, stats)| stats.hp > 0)
        .map(|(m, _, _)| (m.tile_x, m.tile_y))
        .collect();
    occupied.insert((px, py));

    let mut player_dead = false;

    for (mut monster, mut transform, monster_stats) in monster_query.iter_mut() {
        if monster_stats.hp <= 0 { continue; }

        occupied.remove(&(monster.tile_x, monster.tile_y));

        let dx = (monster.tile_x as i32 - px as i32).abs();
        let dy = (monster.tile_y as i32 - py as i32).abs();
        let adjacent = (dx == 1 && dy == 0) || (dx == 0 && dy == 1);

        if adjacent {
            if !player_dead {
                let dmg = calc_damage(monster_stats.attack, player_stats.defense);
                player_stats.hp -= dmg;
                feedback_writer.send(CombatFeedbackEvent {
                    tile_x: px,
                    tile_y: py,
                    hit_entity: player_entity,
                    original_color: Color::YELLOW,
                });
                if player_stats.hp <= 0 {
                    player_dead = true;
                    log_writer.send(LogMessage(format!(
                        "{}에게 {} 데미지! 당신은 죽었습니다.", monster.name, dmg
                    )));
                    commands.entity(player_entity).insert(Defeated);
                } else {
                    log_writer.send(LogMessage(format!(
                        "{}에게 {} 데미지! (HP: {}/{})",
                        monster.name, dmg, player_stats.hp, player_stats.max_hp
                    )));
                }
            }
            occupied.insert((monster.tile_x, monster.tile_y));
        } else {
            let (nx, ny) = move_toward(monster.tile_x, monster.tile_y, px, py, map, &occupied);
            occupied.insert((nx, ny));
            monster.tile_x = nx;
            monster.tile_y = ny;
            let wp = tile_to_world_coords(nx, ny);
            transform.translation = Vec3::new(wp.x, wp.y, Z_MONSTER);
        }
    }
}

fn cleanup_dead(
    mut commands: Commands,
    query: Query<(Entity, &Monster, &CombatStats)>,
) {
    for (entity, _monster, stats) in query.iter() {
        if stats.hp <= 0 {
            commands.entity(entity).despawn();
        }
    }
}

pub fn move_toward(
    x: usize, y: usize,
    tx: usize, ty: usize,
    map: &Map,
    occupied: &HashSet<(usize, usize)>,
) -> (usize, usize) {
    let neighbors = [
        (x.wrapping_sub(1), y),
        (x + 1, y),
        (x, y.wrapping_sub(1)),
        (x, y + 1),
    ];
    let best = neighbors.iter()
        .filter(|&&(nx, ny)| {
            nx < MAP_WIDTH && ny < MAP_HEIGHT
                && map.get_tile(nx, ny) == MapTile::Floor
                && !occupied.contains(&(nx, ny))
        })
        .min_by_key(|&&(nx, ny)| {
            let ddx = nx as i32 - tx as i32;
            let ddy = ny as i32 - ty as i32;
            ddx * ddx + ddy * ddy
        });
    best.copied().unwrap_or((x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor_map(w: usize, h: usize, floors: &[(usize, usize)]) -> Map {
        let mut map = Map::new(w, h);
        for &(x, y) in floors {
            map.set_tile(x, y, MapTile::Floor);
        }
        map
    }

    #[test]
    fn move_toward_moves_closer_to_target() {
        let map = floor_map(10, 10, &[(5,5),(6,5),(7,5)]);
        let occupied = HashSet::new();
        let (nx, ny) = move_toward(5, 5, 7, 5, &map, &occupied);
        assert_eq!((nx, ny), (6, 5), "플레이어 방향 인접 타일로 이동해야 한다");
    }

    #[test]
    fn move_toward_stays_put_when_all_blocked() {
        let map = floor_map(10, 10, &[(5,5),(6,5)]);
        let mut occupied = HashSet::new();
        occupied.insert((6usize, 5usize));
        let result = move_toward(5, 5, 9, 5, &map, &occupied);
        assert_eq!(result, (5, 5), "이동 불가 시 제자리를 유지해야 한다");
    }

    #[test]
    fn move_toward_avoids_occupied_tiles() {
        let map = floor_map(10, 10, &[(5,5),(6,5),(5,6)]);
        let mut occupied = HashSet::new();
        occupied.insert((6usize, 5usize));
        // 목적지 (9,9) — (5,6) 이 더 가깝진 않지만 (6,5)는 막혀 있음
        let (nx, ny) = move_toward(5, 5, 6, 6, &map, &occupied);
        assert_ne!((nx, ny), (6, 5), "점유된 타일로 이동하면 안 된다");
    }

    #[test]
    fn move_toward_does_not_move_to_wall() {
        // (5,5) 주변 중 floor 는 (6,5)만 존재
        let map = floor_map(10, 10, &[(5,5),(6,5)]);
        let occupied = HashSet::new();
        for tx in 0..10usize {
            let (nx, _) = move_toward(5, 5, tx, 5, &map, &occupied);
            assert!(nx < 10, "맵 밖으로 나가면 안 된다");
            let tile = map.get_tile(nx, 5);
            assert_eq!(tile, MapTile::Floor, "Wall 타일로 이동하면 안 된다");
        }
    }

    #[test]
    fn move_toward_picks_best_of_multiple_floors() {
        // (5,5)에서 (8,5) 쪽으로 갈 때 (6,5)가 최선
        let map = floor_map(10, 10, &[(5,5),(6,5),(5,6),(4,5),(5,4)]);
        let occupied = HashSet::new();
        let (nx, ny) = move_toward(5, 5, 8, 5, &map, &occupied);
        assert_eq!((nx, ny), (6, 5));
    }
}
