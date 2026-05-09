use bevy::prelude::*;
use bevy_rapier2d::prelude::*;
use rand::Rng;
use crate::modules::map::{tile_to_world_coords, TILE_SIZE, PlayerActedEvent};

pub const HIT_FLASH_DURATION: f32 = 0.15;
pub const Z_BLOOD: f32 = 0.5;
const BLOOD_LIFETIME_MIN: u32 = 15;
const BLOOD_LIFETIME_MAX: u32 = 30;
const PARTICLE_COUNT: usize = 8;
const PARTICLE_LIFETIME: f32 = 0.45;
const PARTICLE_SIZE: f32 = 3.0;

#[derive(Component)]
pub struct BloodStain {
    pub alpha: f32,
    pub decay_per_turn: f32,
}

#[derive(Component)]
pub struct BloodParticle {
    lifetime: f32,
}

#[derive(Component)]
pub struct HitFlash {
    pub remaining: f32,
    pub original_color: Color,
}

#[derive(Event)]
pub struct CombatFeedbackEvent {
    pub tile_x: usize,
    pub tile_y: usize,
    pub hit_entity: Entity,
    pub original_color: Color,
}

pub struct CombatFeedbackPlugin;

impl Plugin for CombatFeedbackPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<CombatFeedbackEvent>()
            .add_systems(Update, (
                handle_combat_feedback,
                fade_blood_stains,
                apply_hit_flash,
            ).chain())
            .add_systems(Update, update_blood_particles);
    }
}

fn handle_combat_feedback(
    mut events: EventReader<CombatFeedbackEvent>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    flash_query: Query<&HitFlash>,
) {
    for event in events.read() {
        let pos = tile_to_world_coords(event.tile_x, event.tile_y);
        let lifetime = rand::thread_rng().gen_range(BLOOD_LIFETIME_MIN..=BLOOD_LIFETIME_MAX);
        commands.spawn((
            Text2dBundle {
                text: Text::from_section("%", TextStyle {
                    font: asset_server.load("fonts/FiraMono-Medium.ttf"),
                    font_size: TILE_SIZE,
                    color: Color::rgba(0.8, 0.0, 0.0, 1.0),
                }),
                transform: Transform::from_xyz(pos.x, pos.y, Z_BLOOD),
                ..default()
            },
            BloodStain { alpha: 1.0, decay_per_turn: 1.0 / lifetime as f32 },
        ));

        spawn_blood_particles(pos, &mut commands);

        if let Some(mut ec) = commands.get_entity(event.hit_entity) {
            let original_color = flash_query.get(event.hit_entity)
                .map(|f| f.original_color)
                .unwrap_or(event.original_color);
            ec.insert(HitFlash {
                remaining: HIT_FLASH_DURATION,
                original_color,
            });
        }
    }
}

fn spawn_blood_particles(pos: Vec2, commands: &mut Commands) {
    let mut rng = rand::thread_rng();
    for _ in 0..PARTICLE_COUNT {
        let angle = rng.gen_range(0.0_f32..std::f32::consts::TAU);
        let speed = rng.gen_range(30.0_f32..120.0);
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgba(0.85, 0.05, 0.05, 1.0),
                    custom_size: Some(Vec2::splat(PARTICLE_SIZE)),
                    ..default()
                },
                transform: Transform::from_xyz(pos.x, pos.y, Z_BLOOD + 0.1),
                ..default()
            },
            RigidBody::Dynamic,
            Velocity::linear(Vec2::new(angle.cos() * speed, angle.sin() * speed)),
            GravityScale(2.0),
            Collider::ball(1.5),
            Sensor,
            BloodParticle { lifetime: PARTICLE_LIFETIME },
        ));
    }
}

fn update_blood_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut BloodParticle, &mut Sprite)>,
) {
    let dt = time.delta_seconds();
    for (entity, mut particle, mut sprite) in query.iter_mut() {
        particle.lifetime -= dt;
        if particle.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        } else {
            let alpha = particle.lifetime / PARTICLE_LIFETIME;
            sprite.color = Color::rgba(0.85, 0.05, 0.05, alpha);
        }
    }
}

