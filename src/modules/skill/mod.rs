use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use crate::modules::{
    combat::{CombatStats, Defeated, regen_hp},
    elemental::{Element, ElementalApplyEvent},
    map::{
        ExplosionEvent, Map, MapResource, MonsterTiles, OccupiedTiles,
        PlayerActedEvent, is_in_view, tile_to_world_coords, world_to_tile_coords,
        FOV_FRONT, FOV_BACK, MAP_WIDTH, MAP_HEIGHT, TILE_SIZE,
    },
    player::{Player, Facing},
    ranged::RangedTargeting,
    ui::{help::HelpPanelOpen, shop::ShopPanelOpen, LogMessage},
    item::EquipmentPanelOpen,
};

pub struct SkillPlugin;

impl Plugin for SkillPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SkillTargeting>()
            .add_systems(Update, (
                handle_skill_input,
                update_skill_cursor,
            ).chain());
    }
}

// ── 스킬 정의 ───────────────────────────────────────────────────────────────

/// 액티브 스킬 3종. 각 스킬은 MP 를 소모하고 턴을 소비한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skill {
    /// 조준 타일에 폭발(지형 파괴 + 범위 피해) + 화염 원소를 부여한다.
    Fireball,
    /// 시야 내 빈(통과 가능) 타일로 순간이동한다.
    Blink,
    /// MP 를 소모해 자신의 HP 를 회복한다.
    Heal,
}

/// 파이어볼 폭발 반경.
pub const FIREBALL_RADIUS: i32 = 1;
/// 파이어볼 폭발이 주는 엔티티 피해량.
pub const FIREBALL_DAMAGE: i32 = 8;
/// 치유 스킬이 회복하는 HP 량.
pub const HEAL_AMOUNT: i32 = 12;
/// 조준이 필요한 스킬(파이어볼/점멸)의 사거리.
pub const SKILL_RANGE: i32 = 6;

impl Skill {
    /// 시전에 필요한 MP 비용.
    pub fn mp_cost(self) -> i32 {
        match self {
            Skill::Fireball => 8,
            Skill::Blink    => 5,
            Skill::Heal     => 6,
        }
    }

    pub fn name_ko(self) -> &'static str {
        match self {
            Skill::Fireball => "파이어볼",
            Skill::Blink    => "점멸",
            Skill::Heal     => "치유",
        }
    }

    /// 시전에 조준(타깃 타일)이 필요한 스킬인지 여부.
    pub fn needs_target(self) -> bool {
        matches!(self, Skill::Fireball | Skill::Blink)
    }

    /// 단축키(숫자 1/2/3)를 스킬로 매핑한다. 그 외 키는 None.
    pub fn from_key(key: KeyCode) -> Option<Skill> {
        match key {
            KeyCode::Digit1 => Some(Skill::Fireball),
            KeyCode::Digit2 => Some(Skill::Blink),
            KeyCode::Digit3 => Some(Skill::Heal),
            _ => None,
        }
    }
}

// ── 순수 판정 함수 ───────────────────────────────────────────────────────────

/// 현재 MP 가 비용 이상이면 시전 가능.
pub fn can_cast(mp: i32, cost: i32) -> bool {
    mp >= cost
}

/// 치유 후 HP 를 계산한다(최대치 클램프). `regen_hp` 와 동일한 클램프 계약을 따른다.
pub fn heal_result(hp: i32, max_hp: i32) -> i32 {
    regen_hp(hp, max_hp, HEAL_AMOUNT)
}

/// 점멸 목적지가 유효한지 판정하는 순수 함수.
///
/// - 사거리(`SKILL_RANGE`) 안이어야 한다.
/// - 통과 가능한(`is_walkable`) 타일이어야 한다(벽/물 불가).
/// - 다른 엔티티(몬스터/주민)가 점유하지 않아야 한다.
/// - 시야 내(`is_in_view`)여야 한다.
pub fn blink_destination_valid(
    map: &Map,
    facing: IVec2,
    px: usize, py: usize,
    tx: usize, ty: usize,
    monster_tiles: &MonsterTiles,
    occupied: &OccupiedTiles,
) -> bool {
    let dx = tx as i32 - px as i32;
    let dy = ty as i32 - py as i32;
    if dx * dx + dy * dy > SKILL_RANGE * SKILL_RANGE {
        return false;
    }
    if !map.get_tile(tx, ty).is_walkable() {
        return false;
    }
    if monster_tiles.0.contains(&(tx, ty)) || occupied.0.contains(&(tx, ty)) {
        return false;
    }
    if (tx, ty) == (px, py) {
        // 제자리로의 점멸은 의미가 없어 거부한다.
        return false;
    }
    is_in_view(px as i32, py as i32, facing, tx as i32, ty as i32, FOV_FRONT, FOV_BACK, map)
}

/// 파이어볼 목적지가 유효한지 판정한다(사거리 + 시야).
/// 지형/엔티티 점유는 폭발이 처리하므로 통과 가능 여부는 보지 않는다.
pub fn fireball_target_valid(
    map: &Map,
    facing: IVec2,
    px: usize, py: usize,
    tx: usize, ty: usize,
) -> bool {
    let dx = tx as i32 - px as i32;
    let dy = ty as i32 - py as i32;
    if dx * dx + dy * dy > SKILL_RANGE * SKILL_RANGE {
        return false;
    }
    is_in_view(px as i32, py as i32, facing, tx as i32, ty as i32, FOV_FRONT, FOV_BACK, map)
}

