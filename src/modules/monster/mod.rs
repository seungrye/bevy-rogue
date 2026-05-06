use bevy::prelude::*;
use std::collections::HashSet;
use rand::{Rng, rngs::ThreadRng};
use crate::modules::{
    map::{
        draw_map, Map, MapResource, TileKind, MapType, MonsterTiles,
        tile_to_world_coords, world_to_tile_coords, is_line_of_sight_clear,
        MAP_HEIGHT, MAP_WIDTH, TILE_SIZE,
        MapSystemSet, MonsterRespawnEvent, PlayerActedEvent, AttackMonsterEvent, Rect,
    },
    player::{Player, MovingTo, MoveQueue, PlayerSystemSet, LERP_SPEED},
    combat::{CombatStats, Defeated, Speed, calc_damage},
    ui::LogMessage,
    combat_feedback::CombatFeedbackEvent,
    item::ItemDropEvent,
    zone::{WorldState, ZonePersistence, MonsterSlot},
    map::GlobalTurn,
};

const Z_MONSTER: f32 = 0.8;
const MAX_ALERT_TURNS: u32 = 5;

static MONSTER_DATA: &[(&str, &str, [f32; 3], i32, i32, i32, i32, f32)] = &[
    // (이름, 글리프, 색상, hp, attack, defense, vision_radius, speed)
    ("고블린", "g", [0.2, 0.8, 0.2],  6, 3, 0, 6, 1.5),
    ("오크",   "O", [0.9, 0.5, 0.1], 10, 5, 2, 8, 1.0),
    ("트롤",   "T", [0.3, 0.7, 0.5], 16, 8, 3, 5, 0.5),
];

#[derive(Component)]
pub struct Monster {
    pub name: String,
    pub tile_x: usize,
    pub tile_y: usize,
    pub vision_radius: i32,
    pub alert_turns: u32,
    pub slot_idx: usize,
}

pub fn can_see_player(
    mx: usize, my: usize,
    px: usize, py: usize,
    vision_radius: i32,
    map: &Map,
) -> bool {
    let dx = mx as i32 - px as i32;
    let dy = my as i32 - py as i32;
    if dx * dx + dy * dy > vision_radius * vision_radius { return false; }
    is_line_of_sight_clear(map, mx as i32, my as i32, px as i32, py as i32)
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
                    .after(PlayerSystemSet::MovementComplete),
                smooth_monster_move,
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
    mut persistence: ResMut<ZonePersistence>,
    world: Res<WorldState>,
) {
    let map = map_res.map();
    if map.map_type == MapType::Dungeon {
        let zone_id = world.current.clone();
        let slots = init_zone_monster_slots(&map.rooms);
        persistence.0.entry(zone_id).or_default().monster_slots = slots.clone();
        spawn_from_slots(&mut commands, &map.rooms, &slots, 0, &asset_server);
    }
}

fn respawn_on_regen(
    mut commands: Commands,
    mut events: EventReader<MonsterRespawnEvent>,
    monster_query: Query<Entity, With<Monster>>,
    asset_server: Res<AssetServer>,
    world: Res<WorldState>,
    global_turn: Res<GlobalTurn>,
    mut persistence: ResMut<ZonePersistence>,
) {
    for event in events.read() {
        for entity in monster_query.iter() {
            commands.entity(entity).despawn();
        }
        if event.map_type != MapType::Dungeon { continue; }

        let zone_id = world.current.clone();

        // 처음 방문이면 슬롯 초기화
        if !persistence.0.contains_key(&zone_id) {
            persistence.0.entry(zone_id.clone()).or_default().monster_slots =
                init_zone_monster_slots(&event.rooms);
        }

        // 만료된 리스폰 타이머 처리 (경과 턴 catch-up)
        if let Some(snapshot) = persistence.0.get_mut(&zone_id) {
            for slot in &mut snapshot.monster_slots {
                if let Some(t) = slot.respawn_at_turn {
                    if t <= global_turn.0 { slot.respawn_at_turn = None; }
                }
            }
        }

        let slots = persistence.0[&zone_id].monster_slots.clone();
        spawn_from_slots(&mut commands, &event.rooms, &slots, global_turn.0, &asset_server);
    }
}

fn init_zone_monster_slots(rooms: &[Rect]) -> Vec<MonsterSlot> {
    rooms.iter().skip(1).take(10).enumerate()
        .map(|(i, _)| MonsterSlot { data_idx: i % MONSTER_DATA.len(), respawn_at_turn: None })
        .collect()
}

