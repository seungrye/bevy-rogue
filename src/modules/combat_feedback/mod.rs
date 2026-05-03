use bevy::prelude::*;
use crate::modules::map::{tile_to_world_coords, TILE_SIZE, PlayerActedEvent};

pub const BLOOD_DECAY_PER_TURN: f32 = 0.25;
pub const HIT_FLASH_DURATION: f32 = 0.15;
const Z_BLOOD: f32 = 0.5;

#[derive(Component)]
pub struct BloodStain {
    pub alpha: f32,
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
            ).chain());
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
            BloodStain { alpha: 1.0 },
        ));

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

fn fade_blood_stains(
    mut commands: Commands,
    mut turn_events: EventReader<PlayerActedEvent>,
    mut query: Query<(Entity, &mut BloodStain, &mut Text)>,
) {
    if turn_events.read().next().is_none() { return; }
    for (entity, mut stain, mut text) in query.iter_mut() {
        stain.alpha = blood_stain_alpha_after_decay(stain.alpha, BLOOD_DECAY_PER_TURN);
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

    #[test]
    fn blood_stain_starts_fully_visible() {
        assert_eq!(blood_stain_alpha_after_decay(1.0, 0.0), 1.0);
    }

    #[test]
    fn blood_stain_decays_per_turn() {
        let a = blood_stain_alpha_after_decay(1.0, BLOOD_DECAY_PER_TURN);
        assert!((a - 0.75).abs() < 1e-6);
    }

    #[test]
    fn blood_stain_alpha_clamps_to_zero() {
        let a = blood_stain_alpha_after_decay(0.1, BLOOD_DECAY_PER_TURN);
        assert_eq!(a, 0.0);
    }

    #[test]
    fn blood_stain_gone_after_four_turns() {
        let mut a = 1.0_f32;
        for _ in 0..4 { a = blood_stain_alpha_after_decay(a, BLOOD_DECAY_PER_TURN); }
        assert_eq!(a, 0.0);
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