// ── 조준 상태 / 커서 ─────────────────────────────────────────────────────────

/// 조준이 필요한 스킬(파이어볼/점멸)의 조준 상태.
#[derive(Resource, Default)]
pub struct SkillTargeting {
    pub active: bool,
    pub skill: Option<Skill>,
    pub cursor: (usize, usize),
}

#[derive(Component)]
pub struct SkillCursor;

// ── 입력 시스템 ─────────────────────────────────────────────────────────────

/// 스킬 입력을 막아야 하는 모달 패널 상태 묶음. (시스템 파라미터 16개 제한 회피용)
#[derive(SystemParam)]
struct PanelGuards<'w> {
    equipment_open: Res<'w, EquipmentPanelOpen>,
    shop_open: Res<'w, ShopPanelOpen>,
    help_open: Res<'w, HelpPanelOpen>,
}

impl PanelGuards<'_> {
    fn any_open(&self) -> bool {
        self.equipment_open.0 || self.shop_open.0 || self.help_open.0
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_skill_input(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut targeting: ResMut<SkillTargeting>,
    panels: PanelGuards,
    ranged: Res<RangedTargeting>,
    mut player_q: Query<(Entity, &Transform, &Facing, &mut CombatStats), (With<Player>, Without<Defeated>)>,
    monster_q: Query<(Entity, &crate::modules::monster::Monster)>,
    monster_tiles: Res<MonsterTiles>,
    occupied: Res<OccupiedTiles>,
    map_res: Res<MapResource>,
    asset_server: Res<AssetServer>,
    cursor_q: Query<Entity, With<SkillCursor>>,
    mut explosion: EventWriter<ExplosionEvent>,
    mut elemental: EventWriter<ElementalApplyEvent>,
    mut acted: EventWriter<PlayerActedEvent>,
    mut log: EventWriter<LogMessage>,
) {
    if panels.any_open() { return; }
    // 원격 조준 중에는 ranged 가 입력을 가지므로 스킬 입력을 받지 않는다.
    if ranged.active { return; }

    let Ok((entity, transform, facing, mut stats)) = player_q.get_single_mut() else { return };
    let (px, py) = world_to_tile_coords(transform.translation);

    // 조준 활성 상태: 커서 이동 / 취소 / 확정 처리.
    if targeting.active {
        if keyboard.just_pressed(KeyCode::Escape) {
            cancel_targeting(&mut commands, &mut targeting, &cursor_q);
            return;
        }

        let mut dx = 0i32;
        let mut dy = 0i32;
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
            let skill = targeting.skill.expect("조준 활성 상태면 스킬이 정해져 있다");
            let (tx, ty) = targeting.cursor;
            let cast = confirm_targeted_skill(
                skill, &mut commands, entity, facing.0, px, py, tx, ty,
                map_res.map(), &monster_q, &monster_tiles, &occupied, &mut stats,
                &mut explosion, &mut elemental, &mut acted, &mut log,
            );
            if cast {
                cancel_targeting(&mut commands, &mut targeting, &cursor_q);
            }
        }
        return;
    }

    // 비활성 상태: 단축키로 스킬 시전 시작.
    let pressed = keyboard.get_just_pressed().copied()
        .find_map(Skill::from_key);
    let Some(skill) = pressed else { return };

    if !can_cast(stats.mp, skill.mp_cost()) {
        log.send(LogMessage(format!(
            "{} 시전 실패: MP 부족 ({}/{})", skill.name_ko(), stats.mp, skill.mp_cost()
        )));
        return;
    }

    if skill.needs_target() {
        // 조준 모드 진입 — 커서를 플레이어 위치에서 시작.
        targeting.active = true;
        targeting.skill = Some(skill);
        targeting.cursor = (px, py);
        spawn_skill_cursor(&mut commands, &asset_server, (px, py));
    } else {
        // 즉시 시전(치유).
        cast_heal(&mut stats, &mut acted, &mut log);
    }
}