fn fade_blood_stains(
    mut commands: Commands,
    mut turn_events: EventReader<PlayerActedEvent>,
    mut query: Query<(Entity, &mut BloodStain, &mut Text)>,
) {
    if turn_events.read().next().is_none() { return; }
    for (entity, mut stain, mut text) in query.iter_mut() {
        stain.alpha = blood_stain_alpha_after_decay(stain.alpha, stain.decay_per_turn);
        text.sections[0].style.color = Color::rgba(0.8, 0.0, 0.0, stain.alpha);
        if stain.alpha <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

fn apply_hit_flash(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut HitFlash, &mut Text)>,
) {
    let dt = time.delta_seconds();
    for (entity, mut flash, mut text) in query.iter_mut() {
        flash.remaining = hit_flash_remaining_after(flash.remaining, dt);
        if flash.remaining <= 0.0 {
            text.sections[0].style.color = flash.original_color;
            commands.entity(entity).remove::<HitFlash>();
        } else {
            text.sections[0].style.color = Color::rgb(1.0, 0.0, 0.0);
        }
    }
}

pub fn blood_stain_alpha_after_decay(current: f32, per_turn: f32) -> f32 {
    (current - per_turn).max(0.0)
}

pub fn hit_flash_remaining_after(remaining: f32, dt: f32) -> f32 {
    (remaining - dt).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decay_per_turn_for(lifetime_turns: u32) -> f32 {
        1.0 / lifetime_turns as f32
    }

    #[test]
    fn blood_stain_starts_fully_visible() {
        assert_eq!(blood_stain_alpha_after_decay(1.0, 0.0), 1.0);
    }

    #[test]
    fn blood_stain_alpha_clamps_to_zero() {
        let decay = decay_per_turn_for(4);
        let a = blood_stain_alpha_after_decay(0.1, decay);
        assert_eq!(a, 0.0);
    }

    #[test]
    fn blood_stain_gone_at_min_lifetime() {
        let decay = decay_per_turn_for(BLOOD_LIFETIME_MIN);
        let mut a = 1.0_f32;
        for _ in 0..BLOOD_LIFETIME_MIN { a = blood_stain_alpha_after_decay(a, decay); }
        assert!(a < 1e-4, "최소 수명({} 턴) 후 거의 사라져야 한다 (실제: {})", BLOOD_LIFETIME_MIN, a);
    }

    #[test]
    fn blood_stain_gone_at_max_lifetime() {
        let decay = decay_per_turn_for(BLOOD_LIFETIME_MAX);
        let mut a = 1.0_f32;
        for _ in 0..BLOOD_LIFETIME_MAX { a = blood_stain_alpha_after_decay(a, decay); }
        assert!(a < 1e-4, "최대 수명({} 턴) 후 거의 사라져야 한다 (실제: {})", BLOOD_LIFETIME_MAX, a);
    }

    #[test]
    fn blood_stain_still_visible_before_lifetime_ends() {
        let decay = decay_per_turn_for(BLOOD_LIFETIME_MAX);
        let mut a = 1.0_f32;
        for _ in 0..BLOOD_LIFETIME_MIN { a = blood_stain_alpha_after_decay(a, decay); }
        assert!(a > 0.0, "최소 수명보다 적은 턴 후엔 아직 보여야 한다");
    }

    #[test]
    fn lifetime_range_is_valid() {
        assert!(BLOOD_LIFETIME_MIN >= 1);
        assert!(BLOOD_LIFETIME_MAX >= BLOOD_LIFETIME_MIN);
    }

    #[test]
    fn hit_flash_starts_active() {
        assert!(hit_flash_remaining_after(HIT_FLASH_DURATION, 0.0) > 0.0);
    }

    #[test]
    fn hit_flash_remaining_decreases() {
        let r = hit_flash_remaining_after(0.15, 0.08);
        assert!((r - 0.07).abs() < 1e-6);
    }

    #[test]
    fn hit_flash_remaining_clamps_to_zero() {
        let r = hit_flash_remaining_after(0.05, 0.15);
        assert_eq!(r, 0.0);
    }
}
