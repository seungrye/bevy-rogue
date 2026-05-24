# 퀘스트

데이터 주도 퀘스트 스크립팅 시스템 + 개별 퀘스트 콘텐츠 (프롤로그, 메인,
사이드).

## 시스템 개요

`assets/quests/*.ron` 파일로 퀘스트 정의. 런타임에 로드해 상태 머신으로
실행. 코드 수정·재컴파일 없이 퀘스트 추가·수정 가능.

### RON 파일 형식

```ron
QuestDef(
    id: "gem_quest",
    title: "잃어버린 보석",
    giver_npc: "elder",  // villagers.ron 의 VillagerDef.id
    initial_phase: "not_started",
    spawn_chance: 0.8,

    // phases 는 메타데이터(dialog, objective)만 담는다.
    phases: {
        "not_started": QuestPhaseDef(
            dialog: ["...", "..."],
            objective: Some("장로의 부탁을 들어보자."),
        ),
        "active": QuestPhaseDef(
            dialog: ["..."],
            objective: Some("던전 2층에서 영원의 보석을 찾자."),
        ),
        "ready": QuestPhaseDef(
            dialog: ["..."],
            objective: Some("장로에게 보석을 가져가자."),
        ),
        "done": QuestPhaseDef(dialog: ["..."], objective: Some("완료!")),
    },

    // 모든 상태 전환 로직은 평탄한 transitions 목록에 모은다.
    // 같은 (from, trigger) 그룹에서 순서대로 평가, 첫 매칭만 실행.
    transitions: [
        // NPC 마지막 대사 이후 Interact 시 평가
        Transition(from: "not_started", trigger: Interact, to: "active"),
        // 매 프레임 조건 자동 평가
        Transition(from: "active", trigger: Auto, when: HasItem("eternal_gem"), to: "ready"),
        // when 미지정 시 항상 매칭. actions 미지정 시 빈 목록.
        Transition(from: "ready", trigger: Interact,
            actions: [RemoveItem("eternal_gem"), GiveItem("philosophers_stone")],
            to: "done"),
    ],

    spawns: [
        QuestSpawn(
            phase: "active",
            item: "eternal_gem",
            zone: Dungeon(2),
        ),
    ],
)
```

## 데이터 타입

### `QuestCondition`
| 값 | 설명 |
|----|------|
| `HasItem("item_id")` | 인벤토리에 아이템 존재 |
| `InZone(ZoneId)` | 플레이어가 해당 존 |
| `PhaseIs { quest, phase }` | 다른 퀘스트의 현재 단계 |
| `HasFlag("flag")` | 플래그 존재 (값 무관) |
| `FlagIs { flag, value }` | 플래그가 특정 값 |
| `And([cond, ...])` | 모든 충족 |
| `Or([cond, ...])` | 하나 이상 |
| `Not(cond)` | 부정 |

### `QuestAction`
순수 사이드이펙트만 담당한다. 단계 이동은 `Transition.to` 가, 조건 분기는
`transitions` 목록 순서가 담당하므로 `AdvancePhase`·`Branch` 는 없다.

| 값 | 설명 |
|----|------|
| `GiveItem("item_id")` | 아이템 1개 지급 |
| `GiveItems { item, count }` | 수량 지정 지급 (소모품 자동 스택) |
| `RemoveItem("item_id")` | 인벤토리에서 제거 |
| `Log("message")` | 로그 메시지 |
| `SetFlag { flag, value }` | 플래그 설정 |
| `ClearFlag("flag")` | 플래그 해제 |
| `KillNpc("id")` | NPC 월드 제거 (`KillNpcEvent` → `handle_kill_npc`). 인자는 villager `id` (한글 name 아님 — unique 보장) |
| `OpenPortal { zone, generator, placement }` | Named 존 포털 스폰 |
| `ClosePortal("zone")` | Named 존 포털 / 등록 / 마커 정리 |
| `DespawnWorldItem("item_id")` | 월드 아이템 제거 (인벤토리 영향 X) |

### `QuestTransition`
| 필드 | 타입 | 기본 | 설명 |
|------|------|------|------|
| `from` | `String` | (필수) | 출발 phase |
| `trigger` | `TriggerKind` | (필수) | `Interact` (NPC 마지막 대사 후 상호작용) 또는 `Auto` (매 프레임 조건 평가) |
| `when` | `Option<QuestCondition>` | `None` | 없으면 항상 매칭 (unconditional) |
| `actions` | `Vec<QuestAction>` | `[]` | 전환 시 실행할 사이드이펙트 |
| `to` | `String` | (필수) | 도착 phase (`to == from` 이면 같은 phase 유지 — Log 전용 등) |