/// 조준 확정 시 스킬 효과를 적용한다. 시전 성공이면 `true`(MP 차감 + 턴 소비 포함).
#[allow(clippy::too_many_arguments)]
fn confirm_targeted_skill(
    skill: Skill,
    commands: &mut Commands,
    player_entity: Entity,
    facing: IVec2,
    px: usize, py: usize,
    tx: usize, ty: usize,
    map: &Map,
    monster_q: &Query<(Entity, &crate::modules::monster::Monster)>,
    monster_tiles: &MonsterTiles,
    occupied: &OccupiedTiles,
    stats: &mut CombatStats,
    explosion: &mut EventWriter<ExplosionEvent>,
    elemental: &mut EventWriter<ElementalApplyEvent>,
    acted: &mut EventWriter<PlayerActedEvent>,
    log: &mut EventWriter<LogMessage>,
) -> bool {
    match skill {
        Skill::Fireball => {
            if !fireball_target_valid(map, facing, px, py, tx, ty) {
                log.send(LogMessage("파이어볼: 사거리 밖이거나 보이지 않는다.".into()));
                return false;
            }
            explosion.send(ExplosionEvent {
                center: (tx, ty),
                radius: FIREBALL_RADIUS,
                terrain: true,
                entity_damage: FIREBALL_DAMAGE,
            });
            // 폭발 반경 안의 몬스터에게 화염 원소를 부여한다.
            for (e, monster) in monster_q.iter() {
                let dx = monster.tile_x as i32 - tx as i32;
                let dy = monster.tile_y as i32 - ty as i32;
                if dx * dx + dy * dy <= FIREBALL_RADIUS * FIREBALL_RADIUS {
                    elemental.send(ElementalApplyEvent { target: e, element: Element::Fire });
                }
            }
            stats.mp -= skill.mp_cost();
            acted.send(PlayerActedEvent);
            log.send(LogMessage(format!(
                "{} 시전! ({}, {}) 폭발 + 화염 (MP -{})", skill.name_ko(), tx, ty, skill.mp_cost()
            )));
            true
        }
        Skill::Blink => {
            if !blink_destination_valid(map, facing, px, py, tx, ty, monster_tiles, occupied) {
                log.send(LogMessage("점멸: 목적지가 유효하지 않다(벽/점유/사거리/시야).".into()));
                return false;
            }
            let wp = tile_to_world_coords(tx, ty);
            commands.entity(player_entity).insert(Transform::from_xyz(wp.x, wp.y, 1.0));
            stats.mp -= skill.mp_cost();
            acted.send(PlayerActedEvent);
            log.send(LogMessage(format!(
                "{} 시전! ({}, {})로 순간이동 (MP -{})", skill.name_ko(), tx, ty, skill.mp_cost()
            )));
            true
        }
        // 치유는 조준이 필요 없어 이 경로로 오지 않는다. // 도달 불가 방어코드
        Skill::Heal => false,
    }
}

/// 치유 즉시 시전: MP 차감 + HP 회복(클램프) + 턴 소비.
fn cast_heal(
    stats: &mut CombatStats,
    acted: &mut EventWriter<PlayerActedEvent>,
    log: &mut EventWriter<LogMessage>,
) {
    let before = stats.hp;
    stats.hp = heal_result(stats.hp, stats.max_hp);
    stats.mp -= Skill::Heal.mp_cost();
    acted.send(PlayerActedEvent);
    log.send(LogMessage(format!(
        "{} 시전! HP {} → {} (MP -{})",
        Skill::Heal.name_ko(), before, stats.hp, Skill::Heal.mp_cost()
    )));
}

fn cancel_targeting(
    commands: &mut Commands,
    targeting: &mut SkillTargeting,
    cursor_q: &Query<Entity, With<SkillCursor>>,
) {
    targeting.active = false;
    targeting.skill = None;
    for e in cursor_q.iter() {
        commands.entity(e).despawn();
    }
}

fn spawn_skill_cursor(commands: &mut Commands, asset_server: &AssetServer, tile: (usize, usize)) {
    let coord = tile_to_world_coords(tile.0, tile.1);
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");
    commands.spawn((
        Text2dBundle {
            text: Text::from_section("x", TextStyle {
                font,
                font_size: TILE_SIZE,
                color: Color::rgb(1.0, 0.5, 0.1),
            }),
            transform: Transform::from_xyz(coord.x, coord.y, 2.0),
            ..default()
        },
        SkillCursor,
    ));
}

