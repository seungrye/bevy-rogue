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
        app.add_systems(PostStartup, load_if_save_exists)
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

    write_save(&save);
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

fn write_save(save: &SaveData) {
    let content = match ron::ser::to_string_pretty(save, ron::ser::PrettyConfig::default()) {
        Ok(s) => s,
        Err(e) => { error!("세이브 직렬화 실패: {e}"); return; }
    };
    if let Err(e) = std::fs::create_dir_all("save") {
        error!("save/ 디렉터리 생성 실패: {e}"); return;
    }
    if let Err(e) = std::fs::write(SAVE_TMP, &content) {
        error!("세이브 파일 쓰기 실패: {e}"); return;
    }
    if let Err(e) = std::fs::rename(SAVE_TMP, SAVE_PATH) {
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
    mut player_q: Query<(Entity, &mut CombatStats), With<Player>>,
    mut apply_ev: EventWriter<ApplyMapEvent>,
    mut commands: Commands,
) {
    let content = match std::fs::read_to_string(SAVE_PATH) {
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
    let _ = std::fs::remove_file(SAVE_PATH);
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
                items: vec![InventoryItem { kind: ItemKind::Weapon(WeaponKind::SWORD) }],
                consumables: vec![(ConsumableKind::HEALTH_POTION, 2)],
                gold: 75,
            },
            equipment: PlayerEquipment { weapon: Some(WeaponKind::SWORD), armor: None },
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
    fn pack_unpack_b64_roundtrip() {
        let tiles: Vec<bool> = (0..100).map(|i| i % 3 == 0).collect();
        let encoded = pack_b64(&tiles);
        // base64: ceil(ceil(100/8)/3)*4 = ceil(13/3)*4 = 5*4 = 20 chars
        assert_eq!(encoded.len(), 20);
        let decoded = unpack_b64(&encoded, 100);
        assert_eq!(decoded, tiles);
    }

    #[test]
    fn pack_b64_density() {
        // 80×50=4000 tiles → 500 bitpacked bytes → ceil(500/3)*4 = 668 base64 chars
        let tiles = vec![true; 4000];
        let encoded = pack_b64(&tiles);
        assert_eq!(encoded.len(), 668);
        // 원본 bool vec보다 약 6배 작음 (4000 vs 668)
        assert!(encoded.len() < tiles.len() / 5);
    }

    #[test]
    fn active_quests_serde_roundtrip() {
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
    fn save_data_legacy_format_without_active_quests_parses() {
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
    fn save_data_roundtrip_ron() {
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
    fn revealed_tiles_preserved_after_b64_roundtrip() {
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
    fn zone_seed_is_deterministic() {
        use crate::modules::zone::zone_seed;
        let gs = 0x1234567890abcdefu64;
        assert_eq!(zone_seed(gs, &ZoneId::Town), zone_seed(gs, &ZoneId::Town));
        assert_ne!(zone_seed(gs, &ZoneId::Town), zone_seed(gs, &ZoneId::Forest));
        assert_ne!(zone_seed(gs, &ZoneId::Dungeon(1)), zone_seed(gs, &ZoneId::Dungeon(2)));
    }

    #[test]
    fn version_mismatch_detectable() {
        let mut save = make_minimal_save();
        save.version = 999;
        let ron_str = ron::ser::to_string_pretty(&save, ron::ser::PrettyConfig::default()).unwrap();
        let restored: SaveData = ron::from_str(&ron_str).unwrap();
        assert_ne!(restored.version, SAVE_VERSION);
    }

    #[test]
    fn quest_state_phases_preserved() {
        let mut save = make_minimal_save();
        save.quest_state.phases.insert("gem_quest".to_string(), "active".to_string());
        save.quest_state.spawned.insert("gem_quest:eternal_gem".to_string());

        let ron_str = ron::ser::to_string_pretty(&save, ron::ser::PrettyConfig::default()).unwrap();
        let restored: SaveData = ron::from_str(&ron_str).unwrap();

        assert_eq!(restored.quest_state.phases.get("gem_quest").map(|s| s.as_str()), Some("active"));
        assert!(restored.quest_state.spawned.contains("gem_quest:eternal_gem"));
    }


    #[test]
    fn named_zone_config_preserved() {
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
}
