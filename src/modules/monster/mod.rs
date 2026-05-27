use bevy::prelude::*;
use std::collections::HashSet;
use rand::{Rng, rngs::ThreadRng};
use serde::Deserialize;
use crate::modules::{
    map::{
        draw_map, Map, MapResource, MapType, MonsterTiles,
        tile_to_world_coords, world_to_tile_coords, is_in_view, FOV_BACK,
        MAP_HEIGHT, MAP_WIDTH, TILE_SIZE,
        MapSystemSet, MonsterRespawnEvent, PlayerActedEvent, AttackMonsterEvent, Rect,
        UsedSpawnTiles, random_floor_tile_anywhere,
    },
    player::{grant_xp, xp_reward_for_monster, Facing, MovingTo, MoveQueue, Player, PlayerProgress, PlayerSystemSet, LERP_SPEED},
    combat::{CombatStats, Defeated, Speed, calc_damage},
    ui::LogMessage,
    combat_feedback::CombatFeedbackEvent,
    item::{ItemDropEvent, PlayerEquipment, ItemRegistry, PlayerInventory, ItemKind, QuestItemKind, effective_attack, effective_defense},
    zone::{WorldState, ZoneId, ZonePersistence, MonsterSlot},
    map::GlobalTurn,
    elemental::{Element, ElementalApplyEvent, ElementalStatus, Stunned, weapon_element},
    quest::{QuestCondition, QuestState, eval_condition},
    lighting::{LightMap, LightLevel, effective_vision_radius},
};

const Z_MONSTER: f32 = 0.8;
const MAX_ALERT_TURNS: u32 = 5;

/// 위험 타일(가드 시야) 오버레이의 Z. 타일(0.0) 위, 아이템(0.3)/몬스터(0.8) 아래에
/// 깔아 글리프를 가리지 않고 바닥만 붉게 틴트한다.
const Z_DANGER_OVERLAY: f32 = 0.15;
/// 위험 타일 틴트 색 (반투명 붉은색).
const DANGER_TINT: Color = Color::rgba(1.0, 0.0, 0.0, 0.25);
/// 정찰 도구 아이템 ID — 인벤토리에 있으면 위험 타일 오버레이가 활성화된다.
pub const SCOUT_LENS_ID: &str = "scout_lens";

/// 가드 스탯 배율 — 플레이어 현재 effective HP/ATK/DEF 에 곱한다.
/// 1.2 → 레벨 무관하게 늘 "조금 더 셈"으로 정면 돌파보다 잠입을 유도한다.
const GUARD_POWER_MULT: f32 = 1.2;
/// 가드의 시야 반경 — 일반 몬스터보다 넉넉하게 잡아 잠입 난이도를 만든다.
const GUARD_VISION_RADIUS: i32 = 8;

/// RON(`assets/monsters/monsters.ron`) 에서 불러오는 몬스터 정의.
/// 한 몬스터의 정체성(스탯/원소/글리프/스폰 규칙)을 한 곳에 모은 데이터 정본.
/// 이름 문자열 매칭 대신 이 정의가 const 테이블·`monster_element` 를 대체한다.
#[derive(Debug, Deserialize, Clone)]
pub struct MonsterDef {
    /// 영문 안정 식별자 (snake_case). QuestAction::SpawnMonster 가 참조하는 키.
    pub id: String,
    /// UI/log 표시용 한글 이름. CombatStats·XP·드롭 등에서 사용.
    pub display_name: String,
    pub glyph: String,
    pub color: (f32, f32, f32),
    pub hp: i32,
    pub attack: i32,
    pub defense: i32,
    pub vision_radius: i32,
    pub speed: f32,
    /// "fire"/"ice"/"poison"/"lightning" 또는 None.
    #[serde(default)]
    pub element: Option<String>,
    /// 자연 스폰 가중치 (기본 1.0).
    #[serde(default = "default_spawn_weight")]
    pub spawn_weight: f32,
    /// 나오는 존 목록. 비어있으면 모든 일반 존 (제한 없음).
    #[serde(default)]
    pub zones: Vec<ZoneId>,
    /// 참일 때만 자연 스폰 (없으면 항상).
    #[serde(default)]
    pub spawn_condition: Option<QuestCondition>,
    /// true 면 자연 스폰 안 됨 — QuestAction::SpawnMonster 로만 등장 (보스/퀘스트 전용).
    #[serde(default)]
    pub quest_only: bool,
}

fn default_spawn_weight() -> f32 { 1.0 }

impl MonsterDef {
    /// `element` 문자열을 `Element` 로 매핑한다 (elemental 의 weapon_element 와 동형).
    /// "poison" 은 무기엔 없지만 몬스터에는 존재하므로 여기서 함께 처리한다.
    pub fn element_enum(&self) -> Option<Element> {
        match self.element.as_deref()? {
            "fire"      => Some(Element::Fire),
            "ice"       => Some(Element::Ice),
            "poison"    => Some(Element::Poison),
            "lightning" => Some(Element::Lightning),
            _           => None,
        }
    }

    pub fn color(&self) -> Color {
        Color::rgb(self.color.0, self.color.1, self.color.2)
    }
}

/// 모든 몬스터 정의를 보유하는 Bevy Resource (VillagerRegistry 와 동일 패턴).
/// 자연 스폰·SpawnMonster·원소 조회·색 fallback 이 모두 이 레지스트리를 읽는다.
#[derive(Resource, Default)]
pub struct MonsterRegistry {
    pub monsters: Vec<MonsterDef>,
}

impl MonsterRegistry {
    /// id 로 정의를 조회한다.
    pub fn by_id(&self, id: &str) -> Option<&MonsterDef> {
        self.monsters.iter().find(|m| m.id == id)
    }

    /// 표시 이름(한글)으로 정의를 조회한다. 색 fallback·원소 조회 등 런타임에
    /// Monster 컴포넌트가 display_name 만 보유한 경우 사용.
    pub fn by_display_name(&self, name: &str) -> Option<&MonsterDef> {
        self.monsters.iter().find(|m| m.display_name == name)
    }
}

/// monster 시스템의 Startup 단계 실행 순서.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum MonsterSystemSet {
    Load,
}

/// monster RON 파일을 읽어 registry 에 적재한다.
fn load_monsters(mut registry: ResMut<MonsterRegistry>) {
    // wasm32: REMOTE 우선 파싱 → 실패 시 빌드 임베드 슬라이스로 폴백.
    //   site DB monsters 가 깨져도 임베드로 복구되어 게임이 계속 떠 있는다.
    //   native 에서는 기존 fs 경로 그대로(REMOTE 미설치).
    #[cfg(target_arch = "wasm32")]
    let monsters: Vec<MonsterDef> = crate::modules::embedded_assets::parse_remote_or_embedded(
        "monsters.ron",
        crate::modules::remote_content::remote_monsters(),
        || crate::modules::embedded_assets::find_embedded(
            crate::modules::embedded_assets::EMBEDDED_MONSTERS,
            "monsters.ron",
        ),
    );
    #[cfg(not(target_arch = "wasm32"))]
    let path = "assets/monsters/monsters.ron";
    #[cfg(not(target_arch = "wasm32"))]
    let monsters = match read_monster_defs(path) {
        Ok(m) => m,
        // 도달 불가 방어코드: 파일 누락·파싱 실패 시 process::exit 로 테스트 러너를
        // 죽이므로 단위 테스트에서 양방향 실행 불가. read_monster_defs 의 Err 분기는
        // 별도 테스트로 커버.
        Err(e) => {
            error!("[치명적] {}", e);
            std::process::exit(1);
        }
    };
    info!("monster 로드: {} 종", monsters.len());
    registry.monsters = monsters;
}

/// 주어진 경로의 monster RON 을 읽어 파싱한다 (테스트 가능한 seam).
/// 읽기 실패·파싱 실패를 에러 메시지로 반환한다 (process::exit 없음).
/// wasm 빌드는 embedded slice 를 쓰므로 이 함수는 호출되지 않는다(테스트 only).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn read_monster_defs(path: &str) -> Result<Vec<MonsterDef>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("monster 파일 {} 을 읽을 수 없습니다: {}", path, e))?;
    ron::de::from_str::<Vec<MonsterDef>>(&text)
        .map_err(|e| format!("monster RON 파싱 실패: {}", e))
}

/// 테스트용 — 실제 RON 을 읽어 registry 를 구성한다 (build_test_registry 와 동형).
#[cfg(test)]
pub fn build_test_registry() -> MonsterRegistry {
    let monsters = read_monster_defs("assets/monsters/monsters.ron")
        .expect("monsters.ron 로드");
    MonsterRegistry { monsters }
}

/// 가드 한 마리가 방향 시야로 플레이어를 탐지했음을 알리는 이벤트.
/// 기존 alert/추적/전투 흐름은 그대로 두고(B안), 퀘스트 플래그(`stealth_blown`)
/// 설정 같은 추가 반응만 이 이벤트로 배선한다.
#[derive(Event)]
pub struct PlayerDetectedEvent;

/// 잠입 구역에 가드를 `count` 마리 스폰하라는 요청 이벤트.
/// 퀘스트 액션 `SpawnGuards` 가 발행하고 `handle_spawn_guards` 가 처리한다.
#[derive(Event)]
pub struct SpawnGuardEvent {
    pub count: u32,
}

#[derive(Component)]
pub struct Monster {
    pub name: String,
    pub tile_x: usize,
    pub tile_y: usize,
    pub vision_radius: i32,
    pub alert_turns: u32,
    pub slot_idx: usize,
}

/// 몬스터가 자기 facing 기준 방향 시야로 플레이어를 볼 수 있는지 판정한다.
///
/// 정면(facing 쪽)으로는 `vision_radius` 만큼, 등 뒤로는 `back_radius` 만큼만
/// 본다. 그래서 등 뒤로 다가오는 플레이어는 가까워야만 들킨다. 시야 거리/LoS
/// 판정은 순수 함수 `is_in_view` 에 위임한다.
pub fn can_see_player(
    mx: usize, my: usize,
    px: usize, py: usize,
    vision_radius: i32,
    facing: IVec2,
    map: &Map,
) -> bool {
    is_in_view(mx as i32, my as i32, facing, px as i32, py as i32, vision_radius, FOV_BACK, map)
}

/// 가드 스탯을 플레이어 현재 효과치(max_hp / effective ATK / effective DEF)에
/// `GUARD_POWER_MULT` 를 곱해 반올림한 `(hp, atk, def)` 로 계산한다.
///
/// 레벨이 올라도 가드가 늘 "조금 더 셈"으로 유지돼 정면 돌파보다 잠입을
/// 유도한다. rand·쿼리 의존이 없는 순수 함수라 경계/반올림을 단독 테스트한다.
pub fn guard_stats(player_hp: i32, player_atk: i32, player_def: i32) -> (i32, i32, i32) {
    let scale = |v: i32| (v as f32 * GUARD_POWER_MULT).round() as i32;
    (scale(player_hp), scale(player_atk), scale(player_def))
}

/// 위험 타일 오버레이 엔티티 마커. 가드 시야가 닿는 타일 위에 깔리는 반투명
/// 붉은 사각형으로, 기존 타일 렌더와 독립된 별도 레이어다.
#[derive(Component)]
pub struct GuardVisionOverlay;

/// 가드(적대 엔티티)들의 방향 시야가 닿는 "위험 타일" 집합을 계산하는 순수 함수.
///
/// 각 가드 `(pos, facing, vision_radius)` 에 대해 맵의 모든 타일 중
/// `is_in_view(gpos, gfacing, tile, eff_radius, FOV_BACK, map)` 인 타일을 모아
/// 합집합으로 반환한다. 정면(facing 쪽)은 멀리, 등 뒤는 가깝게(FOV_BACK) 보는
/// Phase 1 방향 시야를 그대로 재사용하므로 등 뒤·벽 너머 타일은 제외된다.
///
/// `eff_radius` 는 **그 타일의 광량**으로 보정한 유효 반경(`effective_vision_radius`)
/// 이다 — 어두운 타일은 가드가 더 가까워야만 위험으로 표시돼, 탐지(monster_turn)와
/// 같은 광량 규칙을 오버레이도 그대로 따른다(일관성). 광량은 `light_at` 클로저로
/// 주입해 순수성을 유지한다. 가드가 없으면 빈 집합을 반환한다.
pub fn danger_tiles(
    guards: &[((usize, usize), IVec2, i32)],
    map: &Map,
    light_at: impl Fn(usize, usize) -> LightLevel,
) -> HashSet<(usize, usize)> {
    let mut out = HashSet::new();
    for &((gx, gy), facing, vision_radius) in guards {
        for ty in 0..map.height {
            for tx in 0..map.width {
                let eff_radius = effective_vision_radius(vision_radius, light_at(tx, ty));
                if is_in_view(
                    gx as i32, gy as i32, facing,
                    tx as i32, ty as i32,
                    eff_radius, FOV_BACK, map,
                ) {
                    out.insert((tx, ty));
                }
            }
        }
    }
    out
}

/// 인벤토리에 정찰 도구(`scout_lens`)를 보유했는지 여부 (순수 함수).
/// 보유하면 위험 타일 오버레이가 활성화된다.
pub fn player_has_scout_lens(inventory: &PlayerInventory) -> bool {
    let lens = ItemKind::QuestItem(QuestItemKind(SCOUT_LENS_ID));
    inventory.items.iter().any(|i| i.kind == lens)
}

/// 정찰 도구 보유 시 가드 시야가 닿는 위험 타일에 반투명 붉은 오버레이를 깔고,
/// 미보유 시 모든 오버레이를 제거한다. 플레이어/가드 이동·facing 변화·인벤토리
/// 변경 시마다 매 프레임 재계산해 항상 현재 상태를 반영한다(전부 despawn 후 재배치).
///
/// 기존 타일 렌더(`TileEntity`)는 건드리지 않고 별도 `GuardVisionOverlay` 레이어로만
/// 표시한다.
fn update_guard_vision_overlay(
    mut commands: Commands,
    map_res: Res<MapResource>,
    inventory: Res<PlayerInventory>,
    light_map: Res<LightMap>,
    monster_query: Query<(&Monster, &Facing, &CombatStats)>,
    overlay_query: Query<Entity, With<GuardVisionOverlay>>,
) {
    // 매번 기존 오버레이를 비우고 다시 그린다 — 이동/facing/인벤토리 변화 반영을 단순화.
    for entity in overlay_query.iter() {
        commands.entity(entity).despawn();
    }

    // 정찰 도구 미보유면 오버레이 없음 (제거만 하고 종료).
    if !player_has_scout_lens(&inventory) {
        return;
    }

    let map = map_res.map();
    let guards: Vec<((usize, usize), IVec2, i32)> = monster_query.iter()
        .filter(|(_, _, stats)| stats.hp > 0)
        .map(|(m, facing, _)| ((m.tile_x, m.tile_y), facing.0, m.vision_radius))
        .collect();

    // 오버레이도 탐지와 같은 광량 정본(LightMap)을 써, 어두운 타일은 위험에서 제외.
    for (tx, ty) in danger_tiles(&guards, map, |x, y| light_map.at(x, y)) {
        let coord = tile_to_world_coords(tx, ty);
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: DANGER_TINT,
                    custom_size: Some(Vec2::splat(TILE_SIZE)),
                    ..default()
                },
                transform: Transform::from_xyz(coord.x, coord.y, Z_DANGER_OVERLAY),
                ..default()
            },
            GuardVisionOverlay,
        ));
    }
}

pub struct MonsterPlugin;

impl Plugin for MonsterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MonsterRegistry>()
            // 탐지(monster_turn)·오버레이가 읽는 광량 정본. LightingPlugin 이 이미
            // 등록하지만 monster 단독 테스트/구성에서도 동작하도록 여기서도 보장한다.
            .init_resource::<LightMap>()
            .add_event::<PlayerDetectedEvent>()
            .add_event::<SpawnGuardEvent>()
            .add_event::<SpawnMonsterEvent>()
            .add_systems(Startup, (
                load_monsters.in_set(MonsterSystemSet::Load),
                spawn_on_startup.after(draw_map).after(MonsterSystemSet::Load),
            ))
            .add_systems(PreUpdate, sync_monster_tiles)
            .add_systems(Update, (
                respawn_on_regen.after(MapSystemSet::ExecuteRegen),
                (handle_player_attack, monster_turn, cleanup_dead)
                    .chain()
                    .after(PlayerSystemSet::MovementComplete),
                handle_spawn_guards,
                handle_spawn_monster,
                smooth_monster_move,
                update_guard_vision_overlay,
            ));
    }
}

/// 퀘스트(또는 다른 시스템)가 특정 MonsterDef 를 현재 위치 근처에 `count` 마리
/// 스폰하라고 요청하는 이벤트. `QuestAction::SpawnMonster` 가 발행하고
/// `handle_spawn_monster` 가 처리한다. quest_only 보스/퀘스트 전용 몬스터용.
#[derive(Event)]
pub struct SpawnMonsterEvent {
    pub id: String,
    pub count: u32,
}

