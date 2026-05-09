# 아이템·상점

게임 내 아이템(무기/방어구/소모품/퀘스트 아이템) 의 정의·드롭·상점 거래·
시작 로드아웃 정책. 콘텐츠는 RON 외부화 — 코드 재컴파일 없이 수정 가능.

## 아이템 종류와 상수

| 이름 | 종류 | 글리프 | 색상 | 효과 |
|------|------|--------|------|------|
| 체력 물약 | 소모품 | `!` | 초록 | HP +`POTION_HEAL`(8) |
| 검 | 무기 | `/` | 노랑 | ATK 7 (Fire) |
| 창 | 무기 | `|` | 노랑 | ATK 9 (Ice) |
| 활 | 무기 | `)` | 노랑 | ATK 5 (Lightning) |
| 가죽 갑옷 | 방어구 | `]` | 파랑 | DEF +2 |

상수: `Z_ITEM = 0.3` (바닥 0 위, 핏자국 0.5/몬스터 0.8 아래).

## 데이터 외부화

NPC, 퀘스트 아이템, 무기/방어구/소모품을 RON 으로 분리 — 콘텐츠 수정 시
재컴파일 불필요.

### Phase 1: NPC (`assets/villagers/villagers.ron`)
`VillagerDef { name, color, dialogs, quest_id, speed }`. `VillagerRegistry`
Resource 가 Startup 에 로드. `validate_quest_villager_refs` 가 모든
퀘스트의 `giver_npc` 가 registry 에 존재함을 검증.

### Phase 2: 퀘스트 아이템 (`assets/items/quest_items.ron`)
`QuestItemDef { id, display_name, glyph_*, pickup_message, image_path }`
(29 종). `QuestItemKind` 는 `pub struct QuestItemKind(pub &'static str)`
newtype — `Copy` 보존을 위해 String 대신 `&'static str` + `Box::leak`.
`validate_quest_item_refs` Startup 시스템이 spawns/GiveItem/RemoveItem 의
ID 가 registry 에 존재함을 검증.

### Phase 3: 무기/방어구/소모품
`weapons.ron`, `armors.ron`, `consumables.ron`. 각 `WeaponDef/ArmorDef/
ConsumableDef`. `WeaponKind/ArmorKind/ConsumableKind` 는 `&'static str`
newtype. 상수 `WeaponKind::SWORD/SPEAR/BOW`, `ArmorKind::LEATHER_ARMOR`,
`ConsumableKind::HEALTH_POTION` 호환 편의로 유지. 원소 (`fire/ice/lightning`)
는 RON string ID → elemental 모듈에서 enum 매핑.

### 통합 `ItemRegistry` Resource
`quest_items`, `weapons`, `armors`, `consumables` 4 개 HashMap. 메서드:
`quest_item(k)`, `weapon(k)`, `armor(k)`, `consumable(k)`,
`intern_*(id)`. `QuestItemRegistry` 는 type alias 로 호환 유지. 게임 로직
enum (`Element`, `ConsumableEffect`) 은 그대로 — RON 에서 enum variant 로.

## 아이템 드롭

몬스터 처치 시 `ItemDropEvent` 발행, 몬스터별 드롭 테이블 독립 확률 롤.

### 동작
- 드롭 아이템은 처치 타일 위에 현재 글리프 스타일로 표시 (Z=0.3).
- 플레이어가 타일로 이동하면 자동 수집 → 인벤토리 추가 + 로그.
- 체력 물약은 소모품 스택 누적, 장비 패널에서 사용 시 HP +8 (max 초과 X).
- 무기·방어구는 개별 인벤토리 항목으로 추가.
- 같은 타일 다중 아이템은 한 번에 하나씩 수집.

### 드롭 테이블 (각 항목 독립 확률)

| 몬스터 | 체력 물약 | 검 | 창 | 활 | 가죽 갑옷 |
|--------|-----------|----|----|----|-----------|
| 고블린 | 30% | 15% | - | - | - |
| 오크 | 40% | - | 20% | - | 10% |
| 트롤 | 50% | - | - | 25% | 20% |
| 기타 | 25% | - | - | - | - |

