use bevy::prelude::*;
use rand::{thread_rng, Rng};
use crate::modules::map::{
    draw_map, MapResource, Rect,
    tile_to_world_coords, world_to_tile_coords, TILE_SIZE,
    MapSystemSet, TriggerRespawnEvent,
};
use crate::modules::player::Player;
use crate::modules::ui::LogMessage;

mod chest;
use chest::spawn_chest_on_event;

pub struct TriggerPlugin;

const Z_TRIGGER: f32 = 1.0;
pub const Z_CHEST_UI: i32 = 100;

impl Plugin for TriggerPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<TriggerEvent>()
            .add_systems(Startup, spawn_trigger.after(draw_map))
            .add_systems(Update, (
                (close_ui_on_input, check_triggers, handle_trigger_events).chain(),
                respawn_triggers_on_regen.after(MapSystemSet::ExecuteRegen),
            ));
    }
}

#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub enum TriggerEffect {
    ShowMessage(String),
    OpenChest,
}

#[derive(Component)]
pub struct Trigger {
    pub x: usize,
    pub y: usize,
    pub effect: TriggerEffect,
}

#[derive(Event)]
pub struct TriggerEvent(pub TriggerEffect);

fn spawn_trigger(mut commands: Commands, map_res: Res<MapResource>, asset_server: Res<AssetServer>) {
    spawn_triggers_for_rooms(&mut commands, &map_res.map().rooms, &asset_server);
}

fn respawn_triggers_on_regen(
    mut commands: Commands,
    mut events: EventReader<TriggerRespawnEvent>,
    trigger_query: Query<Entity, With<Trigger>>,
    ui_query: Query<Entity, With<TriggerEffect>>,
    asset_server: Res<AssetServer>,
) {
    for TriggerRespawnEvent(rooms) in events.read() {
        for entity in trigger_query.iter() { commands.entity(entity).despawn(); }
        for entity in ui_query.iter() { commands.entity(entity).despawn_recursive(); }
        spawn_triggers_for_rooms(&mut commands, rooms, &asset_server);
    }
}

pub fn spawn_triggers_for_rooms(commands: &mut Commands, rooms: &[Rect], asset_server: &AssetServer) {
    if rooms.len() < 2 { return; }
    let mut rng = thread_rng();
    let chest_idx = rng.gen_range(0..rooms.len() - 1);

    let font = asset_server.load("fonts/FiraMono-Medium.ttf");

    if let Some(r) = rooms.get(chest_idx) {
        let (x, y) = r.center();
        let coord = tile_to_world_coords(x, y);
        commands.spawn((
            Trigger { x, y, effect: TriggerEffect::OpenChest },
            Text2dBundle {
                text: Text::from_section("?", TextStyle { font, font_size: TILE_SIZE, color: Color::YELLOW }),
                transform: Transform::from_xyz(coord.x, coord.y, Z_TRIGGER),
                ..default()
            },
        ));
    }

    // ">" 트리거는 ZonePortal(StairDown/StairUp) 로 대체됨 — 제거
}

fn check_triggers(
    mut commands: Commands,
    player_query: Query<&Transform, With<Player>>,
    trigger_query: Query<(Entity, &Trigger)>,
    mut trigger_events: EventWriter<TriggerEvent>,
) {
    let Ok(pt) = player_query.get_single() else { return };
    let (px, py) = world_to_tile_coords(pt.translation);
    for (entity, trigger) in trigger_query.iter() {
        if px == trigger.x && py == trigger.y {
            trigger_events.send(TriggerEvent(trigger.effect.clone()));
            commands.entity(entity).despawn();
        }
    }
}

fn close_ui_on_input(
    mut commands: Commands,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    q: Query<(Entity, &TriggerEffect)>,
) {
    let Ok((entity, _)) = q.get_single() else { return };
    let close_keys = [
        KeyCode::ArrowUp, KeyCode::ArrowDown, KeyCode::ArrowLeft, KeyCode::ArrowRight,
        KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD, KeyCode::Escape,
    ];
    if close_keys.iter().any(|&k| keyboard_input.just_pressed(k)) {
        commands.entity(entity).despawn_recursive();
    }
}

fn handle_trigger_events(
    mut commands: Commands,
    mut events: EventReader<TriggerEvent>,
    asset_server: Res<AssetServer>,
    q_effect: Query<Entity, With<TriggerEffect>>,
    mut log_writer: EventWriter<LogMessage>,
) {
    for TriggerEvent(effect) in events.read() {
        match effect {
            TriggerEffect::ShowMessage(msg) => { log_writer.send(LogMessage(msg.clone())); }
            TriggerEffect::OpenChest => {
                spawn_chest_on_event(&mut commands, &*asset_server, !q_effect.is_empty());
            }
        }
    }
}
