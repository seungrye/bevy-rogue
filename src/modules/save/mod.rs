use bevy::prelude::*;
use std::collections::HashMap;
use crate::modules::{
    map::{Map, MapResource, MapGeneratorRegistry, MAP_WIDTH, MAP_HEIGHT, ApplyMapEvent, GlobalTurn, GlobalSeed},
    player::{Player, PlayerProgress},
    item::{PlayerInventory, PlayerEquipment},
    quest::QuestState,
    zone::{WorldState, ZoneId, ZonePersistence, ZoneSnapshot, NamedZoneConfig, zone_seed},
    ui::minimap::DiscoveredMarkers,
    combat::{CombatStats, Defeated},
    combat_feedback::BloodStain,
    map::world_to_tile_coords,
};

pub const SAVE_PATH: &str = "save/progress.ron";
const SAVE_TMP:  &str = "save/progress.ron.tmp";
const SAVE_VERSION: u32 = 5;

/// 세이브 파일 경로 설정. 테스트에서 임시 경로를 주입할 수 있도록 seam 으로 분리한다.
/// Default 는 프로덕션 상수(`save/progress.ron`)와 동일하게 유지한다.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct SaveConfig {
    pub path: String,
    pub tmp: String,
}

impl Default for SaveConfig {
    fn default() -> Self {
        Self { path: SAVE_PATH.to_string(), tmp: SAVE_TMP.to_string() }
    }
}

// ── 비트팩 + Base64 ──────────────────────────────────────────────────────────
//
// Vec<bool> → bitpack → Vec<u8> → Base64 → String
// 80×50=4000 tiles: 4000 bytes(bool) → 500 bytes(bitpack) → 668 chars(base64)

const B64_ENC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn pack_b64(tiles: &[bool]) -> String {
    // step 1: bitpack
    let mut bytes = vec![0u8; tiles.len().div_ceil(8)];
    for (i, &v) in tiles.iter().enumerate() {
        if v { bytes[i / 8] |= 1 << (i % 8); }
    }
    // step 2: base64 encode
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let n = ((chunk[0] as u32) << 16)
              | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8)
              | (chunk.get(2).copied().unwrap_or(0) as u32);
        out.push(B64_ENC[(n >> 18) as usize] as char);
        out.push(B64_ENC[(n >> 12 & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 { B64_ENC[(n >> 6 & 0x3f) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64_ENC[(n & 0x3f) as usize] as char } else { '=' });
    }
    out
}

fn unpack_b64(s: &str, tile_count: usize) -> Vec<bool> {
    // step 1: base64 decode
    let mut dec = [0u8; 256];
    for (i, &c) in B64_ENC.iter().enumerate() { dec[c as usize] = i as u8; }
    let chars: Vec<u8> = s.bytes().collect();
    let mut bytes = Vec::with_capacity(chars.len() / 4 * 3);
    for chunk in chars.chunks(4) {
        if chunk.len() < 4 { break; }
        let n = ((dec[chunk[0] as usize] as u32) << 18)
              | ((dec[chunk[1] as usize] as u32) << 12)
              | ((dec[chunk[2] as usize] as u32) << 6)
              |  (dec[chunk[3] as usize] as u32);
        bytes.push((n >> 16) as u8);
        if chunk[2] != b'=' { bytes.push((n >> 8 & 0xff) as u8); }
        if chunk[3] != b'=' { bytes.push((n & 0xff) as u8); }
    }
    // step 2: unpack bits
    (0..tile_count).map(|i| {
        bytes.get(i / 8).map_or(false, |&b| (b >> (i % 8)) & 1 != 0)
    }).collect()
}


/// MapTile 벡터에서 revealed 필드만 추출하여 비트팩한다.
fn pack_revealed(tiles: &[crate::modules::map::MapTile]) -> String {
    let bools: Vec<bool> = tiles.iter().map(|t| t.revealed).collect();
    pack_b64(&bools)
}

/// 언팩된 bool 벡터를 MapTile 벡터의 revealed 필드에 적용한다.
fn apply_revealed(tiles: &mut [crate::modules::map::MapTile], revealed: &[bool]) {
    for (tile, &r) in tiles.iter_mut().zip(revealed.iter()) {
        tile.revealed = r;
    }
}

// ── SaveData ─────────────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize)]
pub struct SaveData {
    pub version: u32,
    pub global_seed: u64,
    pub global_turn: u64,
    pub player_tile: [usize; 2],
    pub player_hp: i32,
    pub player_max_hp: i32,
    pub player_mp: i32,
    pub player_max_mp: i32,
    pub player_attack: i32,
    pub player_defense: i32,
    #[serde(default)]
    pub player_progress: PlayerProgress,
    pub inventory: PlayerInventory,
    pub equipment: PlayerEquipment,
    pub quest_state: QuestState,
    /// 이번 런에 활성화된 퀘스트 ID — spawn_chance 재롤 방지용으로 저장.
    /// 기존 저장 파일 호환을 위해 #[serde(default)].
    #[serde(default)]
    pub active_quests: std::collections::HashSet<String>,
    pub current_zone: ZoneId,
    /// 방문한 존별 탐험 기록 — 비트팩 후 Base64 인코딩 (80×50 → 668 chars/zone)
    pub zone_revealed: HashMap<ZoneId, String>,
    pub zone_persistence: HashMap<ZoneId, ZoneSnapshot>,
    pub discovered_markers: DiscoveredMarkers,
    pub named_zones: NamedZoneConfig,
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SaveConfig>()
            .add_systems(PostStartup, load_if_save_exists)
            .add_systems(Update, auto_save);
    }
}

// ── 자동 저장 ─────────────────────────────────────────────────────────────────