## 상점

마을 상인 NPC 와 상호작용해 구매·판매.

### 동작
- `상인` NPC 와 부딪히면 `ShopOpenEvent` 발행 → 상점 패널 열림.
- `Esc` 로 닫음. 열려 있는 동안 이동 입력 무시.
- 패널: 너비 280px, 다크 그린 배경. Tab 으로 구매/판매 탭 전환,
  ↑↓ 이동, Enter 실행. 하단에 보유 금화 표시.
- 구매: 금화 부족 시 불가, 성공 시 차감 + 인벤토리 추가.
- 판매: 인벤토리 내 퀘스트 아이템 제외 모든 아이템. 소모품은 1 개씩.
- 판매 항목 없으면 안내 메시지.

### 금화 (Gold)
- `PlayerInventory.gold: u32`. 시작 50G.
- `earn_gold(amount)` / `spend_gold(amount) -> bool`.

### `SHOP_CATALOG`
| 아이템 | 구매 | 판매 |
|--------|------|------|
| 체력 물약 | 50G | 25G |
| 검 | 100G | 50G |
| 창 | 150G | 75G |
| 활 | 80G | 40G |
| 가죽 갑옷 | 100G | 50G |

### 구현 위치
- `src/modules/ui/shop.rs` — `ShopPlugin`, 패널/입력.
- `villager::handle_bump` 의 `name == "상인"` 분기 → `ShopOpenEvent`.
- `ShopPanelOpen(bool)` 리소스, `ShopUiState { cursor, mode }`.
- 맵 생성기 단축키는 Tab → F1 (Tab 은 상점 탭 전환에 사용).

## 시작 로드아웃

새 게임 시작 시 인벤토리/장비/금화를 RON 으로 정의해 손쉽게 조정.

### `assets/items/start_loadout.ron`
```ron
StartLoadout(
    gold: 50,
    weapon: None,
    armor: None,
    items: ["sword", "spear", "bow"],
    consumables: [("health_potion", 10)],
)
```

- `weapon` / `armor` — `Option<String>` 장착 슬롯 ID. 없으면 미장착.
- `items` — 인벤토리에 들어갈 무기/방어구 ID 목록 (중복 = 여러 개).
  등록되지 않은 ID 는 warn 로그 후 스킵.
- `consumables` — `(id, count)` 목록.
- `gold` — 시작 금화.

### 적용 시점
1. 첫 실행 (세이브 없음) → `apply_start_loadout_if_no_save` (`PostStartup`).
2. 게임 오버 후 `R`/`N` → `reset_to_new_game` 안에서 default 초기화 후
   `apply_start_loadout` 호출.

세이브 로드 시는 미적용 (세이브 데이터 우선).

### 로딩 실패 fallback
RON 파일 없거나 파싱 실패 → `StartLoadout { gold: 50, ..Default::default() }`
+ warn 로그.

### 데이터 구조
```rust
#[derive(Debug, Deserialize, Clone, Default)]
pub struct StartLoadout {
    pub gold: u32,
    #[serde(default)] pub weapon: Option<String>,
    #[serde(default)] pub armor: Option<String>,
    #[serde(default)] pub items: Vec<String>,
    #[serde(default)] pub consumables: Vec<(String, u32)>,
}

#[derive(Resource, Default)]
pub struct StartLoadoutRegistry(pub StartLoadout);
```

## 테스트 전략
- registry 로드: 각 RON 파일이 파싱·검증 통과.
- 참조 무결성: 퀘스트 → NPC, 퀘스트 → 아이템, 상점 → 아이템 모두 해석.
- 렌더링 호환: 외부화 전후 glyph/display_name/color 동일.
- 저장/로드 호환: ID 기반 직렬화 — enum variant 변경에 영향 없음.
- 시작 로드아웃: RON 파싱, gold/장비/items/consumables 적용, 알 수 없는
  ID 스킵, 파일 없을 때 fallback.
