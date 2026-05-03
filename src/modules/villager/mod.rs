use bevy::prelude::*;
use rand::prelude::*;
use std::collections::HashSet;
use crate::modules::{
    map::{
        draw_map, Map, MapTile, MapType, OccupiedTiles, Rect,
        tile_to_world_coords, world_to_tile_coords,
        MAP_HEIGHT, MAP_WIDTH, TILE_SIZE,
        MapSystemSet, VillagerRespawnEvent, PlayerActedEvent, BumpTileEvent,
    },
    player::{Player, MovingTo, PlayerSystemSet},
    ui::LogMessage,
};

const VILLAGER_STAY_CHANCE: f64 = 0.3;
const Z_VILLAGER: f32 = 0.9;

static VILLAGER_DATA: &[(&str, [f32; 3], &[&str])] = &[
    // (name, [r,g,b], dialogues)
    ("촌장", [1.0, 0.85, 0.0], &[
        "어서오시게. 이 마을은 평화롭다네.",
        "요즘 주변에 이상한 소문이 들리는군.",
        "먼 길 오셨군. 조심해서 다니게나.",
    ]),
    ("상인", [0.3, 0.9, 0.3], &[
        "좋은 물건 있소이다!",
        "오늘만 특가라네, 어서 보시게.",
        "다음에 또 들르게나.",
    ]),
    ("농부", [0.8, 0.6, 0.3], &[
        "올해 수확이 풍성하길 바라네.",
        "하늘이 맑아 일하기 좋은 날이군.",
        "땅을 일구는 게 내 낙이라네.",
    ]),
    ("아이", [1.0, 1.0, 1.0], &[
        "안녕하세요!",
        "저기 던전에 가면 안 돼요!",
        "같이 놀아요!",
    ]),
    ("노인", [0.65, 0.65, 0.75], &[
        "이 마을에는 오랜 비밀이 있다네.",
        "오래전에 이 땅에 큰 전쟁이 있었지.",
        "젊은이, 몸 조심하게.",
    ]),
];

#[derive(Component)]
pub struct Villager {
    pub dialogues: Vec<String>,
    pub dialogue_idx: usize,
    pub tile_x: usize,
    pub tile_y: usize,
    pub just_bumped: bool,
}

// 이번 턴에 주민이 이동해야 하는지 판단하고 플래그를 초기화한다
pub fn take_turn(villager: &mut Villager) -> bool {
    if villager.just_bumped {
        villager.just_bumped = false;
        return false;
    }
    true
}

pub struct VillagerPlugin;

impl Plugin for VillagerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_on_startup.after(draw_map))
            .add_systems(PreUpdate, sync_occupied_tiles)
            .add_systems(Update, (
                respawn_on_regen.after(MapSystemSet::ExecuteRegen),
                (handle_bump, villager_turn)
                    .chain()
                    .after(PlayerSystemSet::Movement),
            ));
    }
}

// PreUpdate: 주민 위치를 OccupiedTiles에 동기화 (player_movement 이전 실행)
fn sync_occupied_tiles(
    villager_query: Query<&Villager>,
    mut occupied: ResMut<OccupiedTiles>,
) {
    occupied.0.clear();
    for v in villager_query.iter() {
        occupied.0.insert((v.tile_x, v.tile_y));
    }
}

fn spawn_on_startup(
    mut commands: Commands,
    map_res: Res<crate::modules::map::MapResource>,
    asset_server: Res<AssetServer>,
) {
    let map = map_res.map();
    if map.map_type == MapType::Village {
        do_spawn(&mut commands, &map.rooms.clone(), &asset_server);
    }
}

fn respawn_on_regen(
    mut commands: Commands,
    mut events: EventReader<VillagerRespawnEvent>,
    villager_query: Query<Entity, With<Villager>>,
    asset_server: Res<AssetServer>,
) {
    for event in events.read() {
        for entity in villager_query.iter() {
            commands.entity(entity).despawn();
        }
        if event.map_type == MapType::Village {
            do_spawn(&mut commands, &event.rooms, &asset_server);
        }
    }
}

