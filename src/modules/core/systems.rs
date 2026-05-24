use bevy::prelude::*;

pub fn spawn_2d_camera(mut commands: Commands) {
    // 2D 카메라를 추가합니다.
    commands.spawn(Camera2dBundle::default());
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;

    #[test]
    fn 카메라스폰_시스템은_Camera2d_엔티티를_하나_생성한다() {
        let mut app = App::new();
        app.add_systems(Startup, spawn_2d_camera);
        app.update(); // Startup 시스템 실행 + Commands flush

        let count = app.world.query::<&Camera2d>().iter(&app.world).count();
        assert_eq!(count, 1, "spawn_2d_camera 는 Camera2d 엔티티를 정확히 하나 생성해야 한다");
    }
}
