# 게임 시작 기본 로드아웃

## 배경
새 게임 시작 시 인벤토리/장비/금화는 `PlayerInventory::default` /
`PlayerEquipment::default` 상수로 결정된다 (gold 50, 빈 인벤토리).
사용자가 RON 로 손쉽게 조정할 수 있게 외부화한다.

## 동작 명세

### `assets/items/start_loadout.ron` 신설
```ron
StartLoadout(
    gold: 50,
    weapon: None,
    armor: None,
    items: ["sword", "spear", "bow"],
    consumables: [("health_potion", 10)],
)
```

- `weapon` / `armor` — `Option<String>` (id, weapons.ron / armors.ron 의
  식별자). 장착 슬롯. `None` 이면 미장착.
- `items` — 인벤토리에 들어갈 무기/방어구 id 목록. 동일 id 가 여러 번
  등장하면 그 수만큼 추가. 무기/방어구 등록되지 않은 id 는 경고 로그
  후 스킵.
- `consumables` — `(id, count)` 목록. 등록되지 않은 id 는 스킵.
- `gold` — 시작 금화.

### 적용 시점
1. 첫 실행 (세이브 없음) → 시작 시 적용.
2. 게임 오버 후 `R`/`N` 으로 새 게임 → `reset_to_new_game` 안에서 적용.

세이브 로드 시는 적용하지 않는다 (세이브 데이터 우선).

### 로딩 실패 fallback
RON 파일 없거나 파싱 실패 시 `PlayerInventory::default` /
`PlayerEquipment::default` 사용 + warn 로그.

## 구현 메모

### 데이터 구조 (`src/modules/item/mod.rs` 또는 새 파일)
```rust
#[derive(Debug, Deserialize, Clone)]
pub struct StartLoadout {
    pub gold: u32,
    pub weapon: Option<String>,   // 무기 id (weapons.ron)
    pub armor: Option<String>,    // 방어구 id
    #[serde(default)]
    pub items: Vec<String>,       // 인벤토리 무기/방어구 id 목록
    #[serde(default)]
    pub consumables: Vec<(String, u32)>,
}

#[derive(Resource)]
pub struct StartLoadoutRegistry(pub StartLoadout);
```

### 로딩
`load_start_loadout()` — `assets/items/start_loadout.ron` 읽기.
실패 시 기본값 (`StartLoadout { gold: 50, ..Default::default() }`) 반환.

### 적용
`apply_start_loadout(inv, eq, loadout, items_registry)`:
- `inv` 와 `eq` 를 default 로 초기화 후 loadout 적용.
- weapon/armor id → registry 조회 후 `equipment.weapon = Some(WeaponKind(id))`.
- items: weapon/armor 에 모두 검색해 `InventoryItem` 으로 push.
- consumables: `add_consumable` 호출.

### 적용 위치
- `ItemPlugin::build` 에 `load_start_loadout` 호출, Resource 등록.
- 게임 시작 (Startup 시스템 + 세이브 없음 분기) 에 적용.
- `game_over::reset_to_new_game` 의 `*params.inventory = PlayerInventory::default();`
  뒤에 `apply_start_loadout` 호출.

## 테스트
- RON 파싱 — 위 형식 그대로 deserialize 되는지.
- `apply_start_loadout` — gold/weapon/armor/items/consumables 모두 적용.
- 등록되지 않은 id 는 스킵 (warn).
- 파일 없을 때 fallback 기본값 반환.