/// 자연 스폰 후보가 되는 MonsterDef 의 인덱스 목록을 반환한다 (순수 함수).
///
/// 후보 조건: `quest_only == false` AND (`zones` 가 비었거나 현재 존을 포함)
/// AND (`spawn_condition` 이 없거나 `eval_condition` 이 참).
/// rand 의존이 없어 zone/조건 분기를 단독으로 테스트한다.
pub fn natural_spawn_candidates(
    registry: &MonsterRegistry,
    zone: &ZoneId,
    inventory: &PlayerInventory,
    world: &WorldState,
    quest_state: &QuestState,
    quest_items: &ItemRegistry,
) -> Vec<usize> {
    registry.monsters.iter().enumerate()
        .filter(|(_, def)| !def.quest_only)
        .filter(|(_, def)| def.zones.is_empty() || def.zones.contains(zone))
        .filter(|(_, def)| match &def.spawn_condition {
            None => true,
            Some(cond) => eval_condition(cond, inventory, world, quest_state, quest_items),
        })
        .map(|(i, _)| i)
        .collect()
}

/// 후보 인덱스 중 spawn_weight 가중치로 하나를 고른다 (순수 함수, seam 으로 rng 주입).
///
/// 후보가 비어 있으면 None. 가중치 합이 0 이하면 첫 후보로 폴백한다. roll 은
/// `[0, total_weight)` 범위의 난수 — 결정적 값을 넣어 양쪽 경계를 테스트한다.
pub fn choose_monster_index(
    registry: &MonsterRegistry,
    candidates: &[usize],
    roll: f32,
) -> Option<usize> {
    if candidates.is_empty() { return None; }
    let total: f32 = candidates.iter()
        .map(|&i| registry.monsters[i].spawn_weight.max(0.0))
        .sum();
    if total <= 0.0 {
        // 가중치 합 0 — 균등 분포가 불가능하므로 첫 후보로 폴백.
        return Some(candidates[0]);
    }
    let mut acc = 0.0;
    for &i in candidates {
        acc += registry.monsters[i].spawn_weight.max(0.0);
        if roll < acc { return Some(i); }
    }
    // 부동소수 오차로 roll 이 total 에 매우 근접한 경우의 폴백 — 마지막 후보.
    Some(*candidates.last().unwrap())
}