fn do_spawn(commands: &mut Commands, rooms: &[Rect], asset_server: &AssetServer) {
    if rooms.is_empty() { return; }
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");

    // rooms[0] 은 플레이어 스폰 방 — 건너뜀
    for (i, room) in rooms.iter().skip(1).enumerate() {
        let data = &VILLAGER_DATA[i % VILLAGER_DATA.len()];
        let (name, color, lines) = (data.0, data.1, data.2);
        let (cx, cy) = room.center();
        let coord = tile_to_world_coords(cx, cy);

        let dialogues: Vec<String> = lines.iter()
            .map(|&s| format!("{}: {}", name, s))
            .collect();

        commands.spawn((
            Text2dBundle {
                text: Text::from_section("v", TextStyle {
                    font: font.clone(),
                    font_size: TILE_SIZE,
                    color: Color::rgb(color[0], color[1], color[2]),
                }),
                transform: Transform::from_xyz(coord.x, coord.y, Z_VILLAGER),
                ..default()
            },
            Villager {
                dialogues,
                dialogue_idx: 0,
                tile_x: cx,
                tile_y: cy,
                just_bumped: false,
            },
        ));
    }
}

// 플레이어가 주민 타일을 밀어 넣었을 때 대사를 출력한다
fn handle_bump(
    mut events: EventReader<BumpTileEvent>,
    mut villager_query: Query<&mut Villager>,
    mut log_writer: EventWriter<LogMessage>,
) {
    for BumpTileEvent(bx, by) in events.read() {
        for mut villager in villager_query.iter_mut() {
            if villager.tile_x == *bx && villager.tile_y == *by {
                let msg = villager.dialogues[villager.dialogue_idx].clone();
                log_writer.send(LogMessage(msg));
                villager.dialogue_idx = next_dialogue_idx(villager.dialogue_idx, villager.dialogues.len());
                villager.just_bumped = true;
                break;
            }
        }
    }
}

// 플레이어가 행동한 턴에 주민이 한 번 이동한다
fn villager_turn(
    mut events: EventReader<PlayerActedEvent>,
    map_res: Res<crate::modules::map::MapResource>,
    mut villager_query: Query<(&mut Villager, &mut Transform)>,
    player_query: Query<(&Transform, Option<&MovingTo>), (With<Player>, Without<Villager>)>,
) {
    if events.read().next().is_none() { return; }

    let map = map_res.map();
    let mut rng = thread_rng();

    // 주민 위치 + 플레이어 논리적 목적지로 점유셋 구성 (overlap 방지)
    // Transform은 lerp 중간값이므로 MovingTo(목적지)가 있으면 그 타일을 사용
    let mut occupied: HashSet<(usize, usize)> = villager_query.iter()
        .map(|(v, _)| (v.tile_x, v.tile_y))
        .collect();
    if let Ok((pt, moving)) = player_query.get_single() {
        let player_tile = moving
            .map(|m| world_to_tile_coords(m.target))
            .unwrap_or_else(|| world_to_tile_coords(pt.translation));
        occupied.insert(player_tile);
    }

    for (mut villager, mut transform) in villager_query.iter_mut() {
        occupied.remove(&(villager.tile_x, villager.tile_y));
        if !take_turn(&mut villager) {
            occupied.insert((villager.tile_x, villager.tile_y));
            continue;
        }
        let (nx, ny) = pick_next_tile(villager.tile_x, villager.tile_y, map, &occupied, &mut rng);
        occupied.insert((nx, ny));

        villager.tile_x = nx;
        villager.tile_y = ny;
        let wp = tile_to_world_coords(nx, ny);
        transform.translation = Vec3::new(wp.x, wp.y, Z_VILLAGER);
    }
}


pub fn pick_next_tile(
    x: usize, y: usize,
    map: &Map,
    occupied: &HashSet<(usize, usize)>,
    rng: &mut impl Rng,
) -> (usize, usize) {
    if rng.gen_bool(VILLAGER_STAY_CHANCE) {
        return (x, y);
    }
    let neighbors = [
        (x.wrapping_sub(1), y),
        (x + 1, y),
        (x, y.wrapping_sub(1)),
        (x, y + 1),
    ];
    let valid: Vec<(usize, usize)> = neighbors.iter()
        .filter(|&&(nx, ny)| {
            nx < MAP_WIDTH && ny < MAP_HEIGHT
                && map.get_tile(nx, ny) == MapTile::Floor
                && !occupied.contains(&(nx, ny))
        })
        .copied()
        .collect();

    if valid.is_empty() { (x, y) } else { *valid.choose(rng).unwrap() }
}

