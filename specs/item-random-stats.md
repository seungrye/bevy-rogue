# 무기/방어구 랜덤 스탯 + 레어도 등급 (파밍 시스템)

## 목적
무기·방어구가 드롭될 때 **티어별 범위 안에서 스탯을 랜덤**으로 굴리고, **롤 품질(범위 내 백분위)로 레어도 등급**(일반/고급/희귀/영웅/전설)을 부여한다. 같은 아이템도 운에 따라 성능과 등급이 달라지는 파밍 게임 느낌. 저티어 아이템이 초반에 과하게 강하지 않도록 티어로 범위를 통제.

## 1. 데이터 모델 (RON)

`WeaponDef`/`ArmorDef`/`WeaponMeta`/`ArmorMeta`:
- 무기: `attack_power: i32` → **`attack_power_min: i32`, `attack_power_max: i32`** + **`tier: u8`**(1~5). `element` 유지.
- 방어구: `defense_bonus: i32` → **`defense_bonus_min/max: i32`** + **`tier: u8`**.

`assets/items/weapons.ron`, `armors.ron` 갱신. (저장 호환: 이건 게임 데이터라 세이브 포맷과 무관.)

## 2. 레어도 등급 (롤 백분위 파생)

```rust
pub enum Rarity { Common, Uncommon, Rare, Epic, Legendary }
```
`Rarity::from_roll(rolled, min, max) -> Rarity`: 백분위 `p = (rolled - min) / (max - min)` (min==max면 0):
| 백분위 | 등급 | 한글 | 색 |
|--------|------|------|-----|
| [0, 0.40) | Common | 일반 | 회색 (0.7,0.7,0.7) |
| [0.40, 0.70) | Uncommon | 고급 | 초록 (0.3,0.9,0.3) |
| [0.70, 0.90) | Rare | 희귀 | 파랑 (0.3,0.5,1.0) |
| [0.90, 0.98) | Epic | 영웅 | 보라 (0.7,0.3,1.0) |
| [0.98, 1.0] | Legendary | 전설 | 금색 (1.0,0.8,0.2) |

`name_ko()`, `color()` 제공.

## 3. 롤된 스탯 저장 (추가 필드, 세이브 호환 `#[serde(default)]`)
- `Item`(월드 컴포넌트): `rolled_attack: Option<i32>`, `rolled_defense: Option<i32>`.
- `InventoryItem`: 동일 (serde default → 구 세이브는 None).
- `PlayerEquipment`: `weapon_rolled_attack: Option<i32>`, `armor_rolled_defense: Option<i32>` (serde default).

레어도는 저장하지 않고 rolled + 해당 kind 의 range 로 매번 계산.

## 4. 흐름
- `spawn_dropped_items`: Weapon/Armor 드롭 시 `rng.gen_range(min..=max)` 로 롤해 `Item.rolled_*` 에 저장. (Consumable/QuestItem 은 None.)
- `pickup_items`: `Item.rolled_*` → `InventoryItem.rolled_*` 이전.
- 장착(`ui/equipment.rs handle_equipment_input`의 Enter): 선택한 `InventoryItem.rolled_*` → `PlayerEquipment.weapon_rolled_attack`/`armor_rolled_defense` 복사. 해제 시 None.
- `effective_attack(eq, r)`: 무기 장착 시 `weapon_rolled_attack` 가 Some 이면 그 값, 없으면 그 무기 range 의 중앙값(또는 min). 무기 없으면 `PLAYER_ATK`.
- `effective_defense`: 동일 패턴, `PLAYER_DEF + (armor_rolled_defense 또는 중앙값)`.
- `weapon_attack`/`armor_defense_bonus`(registry 단일값): range 중앙값 반환으로 변경(기존 호출처: 장비창 표시·ranged 활공격). 또는 `weapon_attack_range` 추가.

## 5. 표시
- 드롭된 월드 아이템 글리프 색: 무기/방어구는 **레어도 색**(rolled+range 로 계산). 색 없으면(롤 없음) 기존 카테고리 색.
- 장비창(`ui/equipment.rs`): 장착/인벤토리 항목에 **[등급] 이름 (ATK/DEF 롤값)** 표시, 등급 색 적용.

## 6. 5티어 풀세트 (RON 내용)

**무기 (tier: id 범위 element)** — 기존 sword/spear/bow 는 티어 재배치(범위로):
- T1: 단검 `dagger` 3~6 none · 몽둥이 `club` 4~7 none · 검 `sword` 5~9 fire
- T2: 창 `spear` 8~12 ice · 활 `bow` 7~11 lightning · 도끼 `axe` 9~14 fire
- T3: 쇠뇌 `crossbow` 12~17 lightning · 지팡이 `staff` 11~16 ice · 전투도끼 `battle_axe` 13~18 fire
- T4: 대검 `greatsword` 17~24 fire · 전쟁해머 `war_hammer` 16~22 none · 마법활 `magic_bow` 15~21 lightning
- T5: 성검 `holy_sword` 24~34 fire · 용의창 `dragon_spear` 22~32 ice · 천둥활 `thunder_bow` 21~31 lightning

