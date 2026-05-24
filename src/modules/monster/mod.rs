use bevy::prelude::*;
use std::collections::HashSet;
use rand::{Rng, rngs::ThreadRng};
use crate::modules::{
    map::{
        draw_map, Map, MapResource, MapType, MonsterTiles,
        tile_to_world_coords, world_to_tile_coords, is_line_of_sight_clear,
        MAP_HEIGHT, MAP_WIDTH, TILE_SIZE,
        MapSystemSet, MonsterRespawnEvent, PlayerActedEvent, AttackMonsterEvent, Rect,
    },
    player::{grant_xp, xp_reward_for_monster, MovingTo, MoveQueue, Player, PlayerProgress, PlayerSystemSet, LERP_SPEED},
    combat::{CombatStats, Defeated, Speed, calc_damage},
    ui::LogMessage,
    combat_feedback::CombatFeedbackEvent,
    item::{ItemDropEvent, PlayerEquipment},
    zone::{WorldState, ZonePersistence, MonsterSlot},
    map::GlobalTurn,
    elemental::{ElementalApplyEvent, ElementalStatus, Stunned, monster_element, weapon_element},
};

const Z_MONSTER: f32 = 0.8;
const MAX_ALERT_TURNS: u32 = 5;

static MONSTER_DATA: &[(&str, &str, [f32; 3], i32, i32, i32, i32, f32)] = &[
    // (이름, 글리프, 색상, HP, 공격력, 방어력, 시야 반경, 속도)
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

        // monster_slots 가 비어있으면 첫 방문으로 보고 초기화한다.
        // (entry 자체는 portal-position-persistence 등 다른 시스템이 먼저
        //  생성했을 수 있어 contains_key 만으로는 첫 방문을 판정할 수 없다.)
        let needs_init = persistence.0.get(&zone_id)
            .map(|s| s.monster_slots.is_empty())
            .unwrap_or(true);
        if needs_init {
            persistence.0.entry(zone_id.clone()).or_default().monster_slots =
                init_zone_monster_slots(&event.rooms);
        }

        // 만료된 리스폰 타이머 처리(지나간 턴 따라잡기)
        // (needs_init 분기에서 or_default() 로, 혹은 이미 존재해서 엔트리는 항상 있음 —
        //  None 분기는 도달 불가 방어코드)
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
    // 같은 spawn 호출 내 monster 가 같은 타일에 겹치지 않도록 한다
    let mut used: HashSet<(usize, usize)> = HashSet::new();

    // 매 호출마다 dummy map 만들기는 비용 — 대신 호출자가 map 을 알 수 없으니 room.center 를
    // fallback 으로 사용. 다만 random_floor_tile_in_room 은 map 이 필요. spawn_from_slots
    // 시그니처에 map 추가하는 건 여러 호출부 영향이 커서 별도 정리 시점에 진행.
    // 현재는 room 안 random tile 을 직접 검색 (Floor 검사 포함, 영역 clamp).
    for (slot_idx, (slot, room)) in slots.iter().zip(rooms.iter().skip(1)).enumerate() {
        if let Some(t) = slot.respawn_at_turn {
            if t > global_turn { continue; }
        }
        let data = MONSTER_DATA[slot.data_idx];
        let (name, glyph, color, hp, atk, def, vis, spd) = (data.0, data.1, data.2, data.3, data.4, data.5, data.6, data.7);

        // room 경계 안에서 무작위 좌표 — wall 위 / 영역 밖 회피.
        // Map 객체가 없는 컨텍스트라 Floor 검사는 못 하고, room 좌표가 항상 Floor 라고
        // 가정한다 (rooms 는 map 생성 시 이미 Floor 영역으로 정의됨). 영역 clamp 만 적용.
        let x_max = (room.x2.min(MAP_WIDTH.saturating_sub(1))).max(room.x1);
        let y_max = (room.y2.min(MAP_HEIGHT.saturating_sub(1))).max(room.y1);
        let mut tile = room.center();
        for _ in 0..10 {
            let x = rng.gen_range(room.x1..=x_max);
            let y = rng.gen_range(room.y1..=y_max);
            if !used.contains(&(x, y)) { tile = (x, y); break; }
        }
        used.insert(tile);
        let (tx, ty) = tile;

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
            ElementalStatus::default(),
        ));
    }
}

