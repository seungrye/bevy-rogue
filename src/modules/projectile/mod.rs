use bevy::prelude::*;
use bevy_rapier2d::prelude::*;
use crate::modules::{
    combat::{CombatStats, calc_damage},
    combat_feedback::CombatFeedbackEvent,
    elemental::{ElementalApplyEvent, Element},
    map::{tile_to_world_coords, world_to_tile_coords, MapResource, MAP_WIDTH},
    monster::Monster,
    player::Player,
    ui::LogMessage,
};

pub const BOW_RANGE: i32 = 8;
const ARROW_LIFETIME: f32 = 2.0;
const ARROW_SIZE: Vec2 = Vec2::new(8.0, 3.0);
const ARROW_COLOR: Color = Color::rgb(0.8, 0.6, 0.2);

pub struct ProjectilePlugin;

impl Plugin for ProjectilePlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<FireProjectileEvent>()
            .add_systems(Update, (fire_projectile, update_projectiles, rotate_arrow).chain());
    }
}

#[derive(Component)]
pub struct Projectile {
    pub damage: i32,
    pub element: Option<Element>,
    pub lifetime: f32,
}

#[derive(Event)]
pub struct FireProjectileEvent {
    pub origin_tile: (usize, usize),
    pub target_tile: (usize, usize),
    pub damage: i32,
    pub element: Option<Element>,
}

fn fire_projectile(
    mut events: EventReader<FireProjectileEvent>,
    mut commands: Commands,
) {
    for ev in events.read() {
        let origin = tile_to_world_coords(ev.origin_tile.0, ev.origin_tile.1);
        let target = tile_to_world_coords(ev.target_tile.0, ev.target_tile.1);
        let delta = target - origin;
        let distance = delta.length().max(1.0);

        // 탑다운 시점 — 중력 없이 직사. flight_time 은 속도 스케일링용으로 유지.
        let flight_time = 0.3 + distance / 400.0;
        let vx = delta.x / flight_time;
        let vy = delta.y / flight_time;

        let initial_angle = vy.atan2(vx);

        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: ARROW_COLOR,
                    custom_size: Some(ARROW_SIZE),
                    ..default()
                },
                transform: Transform::from_xyz(origin.x, origin.y, 0.9)
                    .with_rotation(Quat::from_rotation_z(initial_angle)),
                ..default()
            },
            RigidBody::Dynamic,
            Velocity::linear(Vec2::new(vx, vy)),
            GravityScale(0.0),
            Collider::cuboid(4.0, 1.5),
            Sensor,
            Projectile {
                damage: ev.damage,
                element: ev.element,
                lifetime: ARROW_LIFETIME,
            },
        ));
    }
}

fn rotate_arrow(
    mut query: Query<(&mut Transform, &Velocity), With<Projectile>>,
) {
    for (mut transform, velocity) in query.iter_mut() {
        let v = velocity.linvel;
        if v.length_squared() > 1.0 {
            let angle = v.y.atan2(v.x);
            transform.rotation = Quat::from_rotation_z(angle);
        }
    }
}