pub fn next_dialogue_idx(current: usize, total: usize) -> usize {
    if total == 0 { return 0; }
    (current + 1) % total
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    #[test]
    fn next_dialogue_advances() {
        assert_eq!(next_dialogue_idx(0, 3), 1);
        assert_eq!(next_dialogue_idx(1, 3), 2);
    }

    #[test]
    fn next_dialogue_wraps_at_end() {
        assert_eq!(next_dialogue_idx(2, 3), 0);
    }

    #[test]
    fn next_dialogue_single_stays_zero() {
        assert_eq!(next_dialogue_idx(0, 1), 0);
    }

    #[test]
    fn next_dialogue_zero_total_returns_zero() {
        assert_eq!(next_dialogue_idx(0, 0), 0);
    }

    #[test]
    fn pick_next_tile_surrounded_by_walls_stays_put() {
        let mut map = Map::new(10, 10);
        map.set_tile(5, 5, MapTile::Floor);
        let occupied = HashSet::new();
        let mut rng = StdRng::seed_from_u64(0);
        for _ in 0..50 {
            let result = pick_next_tile(5, 5, &map, &occupied, &mut rng);
            assert_eq!(result, (5, 5));
        }
    }

    #[test]
    fn pick_next_tile_returns_floor_neighbor() {
        let mut map = Map::new(10, 10);
        map.set_tile(5, 5, MapTile::Floor);
        map.set_tile(6, 5, MapTile::Floor);
        let occupied = HashSet::new();
        let mut moved = false;
        for seed in 0..200u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            if pick_next_tile(5, 5, &map, &occupied, &mut rng) == (6, 5) {
                moved = true;
                break;
            }
        }
        assert!(moved, "인접 Floor 타일로 이동해야 한다");
    }

    #[test]
    fn pick_next_tile_never_moves_to_wall() {
        let mut map = Map::new(10, 10);
        map.set_tile(5, 5, MapTile::Floor);
        map.set_tile(6, 5, MapTile::Floor);
        map.set_tile(4, 5, MapTile::Floor);
        let occupied = HashSet::new();
        for seed in 0..500u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let (_, ny) = pick_next_tile(5, 5, &map, &occupied, &mut rng);
            assert_eq!(ny, 5, "Wall 타일(y!=5)로 이동하면 안 된다");
        }
    }

    #[test]
    fn pick_next_tile_skips_occupied_neighbor() {
        let mut map = Map::new(10, 10);
        map.set_tile(5, 5, MapTile::Floor);
        map.set_tile(6, 5, MapTile::Floor);
        let mut occupied = HashSet::new();
        occupied.insert((6usize, 5usize)); // 유일한 이웃이 점유됨
        for seed in 0..200u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let result = pick_next_tile(5, 5, &map, &occupied, &mut rng);
            assert_eq!(result, (5, 5), "점유된 타일로 이동하면 안 된다");
        }
    }

    #[test]
    fn take_turn_returns_false_and_resets_flag_when_bumped() {
        let mut v = Villager {
            dialogues: vec![],
            dialogue_idx: 0,
            tile_x: 0,
            tile_y: 0,
            just_bumped: true,
        };
        assert!(!take_turn(&mut v), "충돌 직후에는 이동하지 않아야 한다");
        assert!(!v.just_bumped, "플래그는 한 번만 소모된다");
    }

    #[test]
    fn take_turn_returns_true_when_not_bumped() {
        let mut v = Villager {
            dialogues: vec![],
            dialogue_idx: 0,
            tile_x: 0,
            tile_y: 0,
            just_bumped: false,
        };
        assert!(take_turn(&mut v), "충돌 없는 주민은 정상 이동해야 한다");
    }

    // villager_turn 에서 플레이어 타일(Transform 또는 MovingTo 목적지)을
    // occupied 에 추가해 overlap 을 방지하는 메커니즘 검증
    #[test]
    fn pick_next_tile_blocked_by_player_tile() {
        let mut map = Map::new(10, 10);
        map.set_tile(5, 5, MapTile::Floor);
        map.set_tile(6, 5, MapTile::Floor);
        let mut occupied = HashSet::new();
        occupied.insert((6usize, 5usize)); // 플레이어 현재 위치 또는 MovingTo 목적지
        for seed in 0..200u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let result = pick_next_tile(5, 5, &map, &occupied, &mut rng);
            assert_eq!(result, (5, 5), "플레이어 타일로 이동하면 안 된다");
        }
    }
}