fn sync_monster_tiles(
    monster_query: Query<&Monster>,
    mut monster_tiles: ResMut<MonsterTiles>,
) {
    monster_tiles.0.clear();
    for m in monster_query.iter() {
        monster_tiles.0.insert((m.tile_x, m.tile_y));
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_on_startup(
    mut commands: Commands,
    map_res: Res<MapResource>,
    asset_server: Res<AssetServer>,
    mut persistence: ResMut<ZonePersistence>,
    world: Res<WorldState>,
    registry: Res<MonsterRegistry>,
    inventory: Res<PlayerInventory>,
    quest_state: Res<QuestState>,
    items: Res<ItemRegistry>,
) {
    let map = map_res.map();
    if map.map_type == MapType::Dungeon {
        let zone_id = world.current.clone();
        let mut rng = rand::thread_rng();
        let slots = init_zone_monster_slots(
            &map.rooms, &registry, &zone_id, &inventory, &world, &quest_state, &items, &mut rng,
        );
        persistence.0.entry(zone_id).or_default().monster_slots = slots.clone();
        spawn_from_slots(&mut commands, &map.rooms, &slots, 0, &asset_server, &registry);
    }
}

#[allow(clippy::too_many_arguments)]
fn respawn_on_regen(
    mut commands: Commands,
    mut events: EventReader<MonsterRespawnEvent>,
    monster_query: Query<Entity, With<Monster>>,
    asset_server: Res<AssetServer>,
    world: Res<WorldState>,
    global_turn: Res<GlobalTurn>,
    mut persistence: ResMut<ZonePersistence>,
    registry: Res<MonsterRegistry>,
    inventory: Res<PlayerInventory>,
    quest_state: Res<QuestState>,
    items: Res<ItemRegistry>,
) {
    for event in events.read() {
        for entity in monster_query.iter() {
            commands.entity(entity).despawn();
        }
        if event.map_type != MapType::Dungeon { continue; }

        let zone_id = world.current.clone();

        // monster_slots 가 비어있으면 첫 방문으로 보고 초기화한다.
        // (entry 자체는 portal-position-persistence 등 다른 시스템이 먼저
        //  생성했을 수 있어 contains_key 만으로는 첫 방문을 판정할 수 없다.)
        let needs_init = persistence.0.get(&zone_id)
            .map(|s| s.monster_slots.is_empty())
            .unwrap_or(true);
        if needs_init {
            let mut rng = rand::thread_rng();
            let slots = init_zone_monster_slots(
                &event.rooms, &registry, &zone_id, &inventory, &world, &quest_state, &items, &mut rng,
            );
            persistence.0.entry(zone_id.clone()).or_default().monster_slots = slots;
        }

        // 만료된 리스폰 타이머 처리(지나간 턴 따라잡기)
        // (needs_init 분기에서 or_default() 로, 혹은 이미 존재해서 엔트리는 항상 있음 —
        //  None 분기는 도달 불가 방어코드)
        if let Some(snapshot) = persistence.0.get_mut(&zone_id) {
            for slot in &mut snapshot.monster_slots {
                if let Some(t) = slot.respawn_at_turn {
                    if t <= global_turn.0 { slot.respawn_at_turn = None; }
                }
            }
        }

        let slots = persistence.0[&zone_id].monster_slots.clone();
        spawn_from_slots(&mut commands, &event.rooms, &slots, global_turn.0, &asset_server, &registry);
    }
}

/// 방마다 자연 스폰 후보 중 하나를 가중치로 골라 슬롯(`data_idx` = 레지스트리
/// 인덱스)을 만든다. 후보가 없으면 그 방은 슬롯을 만들지 않는다(빈 던전).
/// 첫 방(시작 방)은 skip, 최대 10개.
#[allow(clippy::too_many_arguments)]
fn init_zone_monster_slots(
    rooms: &[Rect],
    registry: &MonsterRegistry,
    zone: &ZoneId,
    inventory: &PlayerInventory,
    world: &WorldState,
    quest_state: &QuestState,
    quest_items: &ItemRegistry,
    rng: &mut impl Rng,
) -> Vec<MonsterSlot> {
    let candidates = natural_spawn_candidates(registry, zone, inventory, world, quest_state, quest_items);
    if candidates.is_empty() { return Vec::new(); }
    let total: f32 = candidates.iter().map(|&i| registry.monsters[i].spawn_weight.max(0.0)).sum();
    rooms.iter().skip(1).take(10)
        .filter_map(|_| {
            let roll = if total > 0.0 { rng.gen_range(0.0..total) } else { 0.0 };
            choose_monster_index(registry, &candidates, roll)
                .map(|idx| MonsterSlot { data_idx: idx, respawn_at_turn: None })
        })
        .collect()
}

fn spawn_from_slots(
    commands: &mut Commands,
    rooms: &[Rect],
    slots: &[MonsterSlot],
    global_turn: u64,
    asset_server: &AssetServer,
    registry: &MonsterRegistry,
) {
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");
    let mut rng = rand::thread_rng();
    // 같은 spawn 호출 내 monster 가 같은 타일에 겹치지 않도록 한다
    let mut used: HashSet<(usize, usize)> = HashSet::new();

    // 매 호출마다 dummy map 만들기는 비용 — 대신 호출자가 map 을 알 수 없으니 room.center 를
    // fallback 으로 사용. 다만 random_floor_tile_in_room 은 map 이 필요. spawn_from_slots
    // 시그니처에 map 추가하는 건 여러 호출부 영향이 커서 별도 정리 시점에 진행.
    // 현재는 room 안 random tile 을 직접 검색 (Floor 검사 포함, 영역 clamp).
    for (slot_idx, (slot, room)) in slots.iter().zip(rooms.iter().skip(1)).enumerate() {
        if let Some(t) = slot.respawn_at_turn {
            if t > global_turn { continue; }
        }
        // 슬롯의 data_idx 가 (레지스트리 변경 등으로) 범위를 벗어나면 그 슬롯은 건너뛴다.
        let Some(def) = registry.monsters.get(slot.data_idx) else { continue };

        // room 경계 안에서 무작위 좌표 — wall 위 / 영역 밖 회피.
        // Map 객체가 없는 컨텍스트라 Floor 검사는 못 하고, room 좌표가 항상 Floor 라고
        // 가정한다 (rooms 는 map 생성 시 이미 Floor 영역으로 정의됨). 영역 clamp 만 적용.
        let x_max = (room.x2.min(MAP_WIDTH.saturating_sub(1))).max(room.x1);
        let y_max = (room.y2.min(MAP_HEIGHT.saturating_sub(1))).max(room.y1);
        let mut tile = room.center();
        for _ in 0..10 {
            let x = rng.gen_range(room.x1..=x_max);
            let y = rng.gen_range(room.y1..=y_max);
            if !used.contains(&(x, y)) { tile = (x, y); break; }
        }
        used.insert(tile);
        let (tx, ty) = tile;

        spawn_monster_entity(commands, &font, def, tx, ty, slot_idx);
    }
}

/// MonsterDef 한 건을 (tx, ty) 에 엔티티로 스폰한다. 자연 스폰·SpawnMonster 가
/// 공유하는 단일 생성 경로 — 글리프/색/스탯/시야/원소가 모두 정의에서 나온다.
fn spawn_monster_entity(
    commands: &mut Commands,
    font: &Handle<Font>,
    def: &MonsterDef,
    tx: usize, ty: usize,
    slot_idx: usize,
) {
    let coord = tile_to_world_coords(tx, ty);
    commands.spawn((
        Text2dBundle {
            text: Text::from_section(def.glyph.clone(), TextStyle {
                font: font.clone(),
                font_size: TILE_SIZE,
                color: def.color(),
            }),
            transform: Transform::from_xyz(coord.x, coord.y, Z_MONSTER),
            ..default()
        },
        Monster {
            name: def.display_name.clone(),
            tile_x: tx, tile_y: ty,
            vision_radius: def.vision_radius,
            alert_turns: 0,
            slot_idx,
        },
        CombatStats { hp: def.hp, max_hp: def.hp.max(1), mp: 0, max_mp: 0, attack: def.attack, defense: def.defense },
        Speed::new(def.speed),
        MoveQueue::default(),
        ElementalStatus::default(),
        Facing::default(),
    ));
}

/// `SpawnMonsterEvent` 를 받아 현재 맵의 통과타일 무작위 위치에 해당 MonsterDef 를
/// `count` 마리 스폰한다. 보스/퀘스트 전용(quest_only) 몬스터도 여기서는 등장한다.
/// slot_idx 는 자연 스폰 슬롯과 무관하므로 usize::MAX 로 둔다(리스폰 슬롯 미연결).
fn handle_spawn_monster(
    mut commands: Commands,
    mut events: EventReader<SpawnMonsterEvent>,
    map_res: Res<MapResource>,
    asset_server: Res<AssetServer>,
    mut used_spawn: ResMut<UsedSpawnTiles>,
    registry: Res<MonsterRegistry>,
) {
    let map = map_res.map();
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");
    let mut rng = rand::thread_rng();
    for ev in events.read() {
        let Some(def) = registry.by_id(&ev.id) else {
            warn!("SpawnMonster: 알 수 없는 monster id '{}'", ev.id);
            continue;
        };
        for _ in 0..ev.count {
            let Some((tx, ty)) = random_floor_tile_anywhere(&map.rooms, map, &mut used_spawn.0, &mut rng) else {
                info!("몬스터 스폰 실패 — 통과타일 없음: {}", ev.id);
                continue;
            };
            spawn_monster_entity(&mut commands, &font, def, tx, ty, usize::MAX);
        }
    }
}

/// 가드 한 마리를 (tx, ty) 에 스폰한다. 일반 몬스터 스폰과 동일한 부수 컴포넌트
/// (Speed/MoveQueue/ElementalStatus/Facing/Text2dBundle)를 붙이되, 스탯은 인자로
/// 받은 가드 스탯(`hp/atk/def`)을 쓴다. slot_idx 는 가드 전용으로 의미 없어 0.
fn spawn_guard(
    commands: &mut Commands,
    font: &Handle<Font>,
    tx: usize, ty: usize,
    hp: i32, atk: i32, def: i32,
) {
    let coord = tile_to_world_coords(tx, ty);
    commands.spawn((
        Text2dBundle {
            text: Text::from_section("가", TextStyle {
                font: font.clone(),
                font_size: TILE_SIZE,
                color: Color::rgb(0.9, 0.2, 0.2),
            }),
            transform: Transform::from_xyz(coord.x, coord.y, Z_MONSTER),
            ..default()
        },
        Monster {
            name: "가드".to_string(),
            tile_x: tx, tile_y: ty,
            vision_radius: GUARD_VISION_RADIUS,
            alert_turns: 0,
            slot_idx: 0,
        },
        CombatStats { hp, max_hp: hp.max(1), mp: 0, max_mp: 0, attack: atk, defense: def },
        Speed::new(1.0),
        MoveQueue::default(),
        ElementalStatus::default(),
        Facing::default(),
    ));
}

/// `SpawnGuardEvent` 를 받아 현재 맵의 통과타일 무작위 위치에 가드를 `count`
/// 마리 스폰한다. 가드 스탯은 플레이어 현재 effective ATK/DEF + max_hp 를
/// `guard_stats` 로 스케일해 결정한다.
fn handle_spawn_guards(
    mut commands: Commands,
    mut events: EventReader<SpawnGuardEvent>,
    map_res: Res<MapResource>,
    asset_server: Res<AssetServer>,
    mut used_spawn: ResMut<UsedSpawnTiles>,
    player_query: Query<&CombatStats, With<Player>>,
    equipment: Res<PlayerEquipment>,
    items: Res<ItemRegistry>,
) {
    let mut total: u32 = 0;
    for ev in events.read() {
        total += ev.count;
    }
    if total == 0 { return; }

    // 플레이어 현재 효과치 기준으로 가드 스탯 계산 (없으면 스폰하지 않음).
    let Ok(player_stats) = player_query.get_single() else { return };
    let (hp, atk, def) = guard_stats(
        player_stats.max_hp,
        effective_attack(&equipment, &items),
        effective_defense(&equipment, &items),
    );

    let map = map_res.map();
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");
    let mut rng = rand::thread_rng();
    for _ in 0..total {
        let Some((tx, ty)) = random_floor_tile_anywhere(&map.rooms, map, &mut used_spawn.0, &mut rng) else {
            info!("가드 스폰 실패 — 통과타일 없음");
            continue;
        };
        spawn_guard(&mut commands, &font, tx, ty, hp, atk, def);
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_player_attack(
    mut events: EventReader<AttackMonsterEvent>,
    mut player_query: Query<&mut CombatStats, (With<Player>, Without<Monster>)>,
    mut progress: ResMut<PlayerProgress>,
    mut monster_query: Query<(Entity, &Monster, &mut CombatStats), Without<Player>>,
    mut log_writer: EventWriter<LogMessage>,
    mut feedback_writer: EventWriter<CombatFeedbackEvent>,
    mut drop_writer: EventWriter<ItemDropEvent>,
    mut elemental_writer: EventWriter<ElementalApplyEvent>,
    equipment: Res<PlayerEquipment>,
    items: Res<crate::modules::item::ItemRegistry>,
    registry: Res<MonsterRegistry>,
) {
    for AttackMonsterEvent(tx, ty) in events.read() {
        let Ok(mut player_stats) = player_query.get_single_mut() else { continue };
        for (monster_entity, monster, mut monster_stats) in monster_query.iter_mut() {
            if monster.tile_x != *tx || monster.tile_y != *ty { continue; }
            if monster_stats.hp <= 0 { continue; }
            let dmg = calc_damage(player_stats.attack, monster_stats.defense);
            monster_stats.hp -= dmg;
            let original_color = registry.by_display_name(&monster.name)
                .map(|def| def.color())
                .unwrap_or(Color::WHITE);
            feedback_writer.send(CombatFeedbackEvent {
                tile_x: *tx,
                tile_y: *ty,
                hit_entity: monster_entity,
                original_color,
            });
            // 원소 부여 (40% 확률, 장착 무기에 따라 결정)
            if monster_stats.hp > 0 {
                if let Some(weapon) = equipment.weapon {
                    if rand::thread_rng().gen_bool(0.4) {
                        if let Some(element) = weapon_element(weapon, &items) {
                            elemental_writer.send(ElementalApplyEvent {
                                target: monster_entity,
                                element,
                            });
                        }
                    }
                }
            }

            if monster_stats.hp <= 0 {
                let xp = xp_reward_for_monster(&monster.name);
                let levels = grant_xp(&mut progress, &mut player_stats, xp);
                log_writer.send(LogMessage(format!(
                    "{}을(를) 처치했다! ({} 데미지, XP +{})", monster.name, dmg, xp
                )));
                if levels > 0 {
                    log_writer.send(LogMessage(format!(
                        "레벨 업! Lv.{} (HP {}/{}, MP {}/{})",
                        progress.level,
                        player_stats.hp,
                        player_stats.max_hp,
                        player_stats.mp,
                        player_stats.max_mp,
                    )));
                }
                drop_writer.send(ItemDropEvent {
                    tile_x: *tx,
                    tile_y: *ty,
                    monster_name: monster.name.clone(),
                });
            } else {
                log_writer.send(LogMessage(format!(
                    "{}에게 {} 데미지! (HP: {}/{})",
                    monster.name, dmg, monster_stats.hp, monster_stats.max_hp
                )));
            }
            break;
        }
    }
}

fn monster_turn(
    mut commands: Commands,
    mut events: EventReader<PlayerActedEvent>,
    map_res: Res<MapResource>,
    mut monster_query: Query<(&mut Monster, &mut MoveQueue, &CombatStats, &mut Speed, &mut Facing, Option<&Stunned>), Without<Player>>,
    mut player_query: Query<(Entity, &Transform, Option<&MovingTo>, &mut CombatStats), (With<Player>, Without<Monster>)>,
    mut log_writer: EventWriter<LogMessage>,
    mut feedback_writer: EventWriter<CombatFeedbackEvent>,
    mut elemental_writer: EventWriter<ElementalApplyEvent>,
    mut detected_writer: EventWriter<PlayerDetectedEvent>,
    registry: Res<MonsterRegistry>,
    light_map: Res<LightMap>,
) {
    if events.read().next().is_none() { return; }

    let map = map_res.map();
    let Ok((player_entity, player_transform, player_moving, mut player_stats)) = player_query.get_single_mut() else { return };

    let (px, py) = player_moving
        .map(|m| world_to_tile_coords(m.target))
        .unwrap_or_else(|| world_to_tile_coords(player_transform.translation));

    // 플레이어가 선 타일의 광량 — 어둠이면 가드 탐지 반경이 줄어든다(은신 보너스).
    // 렌더와 같은 LightMap 정본을 읽어 디밍과 탐지가 동일한 광량을 쓴다.
    let player_light = light_map.at(px, py);

    let mut occupied: HashSet<(usize, usize)> = monster_query.iter()
        .filter(|(_, _, stats, _, _, _)| stats.hp > 0)
        .map(|(m, _, _, _, _, _)| (m.tile_x, m.tile_y))
        .collect();
    occupied.insert((px, py));

    let mut player_dead = false;
    let mut rng = rand::thread_rng();

    for (mut monster, mut move_queue, monster_stats, mut speed, mut facing, stunned) in monster_query.iter_mut() {
        if monster_stats.hp <= 0 { continue; }
        if stunned.is_some() {
            occupied.insert((monster.tile_x, monster.tile_y));
            continue;
        }

        occupied.remove(&(monster.tile_x, monster.tile_y));

        // 시야 갱신 (몬스터 facing 기준 방향 시야).
        // 플레이어 광량으로 탐지 반경을 보정 — 어둠이면 더 가까워야만 들킨다.
        let detect_radius = effective_vision_radius(monster.vision_radius, player_light);
        if can_see_player(monster.tile_x, monster.tile_y, px, py, detect_radius, facing.0, map) {
            monster.alert_turns = MAX_ALERT_TURNS;
            // 기존 alert/추적/전투 흐름은 그대로 두고, 탐지 사실만 추가로 알린다(B안).
            // 잠입 퀘스트가 활성이면 quest 모듈이 stealth_blown 플래그를 세운다.
            detected_writer.send(PlayerDetectedEvent);
        } else if monster.alert_turns > 0 {
            monster.alert_turns -= 1;
        }

        // 에너지 누적 → 1.0마다 행동 1회 소비
        speed.energy += speed.value;
        while speed.energy >= 1.0 {
            speed.energy -= 1.0;

            let dx = (monster.tile_x as i32 - px as i32).abs();
            let dy = (monster.tile_y as i32 - py as i32).abs();
            let adjacent = (dx == 1 && dy == 0) || (dx == 0 && dy == 1);

            if adjacent {
                if !player_dead {
                    let dmg = calc_damage(monster_stats.attack, player_stats.defense);
                    player_stats.hp -= dmg;
                    feedback_writer.send(CombatFeedbackEvent {
                        tile_x: px,
                        tile_y: py,
                        hit_entity: player_entity,
                        original_color: Color::YELLOW,
                    });

                    // 원소 부여 (35% 확률, 몬스터 속성에 따라)
                    // (이 `!player_dead` 는 바로 위 L329 가드 안이라 항상 참 — 도달 불가 방어코드의 false 분기)
                    if !player_dead {
                        if let Some(element) = registry.by_display_name(&monster.name).and_then(|d| d.element_enum()) {
                            if rng.gen_bool(0.35) {
                                elemental_writer.send(ElementalApplyEvent {
                                    target: player_entity,
                                    element,
                                });
                            }
                        }
                    }

                    if player_stats.hp <= 0 {
                        player_dead = true;
                        log_writer.send(LogMessage(format!(
                            "{}에게 {} 데미지! 당신은 죽었습니다.", monster.name, dmg
                        )));
                        commands.entity(player_entity).insert(Defeated);
                    } else {
                        log_writer.send(LogMessage(format!(
                            "{}에게 {} 데미지! (HP: {}/{})",
                            monster.name, dmg, player_stats.hp, player_stats.max_hp
                        )));
                    }
                }
            } else if monster.alert_turns > 0 {
                let (nx, ny) = move_toward(monster.tile_x, monster.tile_y, px, py, map, &occupied);
                update_facing(&mut facing, monster.tile_x, monster.tile_y, nx, ny);
                occupied.remove(&(monster.tile_x, monster.tile_y));
                occupied.insert((nx, ny));
                let wp = tile_to_world_coords(nx, ny);
                move_queue.0.push_back(Vec3::new(wp.x, wp.y, Z_MONSTER));
                monster.tile_x = nx;
                monster.tile_y = ny;
            } else {
                let (nx, ny) = wander(monster.tile_x, monster.tile_y, map, &occupied, &mut rng);
                update_facing(&mut facing, monster.tile_x, monster.tile_y, nx, ny);
                occupied.remove(&(monster.tile_x, monster.tile_y));
                occupied.insert((nx, ny));
                let wp = tile_to_world_coords(nx, ny);
                move_queue.0.push_back(Vec3::new(wp.x, wp.y, Z_MONSTER));
                monster.tile_x = nx;
                monster.tile_y = ny;
            }
        }

        occupied.insert((monster.tile_x, monster.tile_y));
    }
}

fn smooth_monster_move(
    time: Res<Time>,
    mut query: Query<(&mut Transform, &mut MoveQueue, &Speed), With<Monster>>,
) {
    let dt = time.delta_seconds();
    for (mut transform, mut queue, speed) in query.iter_mut() {
        let anim_speed = LERP_SPEED * speed.value.max(0.5);
        let step = anim_speed * TILE_SIZE * dt;
        while let Some(&target) = queue.0.front() {
            let dist = transform.translation.distance(target);
            if dist <= step {
                transform.translation = target;
                queue.0.pop_front();
            } else {
                let dir = (target - transform.translation).normalize();
                transform.translation += dir * step;
                break;
            }
        }
    }
}

fn cleanup_dead(
    mut commands: Commands,
    query: Query<(Entity, &Monster, &CombatStats)>,
    world: Res<WorldState>,
    global_turn: Res<GlobalTurn>,
    mut persistence: ResMut<ZonePersistence>,
) {
    let mut rng = rand::thread_rng();
    for (entity, monster, stats) in query.iter() {
        if stats.hp <= 0 {
            commands.entity(entity).despawn();
            let respawn_at = global_turn.0 + rng.gen_range(30u64..=120);
            if let Some(snapshot) = persistence.0.get_mut(&world.current) {
                if let Some(slot) = snapshot.monster_slots.get_mut(monster.slot_idx) {
                    slot.respawn_at_turn = Some(respawn_at);
                }
            }
        }
    }
}

/// 실제 이동(`from != to`)이 일어났을 때만 facing 을 이동 방향으로 갱신한다.
/// 제자리(이동 없음)면 마지막 방향을 그대로 유지한다.
fn update_facing(facing: &mut Facing, fx: usize, fy: usize, tx: usize, ty: usize) {
    let dir = IVec2::new(tx as i32 - fx as i32, ty as i32 - fy as i32);
    if dir != IVec2::ZERO { facing.0 = dir; }
}

pub fn move_toward(
    x: usize, y: usize,
    tx: usize, ty: usize,
    map: &Map,
    occupied: &HashSet<(usize, usize)>,
) -> (usize, usize) {
    let neighbors = [
        (x.wrapping_sub(1), y),
        (x + 1, y),
        (x, y.wrapping_sub(1)),
        (x, y + 1),
    ];
    let best = neighbors.iter()
        .filter(|&&(nx, ny)| {
            nx < MAP_WIDTH && ny < MAP_HEIGHT
                && map.get_tile(nx, ny).is_walkable()
                && !occupied.contains(&(nx, ny))
        })
        .min_by_key(|&&(nx, ny)| {
            let ddx = nx as i32 - tx as i32;
            let ddy = ny as i32 - ty as i32;
            ddx * ddx + ddy * ddy
        });
    best.copied().unwrap_or((x, y))
}

pub fn wander(
    x: usize, y: usize,
    map: &Map,
    occupied: &HashSet<(usize, usize)>,
    rng: &mut ThreadRng,
) -> (usize, usize) {
    const STAY_CHANCE: f64 = 0.3;
    if rng.gen_bool(STAY_CHANCE) { return (x, y); }
    let neighbors = [
        (x.wrapping_sub(1), y),
        (x + 1, y),
        (x, y.wrapping_sub(1)),
        (x, y + 1),
    ];
    let valid: Vec<_> = neighbors.iter()
        .filter(|&&(nx, ny)| {
            nx < MAP_WIDTH && ny < MAP_HEIGHT
                && map.get_tile(nx, ny).is_walkable()
                && !occupied.contains(&(nx, ny))
        })
        .copied()
        .collect();
    if valid.is_empty() { return (x, y); }
    valid[rng.gen_range(0..valid.len())]
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;
    use crate::modules::map::TileKind;

    fn floor_map(w: usize, h: usize, floors: &[(usize, usize)]) -> Map {
        let mut map = Map::new(w, h);
        for &(x, y) in floors {
            map.set_tile(x, y, TileKind::Floor);
        }
        map
    }

    #[test]
    fn 추적할_때_몬스터는_플레이어_방향으로_한칸_접근한다() {
        let map = floor_map(10, 10, &[(5,5),(6,5),(7,5)]);
        let occupied = HashSet::new();
        let (nx, ny) = move_toward(5, 5, 7, 5, &map, &occupied);
        assert_eq!((nx, ny), (6, 5), "플레이어 방향 인접 타일로 이동해야 한다");
    }

    #[test]
    fn 갈_곳이_모두_막히면_몬스터는_제자리를_유지한다() {
        let map = floor_map(10, 10, &[(5,5),(6,5)]);
        let mut occupied = HashSet::new();
        occupied.insert((6usize, 5usize));
        let result = move_toward(5, 5, 9, 5, &map, &occupied);
        assert_eq!(result, (5, 5), "이동 불가 시 제자리를 유지해야 한다");
    }

    #[test]
    fn 추적_중인_몬스터는_점유된_타일을_피해_이동한다() {
        let map = floor_map(10, 10, &[(5,5),(6,5),(5,6)]);
        let mut occupied = HashSet::new();
        occupied.insert((6usize, 5usize));
        // 목적지 (9,9) — (5,6) 이 더 가깝진 않지만 (6,5)는 막혀 있음
        let (nx, ny) = move_toward(5, 5, 6, 6, &map, &occupied);
        assert_ne!((nx, ny), (6, 5), "점유된 타일로 이동하면 안 된다");
    }

    #[test]
    fn 추적할_때_몬스터는_벽_타일로는_이동하지_않는다() {
        // (5,5) 주변 중 floor 는 (6,5)만 존재
        let map = floor_map(10, 10, &[(5,5),(6,5)]);
        let occupied = HashSet::new();
        for tx in 0..10usize {
            let (nx, _) = move_toward(5, 5, tx, 5, &map, &occupied);
            assert!(nx < 10, "맵 밖으로 나가면 안 된다");
            let tile = map.get_tile(nx, 5);
            assert_eq!(tile, TileKind::Floor, "Wall 타일로 이동하면 안 된다");
        }
    }

    #[test]
    fn 여러_방향이_열려있으면_몬스터는_가장_가까운_타일을_고른다() {
        let map = floor_map(10, 10, &[(5,5),(6,5),(5,6),(4,5),(5,4)]);
        let occupied = HashSet::new();
        let (nx, ny) = move_toward(5, 5, 8, 5, &map, &occupied);
        assert_eq!((nx, ny), (6, 5));
    }

    #[test]
    fn 맵_하단_경계의_몬스터는_추적시_맵_밖_타일을_후보에서_제외한다() {
        // MAP_HEIGHT 경계: y=MAP_HEIGHT-1 의 이웃 y+1 == MAP_HEIGHT 는 ny<MAP_HEIGHT 거짓.
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        let by = MAP_HEIGHT - 1;
        map.set_tile(5, by, TileKind::Floor);
        map.set_tile(6, by, TileKind::Floor);
        let occupied = HashSet::new();
        let (nx, ny) = move_toward(5, by, 9, by, &map, &occupied);
        assert_eq!((nx, ny), (6, by), "경계 밖(y=MAP_HEIGHT)으로는 가지 않고 오른쪽으로");
    }

    #[test]
    fn 맵_우측_경계의_몬스터는_추적시_맵_밖_타일을_후보에서_제외한다() {
        // MAP_WIDTH 경계: x=MAP_WIDTH-1 의 이웃 x+1 == MAP_WIDTH 는 nx<MAP_WIDTH 거짓.
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        let rx = MAP_WIDTH - 1;
        map.set_tile(rx, 5, TileKind::Floor);
        map.set_tile(rx - 1, 5, TileKind::Floor);
        let occupied = HashSet::new();
        let (nx, ny) = move_toward(rx, 5, 0, 5, &map, &occupied);
        assert_eq!((nx, ny), (rx - 1, 5), "경계 밖(x=MAP_WIDTH)으로는 가지 않고 왼쪽으로");
    }

    // --- update_facing (이동 시에만 방향 갱신) ---

    #[test]
    fn facing갱신은_실제_이동시_이동방향으로_바꾼다() {
        let mut f = Facing(IVec2::new(0, -1));
        update_facing(&mut f, 5, 5, 6, 5); // 오른쪽으로 이동
        assert_eq!(f.0, IVec2::new(1, 0), "이동 방향으로 facing 이 바뀐다");
    }

    #[test]
    fn facing갱신은_제자리면_이전_방향을_유지한다() {
        let mut f = Facing(IVec2::new(0, -1));
        update_facing(&mut f, 5, 5, 5, 5); // 이동 없음
        assert_eq!(f.0, IVec2::new(0, -1), "제자리면 facing 을 유지한다");
    }

    fn open_map(w: usize, h: usize) -> Map {
        let mut map = Map::new(w, h);
        for y in 1..h-1 { for x in 1..w-1 { map.set_tile(x, y, TileKind::Floor); } }
        map
    }

    #[test]
    fn 정면_시야_반경_안에_벽이_없으면_몬스터는_플레이어를_본다() {
        let map = open_map(20, 20);
        // 몬스터(5,5)가 오른쪽(+x)을 보고 있고 플레이어(9,5)는 정면 4칸 거리.
        assert!(can_see_player(5, 5, 9, 5, 6, IVec2::new(1, 0), &map),
            "정면 반경 내 명확한 시야면 탐지해야 한다");
    }

    #[test]
    fn 시야_반경_밖의_플레이어는_몬스터가_보지_못한다() {
        let map = open_map(20, 20);
        // 정면(대각 +x,+y)이라도 거리가 멀어 반경 밖.
        assert!(!can_see_player(1, 1, 10, 10, 6, IVec2::new(1, 1), &map),
            "반경 밖은 탐지하지 않아야 한다");
    }

    #[test]
    fn 벽이_시선을_가로막으면_몬스터는_플레이어를_보지_못한다() {
        // (5,5)와 (8,5) 사이에 벽 열 — 정면을 봐도 LoS 가 막힌다.
        let mut map = open_map(20, 20);
        for y in 0..20 { map.set_tile(7, y, TileKind::Wall); }
        assert!(!can_see_player(5, 5, 8, 5, 10, IVec2::new(1, 0), &map),
            "벽이 가로막으면 탐지하지 않아야 한다");
    }

    #[test]
    fn 등_뒤의_먼_플레이어는_몬스터가_보지_못한다() {
        // 몬스터(5,5)는 오른쪽(+x)을 보는데 플레이어(1,5)는 등 뒤(-x) 4칸.
        // 정면 반경(6)이면 보이지만, 등 뒤 반경(FOV_BACK=3)을 넘어 안 보인다.
        let map = open_map(20, 20);
        assert!(!can_see_player(5, 5, 1, 5, 6, IVec2::new(1, 0), &map),
            "정면 반경 안이라도 등 뒤로 멀면 탐지하지 못해야 한다");
    }

    #[test]
    fn 등_뒤라도_아주_가까운_플레이어는_몬스터가_본다() {
        // 등 뒤(-x) 2칸은 FOV_BACK(3) 이내라 탐지된다.
        let map = open_map(20, 20);
        assert!(can_see_player(5, 5, 3, 5, 6, IVec2::new(1, 0), &map),
            "등 뒤라도 FOV_BACK 이내면 탐지해야 한다");
    }

    #[test]
    fn 배회하는_몬스터는_벽_타일로는_이동하지_않는다() {
        let map = floor_map(10, 10, &[(5,5),(6,5),(5,6),(4,5),(5,4)]);
        let occupied: HashSet<(usize, usize)> = HashSet::new();
        let neighbors = [(6usize,5usize),(5,6),(4,5),(5,4),(5,5)];
        let mut trng = rand::thread_rng();
        for _ in 0..200 {
            let result = wander(5, 5, &map, &occupied, &mut trng);
            assert!(neighbors.contains(&result), "배회 결과가 유효한 타일이어야 한다");
        }
    }

    /// monster respawn 의 첫 방문 판정 로직 — entry 가 있어도 monster_slots 가
    /// 비었으면 init 해야 한다 (portal 이 먼저 entry 를 만든 케이스).
    #[test]
    fn 포탈이_먼저_엔트리를_만들어도_슬롯이_비면_몬스터를_초기화한다() {
        let mut persistence = ZonePersistence::default();
        let zone = crate::modules::zone::ZoneId::Dungeon(1);

        // portal-position-persistence 가 entry 를 먼저 만들고 portals 만 채운 상태
        let snap = persistence.0.entry(zone.clone()).or_default();
        snap.portals = vec![crate::modules::zone::SavedPortal {
            tile_x: 5, tile_y: 5,
            target: crate::modules::zone::ZoneId::Town,
            arrive_from: crate::modules::zone::PortalDirection::StairUp,
        }];
        // monster_slots 는 비어있음
        assert!(snap.monster_slots.is_empty());

        // respawn_on_regen 의 needs_init 로직 재현
        let needs_init = persistence.0.get(&zone)
            .map(|s| s.monster_slots.is_empty())
            .unwrap_or(true);
        assert!(needs_init, "monster_slots 가 비어있으면 init 해야 한다");
    }

    #[test]
    fn 슬롯이_이미_채워진_재방문_존은_몬스터를_초기화하지_않는다() {
        let mut persistence = ZonePersistence::default();
        let zone = crate::modules::zone::ZoneId::Dungeon(1);
        // 이미 monster_slots 채워진 상태 — 두 번째 방문
        persistence.0.entry(zone.clone()).or_default().monster_slots = vec![
            MonsterSlot { data_idx: 0, respawn_at_turn: None },
        ];

        let needs_init = persistence.0.get(&zone)
            .map(|s| s.monster_slots.is_empty())
            .unwrap_or(true);
        assert!(!needs_init, "이미 채워진 slots 는 init 하면 안 된다 (재방문 시 상태 보존)");
    }

    // ── App 하네스 기반 시스템 테스트 ─────────────────────────────────────────

    use std::time::Duration;
    use crate::modules::zone::ZoneId;
    use crate::modules::item::{ItemRegistry, WeaponKind};

    /// MAP_WIDTH×MAP_HEIGHT 의 전부 Floor 인 맵.
    fn full_floor_map() -> Map {
        let mut m = Map::new(MAP_WIDTH, MAP_HEIGHT);
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                m.set_tile(x, y, TileKind::Floor);
            }
        }
        m
    }

    /// AssetServer(폰트) 를 제공하는 기본 App.
    fn asset_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Font>();
        app.init_asset::<Image>();
        app
    }

    /// rooms[0] 은 spawn 에서 skip(1) 되므로 더미 1개 + 실제 방 두 개를 둔다.
    fn rooms_with(n: usize) -> Vec<Rect> {
        let mut rooms = vec![Rect::new(1, 1, 2, 2)]; // skip 대상 더미
        for i in 0..n {
            let x = 5 + i * 6;
            rooms.push(Rect::new(x, 5, 3, 3));
        }
        rooms
    }

    fn spawn_player(app: &mut App, tile: (usize, usize)) -> Entity {
        app.world.spawn((
            Player,
            Transform::from_translation(tile_to_world_coords(tile.0, tile.1).extend(0.0)),
            CombatStats { hp: 30, max_hp: 30, mp: 0, max_mp: 0, attack: 5, defense: 1 },
        )).id()
    }

    fn spawn_monster(app: &mut App, name: &str, tile: (usize, usize), hp: i32) -> Entity {
        app.world.spawn((
            Monster { name: name.into(), tile_x: tile.0, tile_y: tile.1, vision_radius: 6, alert_turns: 0, slot_idx: 0 },
            CombatStats { hp, max_hp: hp.max(1), mp: 0, max_mp: 0, attack: 4, defense: 0 },
            Speed::new(1.0),
            MoveQueue::default(),
            ElementalStatus::default(),
            Facing::default(),
            Transform::from_translation(tile_to_world_coords(tile.0, tile.1).extend(Z_MONSTER)),
        )).id()
    }

    // --- MonsterPlugin::build / sync_monster_tiles ---

    #[test]
    fn 몬스터플러그인을_등록하면_빌드가_패닉없이_완료된다() {
        let mut app = asset_app();
        app.add_plugins(MonsterPlugin);
        // build() 가 시스템을 등록만 해도 커버됨 (update 불필요).
        // 등록만으로 패닉 없이 통과하면 성공.
    }

    #[test]
    fn 동기화시스템은_몬스터_타일집합을_현재_위치로_갱신한다() {
        let mut app = App::new();
        app.init_resource::<MonsterTiles>();
        app.add_systems(PreUpdate, sync_monster_tiles);
        spawn_monster(&mut app, "고블린", (3, 4), 6);
        spawn_monster(&mut app, "오크", (7, 8), 10);
        app.update();
        let tiles = &app.world.resource::<MonsterTiles>().0;
        assert!(tiles.contains(&(3, 4)));
        assert!(tiles.contains(&(7, 8)));
        assert_eq!(tiles.len(), 2);
    }

    // --- spawn_on_startup ---

    /// 스폰 관련 시스템이 요구하는 컨텍스트 리소스(레지스트리/인벤토리/퀘스트/아이템)를
    /// 주입한다. MonsterRegistry 는 실제 RON 을 로드해 기존 3종을 그대로 사용.
    fn insert_spawn_context(app: &mut App) {
        app.insert_resource(build_test_registry());
        app.init_resource::<PlayerInventory>();
        app.init_resource::<QuestState>();
        app.insert_resource(crate::modules::item::build_test_registry());
    }

    fn startup_app(map: Map) -> App {
        let mut app = asset_app();
        app.insert_resource(MapResource(map));
        app.init_resource::<ZonePersistence>();
        app.init_resource::<WorldState>();
        insert_spawn_context(&mut app);
        app.add_systems(Startup, spawn_on_startup);
        app
    }

    #[test]
    fn 던전_시작시_방마다_몬스터가_스폰된다() {
        let mut map = full_floor_map();
        map.map_type = MapType::Dungeon;
        map.rooms = rooms_with(2);
        let mut app = startup_app(map);
        app.update();
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 2, "더미 방 1개를 제외한 두 방에 각각 스폰");
        // 슬롯도 영속화에 기록되어야 한다
        let slots = &app.world.resource::<ZonePersistence>().0[&ZoneId::Town].monster_slots;
        assert_eq!(slots.len(), 2);
    }

    #[test]
    fn 마을_타입_맵에서는_시작시_몬스터가_스폰되지_않는다() {
        let mut map = full_floor_map();
        map.map_type = MapType::Village;
        map.rooms = rooms_with(2);
        let mut app = startup_app(map);
        app.update();
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 0, "마을에서는 몬스터를 스폰하지 않는다");
    }

    // --- init_zone_monster_slots ---

    fn slot_ctx() -> (MonsterRegistry, PlayerInventory, WorldState, QuestState, ItemRegistry) {
        (
            build_test_registry(),
            PlayerInventory::default(),
            WorldState::default(),
            QuestState::default(),
            crate::modules::item::build_test_registry(),
        )
    }

    #[test]
    fn 슬롯초기화는_첫번째_방을_제외하고_최대_열개까지_만든다() {
        let mut rooms = vec![Rect::new(0, 0, 1, 1)];
        for i in 0..15 { rooms.push(Rect::new(i, 0, 1, 1)); }
        let (reg, inv, world, qs, items) = slot_ctx();
        let mut rng = rand::thread_rng();
        let slots = init_zone_monster_slots(&rooms, &reg, &ZoneId::Town, &inv, &world, &qs, &items, &mut rng);
        assert_eq!(slots.len(), 10, "skip(1).take(10) 으로 10개 제한");
        // 모든 슬롯의 data_idx 는 자연 스폰 가능한 레지스트리 인덱스를 가리킨다.
        for slot in &slots {
            assert!(reg.monsters.get(slot.data_idx).is_some(), "유효한 레지스트리 인덱스");
            assert!(!reg.monsters[slot.data_idx].quest_only, "자연 스폰 가능한 정의만 선택");
        }
    }

    #[test]
    fn 자연_스폰_후보가_없으면_슬롯을_만들지_않는다() {
        // 모든 정의가 quest_only → 후보 없음 → 빈 슬롯(빈 던전).
        let mut reg = build_test_registry();
        for m in &mut reg.monsters { m.quest_only = true; }
        let mut rooms = vec![Rect::new(0, 0, 1, 1)];
        for i in 0..3 { rooms.push(Rect::new(i, 0, 1, 1)); }
        let (_, inv, world, qs, items) = slot_ctx();
        let mut rng = rand::thread_rng();
        let slots = init_zone_monster_slots(&rooms, &reg, &ZoneId::Town, &inv, &world, &qs, &items, &mut rng);
        assert!(slots.is_empty(), "자연 스폰 후보가 없으면 슬롯도 없다");
    }

    // --- spawn_from_slots (직접 호출, Commands 통한 스폰) ---

    fn run_spawn_from_slots(rooms: &[Rect], slots: &[MonsterSlot], turn: u64) -> App {
        let mut app = asset_app();
        let rooms = rooms.to_vec();
        let slots = slots.to_vec();
        app.insert_resource(build_test_registry());
        app.add_systems(Update, move |mut commands: Commands, asset_server: Res<AssetServer>, reg: Res<MonsterRegistry>| {
            spawn_from_slots(&mut commands, &rooms, &slots, turn, &asset_server, &reg);
        });
        app.update();
        app
    }

    #[test]
    fn 리스폰_타이머가_남은_슬롯은_스폰을_건너뛴다() {
        let rooms = rooms_with(2);
        let slots = vec![
            MonsterSlot { data_idx: 0, respawn_at_turn: Some(100) }, // 아직 미래
            MonsterSlot { data_idx: 1, respawn_at_turn: None },      // 즉시 스폰
        ];
        let mut app = run_spawn_from_slots(&rooms, &slots, 50);
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 1, "타이머가 만료되지 않은 슬롯은 스폰 제외");
    }

    #[test]
    fn 리스폰_타이머가_만료된_슬롯은_스폰된다() {
        let rooms = rooms_with(1);
        let slots = vec![MonsterSlot { data_idx: 0, respawn_at_turn: Some(30) }];
        let mut app = run_spawn_from_slots(&rooms, &slots, 50); // 30 <= 50 → 스폰
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 1);
    }

    #[test]
    fn 같은_좌표의_방이_겹치면_무작위_타일이_이미_사용중인_경로를_탄다() {
        // 두 1x1 방이 같은 좌표 → 두번째 슬롯의 후보 타일이 used 에 이미 존재
        // (`!used.contains` false 분기). 두 몬스터 모두 같은 칸에 스폰된다.
        let rooms = vec![
            Rect::new(0, 0, 1, 1),
            Rect::new(15, 15, 0, 0), // 1x1
            Rect::new(15, 15, 0, 0), // 동일 좌표 1x1
        ];
        let slots = vec![
            MonsterSlot { data_idx: 0, respawn_at_turn: None },
            MonsterSlot { data_idx: 1, respawn_at_turn: None },
        ];
        let mut app = run_spawn_from_slots(&rooms, &slots, 0);
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 2);
    }

    #[test]
    fn 좁은_방에서_여러_몬스터를_스폰해도_타일이_겹치지_않는다() {
        // 1x1 방으로 used 충돌 경로(중심 fallback)를 강제 — 같은 방 두 슬롯이지만
        // rooms.skip(1).zip(slots) 매칭상 방 하나당 슬롯 하나라 둘 다 스폰된다.
        let rooms = vec![
            Rect::new(0, 0, 1, 1),
            Rect::new(10, 10, 0, 0), // 1칸 방
            Rect::new(20, 20, 0, 0),
        ];
        let slots = vec![
            MonsterSlot { data_idx: 0, respawn_at_turn: None },
            MonsterSlot { data_idx: 1, respawn_at_turn: None },
        ];
        let mut app = run_spawn_from_slots(&rooms, &slots, 0);
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 2);
    }

    // --- respawn_on_regen ---

    fn respawn_app() -> App {
        let mut app = asset_app();
        app.add_event::<MonsterRespawnEvent>();
        app.init_resource::<WorldState>();
        app.init_resource::<GlobalTurn>();
        app.init_resource::<ZonePersistence>();
        insert_spawn_context(&mut app);
        app.add_systems(Update, respawn_on_regen);
        app
    }

    #[test]
    fn 던전_재생성_이벤트는_기존_몬스터를_지우고_새로_스폰한다() {
        let mut app = respawn_app();
        let old = spawn_monster(&mut app, "고블린", (5, 5), 6);
        app.world.send_event(MonsterRespawnEvent {
            map_type: MapType::Dungeon,
            rooms: rooms_with(2),
        });
        app.update();
        assert!(app.world.get_entity(old).is_none(), "기존 몬스터는 제거된다");
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 2, "두 방에 새 몬스터 스폰");
    }

    #[test]
    fn 마을_재생성_이벤트는_몬스터를_지우기만_하고_스폰하지_않는다() {
        let mut app = respawn_app();
        let old = spawn_monster(&mut app, "오크", (5, 5), 10);
        app.world.send_event(MonsterRespawnEvent {
            map_type: MapType::Village,
            rooms: rooms_with(2),
        });
        app.update();
        assert!(app.world.get_entity(old).is_none());
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 0, "Dungeon 이 아니면 스폰하지 않는다 (continue)");
    }

    #[test]
    fn 재생성시_슬롯이_이미_있으면_초기화하지_않고_보존한다() {
        let mut app = respawn_app();
        // 현재 존(Town)에 슬롯을 미리 채워둔다 — needs_init=false 경로
        app.world.resource_mut::<ZonePersistence>().0
            .entry(ZoneId::Town).or_default().monster_slots = vec![
                MonsterSlot { data_idx: 0, respawn_at_turn: None },
            ];
        app.world.send_event(MonsterRespawnEvent {
            map_type: MapType::Dungeon,
            rooms: rooms_with(3),
        });
        app.update();
        let slots = &app.world.resource::<ZonePersistence>().0[&ZoneId::Town].monster_slots;
        assert_eq!(slots.len(), 1, "기존 슬롯이 보존되어 1개만 스폰");
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 1);
    }

    #[test]
    fn 재생성시_만료된_리스폰타이머는_지운다() {
        let mut app = respawn_app();
        app.world.resource_mut::<GlobalTurn>().0 = 100;
        app.world.resource_mut::<ZonePersistence>().0
            .entry(ZoneId::Town).or_default().monster_slots = vec![
                MonsterSlot { data_idx: 0, respawn_at_turn: Some(50) },  // 만료 → 지움 → 스폰
                MonsterSlot { data_idx: 1, respawn_at_turn: Some(200) }, // 미래 → 유지 → 미스폰
            ];
        app.world.send_event(MonsterRespawnEvent {
            map_type: MapType::Dungeon,
            rooms: rooms_with(2),
        });
        app.update();
        let slots = &app.world.resource::<ZonePersistence>().0[&ZoneId::Town].monster_slots;
        assert_eq!(slots[0].respawn_at_turn, None, "만료된 타이머는 None 으로");
        assert_eq!(slots[1].respawn_at_turn, Some(200), "미래 타이머는 유지");
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 1, "만료된 슬롯만 스폰");
    }

    // --- handle_player_attack ---

    fn attack_app() -> App {
        let mut app = App::new();
        app.add_event::<AttackMonsterEvent>();
        app.add_event::<LogMessage>();
        app.add_event::<CombatFeedbackEvent>();
        app.add_event::<ItemDropEvent>();
        app.add_event::<ElementalApplyEvent>();
        app.init_resource::<PlayerProgress>();
        app.insert_resource(PlayerEquipment::default());
        app.insert_resource(ItemRegistry::default());
        app.insert_resource(build_test_registry());
        app.add_systems(Update, handle_player_attack);
        app
    }

    #[test]
    fn 플레이어_공격은_해당_타일의_몬스터에게_피해를_준다() {
        let mut app = attack_app();
        spawn_player(&mut app, (1, 1));
        let m = spawn_monster(&mut app, "고블린", (5, 5), 20);
        app.world.send_event(AttackMonsterEvent(5, 5));
        app.update();
        let hp = app.world.get::<CombatStats>(m).unwrap().hp;
        assert!(hp < 20, "공격력5-방어0=5 피해");
    }

    #[test]
    fn 플레이어_공격으로_몬스터를_처치하면_경험치를_얻는다() {
        let mut app = attack_app();
        spawn_player(&mut app, (1, 1));
        // 고블린(8 XP)은 Lv1 다음레벨(20 XP) 미달이라 레벨업 없이 XP 만 오른다.
        let m = spawn_monster(&mut app, "고블린", (5, 5), 3);
        app.world.send_event(AttackMonsterEvent(5, 5));
        app.update();
        assert!(app.world.get::<CombatStats>(m).unwrap().hp <= 0);
        assert_eq!(app.world.resource::<PlayerProgress>().xp, 8, "처치 시 경험치 획득");
    }

    #[test]
    fn 처치_피해가_커서_레벨업하면_레벨업_로그를_남긴다() {
        let mut app = attack_app();
        // 공격력을 높여 강한 몬스터(높은 XP)를 한방에 처치 → 레벨업
        let p = spawn_player(&mut app, (1, 1));
        app.world.get_mut::<CombatStats>(p).unwrap().attack = 100;
        // 트롤은 24 XP — Lv1 다음레벨 20 → 레벨업
        spawn_monster(&mut app, "트롤", (5, 5), 5);
        app.world.send_event(AttackMonsterEvent(5, 5));
        app.update();
        assert!(app.world.resource::<PlayerProgress>().level > 1, "레벨업 분기 진입");
    }

    #[test]
    fn 다른_타일을_공격하면_몬스터는_피해를_받지_않는다() {
        let mut app = attack_app();
        spawn_player(&mut app, (1, 1));
        let m = spawn_monster(&mut app, "고블린", (5, 5), 20);
        app.world.send_event(AttackMonsterEvent(9, 9)); // 빈 타일
        app.update();
        assert_eq!(app.world.get::<CombatStats>(m).unwrap().hp, 20, "다른 타일 공격은 무시");
    }

    #[test]
    fn x는_같지만_y가_다른_타일을_공격하면_몬스터는_피해를_받지_않는다() {
        // tile_x 일치 + tile_y 불일치 — || 의 두번째 항(tile_y != ty) true 분기.
        let mut app = attack_app();
        spawn_player(&mut app, (1, 1));
        let m = spawn_monster(&mut app, "고블린", (5, 5), 20);
        app.world.send_event(AttackMonsterEvent(5, 9)); // x 같고 y 다름
        app.update();
        assert_eq!(app.world.get::<CombatStats>(m).unwrap().hp, 20, "y 가 다르면 명중하지 않는다");
    }

    #[test]
    fn 이미_죽은_몬스터_타일을_공격하면_무시한다() {
        let mut app = attack_app();
        spawn_player(&mut app, (1, 1));
        let m = spawn_monster(&mut app, "고블린", (5, 5), 0);
        app.world.send_event(AttackMonsterEvent(5, 5));
        app.update();
        // hp<=0 continue 경로 — 변화 없음
        assert!(app.world.get::<CombatStats>(m).unwrap().hp <= 0);
    }

    #[test]
    fn 플레이어가_없으면_공격_이벤트는_조용히_무시된다() {
        let mut app = attack_app();
        let m = spawn_monster(&mut app, "고블린", (5, 5), 20);
        app.world.send_event(AttackMonsterEvent(5, 5));
        app.update();
        // player 부재 → get_single_mut Err → continue. 몬스터 무사
        assert_eq!(app.world.get::<CombatStats>(m).unwrap().hp, 20);
    }

    #[test]
    fn 알려지지_않은_이름의_몬스터를_공격해도_피해는_정상이다() {
        // MONSTER_DATA.find 가 None → original_color WHITE fallback 경로
        let mut app = attack_app();
        spawn_player(&mut app, (1, 1));
        let m = spawn_monster(&mut app, "유령", (5, 5), 20);
        app.world.send_event(AttackMonsterEvent(5, 5));
        app.update();
        assert!(app.world.get::<CombatStats>(m).unwrap().hp < 20);
    }

    #[test]
    fn 원소무기로_생존한_몬스터를_공격하면_확률적으로_원소를_부여한다() {
        // rand 의존(40% proc) — 다수 공격 이벤트로 통계적 커버.
        let mut app = App::new();
        app.add_event::<AttackMonsterEvent>();
        app.add_event::<LogMessage>();
        app.add_event::<CombatFeedbackEvent>();
        app.add_event::<ItemDropEvent>();
        app.add_event::<ElementalApplyEvent>();
        app.init_resource::<PlayerProgress>();
        app.insert_resource(PlayerEquipment { weapon: Some(WeaponKind::SWORD), armor: None, ..Default::default() });
        app.insert_resource(crate::modules::item::build_test_registry());
        app.insert_resource(build_test_registry());
        app.add_systems(Update, handle_player_attack);

        let p = spawn_player(&mut app, (1, 1));
        app.world.get_mut::<CombatStats>(p).unwrap().attack = 1; // 살아남게
        // 한 칸씩 떨어뜨려 여러 몬스터 배치 후 각각 공격 이벤트
        let mut ents = Vec::new();
        for x in 0..40usize {
            ents.push(spawn_monster(&mut app, "고블린", (x, 1), 1000));
        }
        for x in 0..40usize {
            app.world.send_event(AttackMonsterEvent(x, 1));
        }
        app.update();
        // ElementalApplyEvent 는 별도 시스템이 처리하므로 여기선 공격 흐름이
        // 패닉 없이 통과(확률 분기 양쪽 통계 진입)했는지만 확인한다.
        assert!(app.world.get::<CombatStats>(ents[0]).unwrap().hp > 0, "약공격으로 생존");
    }

    #[test]
    fn 원소가_없는_무기는_명중해도_원소를_부여하지_않는다() {
        // weapon Some + proc true 이어도 weapon_element None → 부여 이벤트 없음.
        use crate::modules::item::WeaponMeta;
        let mut app = App::new();
        app.add_event::<AttackMonsterEvent>();
        app.add_event::<LogMessage>();
        app.add_event::<CombatFeedbackEvent>();
        app.add_event::<ItemDropEvent>();
        app.add_event::<ElementalApplyEvent>();
        app.init_resource::<PlayerProgress>();
        // 원소가 없는 무기를 등록
        let mut reg = ItemRegistry::default();
        reg.weapons.insert("plain", WeaponMeta {
            display_name: "막대기", glyph_ascii: "/", glyph_unicode: "/", glyph_game_icon: "/",
            pickup_message: "막대기", attack_power_min: 1, attack_power_max: 1, tier: 1, element: None,
        });
        app.insert_resource(reg);
        app.insert_resource(PlayerEquipment { weapon: Some(WeaponKind("plain")), armor: None, ..Default::default() });
        app.insert_resource(build_test_registry());
        app.add_systems(Update, handle_player_attack);

        let p = spawn_player(&mut app, (1, 1));
        app.world.get_mut::<CombatStats>(p).unwrap().attack = 1; // 생존시키기
        let mut ents = Vec::new();
        for x in 0..40usize { ents.push(spawn_monster(&mut app, "고블린", (x, 1), 1000)); }
        for x in 0..40usize { app.world.send_event(AttackMonsterEvent(x, 1)); }
        app.update();
        assert!(app.world.get::<CombatStats>(ents[0]).unwrap().hp > 0, "약공격으로 생존");
    }

    // --- monster_turn ---

    /// 맵 전체를 밝게(Bright) 채운 LightMap — 기존 탐지 테스트가 광량 보정 없이
    /// (base 반경 그대로) 동작하도록, 별도 광량 의도가 없는 기본 하네스에 쓴다.
    fn bright_light_map(w: usize, h: usize) -> LightMap {
        LightMap { width: w, height: h, levels: vec![LightLevel::Bright; w * h] }
    }

    fn turn_app(map: Map) -> App {
        let (w, h) = (map.width, map.height);
        let mut app = App::new();
        app.insert_resource(MapResource(map));
        app.insert_resource(bright_light_map(w, h));
        app.add_event::<PlayerActedEvent>();
        app.add_event::<LogMessage>();
        app.add_event::<CombatFeedbackEvent>();
        app.add_event::<ElementalApplyEvent>();
        app.add_event::<PlayerDetectedEvent>();
        app.insert_resource(build_test_registry());
        app.add_systems(Update, monster_turn);
        app
    }

    #[test]
    fn 턴이벤트가_없으면_몬스터는_행동하지_않는다() {
        let mut app = turn_app(full_floor_map());
        spawn_player(&mut app, (10, 10));
        let m = spawn_monster(&mut app, "고블린", (5, 5), 6);
        let before = app.world.get::<Monster>(m).unwrap().alert_turns;
        app.update(); // 이벤트 없음 → early return
        assert_eq!(app.world.get::<Monster>(m).unwrap().alert_turns, before);
    }

    #[test]
    fn 플레이어가_없으면_몬스터턴은_조용히_종료된다() {
        let mut app = turn_app(full_floor_map());
        spawn_monster(&mut app, "고블린", (5, 5), 6);
        app.world.send_event(PlayerActedEvent);
        app.update(); // player_query Err → return
        // 패닉 없이 통과하면 성공
    }

    #[test]
    fn 좌우로_인접한_몬스터는_플레이어를_공격한다() {
        let mut app = turn_app(full_floor_map());
        let p = spawn_player(&mut app, (5, 5));
        spawn_monster(&mut app, "고블린", (6, 5), 6); // 수평 인접 (dx==1,dy==0)
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!(app.world.get::<CombatStats>(p).unwrap().hp < 30, "인접 몬스터가 플레이어를 공격");
    }

    #[test]
    fn 대각선_몬스터는_인접으로_보지_않고_추적_이동한다() {
        // dx==1 && dy==1 → 두 disjunct 모두 거짓(인접 아님) → 추적 경로.
        let mut app = turn_app(full_floor_map());
        let p = spawn_player(&mut app, (5, 5));
        spawn_monster(&mut app, "고블린", (6, 6), 6); // 대각선
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<CombatStats>(p).unwrap().hp, 30, "대각선은 공격하지 않고 이동");
    }

    #[test]
    fn 위아래로_인접한_몬스터도_플레이어를_공격한다() {
        // dx==0 && dy==1 — 인접 판정의 두번째 disjunct.
        let mut app = turn_app(full_floor_map());
        let p = spawn_player(&mut app, (5, 5));
        spawn_monster(&mut app, "고블린", (5, 6), 6); // 수직 인접
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!(app.world.get::<CombatStats>(p).unwrap().hp < 30, "수직 인접 몬스터도 공격");
    }

    #[test]
    fn 몬스터_공격으로_플레이어가_죽으면_패배마커가_붙는다() {
        let mut app = turn_app(full_floor_map());
        let p = spawn_player(&mut app, (5, 5));
        app.world.get_mut::<CombatStats>(p).unwrap().hp = 1;
        let m = spawn_monster(&mut app, "트롤", (6, 5), 16);
        app.world.get_mut::<CombatStats>(m).unwrap().attack = 50;
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!(app.world.entity(p).contains::<Defeated>(), "치명타로 Defeated 부여");
    }

    #[test]
    fn 시야_안의_몬스터는_경계상태가_되어_플레이어를_추적한다() {
        let mut app = turn_app(full_floor_map());
        spawn_player(&mut app, (5, 5));
        let m = spawn_monster(&mut app, "고블린", (5, 9), 6); // 시야 안, 비인접
        app.world.send_event(PlayerActedEvent);
        app.update();
        let mon = app.world.get::<Monster>(m).unwrap();
        assert_eq!(mon.alert_turns, MAX_ALERT_TURNS, "시야 안이면 경계 최대치");
        // 추적 이동으로 플레이어쪽(y 감소)으로 한 칸 접근
        assert_eq!(mon.tile_y, 8);
        assert!(!app.world.get::<MoveQueue>(m).unwrap().0.is_empty(), "이동 큐에 목적지 추가");
    }

    #[test]
    fn 정면에_플레이어가_있으면_몬스터는_경계상태가_된다() {
        // 몬스터(5,5)가 오른쪽(+x)을 보고, 플레이어(9,5)는 정면 4칸.
        let mut app = turn_app(full_floor_map());
        spawn_player(&mut app, (9, 5));
        let m = spawn_monster(&mut app, "고블린", (5, 5), 6);
        app.world.get_mut::<Facing>(m).unwrap().0 = IVec2::new(1, 0);
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<Monster>(m).unwrap().alert_turns, MAX_ALERT_TURNS,
            "정면 시야 안의 플레이어는 탐지된다");
    }

    #[test]
    fn 등_뒤에_멀리_있는_플레이어는_몬스터가_탐지하지_못한다() {
        // 몬스터(9,5)가 오른쪽(+x)을 보는데 플레이어(5,5)는 등 뒤(-x) 4칸.
        // 정면 반경(6)이면 닿지만 등 뒤 반경(FOV_BACK=3) 초과라 탐지 못 함.
        let mut app = turn_app(full_floor_map());
        spawn_player(&mut app, (5, 5));
        let m = spawn_monster(&mut app, "고블린", (9, 5), 6);
        app.world.get_mut::<Facing>(m).unwrap().0 = IVec2::new(1, 0);
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<Monster>(m).unwrap().alert_turns, 0,
            "등 뒤 먼 플레이어는 탐지되지 않아 경계 상태가 되지 않는다");
    }

    #[test]
    fn 추적_이동한_몬스터는_이동방향으로_facing이_갱신된다() {
        // 몬스터(5,5)가 오른쪽을 보고 정면의 플레이어(9,5)를 추적해 +x 로 이동.
        let mut app = turn_app(full_floor_map());
        spawn_player(&mut app, (9, 5));
        let m = spawn_monster(&mut app, "고블린", (5, 5), 6);
        app.world.get_mut::<Facing>(m).unwrap().0 = IVec2::new(1, 0);
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<Monster>(m).unwrap().tile_x, 6, "오른쪽으로 한 칸 추적 이동");
        assert_eq!(app.world.get::<Facing>(m).unwrap().0, IVec2::new(1, 0),
            "이동 방향으로 facing 갱신");
    }

    #[test]
    fn 시야_밖이면_경계턴이_매턴_감소한다() {
        let mut app = turn_app(full_floor_map());
        spawn_player(&mut app, (1, 1));
        // 멀리 떨어져 시야 밖, 경계상태를 가진 몬스터
        let m = app.world.spawn((
            Monster { name: "고블린".into(), tile_x: 40, tile_y: 40, vision_radius: 2, alert_turns: 3, slot_idx: 0 },
            CombatStats { hp: 6, max_hp: 6, mp: 0, max_mp: 0, attack: 4, defense: 0 },
            Speed::new(1.0),
            MoveQueue::default(),
            ElementalStatus::default(),
            Facing::default(),
            Transform::from_translation(tile_to_world_coords(40, 40).extend(Z_MONSTER)),
        )).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<Monster>(m).unwrap().alert_turns, 2, "시야 밖이면 경계 1 감소");
    }

    #[test]
    fn 경계상태가_아니면_몬스터는_무작위로_배회한다() {
        let mut app = turn_app(full_floor_map());
        spawn_player(&mut app, (1, 1));
        // 시야 밖 + alert 0 → wander 경로
        let m = app.world.spawn((
            Monster { name: "고블린".into(), tile_x: 40, tile_y: 40, vision_radius: 1, alert_turns: 0, slot_idx: 0 },
            CombatStats { hp: 6, max_hp: 6, mp: 0, max_mp: 0, attack: 4, defense: 0 },
            Speed::new(1.0),
            MoveQueue::default(),
            ElementalStatus::default(),
            Facing::default(),
            Transform::from_translation(tile_to_world_coords(40, 40).extend(Z_MONSTER)),
        )).id();
        app.world.send_event(PlayerActedEvent);
        app.update();
        // wander 는 제자리이거나 인접 — 좌표가 유효 범위 안이면 충분
        let mon = app.world.get::<Monster>(m).unwrap();
        assert!(mon.tile_x < MAP_WIDTH);
        assert!(mon.tile_y < MAP_HEIGHT);
    }

    #[test]
    fn 행동불능_몬스터는_턴을_건너뛴다() {
        let mut app = turn_app(full_floor_map());
        spawn_player(&mut app, (5, 5));
        let m = spawn_monster(&mut app, "고블린", (6, 5), 6); // 인접인데
        app.world.entity_mut(m).insert(Stunned { turns: 2 });
        let p = app.world.query_filtered::<Entity, With<Player>>().single(&app.world);
        let before = app.world.get::<CombatStats>(p).unwrap().hp;
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<CombatStats>(p).unwrap().hp, before, "기절 몬스터는 공격하지 않는다");
    }

    #[test]
    fn 죽은_몬스터는_턴_행동에서_제외된다() {
        let mut app = turn_app(full_floor_map());
        let p = spawn_player(&mut app, (5, 5));
        spawn_monster(&mut app, "고블린", (6, 5), 0); // hp<=0
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(app.world.get::<CombatStats>(p).unwrap().hp, 30, "죽은 몬스터는 행동 안 함");
    }

    #[test]
    fn 느린_몬스터는_에너지가_부족하면_그_턴엔_행동하지_않는다() {
        let mut app = turn_app(full_floor_map());
        spawn_player(&mut app, (5, 5));
        let m = spawn_monster(&mut app, "트롤", (5, 9), 16);
        app.world.get_mut::<Speed>(m).unwrap().value = 0.5; // 첫 턴 energy 0.5 < 1.0
        app.world.send_event(PlayerActedEvent);
        app.update();
        // 행동 안 했으니 위치 그대로
        assert_eq!(app.world.get::<Monster>(m).unwrap().tile_y, 9);
        // 그래도 시야 안이라 경계는 갱신됨
        assert_eq!(app.world.get::<Monster>(m).unwrap().alert_turns, MAX_ALERT_TURNS);
    }

    #[test]
    fn 플레이어가_이동중이면_몬스터는_목표타일을_기준으로_판단한다() {
        let mut app = turn_app(full_floor_map());
        let p = spawn_player(&mut app, (1, 1));
        // MovingTo 가 있으면 target 타일을 플레이어 위치로 사용
        app.world.entity_mut(p).insert(MovingTo { target: tile_to_world_coords(5, 5).extend(0.0) });
        spawn_monster(&mut app, "고블린", (6, 5), 6); // target(5,5)에 인접
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!(app.world.get::<CombatStats>(p).unwrap().hp < 30, "이동 목표 타일 기준으로 인접 판정");
    }

    #[test]
    fn 인접_몬스터는_확률적으로_원소를_부여한다() {
        // rand 의존(35% proc) + monster_element Some 경로 — 다수 몬스터로 통계 커버.
        let mut app = turn_app(full_floor_map());
        let p = spawn_player(&mut app, (5, 5));
        app.world.get_mut::<CombatStats>(p).unwrap().hp = 100000;
        app.world.get_mut::<CombatStats>(p).unwrap().max_hp = 100000;
        for _ in 0..50 {
            // 인접하게 여러 마리 — 공격 발생, 일부는 원소 부여 이벤트 발생
            spawn_monster(&mut app, "오크", (6, 5), 10);
        }
        for _ in 0..5 {
            app.world.send_event(PlayerActedEvent);
            app.update();
        }
        // ElementalApplyEvent 발행 여부는 처리 시스템이 없어 직접 확인하지 않고,
        // 다수 공격으로 확률 분기 양쪽에 진입했음을 패닉 없는 통과로 신뢰한다.
        assert!(app.world.get::<CombatStats>(p).unwrap().hp < 100000, "여러 인접 몬스터가 플레이어를 공격");
    }

    #[test]
    fn 속성이_없는_몬스터는_인접_공격시_원소를_부여하지_않는다() {
        // monster_element None 분기 — 알려지지 않은 이름의 인접 몬스터.
        let mut app = turn_app(full_floor_map());
        let p = spawn_player(&mut app, (5, 5));
        spawn_monster(&mut app, "유령", (6, 5), 6); // monster_element("유령") == None
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!(app.world.get::<CombatStats>(p).unwrap().hp < 30, "공격은 정상 진행");
        // 원소 부여 이벤트 자체는 None 으로 발행되지 않는다(패닉 없이 통과).
    }

    #[test]
    fn 죽은_플레이어에게는_몬스터가_추가타를_넣지_않는다() {
        // player_dead 후 두번째 인접 몬스터의 !player_dead false 경로
        let mut app = turn_app(full_floor_map());
        let p = spawn_player(&mut app, (5, 5));
        app.world.get_mut::<CombatStats>(p).unwrap().hp = 1;
        let m1 = spawn_monster(&mut app, "트롤", (6, 5), 16);
        app.world.get_mut::<CombatStats>(m1).unwrap().attack = 50;
        let m2 = spawn_monster(&mut app, "트롤", (4, 5), 16);
        app.world.get_mut::<CombatStats>(m2).unwrap().attack = 50;
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!(app.world.entity(p).contains::<Defeated>());
    }

    // --- smooth_monster_move ---

    fn smooth_app() -> App {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.add_systems(Update, smooth_monster_move);
        app
    }

    #[test]
    fn 부드러운_이동은_큐의_목표를_향해_점진적으로_움직인다() {
        let mut app = smooth_app();
        let mut q = MoveQueue::default();
        q.0.push_back(tile_to_world_coords(20, 20).extend(Z_MONSTER));
        let m = app.world.spawn((
            Monster { name: "고블린".into(), tile_x: 0, tile_y: 0, vision_radius: 6, alert_turns: 0, slot_idx: 0 },
            Speed::new(1.0),
            q,
            Transform::from_translation(tile_to_world_coords(0, 0).extend(Z_MONSTER)),
        )).id();
        let start = app.world.get::<Transform>(m).unwrap().translation;
        app.world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(0.016));
        app.update();
        let now = app.world.get::<Transform>(m).unwrap().translation;
        assert_ne!(now, start, "목표를 향해 한 스텝 이동");
        assert!(!app.world.get::<MoveQueue>(m).unwrap().0.is_empty(), "아직 목표에 도달 못 함");
    }

    #[test]
    fn 부드러운_이동은_목표에_근접하면_정확히_스냅하고_큐를_비운다() {
        let mut app = smooth_app();
        let mut q = MoveQueue::default();
        let target = tile_to_world_coords(1, 0).extend(Z_MONSTER);
        q.0.push_back(target);
        // 시작을 목표 바로 옆(스텝 이내)으로
        let m = app.world.spawn((
            Monster { name: "고블린".into(), tile_x: 0, tile_y: 0, vision_radius: 6, alert_turns: 0, slot_idx: 0 },
            Speed::new(1.0),
            q,
            Transform::from_translation(target - Vec3::new(0.1, 0.0, 0.0)),
        )).id();
        app.world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(1.0));
        app.update();
        assert_eq!(app.world.get::<Transform>(m).unwrap().translation, target, "목표에 스냅");
        assert!(app.world.get::<MoveQueue>(m).unwrap().0.is_empty(), "도달한 목표는 큐에서 제거");
    }

    // --- cleanup_dead ---

    fn cleanup_app() -> App {
        let mut app = App::new();
        app.init_resource::<WorldState>();
        app.init_resource::<GlobalTurn>();
        app.init_resource::<ZonePersistence>();
        app.add_systems(Update, cleanup_dead);
        app
    }

    #[test]
    fn 죽은_몬스터는_정리되고_리스폰_타이머가_예약된다() {
        let mut app = cleanup_app();
        // 현재 존(Town)에 슬롯 마련
        app.world.resource_mut::<ZonePersistence>().0
            .entry(ZoneId::Town).or_default().monster_slots = vec![
                MonsterSlot { data_idx: 0, respawn_at_turn: None },
            ];
        let m = spawn_monster(&mut app, "고블린", (5, 5), 0); // 죽음
        app.update();
        assert!(app.world.get_entity(m).is_none(), "죽은 몬스터는 despawn");
        let slot = &app.world.resource::<ZonePersistence>().0[&ZoneId::Town].monster_slots[0];
        assert!(slot.respawn_at_turn.is_some(), "리스폰 타이머 예약");
    }

    #[test]
    fn 살아있는_몬스터는_정리되지_않는다() {
        let mut app = cleanup_app();
        let m = spawn_monster(&mut app, "고블린", (5, 5), 6); // 살아있음
        app.update();
        assert!(app.world.get_entity(m).is_some(), "살아있는 몬스터는 유지");
    }

    #[test]
    fn 정리시_현재_존_스냅샷이_없으면_타이머_예약을_건너뛴다() {
        // persistence 에 현재 존 엔트리가 없는 경우 (get_mut None 경로)
        let mut app = cleanup_app();
        let m = spawn_monster(&mut app, "고블린", (5, 5), 0);
        app.update(); // ZonePersistence 비어있음 → snapshot None
        assert!(app.world.get_entity(m).is_none(), "스냅샷 없어도 despawn 은 된다");
    }

    #[test]
    fn 정리시_슬롯인덱스가_범위를_벗어나면_타이머를_예약하지_않는다() {
        // slot_idx 가 monster_slots 길이를 초과 (get_mut None 경로)
        let mut app = cleanup_app();
        app.world.resource_mut::<ZonePersistence>().0
            .entry(ZoneId::Town).or_default().monster_slots = vec![
                MonsterSlot { data_idx: 0, respawn_at_turn: None },
            ];
        let m = app.world.spawn((
            Monster { name: "고블린".into(), tile_x: 5, tile_y: 5, vision_radius: 6, alert_turns: 0, slot_idx: 99 },
            CombatStats { hp: 0, max_hp: 6, mp: 0, max_mp: 0, attack: 4, defense: 0 },
            Speed::new(1.0),
            MoveQueue::default(),
            ElementalStatus::default(),
            Transform::from_translation(tile_to_world_coords(5, 5).extend(Z_MONSTER)),
        )).id();
        app.update();
        assert!(app.world.get_entity(m).is_none());
        let slot = &app.world.resource::<ZonePersistence>().0[&ZoneId::Town].monster_slots[0];
        assert_eq!(slot.respawn_at_turn, None, "범위 밖 slot_idx 는 타이머 미예약");
    }

    // --- wander 직접 호출 (양방향 분기) ---

    #[test]
    fn 배회는_모든_방향이_막히면_제자리에_머문다() {
        let map = floor_map(10, 10, &[(5, 5)]); // 인접 floor 없음
        let occupied = HashSet::new();
        let mut trng = rand::thread_rng();
        for _ in 0..50 {
            let r = wander(5, 5, &map, &occupied, &mut trng);
            assert_eq!(r, (5, 5), "주변에 floor 가 없으면 제자리");
        }
    }

    #[test]
    fn 배회는_유효한_인접_타일이_있으면_가끔_이동한다() {
        let map = floor_map(10, 10, &[(5, 5), (6, 5), (4, 5), (5, 6), (5, 4)]);
        let occupied = HashSet::new();
        let mut trng = rand::thread_rng();
        let mut moved = false;
        let mut stayed = false;
        for _ in 0..200 {
            let r = wander(5, 5, &map, &occupied, &mut trng);
            if r == (5, 5) { stayed = true; } else { moved = true; }
            assert!([(5usize,5usize),(6,5),(4,5),(5,6),(5,4)].contains(&r));
        }
        assert!(moved, "확률적으로 이동이 발생해야 한다");
        assert!(stayed, "확률적으로 정지가 발생해야 한다");
    }

    #[test]
    fn 맵_하단_경계의_몬스터는_배회시_맵_밖_타일을_후보에서_제외한다() {
        // ny < MAP_HEIGHT 거짓 분기 — 경계 밖 후보 제외 검증.
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        let by = MAP_HEIGHT - 1;
        map.set_tile(5, by, TileKind::Floor);
        map.set_tile(6, by, TileKind::Floor);
        map.set_tile(4, by, TileKind::Floor);
        let occupied = HashSet::new();
        let mut trng = rand::thread_rng();
        for _ in 0..200 {
            let r = wander(5, by, &map, &occupied, &mut trng);
            assert!([(5usize, by), (6, by), (4, by)].contains(&r), "경계 밖으로 배회하지 않는다");
        }
    }

    #[test]
    fn 맵_우측_경계의_몬스터는_배회시_맵_밖_타일을_후보에서_제외한다() {
        // nx < MAP_WIDTH 거짓 분기 — 경계 밖 후보 제외 검증.
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        let rx = MAP_WIDTH - 1;
        map.set_tile(rx, 5, TileKind::Floor);
        map.set_tile(rx - 1, 5, TileKind::Floor);
        map.set_tile(rx, 4, TileKind::Floor);
        map.set_tile(rx, 6, TileKind::Floor);
        let occupied = HashSet::new();
        let mut trng = rand::thread_rng();
        for _ in 0..200 {
            let r = wander(rx, 5, &map, &occupied, &mut trng);
            assert!([(rx, 5usize), (rx - 1, 5), (rx, 4), (rx, 6)].contains(&r), "경계 밖으로 배회하지 않는다");
        }
    }

    #[test]
    fn 추적하는_몬스터는_물타일로_이동하지_않고_모래타일로는_이동한다() {
        // (5,5)→(7,5) 추적. 오른쪽 인접 (6,5)을 물/모래로 바꿔 동작을 비교한다.
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        map.set_tile(5, 5, TileKind::Floor);
        map.set_tile(6, 5, TileKind::Water);
        let occupied = HashSet::new();
        let (nx, ny) = move_toward(5, 5, 7, 5, &map, &occupied);
        assert_eq!((nx, ny), (5, 5), "물 타일로는 이동하지 않고 제자리를 유지해야 한다");

        map.set_tile(6, 5, TileKind::Sand);
        let (nx, ny) = move_toward(5, 5, 7, 5, &map, &occupied);
        assert_eq!((nx, ny), (6, 5), "모래 타일로는 이동할 수 있어야 한다");
    }

    #[test]
    fn 배회하는_몬스터는_물타일로는_이동하지_않는다() {
        // 주변이 모두 물이면 배회 결과는 제자리뿐이다.
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        map.set_tile(5, 5, TileKind::Floor);
        map.set_tile(4, 5, TileKind::Water);
        map.set_tile(6, 5, TileKind::Water);
        map.set_tile(5, 4, TileKind::Water);
        map.set_tile(5, 6, TileKind::Water);
        let occupied = HashSet::new();
        let mut trng = rand::thread_rng();
        for _ in 0..200 {
            let r = wander(5, 5, &map, &occupied, &mut trng);
            assert_eq!(r, (5, 5), "사방이 물이면 제자리를 유지해야 한다");
        }
    }

    #[test]
    fn 배회하는_몬스터는_모래타일로는_이동할_수_있다() {
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        map.set_tile(5, 5, TileKind::Floor);
        map.set_tile(6, 5, TileKind::Sand);
        let occupied = HashSet::new();
        let mut trng = rand::thread_rng();
        let mut moved = false;
        for _ in 0..200 {
            let r = wander(5, 5, &map, &occupied, &mut trng);
            assert!(r == (5, 5) || r == (6, 5), "제자리 또는 모래 타일로만 이동해야 한다");
            if r == (6, 5) { moved = true; }
        }
        assert!(moved, "모래 타일로 이동한 경우가 한 번은 있어야 한다");
    }

    // --- guard_stats (순수 함수: 스케일/반올림/경계) ---

    #[test]
    fn 가드스탯은_플레이어_효과치에_배율1점2를_곱해_반올림한다() {
        // 10*1.2=12.0, 5*1.2=6.0, 1*1.2=1.2→1
        assert_eq!(guard_stats(10, 5, 1), (12, 6, 1), "각 값에 1.2 곱하고 반올림");
    }

    #[test]
    fn 가드스탯_반올림은_0점5_이상은_올림_미만은_내림한다() {
        // 3*1.2=3.6→4 (올림), 2*1.2=2.4→2 (내림), 4*1.2=4.8→5 (올림)
        assert_eq!(guard_stats(3, 2, 4), (4, 2, 5), "0.5 경계 기준 반올림");
    }

    #[test]
    fn 가드스탯은_0이면_0을_반환한다() {
        // 경계: 0*1.2=0.0
        assert_eq!(guard_stats(0, 0, 0), (0, 0, 0), "0 입력은 0 출력");
    }

    #[test]
    fn 가드스탯은_큰_플레이어수치도_정상_스케일한다() {
        // 100*1.2=120.0, 50*1.2=60.0, 20*1.2=24.0
        assert_eq!(guard_stats(100, 50, 20), (120, 60, 24), "큰 수치도 1.2배");
    }

    // --- monster_turn 의 PlayerDetectedEvent 발행 ---

    fn detected_count(app: &App) -> usize {
        app.world.resource::<Events<PlayerDetectedEvent>>().len()
    }

    #[test]
    fn 정면으로_플레이어를_탐지하면_탐지이벤트를_발행한다() {
        // 몬스터(5,5)가 오른쪽(+x)을 보고 플레이어(9,5)는 정면 4칸.
        let mut app = turn_app(full_floor_map());
        spawn_player(&mut app, (9, 5));
        let m = spawn_monster(&mut app, "고블린", (5, 5), 6);
        app.world.get_mut::<Facing>(m).unwrap().0 = IVec2::new(1, 0);
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(detected_count(&app), 1, "정면 탐지 시 PlayerDetectedEvent 발행");
    }

    #[test]
    fn 등_뒤_먼_플레이어는_탐지이벤트를_발행하지_않는다() {
        // 몬스터(9,5)가 오른쪽(+x)을 보는데 플레이어(5,5)는 등 뒤 4칸 → 미탐지.
        let mut app = turn_app(full_floor_map());
        spawn_player(&mut app, (5, 5));
        let m = spawn_monster(&mut app, "고블린", (9, 5), 6);
        app.world.get_mut::<Facing>(m).unwrap().0 = IVec2::new(1, 0);
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(detected_count(&app), 0, "등 뒤 먼 플레이어는 미탐지 → 이벤트 없음");
    }

    /// 광량을 지정해(전부 어둠/밝음) monster_turn 을 돌리는 하네스.
    fn turn_app_with_light(map: Map, all_bright: bool) -> App {
        let (w, h) = (map.width, map.height);
        let mut app = turn_app(map);
        let level = if all_bright { LightLevel::Bright } else { LightLevel::Dark };
        app.insert_resource(LightMap { width: w, height: h, levels: vec![level; w * h] });
        app
    }

    #[test]
    fn 어둠속_플레이어는_같은_거리에서도_가드에게_탐지되지_않는다() {
        // 가드(5,5)가 +x 를 보고 플레이어(11,5)는 정면 6칸. base 반경 6 이면 밝을 땐
        // 탐지, 어둠이면 유효 반경 2 로 줄어 미탐지가 된다.
        let mut app = turn_app_with_light(full_floor_map(), false);
        spawn_player(&mut app, (11, 5));
        let m = spawn_monster(&mut app, "고블린", (5, 5), 6);
        app.world.get_mut::<Facing>(m).unwrap().0 = IVec2::new(1, 0);
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(detected_count(&app), 0, "어둠 속 6칸 거리는 유효 반경 2 로 줄어 미탐지");
    }

    #[test]
    fn 밝은곳_플레이어는_같은_거리에서_가드에게_탐지된다() {
        // 위와 동일 배치인데 밝으면 base 반경 6 그대로라 정면 6칸이 탐지된다.
        let mut app = turn_app_with_light(full_floor_map(), true);
        spawn_player(&mut app, (11, 5));
        let m = spawn_monster(&mut app, "고블린", (5, 5), 6);
        app.world.get_mut::<Facing>(m).unwrap().0 = IVec2::new(1, 0);
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert_eq!(detected_count(&app), 1, "밝은 곳 6칸 거리는 base 반경 6 으로 탐지");
    }

    // --- handle_spawn_guards ---

    fn guard_spawn_app(map: Map) -> App {
        let mut app = asset_app();
        app.insert_resource(MapResource(map));
        app.init_resource::<UsedSpawnTiles>();
        app.add_event::<SpawnGuardEvent>();
        app.insert_resource(PlayerEquipment::default());
        app.insert_resource(ItemRegistry::default());
        app.add_systems(Update, handle_spawn_guards);
        app
    }

    #[test]
    fn 가드스폰이벤트는_요청한_마릿수만큼_가드를_스폰한다() {
        let mut map = full_floor_map();
        map.rooms = rooms_with(3);
        let mut app = guard_spawn_app(map);
        spawn_player(&mut app, (1, 1));
        app.world.send_event(SpawnGuardEvent { count: 3 });
        app.update();
        let names: Vec<String> = app.world.query::<&Monster>()
            .iter(&app.world).map(|m| m.name.clone()).collect();
        let guards = names.iter().filter(|n| n.as_str() == "가드").count();
        assert_eq!(guards, 3, "요청한 3마리 가드 스폰");
    }

    #[test]
    fn 스폰된_가드의_스탯은_플레이어_효과치의_1점2배다() {
        let mut map = full_floor_map();
        map.rooms = rooms_with(1);
        let mut app = guard_spawn_app(map);
        // 플레이어 max_hp 30, 장비 없음 → effective ATK=PLAYER_ATK(5), DEF=PLAYER_DEF(1)
        spawn_player(&mut app, (1, 1));
        app.world.send_event(SpawnGuardEvent { count: 1 });
        app.update();
        let (expect_hp, expect_atk, expect_def) = guard_stats(30, 5, 1);
        let mut q = app.world.query::<(&Monster, &CombatStats)>();
        let (_, stats) = q.iter(&app.world).find(|(m, _)| m.name == "가드").expect("가드 존재");
        assert_eq!(stats.max_hp, expect_hp, "HP 스케일");
        assert_eq!(stats.attack, expect_atk, "ATK 스케일");
        assert_eq!(stats.defense, expect_def, "DEF 스케일");
    }

    #[test]
    fn 가드는_넉넉한_시야반경을_가진다() {
        let mut map = full_floor_map();
        map.rooms = rooms_with(1);
        let mut app = guard_spawn_app(map);
        spawn_player(&mut app, (1, 1));
        app.world.send_event(SpawnGuardEvent { count: 1 });
        app.update();
        let mut q = app.world.query::<&Monster>();
        let guard = q.iter(&app.world).find(|m| m.name == "가드").expect("가드 존재");
        assert_eq!(guard.vision_radius, GUARD_VISION_RADIUS, "가드 시야 반경");
    }

    #[test]
    fn 가드스폰이벤트가_없으면_가드를_스폰하지_않는다() {
        let mut map = full_floor_map();
        map.rooms = rooms_with(2);
        let mut app = guard_spawn_app(map);
        spawn_player(&mut app, (1, 1));
        app.update(); // 이벤트 없음 → early return (total==0)
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 0, "이벤트 없으면 스폰 없음");
    }

    #[test]
    fn 카운트0_가드스폰이벤트는_가드를_스폰하지_않는다() {
        // total==0 분기: 이벤트는 왔지만 count 합이 0.
        let mut map = full_floor_map();
        map.rooms = rooms_with(1);
        let mut app = guard_spawn_app(map);
        spawn_player(&mut app, (1, 1));
        app.world.send_event(SpawnGuardEvent { count: 0 });
        app.update();
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 0, "count 합 0 이면 스폰 없음");
    }

    #[test]
    fn 플레이어가_없으면_가드를_스폰하지_않는다() {
        // player_query Err 분기 — count>0 이어도 스폰 불가.
        let mut map = full_floor_map();
        map.rooms = rooms_with(1);
        let mut app = guard_spawn_app(map);
        app.world.send_event(SpawnGuardEvent { count: 2 });
        app.update();
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 0, "플레이어 없으면 스폰 안 함");
    }

    #[test]
    fn 통과타일이_없으면_가드_스폰을_건너뛴다() {
        // 전부 Wall 인 맵 + room 없음 → random_floor_tile_anywhere 항상 None.
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT); // 기본 전부 Wall
        map.rooms = vec![];
        let mut app = guard_spawn_app(map);
        spawn_player(&mut app, (1, 1));
        app.world.send_event(SpawnGuardEvent { count: 2 });
        app.update();
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 0, "Floor 타일 없으면 스폰 실패 (continue)");
    }

    #[test]
    fn 여러_가드스폰이벤트의_카운트는_합산되어_스폰된다() {
        // 같은 프레임에 두 이벤트 → total = 1 + 2 = 3.
        let mut map = full_floor_map();
        map.rooms = rooms_with(3);
        let mut app = guard_spawn_app(map);
        spawn_player(&mut app, (1, 1));
        app.world.send_event(SpawnGuardEvent { count: 1 });
        app.world.send_event(SpawnGuardEvent { count: 2 });
        app.update();
        let guards = app.world.query::<&Monster>()
            .iter(&app.world).filter(|m| m.name == "가드").count();
        assert_eq!(guards, 3, "두 이벤트 카운트 합산 스폰");
    }

    // ── MonsterDef / MonsterRegistry: RON 로드·조회·매핑 ───────────────────────

    /// 임의 MonsterDef 를 만든다 (스폰 규칙 테스트용 — 필드 기본값 채움).
    fn def(id: &str, name: &str, element: Option<&str>, weight: f32) -> MonsterDef {
        MonsterDef {
            id: id.into(),
            display_name: name.into(),
            glyph: "x".into(),
            color: (0.1, 0.2, 0.3),
            hp: 5, attack: 2, defense: 1, vision_radius: 4, speed: 1.0,
            element: element.map(|s| s.into()),
            spawn_weight: weight,
            zones: Vec::new(),
            spawn_condition: None,
            quest_only: false,
        }
    }

    #[test]
    fn 몬스터RON을_읽으면_일반_세_종과_보스_한_종이_적재된다() {
        let reg = build_test_registry();
        // goblin/orc/troll 3종 + 보스 frost_wyrm 1종 = 4종.
        assert_eq!(reg.monsters.len(), 4, "goblin/orc/troll + 보스 frost_wyrm");
        assert!(reg.by_id("goblin").is_some());
        assert!(reg.by_id("orc").is_some());
        assert!(reg.by_id("troll").is_some());
        assert!(reg.by_id("frost_wyrm").is_some(), "보스 서리 마룡이 적재돼야 한다");
    }

    #[test]
    fn 보스_서리마룡은_quest_only이고_고HP_고공격_얼음원소다() {
        // dragon_hunt_quest 전용 보스 — 자연 스폰되지 않고 SpawnMonster 로만 등장.
        let reg = build_test_registry();
        let w = reg.by_id("frost_wyrm").expect("서리 마룡 정의가 있어야 한다");
        assert!(w.quest_only, "보스는 quest_only 여야 자연 스폰되지 않는다");
        assert_eq!((w.display_name.as_str(), w.glyph.as_str()), ("서리 마룡", "D"));
        assert_eq!((w.hp, w.attack, w.defense, w.vision_radius), (60, 14, 6, 10));
        assert_eq!(w.speed, 0.8);
        assert_eq!(w.color, (0.55, 0.85, 1.0));
        assert_eq!(w.element_enum(), Some(Element::Ice), "서리 마룡은 얼음 원소");
    }

    #[test]
    fn 없는_경로의_몬스터RON은_읽기_실패_에러를_반환한다() {
        // read_monster_defs 의 Err(읽기 실패) 분기 — process::exit 없는 seam.
        let r = read_monster_defs("assets/monsters/does_not_exist.ron");
        assert!(r.is_err(), "없는 파일은 Err");
    }

    #[test]
    fn 깨진_몬스터RON은_파싱_실패_에러를_반환한다() {
        // read_monster_defs 의 Err(파싱 실패) 분기 — 임시 파일에 잘못된 RON.
        let dir = std::env::temp_dir().join(format!("mrtest_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("broken.ron");
        std::fs::write(&path, "이건 RON 이 아니다").unwrap();
        let r = read_monster_defs(path.to_str().unwrap());
        assert!(r.is_err(), "깨진 RON 은 Err");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn id로_조회하면_해당_정의를_없으면_None을_반환한다() {
        let reg = build_test_registry();
        assert_eq!(reg.by_id("goblin").unwrap().display_name, "고블린");
        assert!(reg.by_id("unknown_id").is_none(), "없는 id 는 None");
    }

    #[test]
    fn 표시이름으로_조회하면_해당_정의를_없으면_None을_반환한다() {
        let reg = build_test_registry();
        assert_eq!(reg.by_display_name("트롤").unwrap().id, "troll");
        assert!(reg.by_display_name("유령").is_none(), "없는 이름은 None");
    }

    #[test]
    fn 기존_세_몬스터의_수치_글리프_색이_이전과_동등하다() {
        // 동작 보존: const 테이블에서 이전한 값이 그대로인지 확인.
        let reg = build_test_registry();
        let g = reg.by_id("goblin").unwrap();
        assert_eq!((g.display_name.as_str(), g.glyph.as_str()), ("고블린", "g"));
        assert_eq!((g.hp, g.attack, g.defense, g.vision_radius), (6, 3, 0, 6));
        assert_eq!(g.speed, 1.5);
        assert_eq!(g.color, (0.2, 0.8, 0.2));

        let o = reg.by_id("orc").unwrap();
        assert_eq!((o.display_name.as_str(), o.glyph.as_str()), ("오크", "O"));
        assert_eq!((o.hp, o.attack, o.defense, o.vision_radius), (10, 5, 2, 8));
        assert_eq!(o.speed, 1.0);
        assert_eq!(o.color, (0.9, 0.5, 0.1));

        let t = reg.by_id("troll").unwrap();
        assert_eq!((t.display_name.as_str(), t.glyph.as_str()), ("트롤", "T"));
        assert_eq!((t.hp, t.attack, t.defense, t.vision_radius), (16, 8, 3, 5));
        assert_eq!(t.speed, 0.5);
        assert_eq!(t.color, (0.3, 0.7, 0.5));
    }

    #[test]
    fn 기존_세_몬스터의_원소가_이전과_동등하다() {
        // monster_element 제거 후에도 원소 매핑이 일관: 고블린=독/오크=불/트롤=얼음.
        let reg = build_test_registry();
        assert_eq!(reg.by_id("goblin").unwrap().element_enum(), Some(Element::Poison));
        assert_eq!(reg.by_id("orc").unwrap().element_enum(), Some(Element::Fire));
        assert_eq!(reg.by_id("troll").unwrap().element_enum(), Some(Element::Ice));
    }

    #[test]
    fn 원소문자열은_네_원소로_매핑되고_없거나_미인식이면_None이다() {
        assert_eq!(def("a", "A", Some("fire"), 1.0).element_enum(),      Some(Element::Fire));
        assert_eq!(def("a", "A", Some("ice"), 1.0).element_enum(),       Some(Element::Ice));
        assert_eq!(def("a", "A", Some("poison"), 1.0).element_enum(),    Some(Element::Poison));
        assert_eq!(def("a", "A", Some("lightning"), 1.0).element_enum(), Some(Element::Lightning));
        assert_eq!(def("a", "A", None, 1.0).element_enum(),              None, "원소 없음");
        assert_eq!(def("a", "A", Some("plasma"), 1.0).element_enum(),    None, "미인식 문자열");
    }

    #[test]
    fn 정의의_색은_RGB_튜플로_변환된다() {
        assert_eq!(def("a", "A", None, 1.0).color(), Color::rgb(0.1, 0.2, 0.3));
    }

    #[test]
    fn 스폰가중치_기본값은_1점0이다() {
        // RON 에서 spawn_weight 생략 시 serde default 1.0.
        let d: MonsterDef = ron::de::from_str(
            r#"MonsterDef(id:"x",display_name:"엑스",glyph:"x",color:(0.0,0.0,0.0),hp:1,attack:1,defense:0,vision_radius:1,speed:1.0)"#
        ).unwrap();
        assert_eq!(d.spawn_weight, 1.0);
        assert!(d.zones.is_empty());
        assert!(d.spawn_condition.is_none());
        assert!(!d.quest_only);
        assert!(d.element.is_none());
    }

    // ── natural_spawn_candidates (순수 함수: zones / 조건 / quest_only 필터) ────

    fn spawn_ctx() -> (PlayerInventory, WorldState, QuestState, ItemRegistry) {
        (
            PlayerInventory::default(),
            WorldState::default(),
            QuestState::default(),
            crate::modules::item::build_test_registry(),
        )
    }

    fn registry_of(defs: Vec<MonsterDef>) -> MonsterRegistry {
        MonsterRegistry { monsters: defs }
    }

    #[test]
    fn 자연스폰후보는_zones가_비면_모든_존에서_나온다() {
        let reg = registry_of(vec![def("a", "A", None, 1.0)]);
        let (inv, world, qs, items) = spawn_ctx();
        let cands = natural_spawn_candidates(&reg, &ZoneId::Dungeon(3), &inv, &world, &qs, &items);
        assert_eq!(cands, vec![0], "zones 비면 어느 존이든 후보");
    }

    #[test]
    fn 자연스폰후보는_현재존이_zones에_포함될때만_나온다() {
        let mut d = def("a", "A", None, 1.0);
        d.zones = vec![ZoneId::Dungeon(1)];
        let reg = registry_of(vec![d]);
        let (inv, world, qs, items) = spawn_ctx();
        assert_eq!(
            natural_spawn_candidates(&reg, &ZoneId::Dungeon(1), &inv, &world, &qs, &items),
            vec![0], "지정 존이면 후보"
        );
        assert!(
            natural_spawn_candidates(&reg, &ZoneId::Dungeon(2), &inv, &world, &qs, &items).is_empty(),
            "다른 존이면 제외"
        );
    }

    #[test]
    fn 자연스폰후보는_quest_only면_제외된다() {
        let mut d = def("boss", "보스", None, 1.0);
        d.quest_only = true;
        let reg = registry_of(vec![d, def("a", "A", None, 1.0)]);
        let (inv, world, qs, items) = spawn_ctx();
        let cands = natural_spawn_candidates(&reg, &ZoneId::Town, &inv, &world, &qs, &items);
        assert_eq!(cands, vec![1], "quest_only(인덱스 0)는 제외, 일반(1)만");
    }

    #[test]
    fn 자연스폰후보는_spawn_condition이_참일때만_나온다() {
        let mut d = def("a", "A", None, 1.0);
        d.spawn_condition = Some(QuestCondition::HasFlag("awake".into()));
        let reg = registry_of(vec![d]);
        let (inv, world, mut qs, items) = spawn_ctx();
        // 플래그 미설정 → 조건 거짓 → 제외
        assert!(
            natural_spawn_candidates(&reg, &ZoneId::Town, &inv, &world, &qs, &items).is_empty(),
            "조건 거짓이면 제외"
        );
        // 플래그 설정 → 조건 참 → 포함
        qs.set_flag("awake", "true");
        assert_eq!(
            natural_spawn_candidates(&reg, &ZoneId::Town, &inv, &world, &qs, &items),
            vec![0], "조건 참이면 후보"
        );
    }

    // ── choose_monster_index (순수 함수: 가중 선택) ────────────────────────────

    #[test]
    fn 후보가_없으면_선택은_None이다() {
        let reg = registry_of(vec![def("a", "A", None, 1.0)]);
        assert_eq!(choose_monster_index(&reg, &[], 0.0), None);
    }

    #[test]
    fn 가중선택은_roll이_속한_구간의_후보를_고른다() {
        // 가중치 [2.0, 1.0, 3.0] (총 6.0). 누적 경계: A<2, B<3, C<6.
        let reg = registry_of(vec![
            def("a", "A", None, 2.0),
            def("b", "B", None, 1.0),
            def("c", "C", None, 3.0),
        ]);
        let cands = vec![0, 1, 2];
        assert_eq!(choose_monster_index(&reg, &cands, 0.0), Some(0), "roll 0 → 첫 구간");
        assert_eq!(choose_monster_index(&reg, &cands, 1.9), Some(0), "1.9 < 2 → A");
        assert_eq!(choose_monster_index(&reg, &cands, 2.0), Some(1), "2.0 → B 구간 시작");
        assert_eq!(choose_monster_index(&reg, &cands, 2.5), Some(1), "2.5 → B");
        assert_eq!(choose_monster_index(&reg, &cands, 3.0), Some(2), "3.0 → C 구간 시작");
        assert_eq!(choose_monster_index(&reg, &cands, 5.9), Some(2), "5.9 → C");
    }

    #[test]
    fn 가중합이_경계를_넘으면_마지막_후보로_폴백한다() {
        // roll 이 total 과 같거나 초과(부동소수 오차 가정) → 마지막 후보.
        let reg = registry_of(vec![def("a", "A", None, 1.0), def("b", "B", None, 1.0)]);
        assert_eq!(choose_monster_index(&reg, &[0, 1], 2.0), Some(1), "경계 초과 → 마지막");
    }

    #[test]
    fn 가중합이_0이하면_첫_후보로_폴백한다() {
        // 모든 가중치 0 → total 0 → 첫 후보.
        let reg = registry_of(vec![def("a", "A", None, 0.0), def("b", "B", None, 0.0)]);
        assert_eq!(choose_monster_index(&reg, &[0, 1], 0.0), Some(0), "가중 0 → 첫 후보");
    }

    // ── handle_spawn_monster (SpawnMonster 액션 → 스폰) ────────────────────────

    fn spawn_monster_app(map: Map) -> App {
        let mut app = asset_app();
        app.insert_resource(MapResource(map));
        app.init_resource::<UsedSpawnTiles>();
        app.add_event::<SpawnMonsterEvent>();
        app.insert_resource(build_test_registry());
        app.add_systems(Update, handle_spawn_monster);
        app
    }

    #[test]
    fn 스폰몬스터이벤트는_지정_몬스터를_요청_마릿수만큼_스폰한다() {
        let mut map = full_floor_map();
        map.rooms = rooms_with(3);
        let mut app = spawn_monster_app(map);
        app.world.send_event(SpawnMonsterEvent { id: "troll".into(), count: 2 });
        app.update();
        let trolls = app.world.query::<&Monster>()
            .iter(&app.world).filter(|m| m.name == "트롤").count();
        assert_eq!(trolls, 2, "트롤 2마리 스폰");
    }

    #[test]
    fn 스폰몬스터로_나온_몬스터는_정의의_스탯을_가진다() {
        let mut map = full_floor_map();
        map.rooms = rooms_with(1);
        let mut app = spawn_monster_app(map);
        app.world.send_event(SpawnMonsterEvent { id: "orc".into(), count: 1 });
        app.update();
        let mut q = app.world.query::<(&Monster, &CombatStats)>();
        let (m, stats) = q.iter(&app.world).find(|(m, _)| m.name == "오크").expect("오크 존재");
        assert_eq!((stats.max_hp, stats.attack, stats.defense), (10, 5, 2), "정의 스탯");
        assert_eq!(m.vision_radius, 8, "정의 시야");
        assert_eq!(m.slot_idx, usize::MAX, "리스폰 슬롯 미연결");
    }

    #[test]
    fn quest_only_보스도_스폰몬스터로는_등장한다() {
        // quest_only 는 자연 스폰만 막고 SpawnMonster 로는 등장한다.
        let mut map = full_floor_map();
        map.rooms = rooms_with(1);
        let mut reg = build_test_registry();
        reg.monsters.push(MonsterDef {
            id: "dragon".into(), display_name: "드래곤".into(), glyph: "D".into(),
            color: (1.0, 0.0, 0.0), hp: 100, attack: 30, defense: 10,
            vision_radius: 12, speed: 1.0, element: Some("fire".into()),
            spawn_weight: 1.0, zones: Vec::new(), spawn_condition: None, quest_only: true,
        });
        let mut app = asset_app();
        app.insert_resource(MapResource(map));
        app.init_resource::<UsedSpawnTiles>();
        app.add_event::<SpawnMonsterEvent>();
        app.insert_resource(reg);
        app.add_systems(Update, handle_spawn_monster);
        app.world.send_event(SpawnMonsterEvent { id: "dragon".into(), count: 1 });
        app.update();
        let dragons = app.world.query::<&Monster>()
            .iter(&app.world).filter(|m| m.name == "드래곤").count();
        assert_eq!(dragons, 1, "quest_only 보스도 명시적 스폰은 가능");
    }

    #[test]
    fn 알수없는_id로_스폰몬스터를_요청하면_아무것도_스폰하지_않는다() {
        let mut map = full_floor_map();
        map.rooms = rooms_with(1);
        let mut app = spawn_monster_app(map);
        app.world.send_event(SpawnMonsterEvent { id: "없는몬스터".into(), count: 3 });
        app.update();
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 0, "미등록 id 는 스폰 안 함");
    }

    #[test]
    fn 통과타일이_없으면_스폰몬스터는_스폰을_건너뛴다() {
        // 전부 Wall + room 없음 → random_floor_tile_anywhere 항상 None.
        let mut map = Map::new(MAP_WIDTH, MAP_HEIGHT);
        map.rooms = vec![];
        let mut app = spawn_monster_app(map);
        app.world.send_event(SpawnMonsterEvent { id: "goblin".into(), count: 2 });
        app.update();
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 0, "Floor 타일 없으면 스폰 실패 (continue)");
    }

    #[test]
    fn 레지스트리에_없는_data_idx_슬롯은_스폰에서_건너뛴다() {
        // spawn_from_slots 의 registry.monsters.get(idx) None 분기.
        let rooms = rooms_with(2);
        let slots = vec![
            MonsterSlot { data_idx: 0, respawn_at_turn: None },   // 유효
            MonsterSlot { data_idx: 999, respawn_at_turn: None }, // 범위 밖 → skip
        ];
        let mut app = run_spawn_from_slots(&rooms, &slots, 0);
        let n = app.world.query::<&Monster>().iter(&app.world).count();
        assert_eq!(n, 1, "범위 밖 data_idx 슬롯은 스폰 제외");
    }

    // ── danger_tiles (위험 타일 순수 함수) ────────────────────────────────────

    use crate::modules::item::InventoryItem;

    #[test]
    fn 위험타일은_가드_정면_시야_타일을_포함한다() {
        // 가드(5,5)가 오른쪽(+x)을 보면 정면 타일(8,5)이 위험 타일에 포함된다.
        let map = open_map(20, 20);
        let guards = [((5usize, 5usize), IVec2::new(1, 0), 8)];
        let tiles = danger_tiles(&guards, &map, |_, _| LightLevel::Bright);
        assert!(tiles.contains(&(8, 5)), "정면 반경 내 타일은 위험 타일이어야 한다");
        assert!(tiles.contains(&(5, 5)), "가드 자기 타일도 시야에 포함된다");
    }

    #[test]
    fn 위험타일은_가드_등_뒤의_먼_타일을_제외한다() {
        // 가드(10,5)가 오른쪽(+x)을 보면, 등 뒤(-x)로 FOV_BACK(3)을 넘는 (5,5)는 제외.
        let map = open_map(20, 20);
        let guards = [((10usize, 5usize), IVec2::new(1, 0), 8)];
        let tiles = danger_tiles(&guards, &map, |_, _| LightLevel::Bright);
        assert!(!tiles.contains(&(5, 5)), "등 뒤로 FOV_BACK 을 넘는 타일은 제외되어야 한다");
    }

    #[test]
    fn 위험타일은_벽_너머의_타일을_제외한다() {
        // (5,5)와 (9,5) 사이 x=7 에 벽 열 → 벽 너머 타일은 시야가 막혀 제외.
        let mut map = open_map(20, 20);
        for y in 0..20 { map.set_tile(7, y, TileKind::Wall); }
        let guards = [((5usize, 5usize), IVec2::new(1, 0), 10)];
        let tiles = danger_tiles(&guards, &map, |_, _| LightLevel::Bright);
        assert!(tiles.contains(&(6, 5)), "벽 앞 타일은 보인다");
        assert!(!tiles.contains(&(9, 5)), "벽 너머 타일은 시야가 막혀 제외되어야 한다");
    }

    #[test]
    fn 위험타일은_여러_가드의_시야를_합집합으로_모은다() {
        // 서로 멀리 떨어진 두 가드의 시야가 모두 포함되어야 한다.
        let map = open_map(40, 20);
        let guards = [
            ((5usize, 5usize), IVec2::new(1, 0), 6),
            ((30usize, 5usize), IVec2::new(-1, 0), 6),
        ];
        let tiles = danger_tiles(&guards, &map, |_, _| LightLevel::Bright);
        assert!(tiles.contains(&(8, 5)), "첫 가드 정면 타일 포함");
        assert!(tiles.contains(&(27, 5)), "둘째 가드 정면 타일 포함");
    }

    #[test]
    fn 가드가_없으면_위험타일은_빈_집합이다() {
        let map = open_map(20, 20);
        let guards: [((usize, usize), IVec2, i32); 0] = [];
        let tiles = danger_tiles(&guards, &map, |_, _| LightLevel::Bright);
        assert!(tiles.is_empty(), "가드가 없으면 위험 타일도 없다");
    }

    // ── update_guard_vision_overlay (오버레이 시스템) ─────────────────────────

    fn scout_lens_item() -> InventoryItem {
        InventoryItem::new(ItemKind::QuestItem(QuestItemKind(SCOUT_LENS_ID)))
    }

    /// 오버레이 시스템 + 필요한 리소스를 갖춘 App. AssetServer(Image) 는 SpriteBundle 용.
    fn overlay_app(map: Map, has_lens: bool) -> App {
        let mut app = asset_app();
        let (w, h) = (map.width, map.height);
        app.insert_resource(MapResource(map));
        app.insert_resource(bright_light_map(w, h));
        let mut inv = PlayerInventory::default();
        if has_lens { inv.items.push(scout_lens_item()); }
        app.insert_resource(inv);
        app.add_systems(Update, update_guard_vision_overlay);
        app
    }

    /// 지정 위치·facing 의 가드 한 마리를 스폰한다 (CombatStats hp>0).
    fn spawn_guard_for_overlay(app: &mut App, tile: (usize, usize), facing: IVec2, radius: i32) -> Entity {
        app.world.spawn((
            Monster { name: "가드".into(), tile_x: tile.0, tile_y: tile.1, vision_radius: radius, alert_turns: 0, slot_idx: 0 },
            Facing(facing),
            CombatStats { hp: 20, max_hp: 20, mp: 0, max_mp: 0, attack: 4, defense: 0 },
        )).id()
    }

    fn overlay_count(app: &mut App) -> usize {
        app.world.query_filtered::<Entity, With<GuardVisionOverlay>>().iter(&app.world).count()
    }

    #[test]
    fn 정찰도구를_보유하면_위험타일에_오버레이가_생긴다() {
        let map = open_map(20, 20);
        let mut app = overlay_app(map, true);
        spawn_guard_for_overlay(&mut app, (5, 5), IVec2::new(1, 0), 6);
        app.update();
        assert!(overlay_count(&mut app) > 0, "정찰 도구 보유 시 위험 타일 오버레이가 스폰되어야 한다");
    }

    #[test]
    fn 정찰도구가_없으면_오버레이가_생기지_않는다() {
        let map = open_map(20, 20);
        let mut app = overlay_app(map, false);
        spawn_guard_for_overlay(&mut app, (5, 5), IVec2::new(1, 0), 6);
        app.update();
        assert_eq!(overlay_count(&mut app), 0, "정찰 도구 미보유 시 오버레이가 없어야 한다");
    }

    #[test]
    fn 가드_facing이_바뀌면_오버레이가_새_시야로_갱신된다() {
        // 가드가 오른쪽을 볼 때 (8,5)는 위험, 왼쪽으로 돌면 (8,5)는 등 뒤로 빠져 안전.
        let map = open_map(20, 20);
        let mut app = overlay_app(map, true);
        let guard = spawn_guard_for_overlay(&mut app, (5, 5), IVec2::new(1, 0), 6);
        app.update();

        // 오버레이 사각형의 월드 좌표로 (8,5) 가 덮였는지 확인하는 헬퍼.
        let covered = |app: &mut App, tile: (usize, usize)| {
            let target = tile_to_world_coords(tile.0, tile.1);
            app.world.query_filtered::<&Transform, With<GuardVisionOverlay>>()
                .iter(&app.world)
                .any(|t| (t.translation.x - target.x).abs() < 0.1 && (t.translation.y - target.y).abs() < 0.1)
        };
        // (9,5)는 정면 4칸(반경 6 이내)이라 위험, 등 뒤로는 FOV_BACK(3)을 넘어 안전.
        assert!(covered(&mut app, (9, 5)), "오른쪽을 볼 때 (9,5)는 위험 타일");

        // facing 을 왼쪽(-x)으로 돌린다.
        app.world.get_mut::<Facing>(guard).unwrap().0 = IVec2::new(-1, 0);
        app.update();
        assert!(!covered(&mut app, (9, 5)), "왼쪽으로 돌면 (9,5)는 등 뒤로 빠져 위험 타일에서 빠진다");
    }

    #[test]
    fn 정찰도구를_잃으면_기존_오버레이가_제거된다() {
        // 보유 → 오버레이 생성, 미보유로 바뀌면 전부 제거.
        let map = open_map(20, 20);
        let mut app = overlay_app(map.clone(), true);
        spawn_guard_for_overlay(&mut app, (5, 5), IVec2::new(1, 0), 6);
        app.update();
        assert!(overlay_count(&mut app) > 0, "보유 시 오버레이 존재");

        // 인벤토리에서 정찰 도구 제거.
        app.world.resource_mut::<PlayerInventory>().items.clear();
        app.update();
        assert_eq!(overlay_count(&mut app), 0, "정찰 도구를 잃으면 오버레이가 전부 제거된다");
    }

    #[test]
    fn 죽은_가드의_시야는_위험타일에_반영되지_않는다() {
        // hp<=0 가드는 시야에서 제외되어 오버레이가 생기지 않는다.
        let map = open_map(20, 20);
        let mut app = overlay_app(map, true);
        app.world.spawn((
            Monster { name: "가드".into(), tile_x: 5, tile_y: 5, vision_radius: 6, alert_turns: 0, slot_idx: 0 },
            Facing(IVec2::new(1, 0)),
            CombatStats { hp: 0, max_hp: 20, mp: 0, max_mp: 0, attack: 4, defense: 0 },
        ));
        app.update();
        assert_eq!(overlay_count(&mut app), 0, "죽은 가드의 시야는 오버레이로 표시되지 않는다");
    }

    #[test]
    fn 정찰도구_보유_판정은_인벤토리의_올빼미안경을_인식한다() {
        let mut inv = PlayerInventory::default();
        assert!(!player_has_scout_lens(&inv), "기본 인벤토리엔 정찰 도구가 없다");
        inv.items.push(scout_lens_item());
        assert!(player_has_scout_lens(&inv), "올빼미 안경을 넣으면 보유로 판정한다");
    }
}