fn update_skill_cursor(
    targeting: Res<SkillTargeting>,
    mut cursor_q: Query<&mut Transform, (With<SkillCursor>, Without<Player>)>,
) {
    if !targeting.active { return; }
    let Ok(mut t) = cursor_q.get_single_mut() else { return };
    let coord = tile_to_world_coords(targeting.cursor.0, targeting.cursor.1);
    t.translation.x = coord.x;
    t.translation.y = coord.y;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::map::TileKind;

    // ── 순수 함수: can_cast ──────────────────────────────────────────────────

    #[test]
    fn MP가_비용_이상이면_시전할_수_있다() {
        assert!(can_cast(8, 8), "MP 가 비용과 같으면 시전 가능");
        assert!(can_cast(20, 5), "MP 가 비용보다 많으면 시전 가능");
    }

    #[test]
    fn MP가_비용보다_적으면_시전할_수_없다() {
        assert!(!can_cast(4, 5), "MP 가 비용보다 적으면 시전 불가");
        assert!(!can_cast(0, 1));
    }

    // ── 순수 함수: 스킬 메타 ─────────────────────────────────────────────────

    #[test]
    fn 각_스킬의_MP비용과_한글이름이_정의되어_있다() {
        assert_eq!(Skill::Fireball.mp_cost(), 8);
        assert_eq!(Skill::Blink.mp_cost(), 5);
        assert_eq!(Skill::Heal.mp_cost(), 6);
        assert_eq!(Skill::Fireball.name_ko(), "파이어볼");
        assert_eq!(Skill::Blink.name_ko(), "점멸");
        assert_eq!(Skill::Heal.name_ko(), "치유");
    }

    #[test]
    fn 파이어볼과_점멸은_조준이_필요하고_치유는_필요없다() {
        assert!(Skill::Fireball.needs_target());
        assert!(Skill::Blink.needs_target());
        assert!(!Skill::Heal.needs_target());
    }

    #[test]
    fn 숫자키_1_2_3은_각각_파이어볼_점멸_치유로_매핑된다() {
        assert_eq!(Skill::from_key(KeyCode::Digit1), Some(Skill::Fireball));
        assert_eq!(Skill::from_key(KeyCode::Digit2), Some(Skill::Blink));
        assert_eq!(Skill::from_key(KeyCode::Digit3), Some(Skill::Heal));
    }

    #[test]
    fn 스킬에_매핑되지_않은_키는_None을_돌려준다() {
        assert_eq!(Skill::from_key(KeyCode::Space), None);
        assert_eq!(Skill::from_key(KeyCode::Digit4), None);
    }

    // ── 순수 함수: heal_result ───────────────────────────────────────────────

    #[test]
    fn 치유는_치유량만큼_HP를_회복한다() {
        assert_eq!(heal_result(10, 30), 10 + HEAL_AMOUNT);
    }

    #[test]
    fn 치유는_최대_HP를_넘지_않게_클램프된다() {
        assert_eq!(heal_result(28, 30), 30, "최대치를 넘지 않아야 한다");
        assert_eq!(heal_result(30, 30), 30, "이미 최대치면 그대로");
    }

    // ── 순수 함수: blink_destination_valid ───────────────────────────────────

    fn visible_floor_map() -> Map {
        let mut map = Map::new(40, 40);
        for y in 0..40 { for x in 0..40 { map.set_tile(x, y, TileKind::Floor); } }
        map
    }

    #[test]
    fn 점멸은_사거리_안의_빈_바닥타일로는_유효하다() {
        let map = visible_floor_map();
        let mt = MonsterTiles::default();
        let occ = OccupiedTiles::default();
        // 오른쪽을 보는 facing, (10,10)에서 (13,10) 으로
        assert!(blink_destination_valid(&map, IVec2::new(1, 0), 10, 10, 13, 10, &mt, &occ));
    }

    #[test]
    fn 점멸은_벽타일로는_무효하다() {
        let mut map = visible_floor_map();
        map.set_tile(13, 10, TileKind::Wall);
        let mt = MonsterTiles::default();
        let occ = OccupiedTiles::default();
        assert!(!blink_destination_valid(&map, IVec2::new(1, 0), 10, 10, 13, 10, &mt, &occ));
    }

    #[test]
    fn 점멸은_사거리_밖이면_무효하다() {
        let map = visible_floor_map();
        let mt = MonsterTiles::default();
        let occ = OccupiedTiles::default();
        // (10,10) → (20,10) 거리 10 > SKILL_RANGE(6)
        assert!(!blink_destination_valid(&map, IVec2::new(1, 0), 10, 10, 20, 10, &mt, &occ));
    }

    #[test]
    fn 점멸은_몬스터가_점유한_타일로는_무효하다() {
        let map = visible_floor_map();
        let mut mt = MonsterTiles::default();
        mt.0.insert((13, 10));
        let occ = OccupiedTiles::default();
        assert!(!blink_destination_valid(&map, IVec2::new(1, 0), 10, 10, 13, 10, &mt, &occ));
    }

    #[test]
    fn 점멸은_주민이_점유한_타일로는_무효하다() {
        let map = visible_floor_map();
        let mt = MonsterTiles::default();
        let mut occ = OccupiedTiles::default();
        occ.0.insert((13, 10));
        assert!(!blink_destination_valid(&map, IVec2::new(1, 0), 10, 10, 13, 10, &mt, &occ));
    }

    #[test]
    fn 점멸은_제자리로는_무효하다() {
        let map = visible_floor_map();
        let mt = MonsterTiles::default();
        let occ = OccupiedTiles::default();
        assert!(!blink_destination_valid(&map, IVec2::new(1, 0), 10, 10, 10, 10, &mt, &occ));
    }

    #[test]
    fn 점멸은_시야_밖_타일로는_무효하다() {
        // 벽으로 시야를 막아 사거리 안이라도 보이지 않게 한다.
        let mut map = visible_floor_map();
        map.set_tile(11, 10, TileKind::Wall); // (10,10)→(13,10) 사이 차단
        let mt = MonsterTiles::default();
        let occ = OccupiedTiles::default();
        assert!(!blink_destination_valid(&map, IVec2::new(1, 0), 10, 10, 13, 10, &mt, &occ));
    }

    // ── 순수 함수: fireball_target_valid ─────────────────────────────────────

    #[test]
    fn 파이어볼은_사거리_안_시야_타일로는_유효하다() {
        let map = visible_floor_map();
        assert!(fireball_target_valid(&map, IVec2::new(1, 0), 10, 10, 13, 10));
    }

    #[test]
    fn 파이어볼은_사거리_밖이면_무효하다() {
        let map = visible_floor_map();
        assert!(!fireball_target_valid(&map, IVec2::new(1, 0), 10, 10, 20, 10));
    }

    #[test]
    fn 파이어볼은_벽_너머_시야밖이면_무효하다() {
        let mut map = visible_floor_map();
        map.set_tile(11, 10, TileKind::Wall);
        assert!(!fireball_target_valid(&map, IVec2::new(1, 0), 10, 10, 13, 10));
    }

    #[test]
    fn 파이어볼은_벽타일을_조준해도_유효하다() {
        // 통과 불가 타일이어도 폭발 대상이므로 유효해야 한다(지형 파괴).
        let mut map = visible_floor_map();
        map.set_tile(13, 10, TileKind::Wall);
        assert!(fireball_target_valid(&map, IVec2::new(1, 0), 10, 10, 13, 10));
    }

    // ── 시스템 App 하네스 ────────────────────────────────────────────────────

    use crate::modules::ranged::RangedTargeting;

    struct SkillHarness {
        app: App,
        player: Entity,
    }

    impl SkillHarness {
        fn new(player_tile: (usize, usize), mp: i32, hp: i32) -> Self {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins)
                .add_plugins(bevy::asset::AssetPlugin::default());
            app.init_asset::<Font>();
            app.insert_resource(ButtonInput::<KeyCode>::default());
            app.init_resource::<SkillTargeting>();
            app.init_resource::<EquipmentPanelOpen>();
            app.init_resource::<ShopPanelOpen>();
            app.init_resource::<HelpPanelOpen>();
            app.init_resource::<RangedTargeting>();
            app.init_resource::<MonsterTiles>();
            app.init_resource::<OccupiedTiles>();
            app.add_event::<ExplosionEvent>();
            app.add_event::<ElementalApplyEvent>();
            app.add_event::<PlayerActedEvent>();
            app.add_event::<LogMessage>();

            let map = visible_floor_map();
            app.insert_resource(MapResource(map));

            let pos = tile_to_world_coords(player_tile.0, player_tile.1);
            let player = app.world.spawn((
                Player,
                Transform::from_xyz(pos.x, pos.y, 1.0),
                Facing(IVec2::new(1, 0)),
                CombatStats { hp, max_hp: 30, mp, max_mp: 20, attack: 5, defense: 1 },
            )).id();

            app.add_systems(Update, handle_skill_input);
            Self { app, player }
        }

        fn press(&mut self, key: KeyCode) {
            self.app.world.resource_mut::<ButtonInput<KeyCode>>().press(key);
        }
        fn clear_keys(&mut self) {
            // 키를 모두 떼고 just_* 상태를 비워, 같은 키를 다시 눌렀을 때
            // just_pressed 가 재발화하도록 한다(이미 pressed 면 재발화 안 함).
            let mut input = self.app.world.resource_mut::<ButtonInput<KeyCode>>();
            input.release_all();
            input.clear();
        }
        fn update(&mut self) { self.app.update(); }
        fn mp(&self) -> i32 { self.app.world.get::<CombatStats>(self.player).unwrap().mp }
        fn hp(&self) -> i32 { self.app.world.get::<CombatStats>(self.player).unwrap().hp }
        fn targeting_active(&self) -> bool { self.app.world.resource::<SkillTargeting>().active }
        fn cursor(&self) -> (usize, usize) { self.app.world.resource::<SkillTargeting>().cursor }
        fn explosions(&mut self) -> Vec<(usize, usize)> {
            let events = self.app.world.resource::<Events<ExplosionEvent>>();
            let mut r = events.get_reader();
            r.read(events).map(|e| e.center).collect()
        }
        fn acted_count(&mut self) -> usize {
            let events = self.app.world.resource::<Events<PlayerActedEvent>>();
            let mut r = events.get_reader();
            r.read(events).count()
        }
        fn last_log(&mut self) -> Option<String> {
            let events = self.app.world.resource::<Events<LogMessage>>();
            let mut r = events.get_reader();
            r.read(events).last().map(|m| m.0.clone())
        }
        fn player_tile(&self) -> (usize, usize) {
            let t = self.app.world.get::<Transform>(self.player).unwrap();
            world_to_tile_coords(t.translation)
        }
    }

    // ── 치유 시전 ────────────────────────────────────────────────────────────

    #[test]
    fn 치유_단축키를_누르면_HP를_회복하고_MP와_턴을_소비한다() {
        let mut h = SkillHarness::new((10, 10), 20, 10);
        h.press(KeyCode::Digit3);
        h.update();
        assert_eq!(h.hp(), 10 + HEAL_AMOUNT, "HP 회복");
        assert_eq!(h.mp(), 20 - Skill::Heal.mp_cost(), "MP 차감");
        assert_eq!(h.acted_count(), 1, "턴 소비");
    }

    #[test]
    fn 치유는_최대HP를_넘지_않게_회복된다() {
        let mut h = SkillHarness::new((10, 10), 20, 28); // max_hp = 30
        h.press(KeyCode::Digit3);
        h.update();
        assert_eq!(h.hp(), 30, "최대치 클램프");
    }

    #[test]
    fn MP가_부족하면_치유는_시전되지_않고_경고만_뜬다() {
        let mut h = SkillHarness::new((10, 10), 3, 10); // Heal 비용 6 > 3
        h.press(KeyCode::Digit3);
        h.update();
        assert_eq!(h.hp(), 10, "HP 변화 없음");
        assert_eq!(h.mp(), 3, "MP 변화 없음");
        assert_eq!(h.acted_count(), 0, "턴 소비 없음");
        assert!(h.last_log().unwrap().contains("MP 부족"));
    }

    // ── 파이어볼 시전 ────────────────────────────────────────────────────────

    #[test]
    fn 파이어볼_단축키를_누르면_조준모드에_진입하고_커서가_생긴다() {
        let mut h = SkillHarness::new((10, 10), 20, 10);
        h.press(KeyCode::Digit1);
        h.update();
        assert!(h.targeting_active(), "조준 모드 진입");
        assert_eq!(h.cursor(), (10, 10), "커서는 플레이어 위치에서 시작");
        assert_eq!(h.app.world.query::<&SkillCursor>().iter(&h.app.world).count(), 1, "커서 1개 스폰");
    }

    #[test]
    fn 조준중_방향키로_커서를_이동한_뒤_Enter로_파이어볼을_발사한다() {
        let mut h = SkillHarness::new((10, 10), 20, 10);
        h.press(KeyCode::Digit1);
        h.update();
        h.clear_keys();
        // 커서를 오른쪽으로 3칸 이동
        for _ in 0..3 {
            h.press(KeyCode::ArrowRight);
            h.update();
            h.clear_keys();
        }
        assert_eq!(h.cursor(), (13, 10));
        h.press(KeyCode::Enter);
        h.update();
        assert_eq!(h.explosions(), vec![(13, 10)], "조준 타일에 폭발 발행");
        assert_eq!(h.mp(), 20 - Skill::Fireball.mp_cost(), "MP 차감");
        assert_eq!(h.acted_count(), 1, "턴 소비");
        assert!(!h.targeting_active(), "발사 후 조준 종료");
        assert_eq!(h.app.world.query::<&SkillCursor>().iter(&h.app.world).count(), 0, "커서 despawn");
    }

    #[test]
    fn 파이어볼_폭발은_지형파괴와_엔티티피해_플래그를_세운다() {
        let mut h = SkillHarness::new((10, 10), 20, 10);
        h.press(KeyCode::Digit1);
        h.update();
        h.clear_keys();
        h.press(KeyCode::ArrowRight);
        h.update();
        h.clear_keys();
        h.press(KeyCode::Enter);
        h.update();
        let events = h.app.world.resource::<Events<ExplosionEvent>>();
        let mut r = events.get_reader();
        let ev = r.read(events).next().expect("폭발 이벤트");
        assert!(ev.terrain, "지형 파괴 true");
        assert_eq!(ev.entity_damage, FIREBALL_DAMAGE, "엔티티 피해 설정");
        assert_eq!(ev.radius, FIREBALL_RADIUS);
    }

    #[test]
    fn 파이어볼은_폭발반경의_몬스터에게_화염원소를_부여한다() {
        let mut h = SkillHarness::new((10, 10), 20, 10);
        // 폭발 중심이 될 (13,10) 에 몬스터를 둔다.
        h.app.world.spawn(crate::modules::monster::Monster {
            name: "고블린".into(), tile_x: 13, tile_y: 10,
            vision_radius: 5, alert_turns: 0, slot_idx: 0,
        });
        h.press(KeyCode::Digit1);
        h.update();
        h.clear_keys();
        for _ in 0..3 {
            h.press(KeyCode::ArrowRight);
            h.update();
            h.clear_keys();
        }
        h.press(KeyCode::Enter);
        h.update();
        let events = h.app.world.resource::<Events<ElementalApplyEvent>>();
        let mut r = events.get_reader();
        let applied: Vec<_> = r.read(events).map(|e| e.element).collect();
        assert_eq!(applied, vec![Element::Fire], "반경 내 몬스터에 화염 1회");
    }

    #[test]
    fn 파이어볼은_반경밖_몬스터에게는_화염을_부여하지_않는다() {
        let mut h = SkillHarness::new((10, 10), 20, 10);
        // 폭발 중심 (13,10) 에서 반경(1) 밖인 (10,10) 근처에 몬스터.
        h.app.world.spawn(crate::modules::monster::Monster {
            name: "고블린".into(), tile_x: 10, tile_y: 13,
            vision_radius: 5, alert_turns: 0, slot_idx: 0,
        });
        h.press(KeyCode::Digit1);
        h.update();
        h.clear_keys();
        for _ in 0..3 {
            h.press(KeyCode::ArrowRight);
            h.update();
            h.clear_keys();
        }
        h.press(KeyCode::Enter);
        h.update();
        let events = h.app.world.resource::<Events<ElementalApplyEvent>>();
        let mut r = events.get_reader();
        assert_eq!(r.read(events).count(), 0, "반경 밖 몬스터엔 화염 없음");
    }

    #[test]
    fn MP가_부족하면_파이어볼은_조준모드에_진입하지_않는다() {
        let mut h = SkillHarness::new((10, 10), 5, 10); // Fireball 비용 8 > 5
        h.press(KeyCode::Digit1);
        h.update();
        assert!(!h.targeting_active(), "MP 부족이면 조준 진입 안 함");
        assert_eq!(h.mp(), 5, "MP 변화 없음");
        assert!(h.last_log().unwrap().contains("MP 부족"));
    }

    #[test]
    fn 파이어볼은_사거리_밖을_조준하면_발사되지_않고_경고가_뜬다() {
        let mut h = SkillHarness::new((10, 10), 20, 10);
        h.press(KeyCode::Digit1);
        h.update();
        h.clear_keys();
        // 커서를 사거리 밖으로 옮긴다(오른쪽 7칸 — SKILL_RANGE 6 초과).
        for _ in 0..7 {
            h.press(KeyCode::ArrowRight);
            h.update();
            h.clear_keys();
        }
        h.press(KeyCode::Enter);
        h.update();
        assert!(h.explosions().is_empty(), "사거리 밖은 폭발 없음");
        assert_eq!(h.mp(), 20, "실패 시 MP 변화 없음");
        assert!(h.targeting_active(), "실패 시 조준 유지");
        assert!(h.last_log().unwrap().contains("사거리"));
    }

    // ── 점멸 시전 ────────────────────────────────────────────────────────────

    #[test]
    fn 점멸은_조준한_빈_타일로_플레이어를_순간이동시킨다() {
        let mut h = SkillHarness::new((10, 10), 20, 10);
        h.press(KeyCode::Digit2);
        h.update();
        h.clear_keys();
        for _ in 0..3 {
            h.press(KeyCode::ArrowRight);
            h.update();
            h.clear_keys();
        }
        assert_eq!(h.cursor(), (13, 10));
        h.press(KeyCode::Enter);
        h.update();
        assert_eq!(h.player_tile(), (13, 10), "목적지로 순간이동");
        assert_eq!(h.mp(), 20 - Skill::Blink.mp_cost(), "MP 차감");
        assert_eq!(h.acted_count(), 1, "턴 소비");
        assert!(!h.targeting_active(), "이동 후 조준 종료");
    }

    #[test]
    fn 점멸은_벽을_조준하면_이동하지_않고_경고가_뜬다() {
        let mut h = SkillHarness::new((10, 10), 20, 10);
        // (13,10) 을 벽으로 만든다.
        h.app.world.resource_mut::<MapResource>().map_mut().set_tile(13, 10, TileKind::Wall);
        h.press(KeyCode::Digit2);
        h.update();
        h.clear_keys();
        for _ in 0..3 {
            h.press(KeyCode::ArrowRight);
            h.update();
            h.clear_keys();
        }
        h.press(KeyCode::Enter);
        h.update();
        assert_eq!(h.player_tile(), (10, 10), "벽이면 이동 안 함");
        assert_eq!(h.mp(), 20, "실패 시 MP 변화 없음");
        assert!(h.targeting_active(), "실패 시 조준 유지");
        assert!(h.last_log().unwrap().contains("점멸"));
    }

    // ── 조준 취소 / 가드 ─────────────────────────────────────────────────────

    #[test]
    fn 조준중_ESC를_누르면_취소되고_커서가_사라진다() {
        let mut h = SkillHarness::new((10, 10), 20, 10);
        h.press(KeyCode::Digit1);
        h.update();
        h.clear_keys();
        h.press(KeyCode::Escape);
        h.update();
        assert!(!h.targeting_active(), "ESC 로 조준 취소");
        assert_eq!(h.app.world.query::<&SkillCursor>().iter(&h.app.world).count(), 0, "커서 despawn");
    }

    #[test]
    fn 조준중_커서이동은_맵_경계에서_클램프된다() {
        let mut h = SkillHarness::new((0, 0), 20, 10);
        h.press(KeyCode::Digit1);
        h.update();
        h.clear_keys();
        h.press(KeyCode::ArrowLeft);
        h.press(KeyCode::ArrowDown);
        h.update();
        assert_eq!(h.cursor(), (0, 0), "0 미만으로 안 내려감");
    }

    #[test]
    fn 조준중_WASD로도_커서가_이동한다() {
        let mut h = SkillHarness::new((10, 10), 20, 10);
        h.press(KeyCode::Digit1);
        h.update();
        h.clear_keys();
        h.press(KeyCode::KeyD); // 오른쪽
        h.press(KeyCode::KeyW); // 위
        h.update();
        assert_eq!(h.cursor(), (11, 11));
    }

    #[test]
    fn 조준중_왼쪽화살표와_아래화살표로_커서가_이동한다() {
        // ArrowLeft(좌변) + ArrowDown(좌변) 분기, 그리고 dx==0 일 때 dy!=0 평가.
        let mut h = SkillHarness::new((10, 10), 20, 10);
        h.press(KeyCode::Digit1);
        h.update();
        h.clear_keys();
        h.press(KeyCode::ArrowLeft);
        h.press(KeyCode::ArrowDown);
        h.update();
        assert_eq!(h.cursor(), (9, 9));
    }

    #[test]
    fn 조준중_위화살표만_누르면_세로로만_이동한다() {
        // dx==0, dy!=0 — `dx != 0 || dy != 0` 의 우변(dy != 0) True 분기.
        let mut h = SkillHarness::new((10, 10), 20, 10);
        h.press(KeyCode::Digit1);
        h.update();
        h.clear_keys();
        h.press(KeyCode::ArrowUp);
        h.update();
        assert_eq!(h.cursor(), (10, 11), "세로로만 이동");
    }

    #[test]
    fn 조준중_A_S키로도_커서가_이동한다() {
        // KeyA(ArrowLeft 의 우변) + KeyS(ArrowDown 의 우변) 분기.
        let mut h = SkillHarness::new((10, 10), 20, 10);
        h.press(KeyCode::Digit1);
        h.update();
        h.clear_keys();
        h.press(KeyCode::KeyA);
        h.press(KeyCode::KeyS);
        h.update();
        assert_eq!(h.cursor(), (9, 9));
    }

    #[test]
    fn 장비패널이_열려있으면_스킬입력을_무시한다() {
        let mut h = SkillHarness::new((10, 10), 20, 10);
        h.app.world.resource_mut::<EquipmentPanelOpen>().0 = true;
        h.press(KeyCode::Digit3);
        h.update();
        assert_eq!(h.hp(), 10, "패널 열림 시 치유 안 됨");
    }

    #[test]
    fn 상점패널이_열려있으면_스킬입력을_무시한다() {
        let mut h = SkillHarness::new((10, 10), 20, 10);
        h.app.world.resource_mut::<ShopPanelOpen>().0 = true;
        h.press(KeyCode::Digit3);
        h.update();
        assert_eq!(h.hp(), 10);
    }

    #[test]
    fn 도움말패널이_열려있으면_스킬입력을_무시한다() {
        let mut h = SkillHarness::new((10, 10), 20, 10);
        h.app.world.resource_mut::<HelpPanelOpen>().0 = true;
        h.press(KeyCode::Digit3);
        h.update();
        assert_eq!(h.hp(), 10);
    }

    #[test]
    fn 원격조준_활성중이면_스킬입력을_무시한다() {
        let mut h = SkillHarness::new((10, 10), 20, 10);
        h.app.world.resource_mut::<RangedTargeting>().active = true;
        h.press(KeyCode::Digit3);
        h.update();
        assert_eq!(h.hp(), 10);
    }

    #[test]
    fn 플레이어가_사망상태면_스킬입력을_무시한다() {
        let mut h = SkillHarness::new((10, 10), 20, 10);
        h.app.world.entity_mut(h.player).insert(Defeated);
        h.press(KeyCode::Digit3);
        h.update();
        assert_eq!(h.hp(), 10, "Defeated 면 query 가 비어 무시");
    }

    #[test]
    fn 매핑되지_않은_키는_아무_스킬도_시전하지_않는다() {
        let mut h = SkillHarness::new((10, 10), 20, 10);
        h.press(KeyCode::Digit4);
        h.update();
        assert_eq!(h.hp(), 10);
        assert!(!h.targeting_active());
    }

    #[test]
    fn 비활성_상태에서_입력이_없으면_아무것도_하지_않는다() {
        let mut h = SkillHarness::new((10, 10), 20, 10);
        h.update();
        assert!(!h.targeting_active());
        assert_eq!(h.hp(), 10);
    }

    // ── update_skill_cursor ──────────────────────────────────────────────────

    fn cursor_app() -> App {
        let mut app = App::new();
        app.init_resource::<SkillTargeting>();
        app.add_systems(Update, update_skill_cursor);
        app
    }

    #[test]
    fn 조준중_커서엔티티는_커서좌표로_이동한다() {
        let mut app = cursor_app();
        let e = app.world.spawn((
            Transform::from_xyz(0.0, 0.0, 2.0),
            SkillCursor,
        )).id();
        {
            let mut t = app.world.resource_mut::<SkillTargeting>();
            t.active = true;
            t.cursor = (8, 5);
        }
        app.update();
        let coord = tile_to_world_coords(8, 5);
        assert_eq!(app.world.get::<Transform>(e).unwrap().translation.x, coord.x);
        assert_eq!(app.world.get::<Transform>(e).unwrap().translation.y, coord.y);
    }

    #[test]
    fn 비활성_상태면_커서를_갱신하지_않는다() {
        let mut app = cursor_app();
        let e = app.world.spawn((
            Transform::from_xyz(0.0, 0.0, 2.0),
            SkillCursor,
        )).id();
        app.update(); // active=false
        assert_eq!(app.world.get::<Transform>(e).unwrap().translation.x, 0.0, "위치 그대로");
    }

    #[test]
    fn 조준중_커서엔티티가_없으면_갱신은_조용히_반환한다() {
        let mut app = cursor_app();
        app.world.resource_mut::<SkillTargeting>().active = true;
        app.update(); // 커서 없음 → get_single_mut Err → 반환 (panic 없음)
    }

    // ── 플러그인 빌드 ────────────────────────────────────────────────────────

    #[test]
    fn 스킬플러그인은_빌드시_조준리소스를_등록한다() {
        let mut app = App::new();
        app.add_plugins(SkillPlugin);
        assert!(app.world.get_resource::<SkillTargeting>().is_some());
    }
}