**방어구 (tier: id 범위)**:
- T1: 천갑옷 `cloth_armor` 1~2 · 가죽갑옷 `leather_armor` 2~4
- T2: 경갑 `light_armor` 3~5 · 사슬갑옷 `chain_mail` 4~7
- T3: 비늘갑옷 `scale_armor` 6~9 · 기사갑옷 `knight_armor` 7~11
- T4: 판금갑옷 `plate_armor` 10~14 · 용병갑주 `mercenary_armor` 9~13
- T5: 성기사갑옷 `paladin_armor` 14~20 · 용비늘갑주 `dragonscale_armor` 17~24

각 항목 glyph_ascii/unicode/game_icon + pickup_message 부여(카테고리 아이콘 재사용 가능). 플레이어 기본 ATK=5/DEF=1 기준 T1은 기본 근처, T5는 엔드게임.

## 7. 드롭 테이블 (`monster_drop_table`) — 기본(Phase 1)
- 고블린 → T1 무기/방어구 + 포션
- 오크 → T2 + 포션
- 트롤 → T3 (+ 낮은 확률 T4) + 포션
- T5 는 향후 보스/희귀 드롭(지금은 등록만, 미드롭 가능)

## 7-B. 레벨 스케일 드롭 (Phase 2 — 기본 기능 완성 후 적용)

플레이어 레벨에 따라 **드롭되는 장비의 티어 분포**를 조정한다. 저레벨에 고티어가 쉽게
안 나오고, 고레벨에 저티어가 과다하게 안 나오게.

- 입력: `PlayerProgress.level`(L). 드롭 시스템이 `Res<PlayerProgress>` 참조.
- **티어 밴드 중심**: `center = (1 + (L-1)/3).clamp(1,5)` (≈ 3레벨마다 +1티어). 튜닝 상수로 분리.
- **티어 가중치** `tier_weight(item_tier, L) -> f32`, `d = item_tier - center`:
  | d (티어−중심) | 가중치 | 의미 |
  |----|------|------|
  | ≥ +2 | 0.0 | 레벨 대비 너무 강함 → 드롭 안 됨(게이트) |
  | +1 | 0.5 | 한 단계 위 — 가끔 |
  | 0 | 1.0 | 적정 — 흔함 |
  | -1 | 0.6 | 한 단계 아래 — 유용 |
  | -2 | 0.3 | 두 단계 아래 |
  | ≤ -3 | 0.1 | 한참 아래 — 고레벨에선 드물게 |
- **드롭 흐름(개편)**: 장비 드롭이 결정되면 레지스트리의 무기/방어구를 **tier 로 그룹화**하고,
  각 후보 티어를 `tier_weight(t, L)` 로 가중 추첨 → 그 티어의 아이템 중 랜덤 선택 → 스탯 롤.
  (몬스터는 드롭 빈도/카테고리에 영향, 플레이어 레벨은 티어 분포에 영향 — 신규 아이템도 tier 만
  맞추면 자동 편입되는 데이터 주도 방식.)
- 결정 로직(`weighted_tier_pick`, `tier_weight`)은 **순수 함수로 분리**해 단위 테스트로
  경계(레벨↔중심, d 각 구간, 가중치 합 0 처리)를 전부 커버. rng 부분만 통계적 커버.
- 테스트: 저레벨에서 고티어 가중치 0, 고레벨에서 저티어 가중치 감소, center 계산 경계.

## 8. start_loadout
시작 무기(검/창/활)는 rolled 없이 지급 → effective 는 중앙값 사용. (또는 시작 시 롤 — 단순화로 중앙값.)

## 9. 테스트 (한글 의도서술형, 100% 커버리지 유지)
- `Rarity::from_roll` 각 구간 경계(0.40/0.70/0.90/0.98) 양쪽, min==max 처리.
- 드롭 시 rolled 가 [min,max] 범위 내, pickup 이 rolled 이전, 장착이 effective 에 반영, 해제 시 중앙값.
- effective_attack/defense: rolled 있음/없음, 무기/방어구 없음.
- 레어도 색/이름, 드롭 글리프 색.
- **기존 테스트 갱신**: `검 공격력 7`·`가죽 방어 2`·`effective_attack_with_sword_is_7`·`weapons_have_correct_attack_power` 등 고정값 단언 → 범위/중앙값 기준으로 수정.
- RON 로드: 신규 전 아이템 tier/range 파싱, 개수 검증.
- 세이브 호환: rolled 없는 구 InventoryItem/PlayerEquipment 역직렬화(None).