fn auto_save(
    mut events: EventReader<crate::modules::map::PlayerActedEvent>,
    inventory: Res<PlayerInventory>,
    equipment: Res<PlayerEquipment>,
    progress: Res<PlayerProgress>,
    quest_state: Res<QuestState>,
    quest_registry: Res<crate::modules::quest::QuestRegistry>,
    world_state: Res<WorldState>,
    persistence: Res<ZonePersistence>,
    markers: Res<DiscoveredMarkers>,
    named_config: Res<NamedZoneConfig>,
    global_turn: Res<GlobalTurn>,
    global_seed: Res<GlobalSeed>,
    map_res: Res<MapResource>,
    config: Res<SaveConfig>,
    player_q: Query<(&Transform, &CombatStats), (With<Player>, Without<Defeated>)>,
    blood_q: Query<(&BloodStain, &Transform), Without<Player>>,
) {
    if events.read().next().is_none() { return; }
    let Ok((transform, stats)) = player_q.get_single() else { return };

    let (tx, ty) = world_to_tile_coords(transform.translation);

    // 현재 존의 혈흔을 스냅샷에 포함
    let mut zone_persistence = persistence.0.clone();
    let cur_snap = zone_persistence.entry(world_state.current.clone()).or_default();
    cur_snap.blood_stains = blood_q.iter().map(|(stain, t)| {
        let (bx, by) = world_to_tile_coords(t.translation);
        crate::modules::zone::SavedBloodStain {
            tile_x: bx, tile_y: by,
            alpha: stain.alpha,
            decay_per_turn: stain.decay_per_turn,
        }
    }).collect();

    let zone_revealed = collect_revealed_by_zone(&world_state, &map_res.0);

    let save = SaveData {
        version: SAVE_VERSION,
        global_seed: global_seed.0,
        global_turn: global_turn.0,
        player_tile: [tx, ty],
        player_hp: stats.hp,
        player_max_hp: stats.max_hp,
        player_mp: stats.mp,
        player_max_mp: stats.max_mp,
        player_attack: stats.attack,
        player_defense: stats.defense,
        player_progress: progress.clone(),
        inventory: inventory.clone(),
        equipment: equipment.clone(),
        quest_state: quest_state.clone(),
        active_quests: quest_registry.active.clone(),
        current_zone: world_state.current.clone(),
        zone_revealed,
        zone_persistence,
        discovered_markers: markers.clone(),
        named_zones: named_config.clone(),
    };

    write_save_to(&save, &config.path, &config.tmp);
}

fn collect_revealed_by_zone(world_state: &WorldState, current_map: &Map) -> HashMap<ZoneId, String> {
    let mut zone_revealed: HashMap<ZoneId, String> = world_state.maps.iter()
        .map(|(id, map)| (id.clone(), pack_revealed(&map.tiles)))
        .collect();
    zone_revealed.insert(
        world_state.current.clone(),
        pack_revealed(&current_map.tiles),
    );
    zone_revealed
}

fn restore_map_for_zone(
    registry: &MapGeneratorRegistry,
    named_config: &NamedZoneConfig,
    global_seed: u64,
    zone_id: &ZoneId,
    revealed_b64: Option<&str>,
) -> Map {
    let seed = zone_seed(global_seed, zone_id);
    let algorithm = get_algo(zone_id, named_config);
    let mut map = registry.generate_with(&algorithm, MAP_WIDTH, MAP_HEIGHT, seed)
        .unwrap_or_else(|| {
            warn!("알 수 없는 맵 생성기 {} - 빈 맵으로 복원합니다", algorithm);
            Map::new(MAP_WIDTH, MAP_HEIGHT)
        });
    map.seed = seed;
    map.algorithm = algorithm;

    if let Some(encoded) = revealed_b64 {
        apply_revealed(&mut map.tiles, &unpack_b64(encoded, map.width * map.height));
    }
    map.tiles.iter_mut().for_each(|tile| tile.visible = false);
    map
}

fn restore_player_stats(stats: &mut CombatStats, save: &SaveData) {
    stats.hp       = save.player_hp;
    stats.max_hp   = save.player_max_hp;
    stats.mp       = save.player_mp;
    stats.max_mp   = save.player_max_mp;
    stats.attack   = save.player_attack;
    stats.defense  = save.player_defense;
}

fn write_save_to(save: &SaveData, path: &str, tmp: &str) {
    let content = match ron::ser::to_string_pretty(save, ron::ser::PrettyConfig::default()) {
        Ok(s) => s,
        // 도달 불가 방어코드: 유효한 SaveData 는 RON 직렬화에 실패하지 않는다.
        Err(e) => { error!("세이브 직렬화 실패: {e}"); return; }
    };
    // tmp 경로의 상위 디렉터리를 보장한다. 부모가 비어있으면(현재 디렉터리) 생성 단계를 건너뛴다.
    let parent = std::path::Path::new(tmp).parent()
        .filter(|p| !p.as_os_str().is_empty());
    if let Some(parent) = parent {
        if let Err(e) = std::fs::create_dir_all(parent) {
            error!("세이브 디렉터리 생성 실패: {e}"); return;
        }
    }
    if let Err(e) = std::fs::write(tmp, &content) {
        error!("세이브 파일 쓰기 실패: {e}"); return;
    }
    if let Err(e) = std::fs::rename(tmp, path) {
        error!("세이브 파일 교체 실패: {e}"); return;
    }
}

// ── 자동 로드 (PostStartup) ──────────────────────────────────────────────────

fn get_algo(zone_id: &ZoneId, named_config: &NamedZoneConfig) -> String {
    match zone_id {
        ZoneId::Named(name) => named_config.zones.get(name)
            .map(|e| e.generator.clone())
            .unwrap_or_else(|| "bsp".to_string()),
        _ => zone_id.algorithm().to_string(),
    }
}

