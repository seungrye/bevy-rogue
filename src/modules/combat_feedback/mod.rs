use bevy::prelude::*;
use bevy_rapier2d::prelude::*;
use rand::Rng;
use crate::modules::map::{tile_to_world_coords, TILE_SIZE, PlayerActedEvent};

pub const HIT_FLASH_DURATION: f32 = 0.15;
pub const Z_BLOOD: f32 = 0.5;
const BLOOD_LIFETIME_MIN: u32 = 15;
const BLOOD_LIFETIME_MAX: u32 = 30;
const PARTICLE_COUNT: usize = 8;
const PARTICLE_LIFETIME: f32 = 0.3;
const PARTICLE_SIZE: f32 = 3.0;
/// 마찰 계수 — 시간에 따라 속도 감소시켜 자연스러운 정지 효과
const PARTICLE_DAMPING: f32 = 6.0;

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
            // 탑다운 시점 — 중력 없음 (이전 GravityScale(2.0) 은 측면 게임 외관)
            GravityScale(0.0),
            // 시간에 따라 속도가 감쇠하여 자연스럽게 정지
            Damping { linear_damping: PARTICLE_DAMPING, angular_damping: 0.0 },
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
    #![allow(non_snake_case)]
    use super::*;

    fn decay_per_turn_for(lifetime_turns: u32) -> f32 {
        1.0 / lifetime_turns as f32
    }

    #[test]
    fn 핏자국은_처음엔_완전히_보인다() {
        assert_eq!(blood_stain_alpha_after_decay(1.0, 0.0), 1.0);
    }

    #[test]
    fn 핏자국_투명도는_0_미만으로_내려가지_않는다() {
        let decay = decay_per_turn_for(4);
        let a = blood_stain_alpha_after_decay(0.1, decay);
        assert_eq!(a, 0.0);
    }

    #[test]
    fn 핏자국은_최소수명_후_거의_사라진다() {
        let decay = decay_per_turn_for(BLOOD_LIFETIME_MIN);
        let mut a = 1.0_f32;
        for _ in 0..BLOOD_LIFETIME_MIN { a = blood_stain_alpha_after_decay(a, decay); }
        assert!(a < 1e-4, "최소 수명({} 턴) 후 거의 사라져야 한다 (실제: {})", BLOOD_LIFETIME_MIN, a);
    }

    #[test]
    fn 핏자국은_최대수명_후_거의_사라진다() {
        let decay = decay_per_turn_for(BLOOD_LIFETIME_MAX);
        let mut a = 1.0_f32;
        for _ in 0..BLOOD_LIFETIME_MAX { a = blood_stain_alpha_after_decay(a, decay); }
        assert!(a < 1e-4, "최대 수명({} 턴) 후 거의 사라져야 한다 (실제: {})", BLOOD_LIFETIME_MAX, a);
    }

    #[test]
    fn 핏자국은_수명이_끝나기_전엔_아직_보인다() {
        let decay = decay_per_turn_for(BLOOD_LIFETIME_MAX);
        let mut a = 1.0_f32;
        for _ in 0..BLOOD_LIFETIME_MIN { a = blood_stain_alpha_after_decay(a, decay); }
        assert!(a > 0.0, "최소 수명보다 적은 턴 후엔 아직 보여야 한다");
    }

    #[test]
    fn 핏자국_수명_범위가_유효하다() {
        assert!(BLOOD_LIFETIME_MIN >= 1);
        assert!(BLOOD_LIFETIME_MAX >= BLOOD_LIFETIME_MIN);
    }

    #[test]
    fn 피격섬광은_시작시_활성상태다() {
        assert!(hit_flash_remaining_after(HIT_FLASH_DURATION, 0.0) > 0.0);
    }

    #[test]
    fn 피격섬광_잔여시간은_시간에_따라_감소한다() {
        let r = hit_flash_remaining_after(0.15, 0.08);
        assert!((r - 0.07).abs() < 1e-6);
    }

    #[test]
    fn 피격섬광_잔여시간은_0_미만으로_내려가지_않는다() {
        let r = hit_flash_remaining_after(0.05, 0.15);
        assert_eq!(r, 0.0);
    }

    #[test]
    fn 핏방울_파티클_상수가_탑다운에_적합하다() {
        // 탑다운 시점 — 중력은 spawn 시 GravityScale(0.0) 으로 설정됨
        // (코드 검증 — 상수만 확인 가능)
        assert!(PARTICLE_LIFETIME <= 0.4, "lifetime 이 너무 길면 정지된 채 남는다");
        assert!(PARTICLE_DAMPING > 0.0, "damping 이 있어야 자연스럽게 정지");
    }

    // ── 시스템 App 하네스 테스트 ────────────────────────────────────────────
    use std::time::Duration;

    fn feedback_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app.add_event::<CombatFeedbackEvent>();
        app.add_systems(Update, handle_combat_feedback);
        app
    }

    #[test]
    fn 피격피드백은_대상에_피격섬광을_부여한다() {
        let mut app = feedback_app();
        let target = app.world.spawn_empty().id();
        app.world.send_event(CombatFeedbackEvent {
            tile_x: 1, tile_y: 1, hit_entity: target, original_color: Color::WHITE,
        });
        app.update();
        assert!(app.world.entity(target).contains::<HitFlash>());
        assert_eq!(app.world.get::<HitFlash>(target).unwrap().original_color, Color::WHITE);
    }

    #[test]
    fn 피격피드백은_이미_섬광중인_대상의_원래색을_보존한다() {
        let mut app = feedback_app();
        let original = Color::rgb(0.1, 0.2, 0.3);
        let target = app.world.spawn(HitFlash { remaining: 0.1, original_color: original }).id();
        app.world.send_event(CombatFeedbackEvent {
            tile_x: 1, tile_y: 1, hit_entity: target, original_color: Color::WHITE,
        });
        app.update();
        assert_eq!(app.world.get::<HitFlash>(target).unwrap().original_color, original,
            "기존 섬광의 원래색을 유지해야 한다");
    }

    #[test]
    fn 피격피드백은_대상이_사라져도_핏자국을_남기고_정상동작한다() {
        let mut app = feedback_app();
        let target = app.world.spawn_empty().id();
        app.world.despawn(target);
        app.world.send_event(CombatFeedbackEvent {
            tile_x: 2, tile_y: 2, hit_entity: target, original_color: Color::WHITE,
        });
        app.update();
        assert!(app.world.query::<&BloodStain>().iter(&app.world).count() > 0);
    }

    #[test]
    fn 핏방울_파티클은_수명이_다하면_제거된다() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.add_systems(Update, update_blood_particles);
        let e = app.world.spawn((BloodParticle { lifetime: 0.1 }, Sprite::default())).id();
        app.world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(0.5));
        app.update();
        assert!(app.world.get_entity(e).is_none());
    }

    #[test]
    fn 핏방울_파티클은_살아있으면_점점_옅어진다() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.add_systems(Update, update_blood_particles);
        let e = app.world.spawn((
            BloodParticle { lifetime: 0.3 },
            Sprite { color: Color::WHITE, ..default() },
        )).id();
        app.world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(0.1));
        app.update();
        assert!(app.world.get_entity(e).is_some());
        assert!(app.world.get::<Sprite>(e).unwrap().color.a() < 1.0, "투명도가 낮아져야 한다");
    }

    #[test]
    fn 턴이벤트가_없으면_핏자국이_바래지_않는다() {
        let mut app = App::new();
        app.add_event::<PlayerActedEvent>();
        app.add_systems(Update, fade_blood_stains);
        let e = app.world.spawn((
            BloodStain { alpha: 1.0, decay_per_turn: 0.5 },
            Text::from_section("%", TextStyle::default()),
        )).id();
        app.update();
        assert_eq!(app.world.get::<BloodStain>(e).unwrap().alpha, 1.0);
    }

    #[test]
    fn 핏자국은_매턴_바래며_투명도가_0이되면_제거된다() {
        let mut app = App::new();
        app.add_event::<PlayerActedEvent>();
        app.add_systems(Update, fade_blood_stains);
        let e = app.world.spawn((
            BloodStain { alpha: 0.05, decay_per_turn: 1.0 },
            Text::from_section("%", TextStyle::default()),
        )).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!(app.world.get_entity(e).is_none());
    }

    #[test]
    fn 핏자국은_바래도_투명도가_남으면_유지된다() {
        let mut app = App::new();
        app.add_event::<PlayerActedEvent>();
        app.add_systems(Update, fade_blood_stains);
        let e = app.world.spawn((
            BloodStain { alpha: 1.0, decay_per_turn: 0.1 },
            Text::from_section("%", TextStyle::default()),
        )).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!((app.world.get::<BloodStain>(e).unwrap().alpha - 0.9).abs() < 1e-6);
    }

    #[test]
    fn 피격섬광은_시간이_다하면_원래색을_복원하고_제거된다() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.add_systems(Update, apply_hit_flash);
        let green = Color::rgb(0.0, 1.0, 0.0);
        let e = app.world.spawn((
            HitFlash { remaining: 0.05, original_color: green },
            Text::from_section("M", TextStyle { color: Color::rgb(1.0, 0.0, 0.0), ..default() }),
        )).id();
        app.world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(0.2));
        app.update();
        assert!(!app.world.entity(e).contains::<HitFlash>());
        assert_eq!(app.world.get::<Text>(e).unwrap().sections[0].style.color, green);
    }

    #[test]
    fn 피격섬광은_지속중에는_빨간색으로_표시된다() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.add_systems(Update, apply_hit_flash);
        let e = app.world.spawn((
            HitFlash { remaining: 0.15, original_color: Color::WHITE },
            Text::from_section("M", TextStyle::default()),
        )).id();
        app.world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(0.05));
        app.update();
        assert!(app.world.entity(e).contains::<HitFlash>());
        assert_eq!(app.world.get::<Text>(e).unwrap().sections[0].style.color, Color::rgb(1.0, 0.0, 0.0));
    }

    #[test]
    fn 전투피드백플러그인이_정상적으로_빌드된다() {
        let mut app = App::new();
        app.add_plugins(CombatFeedbackPlugin);
    }
}
