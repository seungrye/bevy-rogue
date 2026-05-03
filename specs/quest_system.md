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
| `PhaseIs("quest_id", "phase_id")` | 다른 퀘스트의 현재 단계 확인 |

### QuestAction
| 값 | 설명 |
|----|------|
| `AdvancePhase("phase_id")` | 이 퀘스트의 현재 단계를 지정 단계로 이동 |
| `GiveItem("item_id")` | 플레이어에게 아이템 지급 |
| `RemoveItem("item_id")` | 플레이어 인벤토리에서 아이템 제거 |

## 퀘스트 아이템 ID 목록

| item_id | 종류 | 설명 |
|---------|------|------|
| `eternal_gem` | QuestItem | 던전 2층에서 획득, 퀘스트 목표물 |
| `philosophers_stone` | QuestItem | 퀘스트 완료 보상 |

## 동작 명세

- [x] 시작 시 `assets/quests/` 내 모든 `.ron` 파일을 로드해 `QuestRegistry` 에 등록
- [x] `QuestState` 리소스: `HashMap<quest_id, current_phase>` 로 진행상황 추적
- [x] NPC가 퀘스트 수여자(`giver_npc`)이면 `QuestState` 에 따른 조건부 대화 출력
- [x] 마지막 대화 줄에서 Interact(이동키/Esc) 시 `on_interact` 액션 실행
- [x] 매 프레임 `auto_advance` 조건 평가 → 충족 시 자동 단계 전진
- [x] `QuestSpawn` 은 해당 `phase` 활성 + 해당 `zone` 진입 시 아이템 스폰
- [x] 이미 수집한 퀘스트 아이템은 재스폰 안 됨 (`QuestState.spawned` HashSet)
- [ ] 퀘스트 진행상황은 `save/progress.ron` 에 저장·복원 (추후 구현)

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