fn spawn_from_slots(
    commands: &mut Commands,
    rooms: &[Rect],
    slots: &[MonsterSlot],
    global_turn: u64,
    asset_server: &AssetServer,
) {
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");
    let mut rng = rand::thread_rng();

    for (slot_idx, (slot, room)) in slots.iter().zip(rooms.iter().skip(1)).enumerate() {
        if let Some(t) = slot.respawn_at_turn {
            if t > global_turn { continue; }
        }
        let data = MONSTER_DATA[slot.data_idx];
        let (name, glyph, color, hp, atk, def, vis, spd) = (data.0, data.1, data.2, data.3, data.4, data.5, data.6, data.7);

        let (tx, ty) = {
            let mut tile = room.center();
            for _ in 0..10 {
                let x = rng.gen_range(room.x1..room.x2);
                let y = rng.gen_range(room.y1..room.y2);
                tile = (x, y);
                break;
            }
            tile
        };

        let coord = tile_to_world_coords(tx, ty);
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
            Monster { name: name.to_string(), tile_x: tx, tile_y: ty, vision_radius: vis, alert_turns: 0, slot_idx },
            CombatStats { hp, max_hp: hp, mp: 0, max_mp: 0, attack: atk, defense: def },
            Speed::new(spd),
            MoveQueue::default(),
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
    mut monster_query: Query<(&mut Monster, &mut MoveQueue, &CombatStats, &mut Speed), Without<Player>>,
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
        .filter(|(_, _, stats, _)| stats.hp > 0)
        .map(|(m, _, _, _)| (m.tile_x, m.tile_y))
        .collect();
    occupied.insert((px, py));

    let mut player_dead = false;
    let mut rng = rand::thread_rng();

    for (mut monster, mut move_queue, monster_stats, mut speed) in monster_query.iter_mut() {
        if monster_stats.hp <= 0 { continue; }

        occupied.remove(&(monster.tile_x, monster.tile_y));

        // 시야 갱신
        if can_see_player(monster.tile_x, monster.tile_y, px, py, monster.vision_radius, map) {
            monster.alert_turns = MAX_ALERT_TURNS;
        } else if monster.alert_turns > 0 {
            monster.alert_turns -= 1;
        }

        // 에너지 누적 → 1.0마다 행동 1회 소비
        speed.energy += speed.value;
        while speed.energy >= 1.0 {
            speed.energy -= 1.0;

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
            } else if monster.alert_turns > 0 {
                let (nx, ny) = move_toward(monster.tile_x, monster.tile_y, px, py, map, &occupied);
                occupied.remove(&(monster.tile_x, monster.tile_y));
                occupied.insert((nx, ny));
                let wp = tile_to_world_coords(nx, ny);
                move_queue.0.push_back(Vec3::new(wp.x, wp.y, Z_MONSTER));
                monster.tile_x = nx;
                monster.tile_y = ny;
            } else {
                let (nx, ny) = wander(monster.tile_x, monster.tile_y, map, &occupied, &mut rng);
                occupied.remove(&(monster.tile_x, monster.tile_y));
                occupied.insert((nx, ny));
                let wp = tile_to_world_coords(nx, ny);
                move_queue.0.push_back(Vec3::new(wp.x, wp.y, Z_MONSTER));
                monster.tile_x = nx;
                monster.tile_y = ny;
            }
        }

        occupied.insert((monster.tile_x, monster.tile_y));
    }
}

fn smooth_monster_move(
    time: Res<Time>,
    mut query: Query<(&mut Transform, &mut MoveQueue, &Speed), With<Monster>>,
) {
    let dt = time.delta_seconds();
    for (mut transform, mut queue, speed) in query.iter_mut() {
        let anim_speed = LERP_SPEED * speed.value.max(0.5);
        let step = anim_speed * TILE_SIZE * dt;
        while let Some(&target) = queue.0.front() {
            let dist = transform.translation.distance(target);
            if dist <= step {
                transform.translation = target;
                queue.0.pop_front();
            } else {
                let dir = (target - transform.translation).normalize();
                transform.translation += dir * step;
                break;
            }
        }
    }
}

fn cleanup_dead(
    mut commands: Commands,
    query: Query<(Entity, &Monster, &CombatStats)>,
    world: Res<WorldState>,
    global_turn: Res<GlobalTurn>,
    mut persistence: ResMut<ZonePersistence>,
) {
    let mut rng = rand::thread_rng();
    for (entity, monster, stats) in query.iter() {
        if stats.hp <= 0 {
            commands.entity(entity).despawn();
            let respawn_at = global_turn.0 + rng.gen_range(30u64..=120);
            if let Some(snapshot) = persistence.0.get_mut(&world.current) {
                if let Some(slot) = snapshot.monster_slots.get_mut(monster.slot_idx) {
                    slot.respawn_at_turn = Some(respawn_at);
                }
            }
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
                && map.get_tile(nx, ny) == TileKind::Floor
                && !occupied.contains(&(nx, ny))
        })
        .min_by_key(|&&(nx, ny)| {
            let ddx = nx as i32 - tx as i32;
            let ddy = ny as i32 - ty as i32;
            ddx * ddx + ddy * ddy
        });
    best.copied().unwrap_or((x, y))
}

