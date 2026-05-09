# 게임 데이터 외부 파일 분리

## 목적

NPC, 퀘스트 아이템, 무기/방어구/소모품 등 게임 콘텐츠 데이터를
Rust 코드의 enum/상수로 박지 말고 RON 에셋 파일로 빼낸다.
게임 코드는 엔진 프레임워크로만 동작하고, 콘텐츠 변경 시 Rust 재컴파일 없이
데이터 파일만 수정하면 되도록 한다.

## 현재 결합 문제

- 퀘스트 RON 의 `giver_npc: "그레체"` 가 `villager/mod.rs::VILLAGER_DATA` 의 이름과 일치해야 함
- 퀘스트 RON 의 `item: "prototype_hammer"` 가 `quest/mod.rs::item_id_to_kind` + `item/mod.rs::QuestItemKind` 두 곳에 존재해야 함
- 새 NPC/아이템 추가 시 매번 3개 이상의 Rust 파일 수정 필요
- 디자이너가 콘텐츠 이름·대사·글리프만 바꾸려 해도 Rust 재컴파일 필요

## Phase 1 — NPC 데이터 외부화 ✅

### 동작 명세

- [x] `assets/villagers/villagers.ron` 파일 생성 (단일 파일, `Vec<VillagerDef>`)
- [x] `VillagerDef` 구조: `{ name, color: (f32, f32, f32), dialogs: Vec<String>, quest_id: Option<String>, speed: f32 }`
- [x] `VillagerRegistry` Resource 도입 — Startup 단계에서 로드
- [x] `VILLAGER_DATA` 상수 제거 → registry 조회로 대체
- [x] `do_spawn` 이 registry 의 villager 중 `quest_id` 가 활성 퀘스트인 것만 스폰
- [x] 검증: 모든 퀘스트의 `giver_npc` 가 villager registry 에 존재해야 함 (없으면 startup 실패)
- [x] `VillagerSystemSet::Load` 추가 — `QuestSystemSet::Load` 와 함께 spawn 보다 먼저 실행

### 아키텍처

```
Startup:
  QuestSystemSet::Load        → load_quests
  VillagerSystemSet::Load     → load_villagers
  (둘 다 끝난 뒤)              → validate_quest_villager_refs
  draw_map                    → spawn_on_startup (Quest+Villager 둘 다 after)
```

## Phase 2 — 퀘스트 아이템 외부화 ✅

### 동작 명세

- [x] `assets/items/quest_items.ron` 파일 생성 — `Vec<QuestItemDef>` (29 종)
- [x] `QuestItemDef`: `{ id, display_name, glyph_ascii, glyph_unicode, glyph_game_icon, pickup_message, image_path }`
- [x] `QuestItemKind` enum 제거 → `pub struct QuestItemKind(pub &'static str)` newtype
- [x] `quest/mod.rs::item_id_to_kind` 의 QuestItem 분기 제거 — registry 조회로 대체
- [x] 전역 `QUEST_ITEMS: OnceLock<HashMap<&'static str, QuestItemMeta>>` 도입
- [x] 검증: `validate_quest_def` + `item_id_to_kind` 통해 quest item ID 존재 확인
- [x] 명시적 `validate_quest_item_refs` Startup 시스템 — Phase 1 의 `validate_quest_villager_refs` 와 대칭
- [x] 모든 quest 파일의 spawns/GiveItem/RemoveItem 의 ID 가 registry 에 존재함을 테스트로 검증

### 아키텍처 결정

- ItemKind 의 `Copy` 특성 유지를 위해 `String` 대신 `&'static str` 기반 newtype 사용
- 메타데이터는 RON 로드 시점에 `Box::leak` 으로 `&'static` 으로 영속화 (29 종 × 6 필드 ≈ 작은 leak)
- Bevy `Resource` 대신 전역 `OnceLock` 사용 — `ItemKind::glyph()` 등 메서드가 무인자로
  메타데이터에 접근할 수 있게 하여 호출부 변경 최소화 (~250 위치)
- `intern_quest_id(id)` 함수: 같은 ID 의 leak 된 `&'static str` 을 한 번만 만들어 반환

## Phase 3 — 무기/방어구/소모품 외부화

### 동작 명세

- [ ] `assets/items/weapons.ron`, `assets/items/armors.ron`, `assets/items/consumables.ron` 생성
- [ ] `WeaponDef`: `{ id, display_name, glyph, color, attack_power, element, range, image_path }`
- [ ] `ArmorDef`: `{ id, display_name, glyph, color, defense, image_path }`
- [ ] `ConsumableDef`: `{ id, display_name, glyph, color, effect: ConsumableEffect, image_path }`
- [ ] `ConsumableEffect`: enum `{ Heal(i32), ... }` — 효과 종류는 enum 유지 (게임 로직)
- [ ] `WeaponKind`/`ArmorKind`/`ConsumableKind` enum 제거
- [ ] `ItemKind::Weapon(String)`, `ItemKind::Armor(String)`, `ItemKind::Consumable(String)`
- [ ] `PlayerEquipment::weapon: Option<String>`, `armor: Option<String>`
- [ ] `weapon_element`, `weapon_attack` 등 함수가 registry 의 def 를 사용하도록 재작성
- [ ] 상점 catalog 도 RON 으로 외부화 (`assets/shop/catalog.ron`)
- [ ] 모든 테스트가 ID 기반으로 동작하도록 갱신

### 아키텍처

- `ItemRegistry` Resource (혹은 `WeaponRegistry`/`ArmorRegistry`/`ConsumableRegistry` 분리)
- 검증: 상점 catalog, 퀘스트 RewardItem 등에서 참조하는 모든 item_id 가 registry 에 존재
- 게임 로직 enum (`Element`, `ConsumableEffect`)은 유지 — RON 에서 enum variant 로 직렬화

## 단계별 진행

1. **Phase 1** 작성·테스트·커밋
2. **Phase 2** 작성·테스트·커밋
3. **Phase 3** 작성·테스트·커밋

각 단계마다 모든 기존 테스트가 통과해야 다음으로 진행한다.

## 테스트 전략

- **registry 로드 테스트**: 각 RON 파일이 파싱되고 검증 통과
- **참조 무결성 테스트**: 퀘스트 → NPC, 퀘스트 → 아이템, 상점 → 아이템 모두 해석됨
- **렌더링 호환성 테스트**: 외부화 전후 glyph/display_name/color 가 동일하게 나옴
- **저장/로드 호환성**: ID 기반으로 직렬화되어 enum variant 변경에 영향 없음
