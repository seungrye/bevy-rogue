# 퀘스트 스크립팅 시스템

## 목적

`assets/quests/*.ron` 파일로 퀘스트를 정의하고,
런타임에 로드해 상태 머신으로 실행한다.
코드 수정·재컴파일 없이 퀘스트 추가·수정이 가능하다.

## RON 파일 형식

```ron
// assets/quests/gem_quest.ron
QuestDef(
    id: "gem_quest",
    title: "잃어버린 보석",
    giver_npc: "장로",
    initial_phase: "not_started",

    phases: {
        "not_started": QuestPhaseDef(
            dialog: [
                "오, 모험가여. 부탁이 있소.",
                "던전 2층에 '영원의 보석'이 숨겨져 있다오.",
                "그것을 찾아 내게 가져다 주겠소?",
            ],
            on_interact: [AdvancePhase("active")],
            auto_advance: None,
        ),
        "active": QuestPhaseDef(
            dialog: ["아직 보석을 찾지 못했나요? 던전 2층을 살펴보시오."],
            on_interact: [],
            auto_advance: Some(AutoAdvance(
                condition: HasItem("eternal_gem"),
                next_phase: "ready",
            )),
        ),
        "ready": QuestPhaseDef(
            dialog: [
                "오! 보석을 찾아왔군요!",
                "약속대로 '현자의 돌'을 드리겠소.",
            ],
            on_interact: [
                RemoveItem("eternal_gem"),
                GiveItem("philosophers_stone"),
                AdvancePhase("done"),
            ],
            auto_advance: None,
        ),
        "done": QuestPhaseDef(
            dialog: ["덕분에 마을이 평화로워졌소. 감사합니다."],
            on_interact: [],
            auto_advance: None,
        ),
    },

    // 퀘스트 활성 중 특정 존에 아이템 스폰
    spawns: [
        QuestSpawn(
            phase: "active",
            item: "eternal_gem",
            zone: Dungeon(2),
            // count: 1,          ← 기본값 1, 생략 가능
            // condition: None,   ← 기본값 None, 생략 가능
        ),
    ],
)
```

## 데이터 타입

### QuestCondition
| 값 | 설명 |
|----|------|
| `HasItem("item_id")` | 플레이어 인벤토리에 해당 아이템 존재 |
| `InZone(ZoneId)` | 플레이어가 해당 존에 있음 |
| `PhaseIs { quest: "id", phase: "id" }` | 다른 퀘스트의 현재 단계 확인 |
| `And([cond, ...])` | 모든 조건 충족 |
| `Or([cond, ...])` | 하나 이상 조건 충족 |
| `FlagIs { flag, value }` | 퀘스트 플래그가 특정 값인지 확인 |
| `HasFlag("flag")` | 퀘스트 플래그가 존재하는지 확인 |
| `Not(cond)` | 조건 부정 |

### QuestAction
| 값 | 설명 |
|----|------|
| `AdvancePhase("phase_id")` | 이 퀘스트의 현재 단계를 지정 단계로 이동 |
| `GiveItem("item_id")` | 플레이어에게 아이템 1개 지급 |
| `GiveItems { item, count }` | 아이템을 수량 지정하여 지급 (소모품은 자동 스택) |
| `RemoveItem("item_id")` | 플레이어 인벤토리에서 아이템 제거 |
| `Log("message")` | 로그 창에 메시지 출력 |
| `SetFlag { flag, value }` | 퀘스트 플래그 설정 |
| `ClearFlag("flag")` | 퀘스트 플래그 해제 |
| `KillNpc("name")` | NPC 월드 제거 |
| `OpenPortal { zone, generator }` | 현재 맵에 Named 존 포탈 스폰 |
| `DespawnWorldItem("item_id")` | 월드에 놓인 아이템 엔티티 즉시 제거 (인벤토리 영향 없음) |
| `Branch { condition, if_true, if_false }` | 조건 분기 (중첩 가능) |

### auto_advance
- `Vec<AutoAdvance>` — 우선순위 순서, **첫 번째 충족 조건만** 실행
- 빈 배열이면 자동 전진 없음
- `actions: Vec<QuestAction>` 필드 (기본값 빈 배열) — 조건 발동과 동시에 실행
  - `DespawnWorldItem`, `RemoveItem`, `SetFlag` 지원
  - `OpenPortal`, `KillNpc` 등 NPC 상호작용 전용 액션은 미지원