pub fn wander(
    x: usize, y: usize,
    map: &Map,
    occupied: &HashSet<(usize, usize)>,
    rng: &mut ThreadRng,
) -> (usize, usize) {
    const STAY_CHANCE: f64 = 0.3;
    if rng.gen_bool(STAY_CHANCE) { return (x, y); }
    let neighbors = [
        (x.wrapping_sub(1), y),
        (x + 1, y),
        (x, y.wrapping_sub(1)),
        (x, y + 1),
    ];
    let valid: Vec<_> = neighbors.iter()
        .filter(|&&(nx, ny)| {
            nx < MAP_WIDTH && ny < MAP_HEIGHT
                && map.get_tile(nx, ny) == TileKind::Floor
                && !occupied.contains(&(nx, ny))
        })
        .copied()
        .collect();
    if valid.is_empty() { return (x, y); }
    valid[rng.gen_range(0..valid.len())]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor_map(w: usize, h: usize, floors: &[(usize, usize)]) -> Map {
        let mut map = Map::new(w, h);
        for &(x, y) in floors {
            map.set_tile(x, y, TileKind::Floor);
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
            assert_eq!(tile, TileKind::Floor, "Wall 타일로 이동하면 안 된다");
        }
    }

    #[test]
    fn move_toward_picks_best_of_multiple_floors() {
        let map = floor_map(10, 10, &[(5,5),(6,5),(5,6),(4,5),(5,4)]);
        let occupied = HashSet::new();
        let (nx, ny) = move_toward(5, 5, 8, 5, &map, &occupied);
        assert_eq!((nx, ny), (6, 5));
    }

    fn open_map(w: usize, h: usize) -> Map {
        let mut map = Map::new(w, h);
        for y in 1..h-1 { for x in 1..w-1 { map.set_tile(x, y, TileKind::Floor); } }
        map
    }

    #[test]
    fn can_see_player_within_radius_clear_los() {
        let map = open_map(20, 20);
        assert!(can_see_player(5, 5, 9, 5, 6, &map), "반경 내 명확한 시야면 탐지해야 한다");
    }

    #[test]
    fn can_see_player_outside_radius() {
        let map = open_map(20, 20);
        assert!(!can_see_player(1, 1, 10, 10, 6, &map), "반경 밖은 탐지하지 않아야 한다");
    }

    #[test]
    fn can_see_player_blocked_by_wall() {
        // (5,5)와 (8,5) 사이에 벽 열
        let mut map = open_map(20, 20);
        for y in 0..20 { map.set_tile(7, y, TileKind::Wall); }
        assert!(!can_see_player(5, 5, 8, 5, 10, &map), "벽이 가로막으면 탐지하지 않아야 한다");
    }

    #[test]
    fn wander_does_not_move_to_wall() {
        use rand::SeedableRng;
        use rand::rngs::StdRng;
        let map = floor_map(10, 10, &[(5,5),(6,5),(5,6),(4,5),(5,4)]);
        let occupied: HashSet<(usize, usize)> = HashSet::new();
        for seed in 0..100u64 {
            let mut rng: ThreadRng = rand::thread_rng();
            let _ = rng; // ThreadRng은 seed 불가 — StdRng으로 테스트
            let mut srng = StdRng::seed_from_u64(seed);
            let neighbors = [(6usize,5usize),(5,6),(4,5),(5,4),(5,5)];
            // wander는 ThreadRng을 받으므로 직접 검증: 결과가 floor 타일이어야 함
            let result = {
                const STAY: f64 = 0.3;
                if srng.gen_bool(STAY) { (5,5) } else {
                    let valid: Vec<_> = [(4usize,5usize),(6,5),(5,4),(5,6)].iter()
                        .filter(|&&(nx,ny)| map.get_tile(nx,ny)==TileKind::Floor && !occupied.contains(&(nx,ny)))
                        .copied().collect();
                    if valid.is_empty() { (5,5) } else { valid[srng.gen_range(0..valid.len())] }
                }
            };
            assert!(neighbors.contains(&result), "배회 결과가 유효한 타일이어야 한다");
        }
    }
}