- 같은 `(from, trigger)` 그룹의 transition 들을 RON 목록 **순서대로** 평가,
  `when` 충족하는 **첫 번째만** 실행.
- `Interact` 와 `Auto` 는 독립적으로 평가된다 (서로 다른 시스템).
- `Auto` trigger 의 `actions` 는 `DespawnWorldItem`, `RemoveItem`, `SetFlag`
  만 허용 (`OpenPortal`/`KillNpc` 등은 `Interact` 전용). 시작 시 검증.
- **terminal phase** = 해당 phase 에서 시작하는 transition 이 하나도 없는 phase.

### `QuestSpawn`
| 필드 | 타입 | 기본 | 설명 |
|------|------|------|------|
| `phase` | `String` | (필수) | 이 단계일 때 스폰 |
| `item` | `String` | (필수) | 아이템 ID |
| `zone` | `ZoneId` | (필수) | 대상 존 |
| `count` | `u32` | `1` | 수량 |
| `condition` | `Option<QuestCondition>` | `None` | 추가 조건 |

**스폰 dedup**: `QuestState` 가 두 종류의 idempotency 키를 유지한다:
- `spawned: HashSet<"quest_id:item_id">` — 같은 퀘스트가 phase 들락날락에
  도 같은 item 을 한 번만 spawn (per-quest idempotency).
- `zone_spawned: HashSet<"zone_key:item_id">` — 같은 zone 에 같은 item ID
  는 한 인스턴스만. 여러 퀘스트가 같은 (zone, item) 에 spawn 을 시도하면
  먼저 처리된 퀘스트만 실제 spawn 하고 나머지는 skip (cross-quest dedup).
  `zone_key` 는 `format!("{:?}", zone_id)` (예: `"Dungeon(2)"`,
  `"Named(\"d_rank_dungeon\")"`).

결과: 같은 item ID 가 여러 퀘스트의 spawns 에 등장해도 zone 마다 한 인스
턴스만 등장. 픽업하면 모든 퀘스트의 `HasItem` 충족 — **직교성 보장** (각
퀘스트 RON 은 자기 책임만 명시; 다른 퀘스트의 spawn 을 알 필요 없음).
새 퀘스트 추가 시 기존 RON 안 건드림.

같은 (zone, item) 에 여러 인스턴스가 의도일 경우엔 한 spawn 의 `count`
필드를 사용한다 (다른 퀘스트로 분산하지 말 것).

`zone_spawned` 는 `#[serde(default)]` — legacy 세이브 호환.

### `RemoveItem` 작성 가이드

zone-단위 dedup 으로 같은 item 이 여러 퀘스트에 공유되므로, 한 퀘스트의
`RemoveItem` 이 다른 퀘스트의 진행을 막을 수 있다. 디자인 가이드:

1. **공유 가능성 있는 item** 은 `RemoveItem` 대신 `SetFlag`(예:
   `gem_delivered_to_elder`) 로 처리하면 인벤토리 유지 + NPC 가 "이미
   받음" 인식 가능.
2. **보상 교환** 이 의도라면 `RemoveItem` + 새 item `GiveItem` 패턴 (예:
   `eternal_gem` → `philosophers_stone`). 원본은 사라지지만 보상이 다른
   퀘스트의 자원이 될 수 있다 (현재 `gem_quest` 가 이 방식).
3. **진짜 회수** 가 서사상 의미 있을 때만 `RemoveItem` 단독 (예: 봉인
   의식에 마검을 바침 — `demonsword_quest`).
4. 두 퀘스트가 같은 item 을 모두 `RemoveItem` 한다면, player 는 양쪽
   조건이 모두 충족된 상태에서 NPC 방문 순서를 잡아야 둘 다 클리어 가능
   — 의도된 trade-off 면 spec 에 명시.

## 동작 명세

- 시작 시 `assets/quests/` 모든 `.ron` 로드 → `QuestRegistry` 등록.
- `QuestState` 리소스: `HashMap<quest_id, current_phase>` + `flags`.
- NPC 가 퀘스트 수여자 (`giver_npc`) 면 `QuestState` 에 따른 조건부 대화.
- 마지막 대화 줄에서 Interact (이동키/Esc) 시 현재 phase 의 `Interact`
  transition 을 순서대로 평가, 첫 매칭의 `actions` 실행 후 `to` 로 전환.