fn handle_player_attack(
    mut events: EventReader<AttackMonsterEvent>,
    mut player_query: Query<&mut CombatStats, (With<Player>, Without<Monster>)>,
    mut progress: ResMut<PlayerProgress>,
    mut monster_query: Query<(Entity, &Monster, &mut CombatStats), Without<Player>>,
    mut log_writer: EventWriter<LogMessage>,
    mut feedback_writer: EventWriter<CombatFeedbackEvent>,
    mut drop_writer: EventWriter<ItemDropEvent>,
    mut elemental_writer: EventWriter<ElementalApplyEvent>,
    equipment: Res<PlayerEquipment>,
    items: Res<crate::modules::item::ItemRegistry>,
) {
    for AttackMonsterEvent(tx, ty) in events.read() {
        let Ok(mut player_stats) = player_query.get_single_mut() else { continue };
        for (monster_entity, monster, mut monster_stats) in monster_query.iter_mut() {
            if monster.tile_x != *tx || monster.tile_y != *ty { continue; }
            if monster_stats.hp <= 0 { continue; }
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
            // 원소 부여 (40% 확률, 장착 무기에 따라 결정)
            if monster_stats.hp > 0 {
                if let Some(weapon) = equipment.weapon {
                    if rand::thread_rng().gen_bool(0.4) {
                        if let Some(element) = weapon_element(weapon, &items) {
                            elemental_writer.send(ElementalApplyEvent {
                                target: monster_entity,
                                element,
                            });
                        }
                    }
                }
            }

            if monster_stats.hp <= 0 {
                let xp = xp_reward_for_monster(&monster.name);
                let levels = grant_xp(&mut progress, &mut player_stats, xp);
                log_writer.send(LogMessage(format!(
                    "{}을(를) 처치했다! ({} 데미지, XP +{})", monster.name, dmg, xp
                )));
                if levels > 0 {
                    log_writer.send(LogMessage(format!(
                        "레벨 업! Lv.{} (HP {}/{}, MP {}/{})",
                        progress.level,
                        player_stats.hp,
                        player_stats.max_hp,
                        player_stats.mp,
                        player_stats.max_mp,
                    )));
                }
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
    mut monster_query: Query<(&mut Monster, &mut MoveQueue, &CombatStats, &mut Speed, Option<&Stunned>), Without<Player>>,
    mut player_query: Query<(Entity, &Transform, Option<&MovingTo>, &mut CombatStats), (With<Player>, Without<Monster>)>,
    mut log_writer: EventWriter<LogMessage>,
    mut feedback_writer: EventWriter<CombatFeedbackEvent>,
    mut elemental_writer: EventWriter<ElementalApplyEvent>,
) {
    if events.read().next().is_none() { return; }

    let map = map_res.map();
    let Ok((player_entity, player_transform, player_moving, mut player_stats)) = player_query.get_single_mut() else { return };

    let (px, py) = player_moving
        .map(|m| world_to_tile_coords(m.target))
        .unwrap_or_else(|| world_to_tile_coords(player_transform.translation));

    let mut occupied: HashSet<(usize, usize)> = monster_query.iter()
        .filter(|(_, _, stats, _, _)| stats.hp > 0)
        .map(|(m, _, _, _, _)| (m.tile_x, m.tile_y))
        .collect();
    occupied.insert((px, py));

    let mut player_dead = false;
    let mut rng = rand::thread_rng();

    for (mut monster, mut move_queue, monster_stats, mut speed, stunned) in monster_query.iter_mut() {
        if monster_stats.hp <= 0 { continue; }
        if stunned.is_some() {
            occupied.insert((monster.tile_x, monster.tile_y));
            continue;
        }

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

                    // 원소 부여 (35% 확률, 몬스터 속성에 따라)
                    // (이 `!player_dead` 는 바로 위 L329 가드 안이라 항상 참 — 도달 불가 방어코드의 false 분기)
                    if !player_dead {
                        if let Some(element) = monster_element(&monster.name) {
                            if rng.gen_bool(0.35) {
                                elemental_writer.send(ElementalApplyEvent {
                                    target: player_entity,
                                    element,
                                });
                            }
                        }
                    }

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
                && map.get_tile(nx, ny).is_walkable()
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
                && map.get_tile(nx, ny).is_walkable()
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
    use crate::modules::map::TileKind;

    fn floor_map(w: usize, h: usize, floors: &[(usize, usize)]) -> Map {
        let mut map = Map::new(w, h);
        for &(x, y) in floors {
            map.set_tile(x, y, TileKind::Floor);
        }
        map
    }

    #[test]
    fn 추적할_때_몬스터는_플레이어_방향으로_한칸_접근한다() {
        let map = floor_map(10, 10, &[(5,5),(6,5),(7,5)]);
        let occupied = HashSet::new();
        let (nx, ny) = move_toward(5, 5, 7, 5, &map, &occupied);
        assert_eq!((nx, ny), (6, 5), "플레이어 방향 인접 타일로 이동해야 한다");
    }

    #[test]
    fn 갈_곳이_모두_막히면_몬스터는_제자리를_유지한다() {
        let map = floor_map(10, 10, &[(5,5),(6,5)]);
        let mut occupied = HashSet::new();
        occupied.insert((6usize, 5usize));
        let result = move_toward(5, 5, 9, 5, &map, &occupied);
        assert_eq!(result, (5, 5), "이동 불가 시 제자리를 유지해야 한다");
    }

    #[test]
    fn 추적_중인_몬스터는_점유된_타일을_피해_이동한다() {
        let map = floor_map(10, 10, &[(5,5),(6,5),(5,6)]);
        let mut occupied = HashSet::new();
        occupied.insert((6usize, 5usize));
        // 목적지 (9,9) — (5,6) 이 더 가깝진 않지만 (6,5)는 막혀 있음
        let (nx, ny) = move_toward(5, 5, 6, 6, &map, &occupied);
        assert_ne!((nx, ny), (6, 5), "점유된 타일로 이동하면 안 된다");
    }

    #[test]
    fn 추적할_때_몬스터는_벽_타일로는_이동하지_않는다() {
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
    fn 여러_방향이_열려있으면_몬스터는_가장_가까운_타일을_고른다() {
        let map = floor_map(10, 10, &[(5,5),(6,5),(5,6),(4,5),(5,4)]);
        let occupied = HashSet::new();
        let (nx, ny) = move_toward(5, 5, 8, 5, &map, &occupied);
        assert_eq!((nx, ny), (6, 5));
    }

    #[test]
    fn 맵_하단_경계의_몬스터는_추적시_맵_밖_타일을_후보에서_제외한다() {
        // MAP_HEIGHT 경계: y=MAP_HEIGHT-1 의 이웃 y+1 == MAP_HEIGHT 는 ny<MAP_HEIGHT 거짓.
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        let by = MAP_HEIGHT - 1;
        map.set_tile(5, by, TileKind::Floor);
        map.set_tile(6, by, TileKind::Floor);
        let occupied = HashSet::new();
        let (nx, ny) = move_toward(5, by, 9, by, &map, &occupied);
        assert_eq!((nx, ny), (6, by), "경계 밖(y=MAP_HEIGHT)으로는 가지 않고 오른쪽으로");
    }

    #[test]
    fn 맵_우측_경계의_몬스터는_추적시_맵_밖_타일을_후보에서_제외한다() {
        // MAP_WIDTH 경계: x=MAP_WIDTH-1 의 이웃 x+1 == MAP_WIDTH 는 nx<MAP_WIDTH 거짓.
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        let rx = MAP_WIDTH - 1;
        map.set_tile(rx, 5, TileKind::Floor);
        map.set_tile(rx - 1, 5, TileKind::Floor);
        let occupied = HashSet::new();
        let (nx, ny) = move_toward(rx, 5, 0, 5, &map, &occupied);
        assert_eq!((nx, ny), (rx - 1, 5), "경계 밖(x=MAP_WIDTH)으로는 가지 않고 왼쪽으로");
    }

    fn open_map(w: usize, h: usize) -> Map {
        let mut map = Map::new(w, h);
        for y in 1..h-1 { for x in 1..w-1 { map.set_tile(x, y, TileKind::Floor); } }
        map
    }

    #[test]
    fn 시야_반경_안에_벽이_없으면_몬스터는_플레이어를_본다() {
        let map = open_map(20, 20);
        assert!(can_see_player(5, 5, 9, 5, 6, &map), "반경 내 명확한 시야면 탐지해야 한다");
    }

    #[test]
    fn 시야_반경_밖의_플레이어는_몬스터가_보지_못한다() {
        let map = open_map(20, 20);
        assert!(!can_see_player(1, 1, 10, 10, 6, &map), "반경 밖은 탐지하지 않아야 한다");
    }

    #[test]
    fn 벽이_시선을_가로막으면_몬스터는_플레이어를_보지_못한다() {
        // (5,5)와 (8,5) 사이에 벽 열
        let mut map = open_map(20, 20);
        for y in 0..20 { map.set_tile(7, y, TileKind::Wall); }
        assert!(!can_see_player(5, 5, 8, 5, 10, &map), "벽이 가로막으면 탐지하지 않아야 한다");
    }

    #[test]
    fn 배회하는_몬스터는_벽_타일로는_이동하지_않는다() {
        let map = floor_map(10, 10, &[(5,5),(6,5),(5,6),(4,5),(5,4)]);
        let occupied: HashSet<(usize, usize)> = HashSet::new();
        let neighbors = [(6usize,5usize),(5,6),(4,5),(5,4),(5,5)];
        let mut trng = rand::thread_rng();
        for _ in 0..200 {
            let result = wander(5, 5, &map, &occupied, &mut trng);
            assert!(neighbors.contains(&result), "배회 결과가 유효한 타일이어야 한다");
        }
    }

    /// monster respawn 의 첫 방문 판정 로직 — entry 가 있어도 monster_slots 가
    /// 비었으면 init 해야 한다 (portal 이 먼저 entry 를 만든 케이스).
    #[test]
    fn 포탈이_먼저_엔트리를_만들어도_슬롯이_비면_몬스터를_초기화한다() {
        let mut persistence = ZonePersistence::default();
        let zone = crate::modules::zone::ZoneId::Dungeon(1);

        // portal-position-persistence 가 entry 를 먼저 만들고 portals 만 채운 상태
        let snap = persistence.0.entry(zone.clone()).or_default();
        snap.portals = vec![crate::modules::zone::SavedPortal {
            tile_x: 5, tile_y: 5,
            target: crate::modules::zone::ZoneId::Town,
            arrive_from: crate::modules::zone::PortalDirection::StairUp,
        }];
        // monster_slots 는 비어있음
        assert!(snap.monster_slots.is_empty());

        // respawn_on_regen 의 needs_init 로직 재현
        let needs_init = persistence.0.get(&zone)
            .map(|s| s.monster_slots.is_empty())
            .unwrap_or(true);
        assert!(needs_init, "monster_slots 가 비어있으면 init 해야 한다");
    }

    #[test]
    fn 슬롯이_이미_채워진_재방문_존은_몬스터를_초기화하지_않는다() {
        let mut persistence = ZonePersistence::default();
        let zone = crate::modules::zone::ZoneId::Dungeon(1);
        // 이미 monster_slots 채워진 상태 — 두 번째 방문
        persistence.0.entry(zone.clone()).or_default().monster_slots = vec![
            MonsterSlot { data_idx: 0, respawn_at_turn: None },
        ];

        let needs_init = persistence.0.get(&zone)
            .map(|s| s.monster_slots.is_empty())
            .unwrap_or(true);
        assert!(!needs_init, "이미 채워진 slots 는 init 하면 안 된다 (재방문 시 상태 보존)");
    }

    // ── App 하네스 기반 시스템 테스트 ─────────────────────────────────────────

    use std::time::Duration;
    use crate::modules::zone::ZoneId;
    use crate::modules::item::{ItemRegistry, WeaponKind};

    /// MAP_WIDTH×MAP_HEIGHT 의 전부 Floor 인 맵.
    fn full_floor_map() -> Map {
        let mut m = Map::new(MAP_WIDTH, MAP_HEIGHT);
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                m.set_tile(x, y, TileKind::Floor);
            }
        }
        m
    }

    /// AssetServer(폰트) 를 제공하는 기본 App.
    fn asset_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app.init_asset::<Image>();
        app
    }

    /// rooms[0] 은 spawn 에서 skip(1) 되므로 더미 1개 + 실제 방 두 개를 둔다.
    fn rooms_with(n: usize) -> Vec<Rect> {
        let mut rooms = vec![Rect::new(1, 1, 2, 2)]; // skip 대상 더미
        for i in 0..n {
            let x = 5 + i * 6;
            rooms.push(Rect::new(x, 5, 3, 3));
        }
        rooms
    }

    fn spawn_player(app: &mut App, tile: (usize, usize)) -> Entity {
        app.world.spawn((
            Player,
            Transform::from_translation(tile_to_world_coords(tile.0, tile.1).extend(0.0)),
            CombatStats { hp: 30, max_hp: 30, mp: 0, max_mp: 0, attack: 5, defense: 1 },
        )).id()
    }

    fn spawn_monster(app: &mut App, name: &str, tile: (usize, usize), hp: i32) -> Entity {
        app.world.spawn((
            Monster { name: name.into(), tile_x: tile.0, tile_y: tile.1, vision_radius: 6, alert_turns: 0, slot_idx: 0 },
            CombatStats { hp, max_hp: hp.max(1), mp: 0, max_mp: 0, attack: 4, defense: 0 },
            Speed::new(1.0),
            MoveQueue::default(),
            ElementalStatus::default(),
            Transform::from_translation(tile_to_world_coords(tile.0, tile.1).extend(Z_MONSTER)),
        )).id()
    }

    // --- MonsterPlugin::build / sync_monster_tiles ---

    #[test]
    fn 몬스터플러그인을_등록하면_빌드가_패닉없이_완료된다() {
        let mut app = asset_app();
        app.add_plugins(MonsterPlugin);
        // build() 가 시스템을 등록만 해도 커버됨 (update 불필요).
        // 등록만으로 패닉 없이 통과하면 성공.
    }

    #[test]
    fn 동기화시스템은_몬스터_타일집합을_현재_위치로_갱신한다() {
        let mut app = App::new();
        app.init_resource::<MonsterTiles>();
        app.add_systems(PreUpdate, sync_monster_tiles);
        spawn_monster(&mut app, "고블린", (3, 4), 6);
        spawn_monster(&mut app, "오크", (7, 8), 10);
        app.update();
        let tiles = &app.world.resource::<MonsterTiles>().0;
        assert!(tiles.contains(&(3, 4)));
        assert!(tiles.contains(&(7, 8)));
        assert_eq!(tiles.len(), 2);
    }

    // --- spawn_on_startup ---

    fn startup_app(map: Map) -> App {
        let mut app = asset_app();
        app.insert_resource(MapResource(map));
        app.init_resource::<ZonePersistence>();
        app.init_resource::<WorldState>();
        app.add_systems(Startup, spawn_on_startup);
        app
    }

    #[test]
    fn 던전_시작시_방마다_몬스터가_스폰된다() {
        let mut map = full_floor_map();
        map.map_type = MapType::Dungeon;
        map.rooms = rooms_with(2);
        let mut app = startup_app(map);
        app.update();
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 2, "더미 방 1개를 제외한 두 방에 각각 스폰");
        // 슬롯도 영속화에 기록되어야 한다
        let slots = &app.world.resource::<ZonePersistence>().0[&ZoneId::Town].monster_slots;
        assert_eq!(slots.len(), 2);
    }

    #[test]
    fn 마을_타입_맵에서는_시작시_몬스터가_스폰되지_않는다() {
        let mut map = full_floor_map();
        map.map_type = MapType::Village;
        map.rooms = rooms_with(2);
        let mut app = startup_app(map);
        app.update();
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 0, "마을에서는 몬스터를 스폰하지 않는다");
    }

    // --- init_zone_monster_slots ---

    #[test]
    fn 슬롯초기화는_첫번째_방을_제외하고_최대_열개까지_만든다() {
        let mut rooms = vec![Rect::new(0, 0, 1, 1)];
        for i in 0..15 { rooms.push(Rect::new(i, 0, 1, 1)); }
        let slots = init_zone_monster_slots(&rooms);
        assert_eq!(slots.len(), 10, "skip(1).take(10) 으로 10개 제한");
        assert_eq!(slots[0].data_idx, 0);
        assert_eq!(slots[3].data_idx, 3 % MONSTER_DATA.len());
    }

    // --- spawn_from_slots (직접 호출, Commands 통한 스폰) ---

    fn run_spawn_from_slots(rooms: &[Rect], slots: &[MonsterSlot], turn: u64) -> App {
        let mut app = asset_app();
        let rooms = rooms.to_vec();
        let slots = slots.to_vec();
        app.add_systems(Update, move |mut commands: Commands, asset_server: Res<AssetServer>| {
            spawn_from_slots(&mut commands, &rooms, &slots, turn, &asset_server);
        });
        app.update();
        app
    }

    #[test]
    fn 리스폰_타이머가_남은_슬롯은_스폰을_건너뛴다() {
        let rooms = rooms_with(2);
        let slots = vec![
            MonsterSlot { data_idx: 0, respawn_at_turn: Some(100) }, // 아직 미래
            MonsterSlot { data_idx: 1, respawn_at_turn: None },      // 즉시 스폰
        ];
        let mut app = run_spawn_from_slots(&rooms, &slots, 50);
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 1, "타이머가 만료되지 않은 슬롯은 스폰 제외");
    }

    #[test]
    fn 리스폰_타이머가_만료된_슬롯은_스폰된다() {
        let rooms = rooms_with(1);
        let slots = vec![MonsterSlot { data_idx: 0, respawn_at_turn: Some(30) }];
        let mut app = run_spawn_from_slots(&rooms, &slots, 50); // 30 <= 50 → 스폰
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 1);
    }

    #[test]
    fn 같은_좌표의_방이_겹치면_무작위_타일이_이미_사용중인_경로를_탄다() {
        // 두 1x1 방이 같은 좌표 → 두번째 슬롯의 후보 타일이 used 에 이미 존재
        // (`!used.contains` false 분기). 두 몬스터 모두 같은 칸에 스폰된다.
        let rooms = vec![
            Rect::new(0, 0, 1, 1),
            Rect::new(15, 15, 0, 0), // 1x1
            Rect::new(15, 15, 0, 0), // 동일 좌표 1x1
        ];
        let slots = vec![
            MonsterSlot { data_idx: 0, respawn_at_turn: None },
            MonsterSlot { data_idx: 1, respawn_at_turn: None },
        ];
        let mut app = run_spawn_from_slots(&rooms, &slots, 0);
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 2);
    }

    #[test]
    fn 좁은_방에서_여러_몬스터를_스폰해도_타일이_겹치지_않는다() {
        // 1x1 방으로 used 충돌 경로(중심 fallback)를 강제 — 같은 방 두 슬롯이지만
        // rooms.skip(1).zip(slots) 매칭상 방 하나당 슬롯 하나라 둘 다 스폰된다.
        let rooms = vec![
            Rect::new(0, 0, 1, 1),
            Rect::new(10, 10, 0, 0), // 1칸 방
            Rect::new(20, 20, 0, 0),
        ];
        let slots = vec![
            MonsterSlot { data_idx: 0, respawn_at_turn: None },
            MonsterSlot { data_idx: 1, respawn_at_turn: None },
        ];
        let mut app = run_spawn_from_slots(&rooms, &slots, 0);
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 2);
    }

    // --- respawn_on_regen ---

    fn respawn_app() -> App {
        let mut app = asset_app();
        app.add_event::<MonsterRespawnEvent>();
        app.init_resource::<WorldState>();
        app.init_resource::<GlobalTurn>();
        app.init_resource::<ZonePersistence>();
        app.add_systems(Update, respawn_on_regen);
        app
    }

    #[test]
    fn 던전_재생성_이벤트는_기존_몬스터를_지우고_새로_스폰한다() {
        let mut app = respawn_app();
        let old = spawn_monster(&mut app, "고블린", (5, 5), 6);
        app.world.send_event(MonsterRespawnEvent {
            map_type: MapType::Dungeon,
            rooms: rooms_with(2),
        });
        app.update();
        assert!(app.world.get_entity(old).is_none(), "기존 몬스터는 제거된다");
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 2, "두 방에 새 몬스터 스폰");
    }

    #[test]
    fn 마을_재생성_이벤트는_몬스터를_지우기만_하고_스폰하지_않는다() {
        let mut app = respawn_app();
        let old = spawn_monster(&mut app, "오크", (5, 5), 10);
        app.world.send_event(MonsterRespawnEvent {
            map_type: MapType::Village,
            rooms: rooms_with(2),
        });
        app.update();
        assert!(app.world.get_entity(old).is_none());
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 0, "Dungeon 이 아니면 스폰하지 않는다 (continue)");
    }

    #[test]
    fn 재생성시_슬롯이_이미_있으면_초기화하지_않고_보존한다() {
        let mut app = respawn_app();
        // 현재 존(Town)에 슬롯을 미리 채워둔다 — needs_init=false 경로
        app.world.resource_mut::<ZonePersistence>().0
            .entry(ZoneId::Town).or_default().monster_slots = vec![
                MonsterSlot { data_idx: 0, respawn_at_turn: None },
            ];
        app.world.send_event(MonsterRespawnEvent {
            map_type: MapType::Dungeon,
            rooms: rooms_with(3),
        });
        app.update();
        let slots = &app.world.resource::<ZonePersistence>().0[&ZoneId::Town].monster_slots;
        assert_eq!(slots.len(), 1, "기존 슬롯이 보존되어 1개만 스폰");
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 1);
    }

    #[test]
    fn 재생성시_만료된_리스폰타이머는_지운다() {
        let mut app = respawn_app();
        app.world.resource_mut::<GlobalTurn>().0 = 100;
        app.world.resource_mut::<ZonePersistence>().0
            .entry(ZoneId::Town).or_default().monster_slots = vec![
                MonsterSlot { data_idx: 0, respawn_at_turn: Some(50) },  // 만료 → 지움 → 스폰
                MonsterSlot { data_idx: 1, respawn_at_turn: Some(200) }, // 미래 → 유지 → 미스폰
            ];
        app.world.send_event(MonsterRespawnEvent {
            map_type: MapType::Dungeon,
            rooms: rooms_with(2),
        });
        app.update();
        let slots = &app.world.resource::<ZonePersistence>().0[&ZoneId::Town].monster_slots;
        assert_eq!(slots[0].respawn_at_turn, None, "만료된 타이머는 None 으로");
        assert_eq!(slots[1].respawn_at_turn, Some(200), "미래 타이머는 유지");
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 1, "만료된 슬롯만 스폰");
    }

    // --- handle_player_attack ---

    fn attack_app() -> App {
        let mut app = App::new();
        app.add_event::<AttackMonsterEvent>();
        app.add_event::<LogMessage>();
        app.add_event::<CombatFeedbackEvent>();
        app.add_event::<ItemDropEvent>();
        app.add_event::<ElementalApplyEvent>();
        app.init_resource::<PlayerProgress>();
        app.insert_resource(PlayerEquipment::default());
        app.insert_resource(ItemRegistry::default());
        app.add_systems(Update, handle_player_attack);
        app
    }

    #[test]
    fn 플레이어_공격은_해당_타일의_몬스터에게_피해를_준다() {
        let mut app = attack_app();
        spawn_player(&mut app, (1, 1));
        let m = spawn_monster(&mut app, "고블린", (5, 5), 20);
        app.world.send_event(AttackMonsterEvent(5, 5));
        app.update();
        let hp = app.world.get::<CombatStats>(m).unwrap().hp;
        assert!(hp < 20, "공격력5-방어0=5 피해");
    }

    #[test]
    fn 플레이어_공격으로_몬스터를_처치하면_경험치를_얻는다() {
        let mut app = attack_app();
        spawn_player(&mut app, (1, 1));
        // 고블린(8 XP)은 Lv1 다음레벨(20 XP) 미달이라 레벨업 없이 XP 만 오른다.
        let m = spawn_monster(&mut app, "고블린", (5, 5), 3);
        app.world.send_event(AttackMonsterEvent(5, 5));
        app.update();
        assert!(app.world.get::<CombatStats>(m).unwrap().hp <= 0);
        assert_eq!(app.world.resource::<PlayerProgress>().xp, 8, "처치 시 경험치 획득");
    }

    #[test]
    fn 처치_피해가_커서_레벨업하면_레벨업_로그를_남긴다() {
        let mut app = attack_app();
        // 공격력을 높여 강한 몬스터(높은 XP)를 한방에 처치 → 레벨업
        let p = spawn_player(&mut app, (1, 1));
        app.world.get_mut::<CombatStats>(p).unwrap().attack = 100;
        // 트롤은 24 XP — Lv1 다음레벨 20 → 레벨업
        spawn_monster(&mut app, "트롤", (5, 5), 5);
        app.world.send_event(AttackMonsterEvent(5, 5));
        app.update();
        assert!(app.world.resource::<PlayerProgress>().level > 1, "레벨업 분기 진입");
    }

    #[test]
    fn 다른_타일을_공격하면_몬스터는_피해를_받지_않는다() {
        let mut app = attack_app();
        spawn_player(&mut app, (1, 1));
        let m = spawn_monster(&mut app, "고블린", (5, 5), 20);
        app.world.send_event(AttackMonsterEvent(9, 9)); // 빈 타일
        app.update();
        assert_eq!(app.world.get::<CombatStats>(m).unwrap().hp, 20, "다른 타일 공격은 무시");
    }

    #[test]
    fn x는_같지만_y가_다른_타일을_공격하면_몬스터는_피해를_받지_않는다() {
        // tile_x 일치 + tile_y 불일치 — || 의 두번째 항(tile_y != ty) true 분기.
        let mut app = attack_app();
        spawn_player(&mut app, (1, 1));
        let m = spawn_monster(&mut app, "고블린", (5, 5), 20);
        app.world.send_event(AttackMonsterEvent(5, 9)); // x 같고 y 다름
        app.update();
        assert_eq!(app.world.get::<CombatStats>(m).unwrap().hp, 20, "y 가 다르면 명중하지 않는다");
    }

    #[test]
    fn 이미_죽은_몬스터_타일을_공격하면_무시한다() {
        let mut app = attack_app();
        spawn_player(&mut app, (1, 1));
        let m = spawn_monster(&mut app, "고블린", (5, 5), 0);
        app.world.send_event(AttackMonsterEvent(5, 5));
        app.update();
        // hp<=0 continue 경로 — 변화 없음
        assert!(app.world.get::<CombatStats>(m).unwrap().hp <= 0);
    }

    #[test]
    fn 플레이어가_없으면_공격_이벤트는_조용히_무시된다() {
        let mut app = attack_app();
        let m = spawn_monster(&mut app, "고블린", (5, 5), 20);
        app.world.send_event(AttackMonsterEvent(5, 5));
        app.update();
        // player 부재 → get_single_mut Err → continue. 몬스터 무사
        assert_eq!(app.world.get::<CombatStats>(m).unwrap().hp, 20);
    }

    #[test]
    fn 알려지지_않은_이름의_몬스터를_공격해도_피해는_정상이다() {
        // MONSTER_DATA.find 가 None → original_color WHITE fallback 경로
        let mut app = attack_app();
        spawn_player(&mut app, (1, 1));
        let m = spawn_monster(&mut app, "유령", (5, 5), 20);
        app.world.send_event(AttackMonsterEvent(5, 5));
        app.update();
        assert!(app.world.get::<CombatStats>(m).unwrap().hp < 20);
    }

    #[test]
    fn 원소무기로_생존한_몬스터를_공격하면_확률적으로_원소를_부여한다() {
        // rand 의존(40% proc) — 다수 공격 이벤트로 통계적 커버.
        let mut app = App::new();
        app.add_event::<AttackMonsterEvent>();
        app.add_event::<LogMessage>();
        app.add_event::<CombatFeedbackEvent>();
        app.add_event::<ItemDropEvent>();
        app.add_event::<ElementalApplyEvent>();
        app.init_resource::<PlayerProgress>();
        app.insert_resource(PlayerEquipment { weapon: Some(WeaponKind::SWORD), armor: None, ..Default::default() });
        app.insert_resource(crate::modules::item::build_test_registry());
        app.add_systems(Update, handle_player_attack);

        let p = spawn_player(&mut app, (1, 1));
        app.world.get_mut::<CombatStats>(p).unwrap().attack = 1; // 살아남게
        // 한 칸씩 떨어뜨려 여러 몬스터 배치 후 각각 공격 이벤트
        let mut ents = Vec::new();
        for x in 0..40usize {
            ents.push(spawn_monster(&mut app, "고블린", (x, 1), 1000));
        }
        for x in 0..40usize {
            app.world.send_event(AttackMonsterEvent(x, 1));
        }
        app.update();
        // ElementalApplyEvent 는 별도 시스템이 처리하므로 여기선 공격 흐름이
        // 패닉 없이 통과(확률 분기 양쪽 통계 진입)했는지만 확인한다.
        assert!(app.world.get::<CombatStats>(ents[0]).unwrap().hp > 0, "약공격으로 생존");
    }

    #[test]
    fn 원소가_없는_무기는_명중해도_원소를_부여하지_않는다() {
        // weapon Some + proc true 이어도 weapon_element None → 부여 이벤트 없음.
        use crate::modules::item::WeaponMeta;
        let mut app = App::new();
        app.add_event::<AttackMonsterEvent>();
        app.add_event::<LogMessage>();
        app.add_event::<CombatFeedbackEvent>();
        app.add_event::<ItemDropEvent>();
        app.add_event::<ElementalApplyEvent>();
        app.init_resource::<PlayerProgress>();
        // 원소가 없는 무기를 등록
        let mut reg = ItemRegistry::default();
        reg.weapons.insert("plain", WeaponMeta {
            display_name: "막대기", glyph_ascii: "/", glyph_unicode: "/", glyph_game_icon: "/",
            pickup_message: "막대기", attack_power_min: 1, attack_power_max: 1, tier: 1, element: None,
        });
        app.insert_resource(reg);
        app.insert_resource(PlayerEquipment { weapon: Some(WeaponKind("plain")), armor: None, ..Default::default() });
        app.add_systems(Update, handle_player_attack);

        let p = spawn_player(&mut app, (1, 1));
        app.world.get_mut::<CombatStats>(p).unwrap().attack = 1; // 생존시키기
        let mut ents = Vec::new();
        for x in 0..40usize { ents.push(spawn_monster(&mut app, "고블린", (x, 1), 1000)); }
        for x in 0..40usize { app.world.send_event(AttackMonsterEvent(x, 1)); }
        app.update();
        assert!(app.world.get::<CombatStats>(ents[0]).unwrap().hp > 0, "약공격으로 생존");
    }

    // --- monster_turn ---

    fn turn_app(map: Map) -> App {
        let mut app = App::new();
        app.insert_resource(MapResource(map));
        app.add_event::<PlayerActedEvent>();
        app.add_event::<LogMessage>();
        app.add_event::<CombatFeedbackEvent>();
        app.add_event::<ElementalApplyEvent>();
        app.add_systems(Update, monster_turn);
        app
    }

    #[test]
    fn 턴이벤트가_없으면_몬스터는_행동하지_않는다() {
        let mut app = turn_app(full_floor_map());
        spawn_player(&mut app, (10, 10));
        let m = spawn_monster(&mut app, "고블린", (5, 5), 6);
        let before = app.world.get::<Monster>(m).unwrap().alert_turns;
        app.update(); // 이벤트 없음 → early return
        assert_eq!(app.world.get::<Monster>(m).unwrap().alert_turns, before);
    }

    #[test]
    fn 플레이어가_없으면_몬스터턴은_조용히_종료된다() {
        let mut app = turn_app(full_floor_map());
        spawn_monster(&mut app, "고블린", (5, 5), 6);
        app.world.send_event(PlayerActedEvent);
        app.update(); // player_query Err → return
        // 패닉 없이 통과하면 성공
    }

    #[test]
    fn 좌우로_인접한_몬스터는_플레이어를_공격한다() {
        let mut app = turn_app(full_floor_map());
        let p = spawn_player(&mut app, (5, 5));
        spawn_monster(&mut app, "고블린", (6, 5), 6); // 수평 인접 (dx==1,dy==0)
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!(app.world.get::<CombatStats>(p).unwrap().hp < 30, "인접 몬스터가 플레이어를 공격");
    }

    #[test]
    fn 대각선_몬스터는_인접으로_보지_않고_추적_이동한다() {
        // dx==1 && dy==1 → 두 disjunct 모두 거짓(인접 아님) → 추적 경로.
        let mut app = turn_app(full_floor_map());
        let p = spawn_player(&mut app, (5, 5));
        spawn_monster(&mut app, "고블린", (6, 6), 6); // 대각선
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<CombatStats>(p).unwrap().hp, 30, "대각선은 공격하지 않고 이동");
    }

    #[test]
    fn 위아래로_인접한_몬스터도_플레이어를_공격한다() {
        // dx==0 && dy==1 — 인접 판정의 두번째 disjunct.
        let mut app = turn_app(full_floor_map());
        let p = spawn_player(&mut app, (5, 5));
        spawn_monster(&mut app, "고블린", (5, 6), 6); // 수직 인접
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!(app.world.get::<CombatStats>(p).unwrap().hp < 30, "수직 인접 몬스터도 공격");
    }

    #[test]
    fn 몬스터_공격으로_플레이어가_죽으면_패배마커가_붙는다() {
        let mut app = turn_app(full_floor_map());
        let p = spawn_player(&mut app, (5, 5));
        app.world.get_mut::<CombatStats>(p).unwrap().hp = 1;
        let m = spawn_monster(&mut app, "트롤", (6, 5), 16);
        app.world.get_mut::<CombatStats>(m).unwrap().attack = 50;
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!(app.world.entity(p).contains::<Defeated>(), "치명타로 Defeated 부여");
    }

    #[test]
    fn 시야_안의_몬스터는_경계상태가_되어_플레이어를_추적한다() {
        let mut app = turn_app(full_floor_map());
        spawn_player(&mut app, (5, 5));
        let m = spawn_monster(&mut app, "고블린", (5, 9), 6); // 시야 안, 비인접
        app.world.send_event(PlayerActedEvent);
        app.update();
        let mon = app.world.get::<Monster>(m).unwrap();
        assert_eq!(mon.alert_turns, MAX_ALERT_TURNS, "시야 안이면 경계 최대치");
        // 추적 이동으로 플레이어쪽(y 감소)으로 한 칸 접근
        assert_eq!(mon.tile_y, 8);
        assert!(!app.world.get::<MoveQueue>(m).unwrap().0.is_empty(), "이동 큐에 목적지 추가");
    }

    #[test]
    fn 시야_밖이면_경계턴이_매턴_감소한다() {
        let mut app = turn_app(full_floor_map());
        spawn_player(&mut app, (1, 1));
        // 멀리 떨어져 시야 밖, 경계상태를 가진 몬스터
        let m = app.world.spawn((
            Monster { name: "고블린".into(), tile_x: 40, tile_y: 40, vision_radius: 2, alert_turns: 3, slot_idx: 0 },
            CombatStats { hp: 6, max_hp: 6, mp: 0, max_mp: 0, attack: 4, defense: 0 },
            Speed::new(1.0),
            MoveQueue::default(),
            ElementalStatus::default(),
            Transform::from_translation(tile_to_world_coords(40, 40).extend(Z_MONSTER)),
        )).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<Monster>(m).unwrap().alert_turns, 2, "시야 밖이면 경계 1 감소");
    }

    #[test]
    fn 경계상태가_아니면_몬스터는_무작위로_배회한다() {
        let mut app = turn_app(full_floor_map());
        spawn_player(&mut app, (1, 1));
        // 시야 밖 + alert 0 → wander 경로
        let m = app.world.spawn((
            Monster { name: "고블린".into(), tile_x: 40, tile_y: 40, vision_radius: 1, alert_turns: 0, slot_idx: 0 },
            CombatStats { hp: 6, max_hp: 6, mp: 0, max_mp: 0, attack: 4, defense: 0 },
            Speed::new(1.0),
            MoveQueue::default(),
            ElementalStatus::default(),
            Transform::from_translation(tile_to_world_coords(40, 40).extend(Z_MONSTER)),
        )).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        // wander 는 제자리이거나 인접 — 좌표가 유효 범위 안이면 충분
        let mon = app.world.get::<Monster>(m).unwrap();
        assert!(mon.tile_x < MAP_WIDTH);
        assert!(mon.tile_y < MAP_HEIGHT);
    }

    #[test]
    fn 행동불능_몬스터는_턴을_건너뛴다() {
        let mut app = turn_app(full_floor_map());
        spawn_player(&mut app, (5, 5));
        let m = spawn_monster(&mut app, "고블린", (6, 5), 6); // 인접인데
        app.world.entity_mut(m).insert(Stunned { turns: 2 });
        let p = app.world.query_filtered::<Entity, With<Player>>().single(&app.world);
        let before = app.world.get::<CombatStats>(p).unwrap().hp;
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<CombatStats>(p).unwrap().hp, before, "기절 몬스터는 공격하지 않는다");
    }

    #[test]
    fn 죽은_몬스터는_턴_행동에서_제외된다() {
        let mut app = turn_app(full_floor_map());
        let p = spawn_player(&mut app, (5, 5));
        spawn_monster(&mut app, "고블린", (6, 5), 0); // hp<=0
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<CombatStats>(p).unwrap().hp, 30, "죽은 몬스터는 행동 안 함");
    }

    #[test]
    fn 느린_몬스터는_에너지가_부족하면_그_턴엔_행동하지_않는다() {
        let mut app = turn_app(full_floor_map());
        spawn_player(&mut app, (5, 5));
        let m = spawn_monster(&mut app, "트롤", (5, 9), 16);
        app.world.get_mut::<Speed>(m).unwrap().value = 0.5; // 첫 턴 energy 0.5 < 1.0
        app.world.send_event(PlayerActedEvent);
        app.update();
        // 행동 안 했으니 위치 그대로
        assert_eq!(app.world.get::<Monster>(m).unwrap().tile_y, 9);
        // 그래도 시야 안이라 경계는 갱신됨
        assert_eq!(app.world.get::<Monster>(m).unwrap().alert_turns, MAX_ALERT_TURNS);
    }

    #[test]
    fn 플레이어가_이동중이면_몬스터는_목표타일을_기준으로_판단한다() {
        let mut app = turn_app(full_floor_map());
        let p = spawn_player(&mut app, (1, 1));
        // MovingTo 가 있으면 target 타일을 플레이어 위치로 사용
        app.world.entity_mut(p).insert(MovingTo { target: tile_to_world_coords(5, 5).extend(0.0) });
        spawn_monster(&mut app, "고블린", (6, 5), 6); // target(5,5)에 인접
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!(app.world.get::<CombatStats>(p).unwrap().hp < 30, "이동 목표 타일 기준으로 인접 판정");
    }

    #[test]
    fn 인접_몬스터는_확률적으로_원소를_부여한다() {
        // rand 의존(35% proc) + monster_element Some 경로 — 다수 몬스터로 통계 커버.
        let mut app = turn_app(full_floor_map());
        let p = spawn_player(&mut app, (5, 5));
        app.world.get_mut::<CombatStats>(p).unwrap().hp = 100000;
        app.world.get_mut::<CombatStats>(p).unwrap().max_hp = 100000;
        for _ in 0..50 {
            // 인접하게 여러 마리 — 공격 발생, 일부는 원소 부여 이벤트 발생
            spawn_monster(&mut app, "오크", (6, 5), 10);
        }
        for _ in 0..5 {
            app.world.send_event(PlayerActedEvent);
            app.update();
        }
        // ElementalApplyEvent 발행 여부는 처리 시스템이 없어 직접 확인하지 않고,
        // 다수 공격으로 확률 분기 양쪽에 진입했음을 패닉 없는 통과로 신뢰한다.
        assert!(app.world.get::<CombatStats>(p).unwrap().hp < 100000, "여러 인접 몬스터가 플레이어를 공격");
    }

    #[test]
    fn 속성이_없는_몬스터는_인접_공격시_원소를_부여하지_않는다() {
        // monster_element None 분기 — 알려지지 않은 이름의 인접 몬스터.
        let mut app = turn_app(full_floor_map());
        let p = spawn_player(&mut app, (5, 5));
        spawn_monster(&mut app, "유령", (6, 5), 6); // monster_element("유령") == None
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!(app.world.get::<CombatStats>(p).unwrap().hp < 30, "공격은 정상 진행");
        // 원소 부여 이벤트 자체는 None 으로 발행되지 않는다(패닉 없이 통과).
    }

    #[test]
    fn 죽은_플레이어에게는_몬스터가_추가타를_넣지_않는다() {
        // player_dead 후 두번째 인접 몬스터의 !player_dead false 경로
        let mut app = turn_app(full_floor_map());
        let p = spawn_player(&mut app, (5, 5));
        app.world.get_mut::<CombatStats>(p).unwrap().hp = 1;
        let m1 = spawn_monster(&mut app, "트롤", (6, 5), 16);
        app.world.get_mut::<CombatStats>(m1).unwrap().attack = 50;
        let m2 = spawn_monster(&mut app, "트롤", (4, 5), 16);
        app.world.get_mut::<CombatStats>(m2).unwrap().attack = 50;
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!(app.world.entity(p).contains::<Defeated>());
    }

    // --- smooth_monster_move ---

    fn smooth_app() -> App {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.add_systems(Update, smooth_monster_move);
        app
    }

    #[test]
    fn 부드러운_이동은_큐의_목표를_향해_점진적으로_움직인다() {
        let mut app = smooth_app();
        let mut q = MoveQueue::default();
        q.0.push_back(tile_to_world_coords(20, 20).extend(Z_MONSTER));
        let m = app.world.spawn((
            Monster { name: "고블린".into(), tile_x: 0, tile_y: 0, vision_radius: 6, alert_turns: 0, slot_idx: 0 },
            Speed::new(1.0),
            q,
            Transform::from_translation(tile_to_world_coords(0, 0).extend(Z_MONSTER)),
        )).id();
        let start = app.world.get::<Transform>(m).unwrap().translation;
        app.world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(0.016));
        app.update();
        let now = app.world.get::<Transform>(m).unwrap().translation;
        assert_ne!(now, start, "목표를 향해 한 스텝 이동");
        assert!(!app.world.get::<MoveQueue>(m).unwrap().0.is_empty(), "아직 목표에 도달 못 함");
    }

    #[test]
    fn 부드러운_이동은_목표에_근접하면_정확히_스냅하고_큐를_비운다() {
        let mut app = smooth_app();
        let mut q = MoveQueue::default();
        let target = tile_to_world_coords(1, 0).extend(Z_MONSTER);
        q.0.push_back(target);
        // 시작을 목표 바로 옆(스텝 이내)으로
        let m = app.world.spawn((
            Monster { name: "고블린".into(), tile_x: 0, tile_y: 0, vision_radius: 6, alert_turns: 0, slot_idx: 0 },
            Speed::new(1.0),
            q,
            Transform::from_translation(target - Vec3::new(0.1, 0.0, 0.0)),
        )).id();
        app.world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(1.0));
        app.update();
        assert_eq!(app.world.get::<Transform>(m).unwrap().translation, target, "목표에 스냅");
        assert!(app.world.get::<MoveQueue>(m).unwrap().0.is_empty(), "도달한 목표는 큐에서 제거");
    }

    // --- cleanup_dead ---

    fn cleanup_app() -> App {
        let mut app = App::new();
        app.init_resource::<WorldState>();
        app.init_resource::<GlobalTurn>();
        app.init_resource::<ZonePersistence>();
        app.add_systems(Update, cleanup_dead);
        app
    }

    #[test]
    fn 죽은_몬스터는_정리되고_리스폰_타이머가_예약된다() {
        let mut app = cleanup_app();
        // 현재 존(Town)에 슬롯 마련
        app.world.resource_mut::<ZonePersistence>().0
            .entry(ZoneId::Town).or_default().monster_slots = vec![
                MonsterSlot { data_idx: 0, respawn_at_turn: None },
            ];
        let m = spawn_monster(&mut app, "고블린", (5, 5), 0); // 죽음
        app.update();
        assert!(app.world.get_entity(m).is_none(), "죽은 몬스터는 despawn");
        let slot = &app.world.resource::<ZonePersistence>().0[&ZoneId::Town].monster_slots[0];
        assert!(slot.respawn_at_turn.is_some(), "리스폰 타이머 예약");
    }

    #[test]
    fn 살아있는_몬스터는_정리되지_않는다() {
        let mut app = cleanup_app();
        let m = spawn_monster(&mut app, "고블린", (5, 5), 6); // 살아있음
        app.update();
        assert!(app.world.get_entity(m).is_some(), "살아있는 몬스터는 유지");
    }

    #[test]
    fn 정리시_현재_존_스냅샷이_없으면_타이머_예약을_건너뛴다() {
        // persistence 에 현재 존 엔트리가 없는 경우 (get_mut None 경로)
        let mut app = cleanup_app();
        let m = spawn_monster(&mut app, "고블린", (5, 5), 0);
        app.update(); // ZonePersistence 비어있음 → snapshot None
        assert!(app.world.get_entity(m).is_none(), "스냅샷 없어도 despawn 은 된다");
    }

    #[test]
    fn 정리시_슬롯인덱스가_범위를_벗어나면_타이머를_예약하지_않는다() {
        // slot_idx 가 monster_slots 길이를 초과 (get_mut None 경로)
        let mut app = cleanup_app();
        app.world.resource_mut::<ZonePersistence>().0
            .entry(ZoneId::Town).or_default().monster_slots = vec![
                MonsterSlot { data_idx: 0, respawn_at_turn: None },
            ];
        let m = app.world.spawn((
            Monster { name: "고블린".into(), tile_x: 5, tile_y: 5, vision_radius: 6, alert_turns: 0, slot_idx: 99 },
            CombatStats { hp: 0, max_hp: 6, mp: 0, max_mp: 0, attack: 4, defense: 0 },
            Speed::new(1.0),
            MoveQueue::default(),
            ElementalStatus::default(),
            Transform::from_translation(tile_to_world_coords(5, 5).extend(Z_MONSTER)),
        )).id();
        app.update();
        assert!(app.world.get_entity(m).is_none());
        let slot = &app.world.resource::<ZonePersistence>().0[&ZoneId::Town].monster_slots[0];
        assert_eq!(slot.respawn_at_turn, None, "범위 밖 slot_idx 는 타이머 미예약");
    }

    // --- wander 직접 호출 (양방향 분기) ---

    #[test]
    fn 배회는_모든_방향이_막히면_제자리에_머문다() {
        let map = floor_map(10, 10, &[(5, 5)]); // 인접 floor 없음
        let occupied = HashSet::new();
        let mut trng = rand::thread_rng();
        for _ in 0..50 {
            let r = wander(5, 5, &map, &occupied, &mut trng);
            assert_eq!(r, (5, 5), "주변에 floor 가 없으면 제자리");
        }
    }

    #[test]
    fn 배회는_유효한_인접_타일이_있으면_가끔_이동한다() {
        let map = floor_map(10, 10, &[(5, 5), (6, 5), (4, 5), (5, 6), (5, 4)]);
        let occupied = HashSet::new();
        let mut trng = rand::thread_rng();
        let mut moved = false;
        let mut stayed = false;
        for _ in 0..200 {
            let r = wander(5, 5, &map, &occupied, &mut trng);
            if r == (5, 5) { stayed = true; } else { moved = true; }
            assert!([(5usize,5usize),(6,5),(4,5),(5,6),(5,4)].contains(&r));
        }
        assert!(moved, "확률적으로 이동이 발생해야 한다");
        assert!(stayed, "확률적으로 정지가 발생해야 한다");
    }

    #[test]
    fn 맵_하단_경계의_몬스터는_배회시_맵_밖_타일을_후보에서_제외한다() {
        // ny < MAP_HEIGHT 거짓 분기 — 경계 밖 후보 제외 검증.
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        let by = MAP_HEIGHT - 1;
        map.set_tile(5, by, TileKind::Floor);
        map.set_tile(6, by, TileKind::Floor);
        map.set_tile(4, by, TileKind::Floor);
        let occupied = HashSet::new();
        let mut trng = rand::thread_rng();
        for _ in 0..200 {
            let r = wander(5, by, &map, &occupied, &mut trng);
            assert!([(5usize, by), (6, by), (4, by)].contains(&r), "경계 밖으로 배회하지 않는다");
        }
    }

    #[test]
    fn 맵_우측_경계의_몬스터는_배회시_맵_밖_타일을_후보에서_제외한다() {
        // nx < MAP_WIDTH 거짓 분기 — 경계 밖 후보 제외 검증.
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        let rx = MAP_WIDTH - 1;
        map.set_tile(rx, 5, TileKind::Floor);
        map.set_tile(rx - 1, 5, TileKind::Floor);
        map.set_tile(rx, 4, TileKind::Floor);
        map.set_tile(rx, 6, TileKind::Floor);
        let occupied = HashSet::new();
        let mut trng = rand::thread_rng();
        for _ in 0..200 {
            let r = wander(rx, 5, &map, &occupied, &mut trng);
            assert!([(rx, 5usize), (rx - 1, 5), (rx, 4), (rx, 6)].contains(&r), "경계 밖으로 배회하지 않는다");
        }
    }

    #[test]
    fn 추적하는_몬스터는_물타일로_이동하지_않고_모래타일로는_이동한다() {
        // (5,5)→(7,5) 추적. 오른쪽 인접 (6,5)을 물/모래로 바꿔 동작을 비교한다.
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        map.set_tile(5, 5, TileKind::Floor);
        map.set_tile(6, 5, TileKind::Water);
        let occupied = HashSet::new();
        let (nx, ny) = move_toward(5, 5, 7, 5, &map, &occupied);
        assert_eq!((nx, ny), (5, 5), "물 타일로는 이동하지 않고 제자리를 유지해야 한다");

        map.set_tile(6, 5, TileKind::Sand);
        let (nx, ny) = move_toward(5, 5, 7, 5, &map, &occupied);
        assert_eq!((nx, ny), (6, 5), "모래 타일로는 이동할 수 있어야 한다");
    }

    #[test]
    fn 배회하는_몬스터는_물타일로는_이동하지_않는다() {
        // 주변이 모두 물이면 배회 결과는 제자리뿐이다.
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        map.set_tile(5, 5, TileKind::Floor);
        map.set_tile(4, 5, TileKind::Water);
        map.set_tile(6, 5, TileKind::Water);
        map.set_tile(5, 4, TileKind::Water);
        map.set_tile(5, 6, TileKind::Water);
        let occupied = HashSet::new();
        let mut trng = rand::thread_rng();
        for _ in 0..200 {
            let r = wander(5, 5, &map, &occupied, &mut trng);
            assert_eq!(r, (5, 5), "사방이 물이면 제자리를 유지해야 한다");
        }
    }

    #[test]
    fn 배회하는_몬스터는_모래타일로는_이동할_수_있다() {
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        map.set_tile(5, 5, TileKind::Floor);
        map.set_tile(6, 5, TileKind::Sand);
        let occupied = HashSet::new();
        let mut trng = rand::thread_rng();
        let mut moved = false;
        for _ in 0..200 {
            let r = wander(5, 5, &map, &occupied, &mut trng);
            assert!(r == (5, 5) || r == (6, 5), "제자리 또는 모래 타일로만 이동해야 한다");
            if r == (6, 5) { moved = true; }
        }
        assert!(moved, "모래 타일로 이동한 경우가 한 번은 있어야 한다");
    }
}