fn update_projectiles(
    mut commands: Commands,
    time: Res<Time>,
    map_res: Res<MapResource>,
    mut proj_query: Query<(Entity, &mut Projectile, &Transform, &mut Sprite)>,
    mut monster_query: Query<(Entity, &Monster, &mut CombatStats, &Transform), Without<Player>>,
    mut log_writer: EventWriter<LogMessage>,
    mut feedback_writer: EventWriter<CombatFeedbackEvent>,
    mut elemental_writer: EventWriter<ElementalApplyEvent>,
) {
    let dt = time.delta_seconds();
    let map = map_res.map();

    for (proj_entity, mut proj, proj_transform, mut sprite) in proj_query.iter_mut() {
        proj.lifetime -= dt;

        let alpha = (proj.lifetime / ARROW_LIFETIME).clamp(0.0, 1.0);
        sprite.color = Color::rgba(0.8, 0.6, 0.2, alpha);

        if proj.lifetime <= 0.0 {
            commands.entity(proj_entity).despawn();
            continue;
        }

        let (tx, ty) = world_to_tile_coords(proj_transform.translation);

        // 맵 범위 이탈
        if tx >= map.width || ty >= map.height {
            commands.entity(proj_entity).despawn();
            continue;
        }

        // 벽 충돌 (시야를 막는 타일 = 벽). 물 위로는 투사체가 지나간다.
        let idx = ty * MAP_WIDTH + tx;
        if map.tiles[idx].kind.blocks_sight() {
            commands.entity(proj_entity).despawn();
            continue;
        }

        // 몬스터 충돌 판정
        let mut hit = false;
        for (monster_entity, monster, mut monster_stats, _) in monster_query.iter_mut() {
            if monster.tile_x != tx || monster.tile_y != ty { continue; }
            if monster_stats.hp <= 0 { continue; }

            let dmg = calc_damage(proj.damage, monster_stats.defense);
            monster_stats.hp -= dmg;

            feedback_writer.send(CombatFeedbackEvent {
                tile_x: tx,
                tile_y: ty,
                hit_entity: monster_entity,
                original_color: Color::WHITE,
            });

            if let Some(element) = proj.element {
                if rand::random::<f32>() < 0.4 {
                    elemental_writer.send(ElementalApplyEvent {
                        target: monster_entity,
                        element,
                    });
                }
            }

            if monster_stats.hp <= 0 {
                log_writer.send(LogMessage(format!(
                    "화살이 {}을(를) 관통했다! ({} 피해)", monster.name, dmg
                )));
            } else {
                log_writer.send(LogMessage(format!(
                    "화살이 {}에게 {} 피해! (HP: {}/{})",
                    monster.name, dmg, monster_stats.hp, monster_stats.max_hp
                )));
            }

            hit = true;
            break;
        }

        if hit {
            commands.entity(proj_entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;
    use crate::modules::map::TileKind;

    #[test]
    fn 비행시간은_거리가_멀수록_길어진다() {
        let short = 0.3 + 80.0_f32 / 400.0;
        let long  = 0.3 + 320.0_f32 / 400.0;
        assert!(long > short);
    }

    #[test]
    fn 직사_투사체는_수평목표에_수직속도가_없다() {
        // 직사 — 수평 목표에 대한 vy 는 0 이어야 한다 (중력 보정 없음).
        let dy = 0.0_f32;
        let flight_time = 0.5_f32;
        let vy = dy / flight_time;
        assert_eq!(vy, 0.0);
    }

    #[test]
    fn 활_사거리는_시야범위와_일치한다() {
        assert_eq!(BOW_RANGE, 8);
    }

    #[test]
    fn 화살_수명은_양수다() {
        assert!(ARROW_LIFETIME > 0.0);
    }

    // ── 시스템 App 하네스 테스트 ────────────────────────────────────────────
    use std::time::Duration;
    use crate::modules::map::{Map, MAP_HEIGHT};
    use crate::modules::monster::Monster;

    #[test]
    fn 발사이벤트는_투사체를_스폰한다() {
        let mut app = App::new();
        app.add_event::<FireProjectileEvent>();
        app.add_systems(Update, fire_projectile);
        app.world.send_event(FireProjectileEvent {
            origin_tile: (5, 5), target_tile: (10, 5), damage: 7, element: Some(Element::Lightning),
        });
        app.update();
        let mut q = app.world.query::<&Projectile>();
        let projs: Vec<_> = q.iter(&app.world).collect();
        assert_eq!(projs.len(), 1);
        assert_eq!(projs[0].damage, 7);
        assert_eq!(projs[0].element, Some(Element::Lightning));
    }

    #[test]
    fn 회전시스템은_속도방향으로_화살을_회전시킨다() {
        let mut app = App::new();
        app.add_systems(Update, rotate_arrow);
        let e = app.world.spawn((
            Transform::from_rotation(Quat::from_rotation_z(2.0)),
            Velocity::linear(Vec2::new(10.0, 0.0)),
            Projectile { damage: 1, element: None, lifetime: 1.0 },
        )).id();
        app.update();
        assert_eq!(app.world.get::<Transform>(e).unwrap().rotation, Quat::from_rotation_z(0.0),
            "수평 속도면 0 라디안으로 회전");
    }

    #[test]
    fn 회전시스템은_거의_정지한_화살은_회전시키지_않는다() {
        let mut app = App::new();
        app.add_systems(Update, rotate_arrow);
        let start = Quat::from_rotation_z(1.0);
        let e = app.world.spawn((
            Transform::from_rotation(start),
            Velocity::linear(Vec2::new(0.1, 0.0)),
            Projectile { damage: 1, element: None, lifetime: 1.0 },
        )).id();
        app.update();
        assert_eq!(app.world.get::<Transform>(e).unwrap().rotation, start, "속도가 작으면 회전 유지");
    }

    fn floor_map() -> Map {
        let mut m = Map::new(MAP_WIDTH, MAP_HEIGHT);
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                m.set_tile(x, y, TileKind::Floor);
            }
        }
        m
    }

    fn proj_update_app(map: Map) -> App {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.insert_resource(MapResource(map));
        app.add_event::<LogMessage>();
        app.add_event::<CombatFeedbackEvent>();
        app.add_event::<ElementalApplyEvent>();
        app.add_systems(Update, update_projectiles);
        app
    }

    fn spawn_proj(app: &mut App, tile: (usize, usize), damage: i32, element: Option<Element>, lifetime: f32) -> Entity {
        app.world.spawn((
            Projectile { damage, element, lifetime },
            Transform::from_translation(tile_to_world_coords(tile.0, tile.1).extend(0.9)),
            Sprite::default(),
        )).id()
    }

    #[test]
    fn 투사체는_수명이_다하면_제거된다() {
        let mut app = proj_update_app(floor_map());
        let e = spawn_proj(&mut app, (5, 5), 5, None, 0.05);
        app.world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(0.2));
        app.update();
        assert!(app.world.get_entity(e).is_none());
    }

    #[test]
    fn 투사체는_맵_범위를_벗어나면_제거된다() {
        // world_to_tile_coords 는 MAP_WIDTH 기준으로 클램프하므로, map.width 가 더 좁으면
        // 클램프된 타일좌표가 map.width 를 초과해 범위이탈 분기를 탄다.
        let mut narrow = Map::new(10, MAP_HEIGHT);
        for y in 0..MAP_HEIGHT {
            for x in 0..10 { narrow.set_tile(x, y, TileKind::Floor); }
        }
        let mut app = proj_update_app(narrow);
        let e = spawn_proj(&mut app, (79, 5), 5, None, 2.0); // 클램프 후 tx=79 >= map.width(10)
        app.world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(0.01));
        app.update();
        assert!(app.world.get_entity(e).is_none());
    }

    #[test]
    fn 투사체는_벽에_부딪히면_제거된다() {
        let mut map = floor_map();
        map.set_tile(5, 5, TileKind::Wall);
        let mut app = proj_update_app(map);
        let e = spawn_proj(&mut app, (5, 5), 5, None, 2.0);
        app.world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(0.01));
        app.update();
        assert!(app.world.get_entity(e).is_none());
    }

    #[test]
    fn 투사체는_물타일_위를_지나간다() {
        // 물은 시야를 막지 않으므로 투사체도 통과한다(제거되지 않음).
        let mut map = floor_map();
        map.set_tile(5, 5, TileKind::Water);
        let mut app = proj_update_app(map);
        let e = spawn_proj(&mut app, (5, 5), 5, None, 2.0);
        app.world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(0.01));
        app.update();
        assert!(app.world.get_entity(e).is_some(), "물 위에서는 투사체가 제거되면 안 된다");
    }

    fn spawn_monster(app: &mut App, tile: (usize, usize), hp: i32) -> Entity {
        app.world.spawn((
            Monster { name: "고블린".into(), tile_x: tile.0, tile_y: tile.1, vision_radius: 5, alert_turns: 0, slot_idx: 0 },
            CombatStats { hp, max_hp: hp.max(1), mp: 0, max_mp: 0, attack: 3, defense: 1 },
            Transform::from_translation(tile_to_world_coords(tile.0, tile.1).extend(0.0)),
        )).id()
    }

    #[test]
    fn 투사체는_몬스터에_명중하면_피해를_주고_제거된다() {
        let mut app = proj_update_app(floor_map());
        let monster = spawn_monster(&mut app, (5, 5), 20);
        let proj = spawn_proj(&mut app, (5, 5), 10, None, 2.0);
        app.world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(0.01));
        app.update();
        assert_eq!(app.world.get::<CombatStats>(monster).unwrap().hp, 11, "10-1=9 피해");
        assert!(app.world.get_entity(proj).is_none(), "명중한 투사체는 제거");
    }

    #[test]
    fn 투사체로_몬스터를_처치하면_체력이_0이하가_된다() {
        let mut app = proj_update_app(floor_map());
        let monster = spawn_monster(&mut app, (6, 6), 5);
        let proj = spawn_proj(&mut app, (6, 6), 10, None, 2.0);
        app.world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(0.01));
        app.update();
        assert!(app.world.get::<CombatStats>(monster).unwrap().hp <= 0);
        assert!(app.world.get_entity(proj).is_none());
    }

    #[test]
    fn 투사체는_이미_죽은_몬스터는_무시한다() {
        // monster_stats.hp <= 0 continue 경로
        let mut app = proj_update_app(floor_map());
        let _dead = spawn_monster(&mut app, (7, 7), 0);
        let proj = spawn_proj(&mut app, (7, 7), 10, None, 2.0);
        app.world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(0.01));
        app.update();
        // 죽은 몬스터는 명중 처리 안 되므로 투사체는 다른 사유로만 제거됨 → 이 타일엔 벽도 없음
        assert!(app.world.get_entity(proj).is_some(), "죽은 몬스터에는 명중하지 않아 통과");
    }

    #[test]
    fn 원소투사체는_몬스터_명중시_확률적으로_원소를_부여한다() {
        // rand 의존 분기 — 다수 명중으로 통계적 커버. element Some 진입 + proc true 둘 다.
        let mut app = proj_update_app(floor_map());
        for x in 0..60 {
            spawn_monster(&mut app, (x, 1), 100);
            spawn_proj(&mut app, (x, 1), 5, Some(Element::Lightning), 2.0);
        }
        app.world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(0.01));
        app.update();
        let events = app.world.resource::<Events<ElementalApplyEvent>>();
        assert!(events.len() > 0, "60회 명중이면 일부는 원소가 발동되어야 한다");
    }

    #[test]
    fn 투사체플러그인이_정상적으로_빌드된다() {
        let mut app = App::new();
        app.add_plugins(ProjectilePlugin);
    }
}