- **퀘스트 패널 등록 시점**: NPC 와 첫 부딪힘 만으로는 `QuestState.phases`
  에 등록되지 않는다. **`initial_phase` 의 마지막 대화 줄 + Interact**
  시점에 등록 + transition 평가. 즉 인사말만 듣고 퀘스트 패널
  (Q 키) 에 뜨는 일은 없다 — 핵심 의뢰 설명까지 다 들어야 등록된다.
  대화 진행은 `Villager.quest_dialogue_idx` 가 추적하므로 state 등록과
  무관하게 dialog 순회가 가능하다.
- **빈 `dialog` 규칙**: 코드의 마지막 대화 분기
  (`!dialog.is_empty() && idx + 1 >= dialog.len()`) 가 빈 dialog 면
  state 등록·Interact transition 실행으로 가지 않는다. 따라서:
  - `initial_phase` — **dialog 최소 1 줄 필수**. 빈 dialog 면 NPC 부딪힘
    이 영원히 등록되지 않는다.
  - **NPC 부딪힘으로 진행되는 phase** (`Interact` transition 존재) —
    dialog 최소 1 줄 필수. 빈 dialog 면 진행 액션이 트리거되지 않는다.
  - **terminal phase** (시작하는 transition 없음) — 빈 dialog 안전.
    그 phase 는 어차피 더 이상 진행 안 됨. 현재
    `targaryen_quest::conquest_begin`, `stark_quest::king_end`,
    `prologue_fog::{stark,targaryen,nightswatch}_end`,
    `jon_snow_quest::wildling_world` 가 이 패턴.
  - **`Auto` transition 만으로 진행되는 phase** — NPC 대화 없이 자동 전진
    이 의도면 빈 dialog 안전 (단 NPC 와 부딪혀도 어떤 메시지도 안 뜸).
- `Auto` transition (`check_auto_advance` 시스템) 은 매 프레임 현재 phase
  의 Auto transition 을 순서대로 평가, 첫 충족만 전진.
- 시작 시 RON 파싱·시맨틱 (transition from/to 참조, 아이템 ID,
  initial_phase, Auto 액션 허용 여부) 검증. 오류 시 `error!` 로그 후
  `std::process::exit(1)`.
- `PhaseIs` 는 `QuestState` 참조해 다른 퀘스트 단계 비교.
- 옛 포맷의 `Branch` 중첩은 `when` 조건이 다른 여러 transition 을 우선순위
  순으로 나열해 표현한다 (첫 매칭만 실행).
- `QuestSpawn` 은 해당 phase 활성 + 해당 zone 진입 시 스폰.
- 이미 수집한 퀘스트 아이템 재스폰 X (`QuestState.spawned` HashSet).
- 퀘스트 아이템은 플레이어 스폰 방 제외 랜덤 방의 랜덤 Floor —
  `UsedSpawnTiles` 공유로 중복 X.
- 진행상황은 `save/progress.ron` 자동 저장·복원.

## 등장 확률 (`spawn_chance`)

매 게임 시작 시 각 퀘스트를 `spawn_chance` 확률로 활성화 → 런마다 다른
조합으로 재플레이 가치 ↑.

- `QuestDef.spawn_chance: f32` (0.0~1.0, 기본 1.0).
- Startup 의 `load_quests` 가 각 퀘스트를 `rand() < spawn_chance` 로
  `QuestRegistry.active` 에 추가.
- 비활성 퀘스트는 NPC 스폰 X, 아이템 스폰 X, `auto_advance` 평가 X.
- 미지정 시 1.0.

| 퀘스트 | spawn_chance | 이유 |
|--------|:---:|------|
| `prologue_fog` | 1.0 | 항상 등장하는 프롤로그 |
| `gem_quest` | 0.8 | 기본 의뢰 |
| `herb_quest` | 0.8 | 기본 수집 |
| `alchemist_quest` | 0.7 | 중급 의뢰 |
| `parry_quest` | 0.75 | 모험 |
| `demonsword_quest` | 0.7 | 모험 |
| `stark_quest` | 0.6 | 스토리 이벤트 |
| `targaryen_quest` | 0.6 | 스토리 이벤트 |
| `jon_snow_quest` | 0.6 | 스토리 이벤트 |
| `world_fracture` | 0.5 | 희귀 엔드게임 |

### 아키텍처
- `QuestSystemSet::Load` — villager 스폰 순서 보장.
- `check_auto_advance` / `spawn_quest_items`: 비활성 퀘스트 스킵.
- `do_spawn`: `&QuestRegistry` 파라미터로 active 체크.

## 플래그 시스템

