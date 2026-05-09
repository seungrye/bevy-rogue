use bevy::prelude::*;
use crate::modules::{
    combat::Defeated,
    item::{EquipmentPanelOpen, PlayerEquipment, WeaponKind, weapon_attack},
    map::{
        MapResource, MAP_WIDTH, MAP_HEIGHT, TILE_SIZE,
        is_line_of_sight_clear, tile_to_world_coords, world_to_tile_coords, MonsterTiles,
        PlayerActedEvent,
    },
    player::Player,
    projectile::{FireProjectileEvent, BOW_RANGE},
    ui::{help::HelpPanelOpen, shop::ShopPanelOpen, LogMessage},
};

pub struct RangedPlugin;

impl Plugin for RangedPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RangedTargeting>()
            .add_systems(Update, (
                handle_ranged_input,
                handle_ranged_mouse,
                update_ranged_cursor,
            ).chain());
    }
}

#[derive(Resource, Default)]
pub struct RangedTargeting {
    pub active: bool,
    pub cursor: (usize, usize),
}

#[derive(Component)]
pub struct RangedCursor;

fn handle_ranged_input(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut targeting: ResMut<RangedTargeting>,
    equipment: Res<PlayerEquipment>,
    equipment_open: Res<EquipmentPanelOpen>,
    shop_open: Res<ShopPanelOpen>,
    help_open: Res<HelpPanelOpen>,
    player_q: Query<&Transform, (With<Player>, Without<Defeated>)>,
    monster_tiles: Res<MonsterTiles>,
    map_res: Res<MapResource>,
    asset_server: Res<AssetServer>,
    cursor_q: Query<Entity, With<RangedCursor>>,
    mut fire_writer: EventWriter<FireProjectileEvent>,
    mut acted_writer: EventWriter<PlayerActedEvent>,
    mut log: EventWriter<LogMessage>,
    items: Res<crate::modules::item::ItemRegistry>,
) {
    if equipment_open.0 || shop_open.0 || help_open.0 { return; }

    let Ok(player_transform) = player_q.get_single() else { return };
    let (px, py) = world_to_tile_coords(player_transform.translation);

    // 진입
    if !targeting.active {
        if keyboard.just_pressed(KeyCode::KeyF) {
            if equipment.weapon != Some(WeaponKind::BOW) {
                log.send(LogMessage("활을 장착해야 원격 공격이 가능하다.".into()));
                return;
            }
            let initial = nearest_target(px, py, &monster_tiles, map_res.map())
                .unwrap_or((px, py));
            targeting.active = true;
            targeting.cursor = initial;
            spawn_cursor(&mut commands, &asset_server, initial);
        }
        return;
    }

    // 활성 상태 — 입력 처리
    if keyboard.just_pressed(KeyCode::Escape) {
        targeting.active = false;
        for e in cursor_q.iter() { commands.entity(e).despawn(); }
        return;
    }

    if keyboard.just_pressed(KeyCode::Tab) {
        let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
        let targets = sorted_targets(px, py, &monster_tiles, map_res.map());
        if !targets.is_empty() {
            // 현재 cursor 와 일치하는 위치 idx 찾고 다음/이전.
            let cur_idx = targets.iter().position(|&t| t == targeting.cursor);
            let next_idx = match cur_idx {
                Some(i) if shift => (i + targets.len() - 1) % targets.len(),
                Some(i)          => (i + 1) % targets.len(),
                None             => 0,
            };
            targeting.cursor = targets[next_idx];
        }
        return;
    }

    // 자유 커서 이동 — 한 키 이벤트당 한 칸 (just_pressed)
    let mut dx: i32 = 0;
    let mut dy: i32 = 0;
    if keyboard.just_pressed(KeyCode::ArrowLeft)  || keyboard.just_pressed(KeyCode::KeyA) { dx -= 1; }
    if keyboard.just_pressed(KeyCode::ArrowRight) || keyboard.just_pressed(KeyCode::KeyD) { dx += 1; }
    if keyboard.just_pressed(KeyCode::ArrowUp)    || keyboard.just_pressed(KeyCode::KeyW) { dy += 1; }
    if keyboard.just_pressed(KeyCode::ArrowDown)  || keyboard.just_pressed(KeyCode::KeyS) { dy -= 1; }
    if dx != 0 || dy != 0 {
        let nx = (targeting.cursor.0 as i32 + dx).clamp(0, MAP_WIDTH as i32 - 1) as usize;
        let ny = (targeting.cursor.1 as i32 + dy).clamp(0, MAP_HEIGHT as i32 - 1) as usize;
        targeting.cursor = (nx, ny);
        return;
    }

    if keyboard.just_pressed(KeyCode::Enter) {
        let (tx, ty) = targeting.cursor;
        let map = map_res.map();
        let dx = tx as i32 - px as i32;
        let dy = ty as i32 - py as i32;
        let in_range = dx * dx + dy * dy <= BOW_RANGE * BOW_RANGE;
        if !in_range {
            log.send(LogMessage("사거리를 벗어났다.".into()));
            return;
        }
        if !is_line_of_sight_clear(map, px as i32, py as i32, tx as i32, ty as i32) {
            log.send(LogMessage("장애물에 막혔다.".into()));
            return;
        }
        fire_writer.send(FireProjectileEvent {
            origin_tile: (px, py),
            target_tile: (tx, ty),
            damage: weapon_attack(WeaponKind::BOW, &items),
            element: Some(crate::modules::elemental::Element::Lightning),
        });
        acted_writer.send(PlayerActedEvent);
        targeting.active = false;
        for e in cursor_q.iter() { commands.entity(e).despawn(); }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_ranged_mouse(
    mut commands: Commands,
    mut targeting: ResMut<RangedTargeting>,
    mouse_input: Res<ButtonInput<MouseButton>>,
    mut cursor_moved: EventReader<bevy::window::CursorMoved>,
    equipment_open: Res<EquipmentPanelOpen>,
    shop_open: Res<ShopPanelOpen>,
    help_open: Res<HelpPanelOpen>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<Camera>>,
    player_q: Query<&Transform, (With<Player>, Without<Defeated>)>,
    cursor_q: Query<Entity, With<RangedCursor>>,
    map_res: Res<MapResource>,
    mut fire_writer: EventWriter<FireProjectileEvent>,
    mut acted_writer: EventWriter<PlayerActedEvent>,
    mut log: EventWriter<LogMessage>,
    items: Res<crate::modules::item::ItemRegistry>,
) {
    if !targeting.active { return; }
    if equipment_open.0 || shop_open.0 || help_open.0 { return; }

    // 우클릭 — 취소.
    if mouse_input.just_pressed(MouseButton::Right) {
        targeting.active = false;
        for e in cursor_q.iter() { commands.entity(e).despawn(); }
        return;
    }

    let Ok(window) = windows.get_single() else { return };
    let Ok((camera, cam_transform)) = camera_q.get_single() else { return };
    let Ok(player_transform) = player_q.get_single() else { return };

    // 마우스 hover — 커서 갱신.
    if cursor_moved.read().next().is_some() {
        if let Some(cursor_pos) = window.cursor_position() {
            if let Some(world_pos) = camera.viewport_to_world_2d(cam_transform, cursor_pos) {
                let world_vec3 = Vec3::new(world_pos.x, world_pos.y, 0.0);
                let (tx, ty) = world_to_tile_coords(world_vec3);
                if tx < MAP_WIDTH && ty < MAP_HEIGHT {
                    targeting.cursor = (tx, ty);
                }
            }
        }
    }

    // 좌클릭 — 발사.
    if mouse_input.just_pressed(MouseButton::Left) {
        let (px, py) = world_to_tile_coords(player_transform.translation);
        let (tx, ty) = targeting.cursor;
        let map = map_res.map();
        let dx = tx as i32 - px as i32;
        let dy = ty as i32 - py as i32;
        let in_range = dx * dx + dy * dy <= BOW_RANGE * BOW_RANGE;
        if !in_range {
            log.send(LogMessage("사거리를 벗어났다.".into()));
            return;
        }
        if !is_line_of_sight_clear(map, px as i32, py as i32, tx as i32, ty as i32) {
            log.send(LogMessage("장애물에 막혔다.".into()));
            return;
        }
        fire_writer.send(FireProjectileEvent {
            origin_tile: (px, py),
            target_tile: (tx, ty),
            damage: weapon_attack(WeaponKind::BOW, &items),
            element: Some(crate::modules::elemental::Element::Lightning),
        });
        acted_writer.send(PlayerActedEvent);
        targeting.active = false;
        for e in cursor_q.iter() { commands.entity(e).despawn(); }
    }
}

fn update_ranged_cursor(
    targeting: Res<RangedTargeting>,
    map_res: Res<MapResource>,
    player_q: Query<&Transform, (With<Player>, Without<Defeated>)>,
    mut cursor_q: Query<(&mut Transform, &mut Text), (With<RangedCursor>, Without<Player>)>,
) {
    if !targeting.active { return; }
    let Ok((mut t, mut text)) = cursor_q.get_single_mut() else { return };
    let (cx, cy) = targeting.cursor;
    let coord = tile_to_world_coords(cx, cy);
    t.translation.x = coord.x;
    t.translation.y = coord.y;

    // 색상 — 사거리 + LoS 통과면 노랑, 실패면 빨강.
    let Ok(player_transform) = player_q.get_single() else { return };
    let (px, py) = world_to_tile_coords(player_transform.translation);
    let dx = cx as i32 - px as i32;
    let dy = cy as i32 - py as i32;
    let in_range = dx * dx + dy * dy <= BOW_RANGE * BOW_RANGE;
    let los = is_line_of_sight_clear(map_res.map(), px as i32, py as i32, cx as i32, cy as i32);
    let color = if in_range && los {
        Color::rgb(1.0, 0.95, 0.4)
    } else {
        Color::rgb(1.0, 0.3, 0.3)
    };
    if text.sections[0].style.color != color {
        text.sections[0].style.color = color;
    }
}

fn spawn_cursor(
    commands: &mut Commands,
    asset_server: &AssetServer,
    tile: (usize, usize),
) {
    let coord = tile_to_world_coords(tile.0, tile.1);
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");
    commands.spawn((
        Text2dBundle {
            text: Text::from_section("+", TextStyle {
                font,
                font_size: TILE_SIZE,
                color: Color::rgb(1.0, 0.95, 0.4),
            }),
            transform: Transform::from_xyz(coord.x, coord.y, 2.0),
            ..default()
        },
        RangedCursor,
    ));
}

/// FOV 안 + 사거리 안 적 위치를 player 와의 거리 오름차순으로 반환.
/// (tile_x, tile_y) 로 tie-break.
fn sorted_targets(
    px: usize, py: usize,
    monster_tiles: &MonsterTiles,
    map: &crate::modules::map::Map,
) -> Vec<(usize, usize)> {
    let mut v: Vec<(usize, usize)> = monster_tiles.0.iter()
        .copied()
        .filter(|&(x, y)| {
            let idx = y * MAP_WIDTH + x;
            if !map.tiles[idx].visible { return false; }
            let dx = x as i32 - px as i32;
            let dy = y as i32 - py as i32;
            dx * dx + dy * dy <= BOW_RANGE * BOW_RANGE
        })
        .collect();
    v.sort_by_key(|&(x, y)| {
        let dx = x as i32 - px as i32;
        let dy = y as i32 - py as i32;
        (dx * dx + dy * dy, x, y)
    });
    v
}

fn nearest_target(
    px: usize, py: usize,
    monster_tiles: &MonsterTiles,
    map: &crate::modules::map::Map,
) -> Option<(usize, usize)> {
    sorted_targets(px, py, monster_tiles, map).into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::map::{Map, TileKind};

    fn make_visible_map() -> Map {
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                map.set_tile(x, y, TileKind::Floor);
                let idx = y * MAP_WIDTH + x;
                map.tiles[idx].visible = true;
            }
        }
        map
    }

    #[test]
    fn sorted_targets_orders_by_distance() {
        let map = make_visible_map();
        let mut mt = MonsterTiles::default();
        mt.0.insert((10, 10));
        mt.0.insert((6, 6));
        mt.0.insert((8, 8));
        let v = sorted_targets(5, 5, &mt, &map);
        assert_eq!(v[0], (6, 6));
        assert_eq!(v[1], (8, 8));
        assert_eq!(v[2], (10, 10));
    }

    #[test]
    fn sorted_targets_excludes_out_of_range() {
        let map = make_visible_map();
        let mut mt = MonsterTiles::default();
        mt.0.insert((20, 5));  // 거리 15 — BOW_RANGE(8) 밖
        mt.0.insert((6, 6));
        let v = sorted_targets(5, 5, &mt, &map);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0], (6, 6));
    }

    #[test]
    fn sorted_targets_excludes_unseen() {
        let mut map = make_visible_map();
        // (6, 6) 만 invisible
        let idx = 6 * MAP_WIDTH + 6;
        map.tiles[idx].visible = false;
        let mut mt = MonsterTiles::default();
        mt.0.insert((6, 6));
        mt.0.insert((7, 7));
        let v = sorted_targets(5, 5, &mt, &map);
        assert_eq!(v, vec![(7, 7)]);
    }
}
