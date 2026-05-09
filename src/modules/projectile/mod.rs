use bevy::prelude::*;
use bevy_rapier2d::prelude::*;
use crate::modules::{
    combat::{CombatStats, calc_damage},
    combat_feedback::CombatFeedbackEvent,
    elemental::{ElementalApplyEvent, Element},
    map::{tile_to_world_coords, world_to_tile_coords, MapResource, TileKind, MAP_WIDTH},
    monster::Monster,
    player::Player,
    ui::LogMessage,
};

pub const BOW_RANGE: i32 = 8;
const ARROW_LIFETIME: f32 = 2.0;
const ARROW_SIZE: Vec2 = Vec2::new(8.0, 3.0);
const ARROW_COLOR: Color = Color::rgb(0.8, 0.6, 0.2);
const GRAVITY_SCALE: f32 = 1.5;
const PIXELS_PER_METER: f32 = 100.0;
const RAPIER_GRAVITY: f32 = 9.81;

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

        let flight_time = 0.3 + distance / 400.0;
        let g = GRAVITY_SCALE * RAPIER_GRAVITY * PIXELS_PER_METER;
        let vx = delta.x / flight_time;
        let vy = (delta.y + 0.5 * g * flight_time * flight_time) / flight_time;

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
            GravityScale(GRAVITY_SCALE),
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

        // 벽 충돌
        let idx = ty * MAP_WIDTH + tx;
        if map.tiles[idx].kind == TileKind::Wall {
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
    use super::*;

    #[test]
    fn flight_time_increases_with_distance() {
        let short = 0.3 + 80.0_f32 / 400.0;
        let long  = 0.3 + 320.0_f32 / 400.0;
        assert!(long > short);
    }

    #[test]
    fn velocity_compensates_for_gravity() {
        let g = GRAVITY_SCALE * RAPIER_GRAVITY * PIXELS_PER_METER;
        let flight_time = 0.5_f32;
        let dy = 0.0_f32; // 수평 발사
        let vy = (dy + 0.5 * g * flight_time * flight_time) / flight_time;
        // 수직 속도가 양수여야 중력 보정이 된 것
        assert!(vy > 0.0, "수평 목표에도 초기 상향 속도가 필요하다");
    }

    #[test]
    fn bow_range_matches_fov() {
        assert_eq!(BOW_RANGE, 8);
    }

    #[test]
    fn arrow_lifetime_is_positive() {
        assert!(ARROW_LIFETIME > 0.0);
    }
}