퀘스트 RON 안에서 임의의 이름 있는 상태값을 읽고 쓰기. NPC 관계, 감정,
세계 변형 (마을 소각, NPC 사망) 표현 → 비선형 서사.

- `QuestState.flags: HashMap<String, String>` — 자유 문자열 값 ("high",
  "alive", "true", 숫자 등).
- `SetFlag` / `ClearFlag` 는 transition 의 `actions` 안에서.
- `FlagIs` / `HasFlag` 는 transition 의 `when` 조건에서.
- 같은 플래그 여러 번 `SetFlag` 시 마지막 값으로 덮어씀.
- 존재하지 않는 플래그 `ClearFlag` 는 no-op.

## 동적 퀘스트 존 + 포털 정리

### `ZoneId::Named(String)`
기존 `Town/Forest/Dungeon(u32)` 외에 퀘스트가 동적으로 등록하는 존.
- `display_name()` → 그대로.
- `algorithm()` → `"bsp"` 폴백 (실제 생성기는 `NamedZoneConfig`).
- `zone_portals()` → 빈 vec (정적 포털 없음, 귀환 포털은 동적).

### `NamedZoneConfig` (Resource)
```rust
pub struct NamedZoneEntry { pub generator: String, pub origin: ZoneId }
pub struct NamedZoneConfig { pub zones: HashMap<String, NamedZoneEntry> }
```
퀘스트 포털이 등록한 Named 존 목록. 영속.

### `SpawnQuestPortalEvent`
```rust
SpawnQuestPortalEvent {
    zone: String, generator: String,
    placement: PortalPlacement, quest_id: String,
}
```
`QuestAction::OpenPortal` 발행 → `handle_spawn_quest_portal` 처리.

### 흐름
1. NPC 대화 마지막 → `OpenPortal` 액션.
2. `handle_spawn_quest_portal`:
   - `NamedZoneConfig` 에 `{ generator, origin = world.current }` 등록.
   - 현재 맵에 `ZonePortal { target: Named(zone), arrive_from: StairDown }`
     스폰 (`compute_portal_pos` 로 `PortalPlacement` 별 위치 결정).
3. 플레이어가 포털 접촉 → `ZoneTransitionEvent` →
   `handle_zone_transition`: Named 면 `NamedZoneConfig` 에서 생성기 조회.
4. `spawn_portals_after_apply`:
   - 현재 존이 Named → `StairUp` 방향 귀환 포털 (origin 으로).
   - 현재 존이 Named 들의 origin → 해당 Named 포털 재스폰 (재방문 시).
5. Named 존 귀환 포털 접촉 → origin 으로 복귀.

### 포털 색상

| 방향 | 색상 | 의미 |
|------|------|------|
| `South` / `North` | 초록 | 인접 존 (마을 ↔ 숲) |
| `StairDown` | 노랑 | 던전 하강 |
| `StairUp` | 청록 | 상승 / Named 귀환 |
| Named 포털 | 보라 | 퀘스트 전용 동적 |

### `PortalPlacement`
`OpenPortal { zone, generator, placement }` 의 placement 옵션. 기본
`InsideRoom`.

```rust
pub enum PortalPlacement {
    InsideRoom,                       // 기본 — 랜덤 방 floor
    Border,                           // 외곽선 가까운 floor (마을 입구)
    Random,                           // 맵 전체 랜덤 floor
    NearGiver { radius: usize },      // 퀘스트 giver NPC 반경 안 floor
}
```

- `InsideRoom` — 랜덤 방의 `random_floor_tile_in_room`. `map.rooms`
  비면 Random fallback.
- `Border` — 외곽 ring 부터 안쪽으로 BFS, 첫 Floor.
- `Random` — `random_floor_tile_anywhere`.
- `NearGiver` — giver 위치 ± radius 안 Floor 후보 중 랜덤. giver 위치 못
  찾으면 InsideRoom fallback.

`compute_portal_pos(map, placement, giver_pos, used, rng)` 가 위치 결정.
giver 위치는 `handle_spawn_quest_portal` 가 quest registry 의 `giver_npc`
이름으로 villager query 해서 transform → tile coord.

### 포털 닫기 (`ClosePortal`)
퀘스트 종료 후 더 이상 필요 없는 Named zone 포털 명시적 닫기.

`QuestAction::ClosePortal(zone: String)` → `CloseQuestPortalEvent` →
`handle_close_quest_portal`:
1. `target == ZoneId::Named(zone)` 인 모든 활성 `ZonePortal` entity
   despawn.
