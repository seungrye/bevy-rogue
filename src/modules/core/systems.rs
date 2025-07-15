use bevy::prelude::*;

pub fn spawn_2d_camera(mut commands: Commands) {
    // 2D 카메라를 추가합니다.
    commands.spawn(Camera2dBundle::default());
}