### QuestSpawn
| 필드 | 타입 | 기본값 | 설명 |
|------|------|--------|------|
| `phase` | `String` | (필수) | 이 단계일 때 스폰 |
| `item` | `String` | (필수) | 스폰할 아이템 ID |
| `zone` | `ZoneId` | (필수) | 스폰 대상 존 |
| `count` | `u32` | `1` | 스폰할 아이템 수량 |
| `condition` | `Option<QuestCondition>` | `None` | 추가 스폰 조건 (플래그/존/페이즈 조건 지원) |
## 퀘스트 아이템 ID 목록

| item_id | 종류 | 설명 |
|---------|------|------|
| `eternal_gem` | QuestItem | 던전 2층에서 획득, 보석 퀘스트 목표물 |
| `philosophers_stone` | QuestItem | 보석 퀘스트 완료 보상 |
| `dragon_scale` | QuestItem | 던전 2층에서 획득, 연금술사 재료 |
| `ancient_scroll` | QuestItem | 던전 1층에서 획득, 연금술사 재료 |


## 예시: 전체 기능 시나리오 (약초 구하기)

`assets/quests/herb_quest.ron` 참조. 사용된 기능 전체 목록:

| 분류 | 기능 | 사용 위치 |
|------|------|----------|
| **QuestCondition** | `HasItem` | auto_advance에서 은방울 뿌리/독초 소지 확인 |
| | `InZone` | travel→gathering 전환 (숲속 공터 도착 감지) |
| | `HasFlag` | spawn condition, 독초술사 처치 분기 |
| | `FlagIs` | Branch에서 독초 경험 여부로 보상 분기 |
| | `PhaseIs` | done→done_with_hint (gem_quest 교차 참조) |
| | `And` | 독초 소지 + 뿌리 미소지 복합 조건 |
| | `Or` | 뿌리 소지 **또는** (독초술사 처치 + 마을 귀환) |
| | `Not` | 뿌리 미소지, 독초술사 미처치 확인 |
| **QuestAction** | `AdvancePhase` | 모든 단계 전환 |
| | `GiveItem` | 해독제 포션 1개 |
| | `GiveItems` | 보상 포션 x3/x5 수량 지급 |
| | `RemoveItem` | 독초/뿌리 인벤토리 회수 |
| | `Log` | 상황 메시지 출력 |
| | `SetFlag`/`ClearFlag` | 플래그 설정/해제 |
| | `OpenPortal` | 숲속 공터 포탈 생성 |
| | `DespawnWorldItem` | 독초 월드 엔티티 정리 |
| | `KillNpc` | 퀘스트 완료 시 독초술사 제거 |
| | `Branch` | 3단계 중첩 보상 분기 (독초술사 처치 > 독초 경험 > 기본) |
| **QuestSpawn** | `count` | 은방울 뿌리 10개, 독초 3개 분산 배치 |
| | `condition` | 플래그 기반 조건부 스폰 |
| **AutoAdvance** | `actions` | 자동 전환 시 RemoveItem/DespawnWorldItem/SetFlag 동시 실행 |
| **QuestPhaseDef** | `objective` | 각 단계별 퀘스트 목표 표시 |

**퀘스트 흐름:**
```
[마을] 엘렌 대화 (not_started → travel)
    │  SetFlag, OpenPortal
    ↓
[숲속 공터] 도착 감지 (travel → gathering)     ← InZone
    │  은방울 뿌리 10개 + 독초 3개 스폰        ← count, condition
    ↓
[숲속 공터] 채집 중
    ├─ 독초 주움 → poisoned_warning → 해독 후 gathering으로 복귀
    ├─ 뿌리 주움 → collected                    ← Or 첫 번째 조건
    └─ 독초술사 처치 + 마을 귀환 → collected    ← Or + And 조합
    ↓
[마을] 엘렌 대화 (collected → done)
    │  Branch: 독초술사 처치 → 최고 보상
    │  Branch: 독초 경험 → 중간 보상            ← 중첩 Branch
    │  Branch: 기본 → 기본 보상
    ↓
[마을] done
    ├─ 독초술사 미처치 시 KillNpc 으로 제거
    └─ gem_quest active면 → done_with_hint      ← PhaseIs 교차 참조
```