2. 모든 `ZonePersistence[*].portals` 에서 동일 target saved portal 제거
   (재방문 시 재스폰 방지).
3. `NamedZoneConfig.zones` 에서 zone 제거.
4. `DiscoveredMarkers` 에서 그 portal 위치 마커 제거.

`Auto` transition 에서는 미지원 (`Interact` transition 만 OK) — 일관성.

## 퀘스트 패널 (Q 키)

- `Q` 토글, 좌측 상단 고정. 너비 = 미니맵 폭 (`MINIMAP_DISPLAY_SIZE + 10
  = 190px`). 다크 그린 배경.
- `QuestState.phases` 에 등록된 퀘스트만 (NPC 첫 대화 이후).
- 표시: 제목 + 현재 `objective` + 완료 여부. 완료 (`done` phase) 는 흐린 색.

### 추가 필드
`QuestPhaseDef.objective: Option<String>` — 패널·로그에 표시할 목표.

## 목표 안내 강화

플레이어가 다음 행동을 판단할 수 있는 힌트.

### 목표 존 안내
- 현재 phase 의 퀘스트 스폰 `zone` 을 위치 힌트로.
- 현재 phase 에서 시작하는 `Auto` transition 의 `when` 안 `InZone` 도.
  중첩 `And`/`Or`/`Not` 안 `InZone` 도 탐색.
- 같은 존 한 번만.
- 목표 존 = `WorldState.current` 면 `현재 위치` 힌트 추가.

### 수집 진행도
- 현재 phase 퀘스트 스폰 기준 목표 아이템.
- 인벤토리의 같은 `ItemKind` 개수로 보유/필요.
- 미충족 = `진행`, 충족 = `완료`. 이미 스폰 완료 항목은 제외.

### NPC 대화 안내
- 현재 phase 에서 시작하는 `Interact` transition 이 있으면 퀘스트 제공자
  대화 힌트.
- 완료 phase 에서는 표시 X.
- 현재 존에 `QuestGiver` 마커 발견 시 `현재 존 / 미니맵 표시` 힌트 추가.

### 미니맵 마커
- `QuestTarget` 마커: 퀘스트 목표 아이템 — 자홍 색.
- 퀘스트 아이템이 월드에 스폰될 때 같은 타일에 `QuestTarget` 등록.
- `DiscoveredMarkers` 중복 방지 그대로.

## 퀘스트 아이템 회귀 픽스

### Wall 위 스폰 race condition
- 원인: `execute_apply` (맵 교체, `MapSystemSet::ExecuteRegen`) 와
  `spawn_quest_items` (`Update`, ordering 없음) 가 같은 frame 에 실행되면
  `spawn_quest_items` 가 옛 map 의 `rooms`/`tiles` 보고 좌표 결정. 새 map
  적용 후 그 좌표가 wall 일 수 있다.
- 수정:
  - `spawn_quest_items` 에 `.after(MapSystemSet::ExecuteRegen)` ordering.
  - 안전망: spawn 직전 `map.get_tile(tx, ty) == Floor` 검증, 아니면 스킵 +
    error.

## 퀘스트 아이템 획득 팝업

- chest "?" 트리거 심볼 제거 (trigger 모듈 삭제).
- 퀘스트 아이템 위 통과 시 자동 획득 + `QuestItemAcquiredEvent`. 픽업은
  이동 애니메이션 완료 (`PlayerSystemSet::MovementComplete`) 이후.
- 이미지 팝업 화면 중앙 (z-index 100).
- 팝업 닫기: 픽업 타일 (`tile_x`, `tile_y`) 저장, 매 프레임 플레이어
  위치와 비교해 벗어나면 닫음. Esc 즉시 닫기.
- 중복 스폰 방지 — 이벤트 루프 전 드레인 후 첫 번째만 처리, 팝업 존재
  여부는 루프 외부에서 한 번만 확인.
- `iter()` 순회로 복수 팝업 entity 도 모두 제거 (`get_single` 실패 방지).

### 이미지 매핑
`quest_item_image_path()` 함수에서 관리. 현재 `scene/open-chest.png`
공통, 추후 아이템별 교체.

## 저장 데이터에 active 보존

저장된 게임 로드 시 진행 중이던 퀘스트가 `spawn_chance` 재롤로 사라지지
않도록.

- `SaveData.active_quests: HashSet<String>` (`#[serde(default)]` 호환).
- 저장 시 `QuestRegistry.active` 클론 → `active_quests`.
- 로드 시 `QuestRegistry.active` 를 `save.active_quests` 로 덮어쓰기 —
  `load_quests` 가 `spawn_chance` 로 롤한 값 무시.

