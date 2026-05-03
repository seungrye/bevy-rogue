use crate::modules::{
    map::{
        draw_map, Map, MapResource, MapTile,
        tile_to_world_coords, world_to_tile_coords,
        MAP_HEIGHT, MAP_WIDTH, TILE_SIZE,
        MapSystemSet, PlayerRespawnEvent,
    },
    ui::LogMessage,
};
use bevy::prelude::*;

const LERP_SPEED: f32 = 7.5;

#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct MovingTo {
    pub target: Vec3,
}

fn spawn_player(mut commands: Commands, asset_server: Res<AssetServer>, map_res: Res<MapResource>) {
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");
    let (px, py) = if let Some(r) = map_res.map().rooms.first() {
        r.center()
    } else {
        warn!("방이 없어 맵 중앙에 스폰합니다.");
        (MAP_WIDTH / 2, MAP_HEIGHT / 2)
    };
    let coord = tile_to_world_coords(px, py);
    commands.spawn((
        Text2dBundle {
            text: Text::from_section("@", TextStyle {
                font,
                font_size: TILE_SIZE,
                color: Color::YELLOW,
            }),
            transform: Transform::from_xyz(coord.x, coord.y, 1.0),
            ..default()
        },
        Player,
    ));
}

fn player_movement(
    mut commands: Commands,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    player_query: Query<(Entity, &Transform), (With<Player>, Without<MovingTo>)>,
    map_res: Res<MapResource>,
    mut log_writer: EventWriter<LogMessage>,
) {
    let Ok((entity, transform)) = player_query.get_single() else { return };
    let mut delta = IVec2::ZERO;
    if keyboard_input.pressed(KeyCode::ArrowLeft)  || keyboard_input.pressed(KeyCode::KeyA) { delta.x -= 1; }
    if keyboard_input.pressed(KeyCode::ArrowRight) || keyboard_input.pressed(KeyCode::KeyD) { delta.x += 1; }
    if keyboard_input.pressed(KeyCode::ArrowUp)    || keyboard_input.pressed(KeyCode::KeyW) { delta.y += 1; }
    if keyboard_input.pressed(KeyCode::ArrowDown)  || keyboard_input.pressed(KeyCode::KeyS) { delta.y -= 1; }
    if delta == IVec2::ZERO { return; }

    let (cx, cy) = world_to_tile_coords(transform.translation);
    let tx = (cx as i32 + delta.x) as usize;
    let ty = (cy as i32 + delta.y) as usize;

    if map_res.map().get_tile(tx, ty) == MapTile::Floor {
        log_writer.send(LogMessage(format!("({}, {}) 로 이동합니다.", tx, ty)));
        let wp = tile_to_world_coords(tx, ty);
        commands.entity(entity).insert(MovingTo { target: Vec3::new(wp.x, wp.y, 1.0) });
    }
}

fn smooth_player_lerp(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut Transform, &MovingTo), With<Player>>,
) {
    for (entity, mut transform, moving) in query.iter_mut() {
        let dist = transform.translation.distance(moving.target);
        let step = LERP_SPEED * TILE_SIZE * time.delta_seconds();
        if dist < step {
            transform.translation = moving.target;
            commands.entity(entity).remove::<MovingTo>();
        } else {
            let dir = (moving.target - transform.translation).normalize();
            transform.translation += dir * step;
        }
    }
}

fn respawn_player_on_regen(
    mut commands: Commands,
    mut events: EventReader<PlayerRespawnEvent>,
    mut player_query: Query<(Entity, &mut Transform), With<Player>>,
) {
    for PlayerRespawnEvent(x, y) in events.read() {
        if let Ok((entity, mut transform)) = player_query.get_single_mut() {
            let wp = tile_to_world_coords(*x, *y);
            transform.translation = Vec3::new(wp.x, wp.y, 1.0);
            commands.entity(entity).remove::<MovingTo>();
        }
    }
}

fn camera_follow_player(
    player_query: Query<&Transform, With<Player>>,
    mut camera_query: Query<&mut Transform, (With<Camera>, Without<Player>)>,
) {
    let Ok(pt) = player_query.get_single() else { return };
    let Ok(mut ct) = camera_query.get_single_mut() else { return };
    ct.translation.x = pt.translation.x;
    ct.translation.y = pt.translation.y;
}

fn is_line_of_sight_clear(map: &Map, x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
    let (dx, dy) = ((x1 - x0).abs(), (y1 - y0).abs());
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let mut err = dx - dy;
    let (mut x, mut y) = (x0, y0);
    loop {
        if x < 0 || x >= map.width as i32 || y < 0 || y >= map.height as i32 { return false; }
        if x == x1 && y == y1 { return true; }
        if (x != x0 || y != y0) && map.tiles[map.index(x as usize, y as usize)] == MapTile::Wall {
            return false;
        }
        let e2 = 2 * err;
        if e2 > -dy { err -= dy; x += sx; }
        if e2 < dx  { err += dx; y += sy; }
    }
}

fn update_fov(
    player_query: Query<&Transform, With<Player>>,
    mut map_res: ResMut<MapResource>,
    mut last_pos: Local<Option<IVec2>>,
) {
    // 맵이 교체되면 강제 재계산
    if map_res.is_changed() {
        *last_pos = None;
    }

    let Ok(transform) = player_query.get_single() else { return };
    let (px, py) = world_to_tile_coords(transform.translation);
    let cur = IVec2::new(px as i32, py as i32);
    if Some(cur) == *last_pos { return; }
    *last_pos = Some(cur);

    let start = std::time::Instant::now();
    let map = map_res.map_mut();
    map.visible_tiles.iter_mut().for_each(|v| *v = false);

    let radius = 8i32;
    for y in (cur.y - radius)..=(cur.y + radius) {
        for x in (cur.x - radius)..=(cur.x + radius) {
            if x < 0 || x >= map.width as i32 || y < 0 || y >= map.height as i32 { continue; }
            let (dx, dy) = (x - cur.x, y - cur.y);
            if dx * dx + dy * dy > radius * radius { continue; }
            if is_line_of_sight_clear(map, cur.x, cur.y, x, y) {
                let idx = map.index(x as usize, y as usize);
                map.visible_tiles[idx] = true;
                map.revealed_tiles[idx] = true;
            }
        }
    }
    let elapsed = start.elapsed();
    if elapsed.as_micros() > 0 { info!("FOV: {:?}", elapsed); }
}

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_player.after(draw_map))
            .add_systems(Update, (
                player_movement,
                smooth_player_lerp.after(player_movement),
                update_fov.after(smooth_player_lerp),
                camera_follow_player.after(update_fov),
                respawn_player_on_regen.after(MapSystemSet::ExecuteRegen),
            ));
    }
}