## 동작 명세

- [x] 시작 시 `assets/quests/` 내 모든 `.ron` 파일을 로드해 `QuestRegistry` 에 등록
- [x] `QuestState` 리소스: `HashMap<quest_id, current_phase>` 로 진행상황 추적
- [x] NPC가 퀘스트 수여자(`giver_npc`)이면 `QuestState` 에 따른 조건부 대화 출력
- [x] 마지막 대화 줄에서 Interact(이동키/Esc) 시 `on_interact` 액션 실행
- [x] `auto_advance` 는 Vec 순서로 평가, 첫 번째 충족 조건이 단계를 전진시킨다
- [x] `AutoAdvance.actions` 는 조건 발동 직후 실행 (DespawnWorldItem, RemoveItem, SetFlag 지원)
- [x] `assets/quests/*.ron` 파일 전체를 테스트에서 파싱·시맨틱 검증 (페이즈 참조, 아이템 ID, initial_phase)
- [x] 앱 시작(Startup)에 파싱·시맨틱 오류 발생 시 `error!` 로그 출력 후 즉시 종료 (`std::process::exit(1)`)
- [x] `PhaseIs` 조건은 `QuestState` 를 참조해 다른 퀘스트의 단계를 비교한다
- [x] `Branch` 액션은 중첩 가능하며 런타임 조건에 따라 액션 목록을 선택한다
- [x] `QuestSpawn` 은 해당 `phase` 활성 + 해당 `zone` 진입 시 아이템 스폰
- [x] `QuestSpawn.count` — 동일 아이템을 지정 수량만큼 랜덤 방에 분산 스폰 (기본 1)
- [x] `QuestSpawn.condition` — 추가 조건(플래그/존/페이즈) 충족 시에만 스폰
- [x] 이미 수집한 퀘스트 아이템은 재스폰 안 됨 (`QuestState.spawned` HashSet)
- [x] 퀘스트 아이템은 플레이어 스폰 방(첫 번째)을 제외한 랜덤 방의 랜덤 Floor 타일에 배치 — 계단·다른 아이템과 중복 없음 (`UsedSpawnTiles` 공유)
- [x] 퀘스트 진행상황은 `save/progress.ron` 에 저장·복원 (`QuestState` 포함 전체 게임 상태 자동 저장)

## 퀘스트 패널 (Q 키)

- Q 키로 토글, 좌측 상단 고정
- 패널 폭: 미니맵 폭과 동일 (`MINIMAP_DISPLAY_SIZE + 10 = 190px`)
- 배경: 다크 그린 (`rgba(0, 0.05, 0, 0.97)`)
- `QuestState.phases` 에 등록된 퀘스트만 표시 (NPC 첫 대화 이후)
- 퀘스트별 표시 항목: 제목 + 현재 목표(`objective`) + 완료 여부
- 완료(`done` 페이즈) 퀘스트는 흐린 색으로 표시

### QuestPhaseDef 추가 필드

| 필드 | 타입 | 설명 |
|------|------|------|
| `objective` | `Option<String>` | 퀘스트 로그에 표시할 목표 문구 |

## 구현 세부사항

- `Villager` 컴포넌트에 `quest_id: Option<String>`, `quest_dialogue_idx: usize` 추가
- `VILLAGER_DATA` 에 "장로" 항목 추가 (`quest_id: Some("gem_quest")`)
- `handle_bump` 에서 `quest_id` 여부에 따라 퀘스트 대화 또는 일반 대화 분기
- `QuestItemKind { EternalGem, PhilosophersStone }` 추가, `ItemKind::QuestItem` 변형 신설
- `spawn_quest_items` 시스템: 맵 변경 시 퀘스트 스폰 조건 평가 후 아이템 스폰

## NPC 대화 우선순위

1. 퀘스트 수여자 NPC: 현재 퀘스트 단계의 dialog 배열 사용
2. 일반 NPC: 기존 dialogues 배열 사용 (변경 없음)

## 보석 퀘스트 흐름

```
[마을] 장로 대화 (not_started → active)
    ↓
[숲] 통과
    ↓
[던전 1층] 내려가기
    ↓
[던전 2층] 영원의 보석 획득 (active → ready 자동 전진)
    ↓
[마을] 장로 대화 (ready → done, 현자의 돌 수령)
```