## 콘텐츠 — 개별 퀘스트

### 보석 퀘스트 (`gem_quest`)

```
[마을] 장로 대화 (not_started → active)
  ↓
[던전 2층] 영원의 보석 획득 (active → ready 자동)
  ↓
[마을] 장로 대화 (ready → done, 현자의 돌 수령)
```
- giver: 장로. 보상: `philosophers_stone`.

### 약초 퀘스트 (`herb_quest`)

엘렌이 마을 병자를 위한 은방울 뿌리 채집을 의뢰. 모든 시스템 기능 사용
예시:
- `QuestCondition`: HasItem, InZone, HasFlag, FlagIs, PhaseIs, And, Or, Not.
- `QuestAction`: GiveItem, GiveItems, RemoveItem, Log,
  SetFlag, ClearFlag, OpenPortal (`Border`), ClosePortal,
  DespawnWorldItem, KillNpc.
- `QuestSpawn`: count, condition.
- `Auto` transition 의 `actions`.
- `QuestPhaseDef.objective`.

흐름:
```
[마을] 엘렌 (not_started → travel) — SetFlag, OpenPortal(Border)
  ↓ Auto: InZone(herb_glade)
[숲속 공터] (travel → gathering) — 은방울 뿌리 10 + 독초 3 스폰
  ├ Auto: 독초 주움 → poisoned_warning → 해독 후 gathering
  ├ Auto: 뿌리 주움 → collected
  └ Auto: 독초술사 처치 + 마을 귀환 → collected
  ↓
[마을] 엘렌 (collected → done | done_with_hint)
  보상 분기를 우선순위 Interact transition 으로 표현 (cross-product):
    독초술사 처치(A) > 독초 지식(B), 그리고 gem_quest=active(C) 여부
  공통 actions: RemoveItem + 보상 + ClearFlag + (KillNpc) + ClosePortal
  C 충족 시 SetFlag(ellen_gave_hint) + → done_with_hint, 아니면 → done
```

### 프롤로그 — 안개 속의 발자국 (`prologue_fog`)

기억 잃은 채 시작. 무기 + 가치관 조합으로 3 각성 루트 결정.
- giver: 부상당한 병사 (각성 후 `KillNpc` 로 소멸).

#### 1단계: 무기 (본능 선택)
던전 1 에 세 무기 스폰. 하나 집는 즉시 나머지 둘 `DespawnWorldItem`.

| 아이템 | item_id | 의미 |
|--------|---------|------|
| 대검 | `prologue_greatsword` | 근력·명예 → 스타크 |
| 단검+투척물 | `prologue_daggers` | 민첩·실리 → 나이트워치 |
| 부러진 활+횃불 | `prologue_bowtorch` | 원거리·생존 → 타르가르옌 |

#### 2단계: 가치관 (병사 대화)

| 무기 | 가치관 플래그 |
|------|-------------|
| 대검 | `values = "honor"` |
| 단검 | `values = "pragmatism"` |
| 활+횃불 | `values = "survival"` |

#### 각성 (3 루트)

| 루트 | 조건 | 각성 NPC | 보상 |
|------|------|----------|------|
| 스타크 | greatsword + honor | 에다드 | `ice_sword` |
| 타르가르옌 | bowtorch + survival | 대너리스 | `dragon_egg` |
| 나이트워치 | 그 외 | 존 스노우 | `ghost_wolf` |

`flags["character"] = "stark" | "targaryen" | "jon_snow"`.

페이즈: `dormant → weapon_hunt → soldier_test_{...} → crest_hunt →
awakening_ready → {루트}_dawn → {루트}_end (terminal)`.

스폰: `prologue_greatsword`, `prologue_daggers`, `prologue_bowtorch` →
Dungeon(1). `family_crest` → Dungeon(1).

설계: 명시적 선택지 없음 (행동이 곧 선택). Log 3연속으로 각성 연출.
KillNpc 로 병사 영구 제거.

### 각성 루트 퀘스트

`prologue_fog` 완료 후 `flags["character"]` 로 자동 활성화. `dormant`
페이즈에서 `FlagIs(character, ...)` auto_advance.

| 루트 | 퀘스트 ID | NPC | 핵심 서사 |
|------|----------|-----|-----------|
| 스타크 | `stark_quest` | 캣린 | 제이미 라니스터 생포 + 대관식 |
| 타르가르옌 | `targaryen_quest` | 조라 | 드래곤 해방 + 드라카리스 |
| 나이트워치 | `jon_snow_quest` | 샘웰 | 화이트 워커 탈출 + 이그리트 조우 |

