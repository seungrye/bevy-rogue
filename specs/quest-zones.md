# 동적 퀘스트 존 시스템

## 목표

퀘스트 서사가 요구하는 "장소 전환" (붉은 사막, 콰스, 장벽 너머 등)을
실제 게임 맵으로 구현한다. 퀘스트 액션 하나로 현재 맵에 포탈이 생기고,
플레이어가 그 포탈로 들어가면 새 Named 존이 생성된다.

## 구성 요소

### ZoneId::Named(String)

기존 `Town / Forest / Dungeon(u32)` 외에 퀘스트가 동적으로 등록하는 존.
- `display_name()` → 존 이름 그대로 반환 (예: "사막")
- `algorithm()` → `"bsp"` 폴백 (실제 생성기는 NamedZoneConfig에서 조회)
- `zone_portals()` → 빈 vec (정적 포탈 없음; 귀환 포탈은 동적으로 스폰)

### NamedZoneConfig (Resource)

```rust
pub struct NamedZoneEntry { pub generator: String, pub origin: ZoneId }
pub struct NamedZoneConfig { pub zones: HashMap<String, NamedZoneEntry> }
```

퀘스트 포탈이 등록한 Named 존 목록. 프레임 전반에 걸쳐 영속.

### SpawnQuestPortalEvent (Event)

```
SpawnQuestPortalEvent { zone: String, generator: String }
```

`QuestAction::OpenPortal`이 발행 → `handle_spawn_quest_portal` 시스템이 처리.

### QuestAction::OpenPortal

```ron
OpenPortal(zone: "desert", generator: "desert_gen")
```

RON 퀘스트 파일에서 사용 가능한 새 액션.
- `NamedZoneConfig`에 존 등록
- 현재 맵에 보라색 `">"` 포탈 엔티티 스폰 (StairDown 방향, 보라색)

## 흐름

1. NPC 대화 마지막 줄 → `OpenPortal` 액션 실행
2. `SpawnQuestPortalEvent` 발행 → `handle_spawn_quest_portal`:
   - `NamedZoneConfig`에 `{ generator, origin = world.current }` 등록
   - 현재 맵에 `ZonePortal { target: Named("desert"), arrive_from: StairDown }` 스폰
3. 플레이어가 포탈에 접촉 → `ZoneTransitionEvent` → `handle_zone_transition`:
   - Named 존이면 `NamedZoneConfig`에서 생성기 조회
   - 맵 생성 후 캐시 → `ApplyMapEvent`
4. `spawn_portals_after_apply`:
   - 현재 존이 Named → `StairUp` 방향 귀환 포탈 스폰 (origin으로 복귀)
   - 현재 존이 Named 존들의 origin → 해당 Named 존 포탈 재스폰 (존 재방문 시)
5. Named 존 귀환 포탈 접촉 → origin 존으로 복귀, 기존 흐름 재개

## 포탈 색상

| 방향        | 색상    | 의미                      |
|-------------|---------|---------------------------|
| `South`     | 초록    | 인접 존 (마을 ↔ 숲)       |
| `North`     | 초록    | 인접 존 귀환               |
| `StairDown` | 노란색  | 던전 하강                  |
| `StairUp`   | 청록    | 던전 상승 / Named 존 귀환  |
| Named 포탈  | 보라색  | 퀘스트 전용 동적 포탈      |

## 사용 예 (RON)

```ron
"desert_crossing": QuestPhaseDef(
    dialog: ["붉은 사막 입구가 열렸다."],
    on_interact: [
        Log("포탈이 모래 속에서 솟아올랐다."),
        OpenPortal(zone: "desert", generator: "desert_gen"),
        AdvancePhase("waiting_desert"),
    ],
    ...
),
"waiting_desert": QuestPhaseDef(
    dialog: [...],
    auto_advance: [
        AutoAdvance(condition: InZone(Named("desert")), next_phase: "in_desert"),
    ],
    ...
),
```