fn load_if_save_exists(
    mut inventory: ResMut<PlayerInventory>,
    mut equipment: ResMut<PlayerEquipment>,
    mut progress: ResMut<PlayerProgress>,
    mut quest_state: ResMut<QuestState>,
    mut quest_registry: ResMut<crate::modules::quest::QuestRegistry>,
    mut world_state: ResMut<WorldState>,
    mut persistence: ResMut<ZonePersistence>,
    mut markers: ResMut<DiscoveredMarkers>,
    mut global_turn: ResMut<GlobalTurn>,
    mut global_seed: ResMut<GlobalSeed>,
    registry: Res<MapGeneratorRegistry>,
    mut named_config: ResMut<NamedZoneConfig>,
    config: Res<SaveConfig>,
    mut player_q: Query<(Entity, &mut CombatStats), With<Player>>,
    mut apply_ev: EventWriter<ApplyMapEvent>,
    mut commands: Commands,
) {
    let content = match std::fs::read_to_string(&config.path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let save: SaveData = match ron::from_str(&content) {
        Ok(s) => s,
        Err(e) => {
            warn!("세이브 파일 파싱 실패, 신규 게임 시작: {e}");
            return;
        }
    };

    if save.version != SAVE_VERSION {
        warn!("세이브 버전 불일치 ({}), 신규 게임 시작", save.version);
        return;
    }

    *named_config = save.named_zones.clone();

    // 현재 존 맵 재생성 — global_seed + zone_id로 결정론적 복원
    let map = restore_map_for_zone(
        &registry,
        &named_config,
        save.global_seed,
        &save.current_zone,
        save.zone_revealed.get(&save.current_zone).map(String::as_str),
    );

    // 플레이어 스탯은 SaveData 리소스 필드를 이동하기 전에 복원한다.
    // HP<=0 으로 저장된 경우 (이전 버그 방어) 즉시 Defeated 부여 — 게임 오버 UI 트리거.
    if let Ok((player_entity, mut stats)) = player_q.get_single_mut() {
        restore_player_stats(&mut stats, &save);
        if stats.hp <= 0 {
            commands.entity(player_entity).insert(Defeated);
        }
    }

    // 리소스 복원
    *inventory    = save.inventory;
    *equipment    = save.equipment;
    *progress     = save.player_progress;
    *quest_state  = save.quest_state;
    // 활성 퀘스트 복원 — load_quests 가 startup 에 spawn_chance 로 재롤한 값을 덮어쓴다.
    // saved 가 비어있으면(legacy 저장 데이터) 재롤한 값 그대로 둔다.
    if !save.active_quests.is_empty() {
        quest_registry.active = save.active_quests;
    }
    *persistence  = ZonePersistence(save.zone_persistence);
    *markers      = save.discovered_markers;
    global_turn.0 = save.global_turn;
    global_seed.0 = save.global_seed;

    // 이전 방문 존 맵 재생성 후 캐시
    let zone_maps: HashMap<ZoneId, Map> = save.zone_revealed.iter()
        .filter(|(id, _)| **id != save.current_zone)
        .map(|(id, revealed)| {
            let map = restore_map_for_zone(
                &registry,
                &named_config,
                save.global_seed,
                id,
                Some(revealed.as_str()),
            );
            (id.clone(), map)
        })
        .collect();
    *world_state = WorldState { current: save.current_zone.clone(), maps: zone_maps };

    let [tx, ty] = save.player_tile;
    apply_ev.send(ApplyMapEvent { map, spawn_pos: Some((tx, ty)) });

    info!("세이브 로드 완료 — 존: {:?}, 시드: {:#x}, 턴: {}", save.current_zone, save.global_seed, save.global_turn);
}

// ── 세이브 파일 삭제 (외부 유틸) ─────────────────────────────────────────────

#[allow(dead_code)]
pub fn delete_save() {
    delete_save_at(SAVE_PATH);
}

fn delete_save_at(path: &str) {
    let _ = std::fs::remove_file(path);
}

// ── 단위 테스트 ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::{
        item::{InventoryItem, ItemKind, WeaponKind, ConsumableKind},
        player::xp_to_next_level,
        zone::ZoneId,
    };

    fn xp_to_next_level_for_test(level: u32) -> u32 {
        xp_to_next_level(level)
    }

    fn make_minimal_save() -> SaveData {
        let mut revealed = vec![false; 10 * 10];
        revealed[5] = true;
        revealed[42] = true;
        SaveData {
            version: SAVE_VERSION,
            global_seed: 0xdeadbeef_cafebabe,
            global_turn: 42,
            player_tile: [2, 2],
            player_hp: 18,
            player_max_hp: 20,
            player_mp: 5,
            player_max_mp: 5,
            player_attack: 8,
            player_defense: 3,
            player_progress: PlayerProgress { level: 3, xp: 7, next_level_xp: xp_to_next_level_for_test(3), kills: 12 },
            inventory: PlayerInventory {
                items: vec![InventoryItem::new(ItemKind::Weapon(WeaponKind::SWORD))],
                consumables: vec![(ConsumableKind::HEALTH_POTION, 2)],
                gold: 75,
            },
            equipment: PlayerEquipment { weapon: Some(WeaponKind::SWORD), armor: None, ..Default::default() },
            quest_state: QuestState::default(),
            active_quests: std::collections::HashSet::new(),
            current_zone: ZoneId::Town,
            zone_revealed: {
                let mut m = HashMap::new();
                m.insert(ZoneId::Town, pack_b64(&revealed));
                m
            },
            zone_persistence: HashMap::new(),
            discovered_markers: DiscoveredMarkers::default(),
            named_zones: NamedZoneConfig::default(),
        }
    }

    #[test]
    fn 비트팩_후_base64로_인코딩하면_언팩시_원본_불값이_복원된다() {
        let tiles: Vec<bool> = (0..100).map(|i| i % 3 == 0).collect();
        let encoded = pack_b64(&tiles);
        // base64: ceil(ceil(100/8)/3)*4 = ceil(13/3)*4 = 5*4 = 20 chars
        assert_eq!(encoded.len(), 20);
        let decoded = unpack_b64(&encoded, 100);
        assert_eq!(decoded, tiles);
    }

    #[test]
    fn 타일_4000개를_비트팩하면_base64는_668자로_압축된다() {
        // 80×50=4000 tiles → 500 bitpacked bytes → ceil(500/3)*4 = 668 base64 chars
        let tiles = vec![true; 4000];
        let encoded = pack_b64(&tiles);
        assert_eq!(encoded.len(), 668);
        // 원본 bool vec보다 약 6배 작음 (4000 vs 668)
        assert!(encoded.len() < tiles.len() / 5);
    }

    #[test]
    fn 활성_퀘스트를_직렬화하고_역직렬화하면_그대로_복원된다() {
        let mut save = make_minimal_save();
        save.active_quests.insert("herb_quest".to_string());
        save.active_quests.insert("gem_quest".to_string());
        let ron_str = ron::ser::to_string_pretty(&save, ron::ser::PrettyConfig::default()).unwrap();
        let restored: SaveData = ron::from_str(&ron_str).unwrap();
        assert_eq!(restored.active_quests.len(), 2);
        assert!(restored.active_quests.contains("herb_quest"));
        assert!(restored.active_quests.contains("gem_quest"));
    }

    #[test]
    fn 활성_퀘스트_필드가_없는_레거시_세이브도_파싱에_성공한다() {
        // 기존 저장 파일(active_quests 필드 없음) 호환성 — #[serde(default)]
        let legacy = r#"(
            version: 1,
            global_seed: 1,
            global_turn: 0,
            player_tile: (0, 0),
            player_hp: 10, player_max_hp: 10, player_mp: 0, player_max_mp: 0,
            player_attack: 1, player_defense: 1,
            inventory: (items: [], consumables: [], gold: 0),
            equipment: (weapon: None, armor: None),
            quest_state: (phases: {}, spawned: [], flags: {}),
            current_zone: Town,
            zone_revealed: {},
            zone_persistence: {},
            discovered_markers: ([]),
            named_zones: (zones: {}),
        )"#;
        let parsed: Result<SaveData, _> = ron::from_str(legacy);
        // 호환성 — version mismatch 는 ok (다른 테스트), 단지 파싱은 성공해야 함
        assert!(parsed.is_ok(), "legacy 저장 데이터 파싱 실패: {:?}", parsed.err());
        let s = parsed.unwrap();
        assert!(s.active_quests.is_empty());
    }

    #[test]
    fn 세이브데이터를_ron으로_왕복하면_모든_필드가_보존된다() {
        let original = make_minimal_save();
        let ron_str = ron::ser::to_string_pretty(&original, ron::ser::PrettyConfig::default())
            .expect("직렬화 실패");
        let restored: SaveData = ron::from_str(&ron_str).expect("역직렬화 실패");

        assert_eq!(restored.version, SAVE_VERSION);
        assert_eq!(restored.global_seed, 0xdeadbeef_cafebabe);
        assert_eq!(restored.global_turn, 42);
        assert_eq!(restored.player_tile, [2, 2]);
        assert_eq!(restored.player_hp, 18);
        assert_eq!(restored.player_progress.level, 3);
        assert_eq!(restored.player_progress.xp, 7);
        assert_eq!(restored.player_progress.kills, 12);
        assert_eq!(restored.inventory.gold, 75);
        assert!(matches!(restored.inventory.items[0].kind, ItemKind::Weapon(WeaponKind::SWORD)));
        assert!(matches!(restored.equipment.weapon, Some(WeaponKind::SWORD)));
        assert_eq!(restored.current_zone, ZoneId::Town);
        assert!(restored.zone_revealed.contains_key(&ZoneId::Town));
    }

    #[test]
    fn 탐험_타일은_base64_왕복_후에도_보존된다() {
        let save = make_minimal_save();
        let ron_str = ron::ser::to_string_pretty(&save, ron::ser::PrettyConfig::default()).unwrap();
        let restored: SaveData = ron::from_str(&ron_str).unwrap();

        let s = restored.zone_revealed.get(&ZoneId::Town).unwrap();
        let unpacked = unpack_b64(s, 10 * 10);
        assert!(unpacked[5]);
        assert!(unpacked[42]);
        assert!(!unpacked[0]);
        assert!(!unpacked[99]);
    }

    #[test]
    fn 같은_입력이면_존_시드는_항상_같고_다른_존은_다른_시드를_낳는다() {
        use crate::modules::zone::zone_seed;
        let gs = 0x1234567890abcdefu64;
        assert_eq!(zone_seed(gs, &ZoneId::Town), zone_seed(gs, &ZoneId::Town));
        assert_ne!(zone_seed(gs, &ZoneId::Town), zone_seed(gs, &ZoneId::Forest));
        assert_ne!(zone_seed(gs, &ZoneId::Dungeon(1)), zone_seed(gs, &ZoneId::Dungeon(2)));
    }

    #[test]
    fn 버전이_다르게_저장되면_역직렬화_후_현재_버전과_달라진다() {
        let mut save = make_minimal_save();
        save.version = 999;
        let ron_str = ron::ser::to_string_pretty(&save, ron::ser::PrettyConfig::default()).unwrap();
        let restored: SaveData = ron::from_str(&ron_str).unwrap();
        assert_ne!(restored.version, SAVE_VERSION);
    }

    #[test]
    fn 퀘스트_상태의_단계와_스폰_기록은_세이브_왕복_후에도_보존된다() {
        let mut save = make_minimal_save();
        save.quest_state.phases.insert("gem_quest".to_string(), "active".to_string());
        save.quest_state.spawned.insert("gem_quest:eternal_gem".to_string());

        let ron_str = ron::ser::to_string_pretty(&save, ron::ser::PrettyConfig::default()).unwrap();
        let restored: SaveData = ron::from_str(&ron_str).unwrap();

        assert_eq!(restored.quest_state.phases.get("gem_quest").map(|s| s.as_str()), Some("active"));
        assert!(restored.quest_state.spawned.contains("gem_quest:eternal_gem"));
    }


    #[test]
    fn 명명된_존_설정은_세이브_왕복_후에도_보존된다() {
        let mut save = make_minimal_save();
        save.named_zones.zones.insert(
            "desert".to_string(),
            crate::modules::zone::NamedZoneEntry {
                generator: "forest".to_string(),
                origin: ZoneId::Town,
            },
        );

        let ron_str = ron::ser::to_string_pretty(&save, ron::ser::PrettyConfig::default()).unwrap();
        let restored: SaveData = ron::from_str(&ron_str).unwrap();
        let entry = restored.named_zones.zones.get("desert").expect("Named zone config should survive save/load");

        assert_eq!(entry.generator, "forest");
        assert_eq!(entry.origin, ZoneId::Town);
    }

    // ── 테스트 유틸 ──────────────────────────────────────────────────────────

    use crate::modules::map::{Map, MapTile, TileKind, MapGeneratorRegistry, MapResource, GlobalTurn, GlobalSeed, MAP_WIDTH, MAP_HEIGHT, ApplyMapEvent, PlayerActedEvent, tile_to_world_coords};
    use crate::modules::zone::{WorldState, NamedZoneConfig, NamedZoneEntry, ZonePersistence};
    use crate::modules::ui::minimap::DiscoveredMarkers;
    use crate::modules::quest::{QuestRegistry, QuestState};
    use crate::modules::combat::{CombatStats, Defeated};
    use crate::modules::player::{Player, PlayerProgress};
    use crate::modules::item::{PlayerInventory, PlayerEquipment};
    use std::sync::atomic::{AtomicU64, Ordering};

    static UNIQUE: AtomicU64 = AtomicU64::new(0);

    /// 충돌하지 않는 고유 임시 경로를 생성한다. 실제 `save/progress.ron` 은 절대 건드리지 않는다.
    fn 임시_경로(suffix: &str) -> String {
        let n = UNIQUE.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        std::env::temp_dir()
            .join(format!("bevy_rogue_save_test_{pid}_{n}_{suffix}"))
            .to_string_lossy()
            .into_owned()
    }

    fn 테스트_레지스트리() -> MapGeneratorRegistry {
        use crate::modules::map::generators::{bsp, forest, organic_village};
        let mut r = MapGeneratorRegistry::new();
        r.register(Box::new(bsp::BspGenerator));
        r.register(Box::new(forest::ForestGenerator));
        r.register(Box::new(organic_village::OrganicVillageGenerator));
        r
    }

    fn 타일_벡터(revealed_idx: &[usize], n: usize) -> Vec<MapTile> {
        let mut tiles = vec![MapTile::new(TileKind::Floor); n];
        for &i in revealed_idx {
            tiles[i].revealed = true;
        }
        tiles
    }

    // ── 순수 헬퍼: pack_revealed / apply_revealed ─────────────────────────────

    #[test]
    fn revealed_필드만_추출해_비트팩하고_다시_적용하면_원본_탐험상태가_복원된다() {
        let tiles = 타일_벡터(&[1, 3, 7], 10);
        let packed = pack_revealed(&tiles);
        let unpacked = unpack_b64(&packed, 10);
        for &i in &[1usize, 3, 7] { assert!(unpacked[i]); }
        for &i in &[0usize, 2] { assert!(!unpacked[i]); }

        let mut blank = vec![MapTile::new(TileKind::Floor); 10];
        apply_revealed(&mut blank, &unpacked);
        for &i in &[1usize, 3, 7] { assert!(blank[i].revealed); }
        assert!(!blank[0].revealed);
    }

    #[test]
    fn 적용할_불벡터가_타일보다_짧으면_겹치는_앞부분만_갱신된다() {
        let mut tiles = vec![MapTile::new(TileKind::Floor); 5];
        apply_revealed(&mut tiles, &[true, true]);
        for &i in &[0usize, 1] { assert!(tiles[i].revealed); }
        for &i in &[2usize, 3, 4] { assert!(!tiles[i].revealed); }
    }

    // ── 순수 헬퍼: unpack_b64 짧은 청크 break 분기 ────────────────────────────

    #[test]
    fn base64_길이가_4의_배수가_아니면_마지막_불완전_청크는_무시된다() {
        // 100 tiles → 20자 정상 base64. 뒤에 2자만 더 붙이면 마지막 청크(len 2 < 4)에서 break.
        let tiles: Vec<bool> = (0..100).map(|i| i % 3 == 0).collect();
        let mut encoded = pack_b64(&tiles);
        encoded.push_str("AB"); // 길이 22 — 마지막 청크는 2자뿐
        let decoded = unpack_b64(&encoded, 100);
        assert_eq!(decoded, tiles, "불완전 청크는 무시되고 앞부분이 그대로 디코딩돼야 한다");
    }

    // ── 순수 헬퍼: get_algo ───────────────────────────────────────────────────

    #[test]
    fn 명명존이_설정에_등록돼_있으면_그_생성기_이름을_쓴다() {
        let mut cfg = NamedZoneConfig::default();
        cfg.zones.insert("desert".to_string(), NamedZoneEntry {
            generator: "forest".to_string(),
            origin: ZoneId::Town,
        });
        let algo = get_algo(&ZoneId::Named("desert".to_string()), &cfg);
        assert_eq!(algo, "forest");
    }

    #[test]
    fn 명명존이_설정에_없으면_기본_생성기_bsp로_대체된다() {
        let cfg = NamedZoneConfig::default();
        let algo = get_algo(&ZoneId::Named("unknown".to_string()), &cfg);
        assert_eq!(algo, "bsp");
    }

    #[test]
    fn 명명존이_아니면_존_자체의_기본_생성기를_쓴다() {
        let cfg = NamedZoneConfig::default();
        assert_eq!(get_algo(&ZoneId::Town, &cfg), "organic_village");
        assert_eq!(get_algo(&ZoneId::Forest, &cfg), "forest");
        assert_eq!(get_algo(&ZoneId::Dungeon(1), &cfg), "bsp");
    }

    // ── 순수 헬퍼: restore_player_stats ───────────────────────────────────────

    #[test]
    fn 세이브의_플레이어_스탯이_전투_스탯에_그대로_복원된다() {
        let save = make_minimal_save();
        let mut stats = CombatStats { hp: 0, max_hp: 0, mp: 0, max_mp: 0, attack: 0, defense: 0 };
        restore_player_stats(&mut stats, &save);
        assert_eq!(stats.hp, 18);
        assert_eq!(stats.max_hp, 20);
        assert_eq!(stats.mp, 5);
        assert_eq!(stats.max_mp, 5);
        assert_eq!(stats.attack, 8);
        assert_eq!(stats.defense, 3);
    }

    // ── 순수 헬퍼: collect_revealed_by_zone ───────────────────────────────────

    #[test]
    fn 존별_탐험기록_수집은_캐시된_존과_현재_맵을_모두_포함한다() {
        let mut other = Map::new(10, 10);
        other.tiles = 타일_벡터(&[2], 100);
        let mut maps = HashMap::new();
        maps.insert(ZoneId::Forest, other);
        let world = WorldState { current: ZoneId::Town, maps };

        let mut current = Map::new(10, 10);
        current.tiles = 타일_벡터(&[9], 100);

        let result = collect_revealed_by_zone(&world, &current);
        assert!(result.contains_key(&ZoneId::Forest));
        assert!(result.contains_key(&ZoneId::Town));
        // 현재 존(Town)의 탐험기록은 current_map 기준
        let town_unpacked = unpack_b64(result.get(&ZoneId::Town).unwrap(), 100);
        assert!(town_unpacked[9]);
        let forest_unpacked = unpack_b64(result.get(&ZoneId::Forest).unwrap(), 100);
        assert!(forest_unpacked[2]);
    }

    #[test]
    fn 현재존이_캐시에도_있으면_현재_맵의_탐험기록으로_덮어쓴다() {
        // maps 에 current 존이 들어있어도 insert 로 current_map 기준으로 덮어써야 한다.
        let mut stale = Map::new(10, 10);
        stale.tiles = 타일_벡터(&[0], 100); // 오래된 기록
        let mut maps = HashMap::new();
        maps.insert(ZoneId::Town, stale);
        let world = WorldState { current: ZoneId::Town, maps };

        let mut current = Map::new(10, 10);
        current.tiles = 타일_벡터(&[5], 100); // 최신 기록

        let result = collect_revealed_by_zone(&world, &current);
        let town = unpack_b64(result.get(&ZoneId::Town).unwrap(), 100);
        assert!(town[5], "현재 맵의 최신 탐험기록이어야 한다");
        assert!(!town[0], "오래된 캐시 기록은 덮어써져야 한다");
    }

    // ── 순수 헬퍼: restore_map_for_zone ───────────────────────────────────────

    #[test]
    fn 맵_복원은_시드와_생성기를_설정하고_탐험기록을_적용하며_가시성을_끈다() {
        let registry = 테스트_레지스트리();
        let cfg = NamedZoneConfig::default();
        // 80×50 맵에 대해 인덱스 7 을 탐험으로 표시한 base64
        let mut bools = vec![false; MAP_WIDTH * MAP_HEIGHT];
        bools[7] = true;
        let encoded = pack_b64(&bools);

        let map = restore_map_for_zone(&registry, &cfg, 12345, &ZoneId::Forest, Some(&encoded));
        assert_eq!(map.algorithm, "forest");
        assert_eq!(map.seed, crate::modules::zone::zone_seed(12345, &ZoneId::Forest));
        assert!(map.tiles[7].revealed, "탐험기록이 적용돼야 한다");
        assert!(map.tiles.iter().all(|t| !t.visible), "모든 타일의 가시성이 꺼져야 한다");
    }

    #[test]
    fn 탐험기록이_없으면_맵을_생성만_하고_탐험은_비워둔다() {
        let registry = 테스트_레지스트리();
        let cfg = NamedZoneConfig::default();
        let map = restore_map_for_zone(&registry, &cfg, 999, &ZoneId::Town, None);
        assert_eq!(map.algorithm, "organic_village");
        assert!(map.tiles.iter().all(|t| !t.visible));
    }

    #[test]
    fn 알수없는_생성기면_빈_맵으로_복원한다() {
        // 레지스트리에 등록되지 않은 생성기를 명명존 설정으로 지정 → generate_with 가 None → 빈 맵
        let registry = 테스트_레지스트리();
        let mut cfg = NamedZoneConfig::default();
        cfg.zones.insert("ghost".to_string(), NamedZoneEntry {
            generator: "does_not_exist".to_string(),
            origin: ZoneId::Town,
        });
        let map = restore_map_for_zone(&registry, &cfg, 1, &ZoneId::Named("ghost".to_string()), None);
        assert_eq!(map.width, MAP_WIDTH);
        assert_eq!(map.height, MAP_HEIGHT);
        assert_eq!(map.algorithm, "does_not_exist");
        // 빈 맵은 모두 벽
        assert!(map.tiles.iter().all(|t| t.kind == TileKind::Wall));
    }

    // ── write_save_to ─────────────────────────────────────────────────────────

    #[test]
    fn 세이브_쓰기는_상위_디렉터리를_만들고_파일을_생성한다() {
        let dir = 임시_경로("dir");
        let path = format!("{dir}/progress.ron");
        let tmp = format!("{dir}/progress.ron.tmp");
        let save = make_minimal_save();

        write_save_to(&save, &path, &tmp);

        assert!(std::path::Path::new(&path).exists(), "세이브 파일이 생성돼야 한다");
        assert!(!std::path::Path::new(&tmp).exists(), "임시 파일은 rename 후 사라져야 한다");
        let content = std::fs::read_to_string(&path).unwrap();
        let restored: SaveData = ron::from_str(&content).unwrap();
        assert_eq!(restored.global_turn, 42);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 상위_디렉터리가_없는_단순_파일명이면_디렉터리_생성을_건너뛴다() {
        // tmp 의 parent 가 비어있는 경우 — create_dir_all 분기를 건너뜀
        let cwd = std::env::current_dir().unwrap();
        let n = UNIQUE.fetch_add(1, Ordering::Relaxed);
        let path = format!("bevy_rogue_bare_{}_{n}.ron", std::process::id());
        let tmp = format!("bevy_rogue_bare_{}_{n}.ron.tmp", std::process::id());
        let save = make_minimal_save();

        write_save_to(&save, &path, &tmp);

        let full = cwd.join(&path);
        assert!(full.exists(), "현재 디렉터리에 세이브가 생성돼야 한다");
        std::fs::remove_file(&full).ok();
        std::fs::remove_file(cwd.join(&tmp)).ok();
    }

    #[test]
    fn 디렉터리_생성이_실패하면_쓰기를_중단한다() {
        // 일반 파일을 parent 로 가지는 tmp 경로 → create_dir_all 실패
        let file = 임시_경로("blocker");
        std::fs::write(&file, "x").unwrap();
        let tmp = format!("{file}/sub/progress.ron.tmp"); // file 은 디렉터리가 아님
        let path = format!("{file}/sub/progress.ron");
        let save = make_minimal_save();

        write_save_to(&save, &path, &tmp); // panic 하지 않고 조용히 반환

        assert!(!std::path::Path::new(&path).exists());
        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn 임시파일_쓰기가_실패하면_교체를_중단한다() {
        // tmp 자체가 디렉터리로 존재 → std::fs::write 실패
        let tmpdir = 임시_경로("isdir");
        std::fs::create_dir_all(&tmpdir).unwrap();
        let path = 임시_경로("target");
        let save = make_minimal_save();

        write_save_to(&save, &path, &tmpdir); // tmp 가 디렉터리라 write 실패

        assert!(!std::path::Path::new(&path).exists());
        std::fs::remove_dir_all(&tmpdir).ok();
    }

    #[test]
    fn 임시파일을_최종경로로_교체하지_못하면_조용히_중단한다() {
        // path 가 이미 디렉터리로 존재 → rename 실패
        let dir = 임시_경로("renamefail");
        std::fs::create_dir_all(&dir).unwrap();
        let path = format!("{dir}/target_is_dir");
        std::fs::create_dir_all(&path).unwrap(); // 최종 경로가 디렉터리
        let tmp = format!("{dir}/progress.ron.tmp");
        let save = make_minimal_save();

        write_save_to(&save, &path, &tmp); // rename(tmp -> path/디렉터리) 실패

        // tmp 는 쓰였지만 rename 실패로 path(디렉터리)는 그대로 디렉터리
        assert!(std::path::Path::new(&path).is_dir());
        std::fs::remove_dir_all(&dir).ok();
    }

    // ── delete_save / delete_save_at ──────────────────────────────────────────

    #[test]
    fn 세이브_삭제는_존재하는_임시_세이브를_제거한다() {
        let path = 임시_경로("delete_me");
        std::fs::write(&path, "dummy").unwrap();
        assert!(std::path::Path::new(&path).exists());
        delete_save_at(&path);
        assert!(!std::path::Path::new(&path).exists());
    }

    #[test]
    fn 세이브_삭제는_파일이_없어도_패닉하지_않는다() {
        let path = 임시_경로("nonexistent");
        delete_save_at(&path); // 존재하지 않아도 조용히 무시
        assert!(!std::path::Path::new(&path).exists());
    }

    #[test]
    fn 기본_세이브_삭제_래퍼는_상수_경로를_사용한다() {
        // delete_save() 래퍼 자체의 커버리지 — 상수 경로(SAVE_PATH)에 위임한다.
        // 실제 save/progress.ron 을 파괴하지 않도록, 호출 전에 임시 백업으로 rename 해 둔다.
        // (파일이 없으면 rename 은 Err 를 반환하지만 무시한다 — 분기 없이 안전.)
        let backup = format!("{SAVE_PATH}.test_backup");
        let _ = std::fs::rename(SAVE_PATH, &backup);
        delete_save(); // 상수 경로 삭제 시도 — 파일 없으면 no-op
        assert!(!std::path::Path::new(SAVE_PATH).exists());
        // 원래 파일이 있었다면 복원한다 (없었으면 Err 무시).
        let _ = std::fs::rename(&backup, SAVE_PATH);
    }

    // ── 시스템 하네스: SavePlugin::build ──────────────────────────────────────

    #[test]
    fn 세이브_플러그인을_추가하면_세이브_설정_리소스가_초기화된다() {
        let mut app = App::new();
        app.add_plugins(SavePlugin);
        // build() 가 실행되며 SaveConfig 가 init 된다.
        assert!(app.world.get_resource::<SaveConfig>().is_some());
        let cfg = app.world.get_resource::<SaveConfig>().unwrap();
        assert_eq!(cfg.path, SAVE_PATH);
        assert_eq!(cfg.tmp, SAVE_TMP);
    }

    // ── 시스템 하네스: auto_save ──────────────────────────────────────────────

    /// auto_save 가 필요로 하는 모든 리소스를 채워넣은 App 을 만든다.
    fn auto_save_앱(config: SaveConfig) -> App {
        let mut app = App::new();
        app.add_event::<PlayerActedEvent>();
        app.insert_resource(PlayerInventory::default());
        app.insert_resource(PlayerEquipment::default());
        app.insert_resource(PlayerProgress::default());
        app.insert_resource(QuestState::default());
        app.insert_resource(QuestRegistry::default());
        app.insert_resource(WorldState::default());
        app.insert_resource(ZonePersistence::default());
        app.insert_resource(DiscoveredMarkers::default());
        app.insert_resource(NamedZoneConfig::default());
        app.insert_resource(GlobalTurn(7));
        app.insert_resource(GlobalSeed(0xabcdef));
        app.insert_resource(MapResource(Map::new(10, 10)));
        app.insert_resource(config);
        app.add_systems(Update, auto_save);
        app
    }

    #[test]
    fn 행동_이벤트가_없으면_자동저장은_파일을_쓰지_않는다() {
        let path = 임시_경로("noevent");
        let tmp = format!("{path}.tmp");
        let mut app = auto_save_앱(SaveConfig { path: path.clone(), tmp });
        // 플레이어는 있지만 이벤트가 없음
        app.world.spawn((
            Player,
            Transform::from_translation(tile_to_world_coords(2, 2).extend(0.0)),
            CombatStats { hp: 10, max_hp: 10, mp: 0, max_mp: 0, attack: 1, defense: 1 },
        ));
        app.update();
        assert!(!std::path::Path::new(&path).exists(), "이벤트 없으면 저장 안 함");
    }

    #[test]
    fn 행동_이벤트가_있어도_플레이어가_없으면_자동저장은_파일을_쓰지_않는다() {
        let path = 임시_경로("noplayer");
        let tmp = format!("{path}.tmp");
        let mut app = auto_save_앱(SaveConfig { path: path.clone(), tmp });
        app.world.send_event(PlayerActedEvent);
        app.update();
        assert!(!std::path::Path::new(&path).exists(), "플레이어 없으면 저장 안 함");
    }

    #[test]
    fn 행동_이벤트와_플레이어가_있으면_자동저장이_세이브_파일을_기록한다() {
        let path = 임시_경로("autosave_ok");
        let tmp = format!("{path}.tmp");
        let mut app = auto_save_앱(SaveConfig { path: path.clone(), tmp: tmp.clone() });
        // 혈흔도 하나 두어 blood_q 분기를 탄다
        app.world.spawn((
            BloodStain { alpha: 0.5, decay_per_turn: 0.1 },
            Transform::from_translation(tile_to_world_coords(3, 3).extend(0.0)),
        ));
        app.world.spawn((
            Player,
            Transform::from_translation(tile_to_world_coords(4, 4).extend(0.0)),
            CombatStats { hp: 12, max_hp: 20, mp: 2, max_mp: 3, attack: 9, defense: 4 },
        ));
        app.world.send_event(PlayerActedEvent);
        app.update();

        assert!(std::path::Path::new(&path).exists(), "자동저장이 파일을 만들어야 한다");
        let content = std::fs::read_to_string(&path).unwrap();
        let restored: SaveData = ron::from_str(&content).unwrap();
        assert_eq!(restored.player_hp, 12);
        assert_eq!(restored.player_tile, [4, 4]);
        assert_eq!(restored.global_turn, 7);
        // 현재 존 스냅샷에 혈흔이 포함됐는지 확인
        let snap = restored.zone_persistence.get(&ZoneId::Town).unwrap();
        assert_eq!(snap.blood_stains.len(), 1);

        std::fs::remove_file(&path).ok();
        std::fs::remove_file(&tmp).ok();
    }

    // ── 시스템 하네스: load_if_save_exists ────────────────────────────────────

    /// load_if_save_exists 가 필요로 하는 리소스를 모두 채운 App 을 만든다.
    fn load_앱(config: SaveConfig) -> App {
        let mut app = App::new();
        app.add_event::<ApplyMapEvent>();
        app.insert_resource(PlayerInventory::default());
        app.insert_resource(PlayerEquipment::default());
        app.insert_resource(PlayerProgress::default());
        app.insert_resource(QuestState::default());
        app.insert_resource(QuestRegistry::default());
        app.insert_resource(WorldState::default());
        app.insert_resource(ZonePersistence::default());
        app.insert_resource(DiscoveredMarkers::default());
        app.insert_resource(GlobalTurn(0));
        app.insert_resource(GlobalSeed(0));
        app.insert_resource(테스트_레지스트리());
        app.insert_resource(NamedZoneConfig::default());
        app.insert_resource(config);
        app.add_systems(Update, load_if_save_exists);
        app
    }

    #[test]
    fn 세이브_파일이_없으면_로드는_아무것도_하지_않는다() {
        let path = 임시_경로("missing");
        let mut app = load_앱(SaveConfig { path: path.clone(), tmp: format!("{path}.tmp") });
        app.world.spawn((
            Player,
            CombatStats { hp: 5, max_hp: 5, mp: 0, max_mp: 0, attack: 1, defense: 1 },
        ));
        app.update();
        // 글로벌 턴이 그대로면 로드가 발생하지 않은 것
        assert_eq!(app.world.resource::<GlobalTurn>().0, 0);
    }

    #[test]
    fn 세이브_파일이_손상돼_파싱에_실패하면_로드는_무시한다() {
        let path = 임시_경로("corrupt");
        std::fs::write(&path, "this is not valid ron )(").unwrap();
        let mut app = load_앱(SaveConfig { path: path.clone(), tmp: format!("{path}.tmp") });
        app.world.spawn((
            Player,
            CombatStats { hp: 5, max_hp: 5, mp: 0, max_mp: 0, attack: 1, defense: 1 },
        ));
        app.update();
        assert_eq!(app.world.resource::<GlobalTurn>().0, 0, "파싱 실패 시 로드 안 함");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn 세이브_버전이_다르면_로드는_무시한다() {
        let path = 임시_경로("badversion");
        let mut save = make_minimal_save();
        save.version = 999;
        let content = ron::ser::to_string_pretty(&save, ron::ser::PrettyConfig::default()).unwrap();
        std::fs::write(&path, content).unwrap();

        let mut app = load_앱(SaveConfig { path: path.clone(), tmp: format!("{path}.tmp") });
        app.world.spawn((
            Player,
            CombatStats { hp: 5, max_hp: 5, mp: 0, max_mp: 0, attack: 1, defense: 1 },
        ));
        app.update();
        assert_eq!(app.world.resource::<GlobalTurn>().0, 0, "버전 불일치 시 로드 안 함");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn 유효한_세이브가_있으면_리소스와_플레이어_스탯을_복원하고_맵을_적용한다() {
        let path = 임시_경로("valid");
        let mut save = make_minimal_save();
        save.global_turn = 99;
        save.global_seed = 0x1111;
        save.active_quests.insert("herb_quest".to_string());
        // 다른 존을 zone_revealed 에 추가 — current(Town) 이 아닌 항목 → filter 분기 True
        let mut bools = vec![false; MAP_WIDTH * MAP_HEIGHT];
        bools[3] = true;
        save.zone_revealed.insert(ZoneId::Forest, pack_b64(&bools));
        let content = ron::ser::to_string_pretty(&save, ron::ser::PrettyConfig::default()).unwrap();
        std::fs::write(&path, content).unwrap();

        let mut app = load_앱(SaveConfig { path: path.clone(), tmp: format!("{path}.tmp") });
        let player = app.world.spawn((
            Player,
            CombatStats { hp: 1, max_hp: 1, mp: 0, max_mp: 0, attack: 1, defense: 1 },
        )).id();
        app.update();

        // 리소스 복원 검증
        assert_eq!(app.world.resource::<GlobalTurn>().0, 99);
        assert_eq!(app.world.resource::<GlobalSeed>().0, 0x1111);
        // 플레이어 스탯 복원 (hp=18 > 0 이므로 Defeated 미부여)
        let stats = app.world.get::<CombatStats>(player).unwrap();
        assert_eq!(stats.hp, 18);
        assert_eq!(stats.max_hp, 20);
        assert!(!app.world.entity(player).contains::<Defeated>(), "hp>0 이면 Defeated 없음");
        // 활성 퀘스트 복원
        assert!(app.world.resource::<QuestRegistry>().active.contains("herb_quest"));
        // WorldState 의 current 와 이전 존 캐시 (Forest) 검증 — filter 가 Forest 만 남김
        let ws = app.world.resource::<WorldState>();
        assert_eq!(ws.current, ZoneId::Town);
        assert!(ws.maps.contains_key(&ZoneId::Forest), "현재 존이 아닌 존만 캐시돼야 한다");
        assert!(!ws.maps.contains_key(&ZoneId::Town), "현재 존은 캐시 맵에 없음 (apply 로 전달)");
        // ApplyMapEvent 발행 검증
        let events = app.world.resource::<bevy::ecs::event::Events<ApplyMapEvent>>();
        let mut reader = events.get_reader();
        let ev: Vec<_> = reader.read(events).collect();
        assert_eq!(ev.len(), 1, "맵 적용 이벤트가 하나 발행돼야 한다");
        assert_eq!(ev[0].spawn_pos, Some((2, 2)));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn hp가_0이하로_저장됐으면_로드시_즉시_패배_상태가_부여된다() {
        let path = 임시_경로("dead");
        let mut save = make_minimal_save();
        save.player_hp = 0; // 사망 상태로 저장
        let content = ron::ser::to_string_pretty(&save, ron::ser::PrettyConfig::default()).unwrap();
        std::fs::write(&path, content).unwrap();

        let mut app = load_앱(SaveConfig { path: path.clone(), tmp: format!("{path}.tmp") });
        let player = app.world.spawn((
            Player,
            CombatStats { hp: 10, max_hp: 10, mp: 0, max_mp: 0, attack: 1, defense: 1 },
        )).id();
        app.update();

        let stats = app.world.get::<CombatStats>(player).unwrap();
        assert_eq!(stats.hp, 0);
        assert!(app.world.entity(player).contains::<Defeated>(), "hp<=0 이면 Defeated 부여");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn 플레이어_엔티티가_없는_유효한_세이브는_스탯복원을_건너뛰고_나머지를_복원한다() {
        // player_q.get_single_mut() 가 Err → if let Ok 분기 False
        let path = 임시_경로("noplayer_load");
        let mut save = make_minimal_save();
        save.global_turn = 55;
        let content = ron::ser::to_string_pretty(&save, ron::ser::PrettyConfig::default()).unwrap();
        std::fs::write(&path, content).unwrap();

        let mut app = load_앱(SaveConfig { path: path.clone(), tmp: format!("{path}.tmp") });
        // 플레이어 엔티티를 스폰하지 않는다
        app.update();

        // 스탯 복원은 건너뛰지만 리소스는 복원된다
        assert_eq!(app.world.resource::<GlobalTurn>().0, 55);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn 활성퀘스트가_빈_세이브는_재롤된_활성퀘스트를_덮어쓰지_않는다() {
        // save.active_quests 가 비어있으면 quest_registry.active 를 보존한다 (if 분기 False)
        let path = 임시_경로("empty_quests");
        let save = make_minimal_save(); // active_quests 비어있음
        let content = ron::ser::to_string_pretty(&save, ron::ser::PrettyConfig::default()).unwrap();
        std::fs::write(&path, content).unwrap();

        let mut app = load_앱(SaveConfig { path: path.clone(), tmp: format!("{path}.tmp") });
        app.world.resource_mut::<QuestRegistry>().active.insert("rerolled".to_string());
        app.world.spawn((
            Player,
            CombatStats { hp: 1, max_hp: 1, mp: 0, max_mp: 0, attack: 1, defense: 1 },
        ));
        app.update();

        // 재롤된 값이 보존돼야 한다
        assert!(app.world.resource::<QuestRegistry>().active.contains("rerolled"));
        std::fs::remove_file(&path).ok();
    }
}