각 9 단계. 전투 기믹 (광역 휘두르기, 드라카리스, 협공) 은 실제 메카닉
없이 `Log` 액션 연속으로 묘사.

#### `stark_quest` 보상
`jaime_sword`, `kings_north_crown`, `flags["title"] = "king_in_the_north"`.

#### `targaryen_quest` 보상
`warlock_key`, `dragon_chain`, `essos_sail_map`,
`flags["dracarys_learned"] = "true"`.

#### `jon_snow_quest` 보상
`dragonglass_arrows`, `rangers_note`, `ygritte_bow`,
`flags["wildling_contact"] = "ygritte"`.

### 마검 퀘스트 (`demonsword_quest`)

성기사단 노기사 바스티안이 봉인 풀린 마검 위험을 알림. 마귀 동굴에서
마검 + 엘레나의 메모, 폐허 요새에서 고대 의식서.

| NPC | 위치 | 역할 |
|-----|------|------|
| 바스티안 | 마을 | 퀘스트 수여자 |
| 엘레나 | 동굴 | 의식서 위치 메모 (아이템) |

| item_id | 표시 이름 | 위치 | 역할 |
|---------|----------|------|------|
| `demon_sword` | 마검 | `Named("demon_cave")` | 봉인 재료 |
| `elenas_memo` | 엘레나의 메모 | `Named("demon_cave")` | 단서 |
| `ancient_ritual_book` | 고대 의식서 | `Named("ruined_fortress")` | 봉인 재료 |

존: `demon_cave` (cellular_automata), `ruined_fortress` (bsp_indoor).

페이즈:
```
not_started → awaiting_cave (Interact: OpenPortal demon_cave + 스폰)
awaiting_cave → cave_done [Auto: HasItem(demon_sword) AND HasItem(elenas_memo)]
cave_done → awaiting_fortress (Interact: OpenPortal ruined_fortress + 스폰)
awaiting_fortress → ritual_ready [Auto: HasItem(ancient_ritual_book)]
ritual_ready → done (Interact when 두 아이템 보유: RemoveItem×2 + Log 희생)
          → ritual_ready (Interact fallback: 재료 부족 힌트 self-loop)
```

설계: 엘레나/루시퍼는 NPC 대신 아이템 (Named 존 NPC 스폰 불가). 조건부
Interact transition + RemoveItem + Log 조합으로 희생 표현.

### 패리 퀘스트 (`parry_quest`) — 浦島太郎なおっさん 각색

기계공학자 그레체가 시제 무기 '파암추' 테스트 파일럿 모집. D 급 던전에서
강철 갑주 보스 격파 후 채용.

| NPC | 위치 | 역할 |
|-----|------|------|
| 그레체 | 마을 | 퀘스트 수여자 |

| item_id | 표시 이름 | 획득 |
|---------|----------|------|
| `prototype_hammer` | 시제 6식 파암추 | 그레체 지급 |
| `steel_core` | 강철 갑주 심장 | `Named("d_rank_dungeon")` 스폰 |
| `pilot_badge` | 전속 파일럿 인증서 | 보상 |

존: `d_rank_dungeon` (bsp).

페이즈:
```
not_started → dungeon_ready (Interact: 그레체 GiveItem hammer + OpenPortal + 스폰)
dungeon_ready → boss_defeated [Auto: HasItem(steel_core)]
boss_defeated → done (Interact: RemoveItem hammer/core, GiveItem pilot_badge)
```

설계: 패리 메카닉은 추가 안 함 — Log 메시지로 묘사. 보스 격파 = steel_core
습득. 그레체는 마을 NPC (Named 존 NPC 스폰 불가 우회).

### 봉인의 각성 (`world_fracture`) — Giga 메인 퀘스트

비선형 멀티 엔딩. 4 성물 (영원의 보석, 현자의 돌, 용비늘, 고대 주문서)
수집 + gem_quest / alchemist_quest 진행 상태 + 보유 아이템 조합으로
5 결말 분기.

- giver: 노인 (`world_fracture` quest_id).
- 선행: `gem_quest` 완료 (`dormant → awakened` 조건).

#### 성물

| 아이템 | 획득 |
|--------|------|
| `eternal_gem` | 던전 2층 |
| `philosophers_stone` | gem_quest 보상 (장로 교환) |
| `dragon_scale` | 던전 2층 |
| `ancient_scroll` | 던전 1층 |

#### 페이즈 (25 단계)

```
dormant ─[Auto: gem_quest.done]→ awakened
awakened ─[Interact]→ need_alchemist | prologue_done
need_alchemist ─[Auto: alchemist 시작]→ prologue_done
prologue_done ─[Interact]→ gathering_all

gathering_all  ← 핵심 수집
  ├ [Auto 1] 4성물 + gem_done + alchemist_legendary → legendary_ready
  ├ [Auto 2] 4성물 + alchemist_normal              → normal_ready
  ├ [Auto 3] 4성물                                 → all_gathered
  ├ [Auto 4] 현자의 길 (gem+stone only)            → wisdom_path_entry
  ├ [Auto 5] 전사의 길 (scale+scroll only)         → warrior_path_entry
  ├ [Auto 6-9] 힌트 페이즈 4종                     → missing_*_hint
  └ [Auto 10-11] 초기 힌트                         → deep_dungeon_hint / get_stone_hint

missing_*_hint / deep_dungeon_hint / get_stone_hint ─[Auto]→ gathering_all 복귀
{wisdom,warrior}_path_entry ─[Interact]→ {wisdom,warrior}_ritual_choice | 복귀
{wisdom,warrior}_ritual_choice ─[Interact]→ {wisdom,warrior}_ending | self-loop
all_gathered ─[Interact]→ ritual_confirmation | ritual_now_or_wait
ritual_now_or_wait ─[Interact]→ ritual_confirmation | incomplete_ending | self-loop
ritual_confirmation ─[Interact]→ legendary_ending | normal_ending | gathering_all
{legendary,normal}_ready ─[Interact]→ {legendary,normal}_ending | gathering_all
```

#### 5 결말

| 결말 | 조건 |
|------|------|
| `legendary_ending` | 4성물 + gem_done + alchemist_legendary |
| `normal_ending` | 4성물 + alchemist_normal/legendary |
| `incomplete_ending` | 4성물, alchemist 미완료 (강행) |
| `wisdom_ending` | 보석 + 현자의 돌 (gem 전용 경로) |
| `warrior_ending` | 용비늘 + 주문서 (alchemist_legendary 전용) |

#### 비선형성 설계
- `Auto` transition 11 우선순위 — 충족 즉시 자동 전환.
- 옛 3 단계 중첩 Branch → 우선순위 Interact transition 목록으로 평탄화.
- 교차 참조: `PhaseIs(quest: "gem_quest")`, `PhaseIs(quest:
  "alchemist_quest")`.
- 대안 경로 (현자/전사) — 4 성물 없이도 2 성물로 클리어.
- `alchemist_quest` 완료 수준 (normal vs legendary) 으로 결말 품질 차이.

#### 스폰

| 아이템 | 존 | 키 |
|--------|-----|----|
| `eternal_gem` | Dungeon(1) | `world_fracture_gem_d1` |
| `eternal_gem` | Dungeon(2) | `world_fracture_gem_d2` |
| `dragon_scale` | Dungeon(2) | `world_fracture_scale` |
| `ancient_scroll` | Dungeon(1) | `world_fracture_scroll` |
| `ancient_scroll` | Forest | `world_fracture_scroll_forest` |

**`gem_quest` / `alchemist_quest` 와의 spawn 공유**

`world_fracture` 의 `eternal_gem` / `dragon_scale` / `ancient_scroll` 는
`gem_quest` / `alchemist_quest` 의 spawn 과 동일 zone 에 등장한다. 시스템
의 zone-단위 dedup (`QuestSpawn` 절 참조) 으로 zone 마다 **한 인스턴스만**
실제로 spawn 되며, 어느 퀘스트가 먼저 처리되든 player 가 그 인스턴스를
픽업하면 두 퀘스트의 `HasItem` 모두 충족.

이는 직교성 설계:
- 각 퀘스트 RON 은 자기 spawn 만 명시 — 다른 퀘스트의 존재를 모름.
- `world_fracture` 단독 실행 (alchemist/gem 비활성) 시에도 자체 spawn 으로
  진행 가능.
- 두 퀘스트 모두 활성이면 한 던전에 한 외형, 시각 일관성 유지.

**RemoveItem 충돌 (디자인 trade-off)**: dedup 으로 한 인스턴스만 등장하므
로 한 퀘스트가 `RemoveItem` 으로 회수하면 다른 퀘스트는 그 시점 이후 진
행 불가. player 는 양쪽 조건을 모두 충족시킨 상태에서 두 NPC 를 차례로
방문해야 둘 다 클리어 가능. RemoveItem 사용 가이드는 별도 절 참조.
