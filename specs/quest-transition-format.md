# 퀘스트 Transition 포맷 재설계

## 목적

기존 `on_interact` + `auto_advance` + `Branch` 중첩 구조를 제거하고,
**평탄한 `transitions` 목록**으로 대체하여 가독성을 높인다.

## 동작 명세

- [x] `QuestPhaseDef` 는 `dialog` 와 `objective` 만 가진다
- [x] `QuestDef` 는 `transitions: Vec<QuestTransition>` 필드를 가진다
- [x] `QuestTransition` 은 `from`, `trigger`, `when?`, `actions`, `to` 를 가진다 (RON 표기 `Transition(...)`)
- [x] `TriggerKind::Interact` — NPC 마지막 대사 이후 실행
- [x] `TriggerKind::Auto` — 매 프레임 조건 평가, 첫 번째 매칭 실행
- [x] `when` 미지정 시 항상 매칭 (unconditional transition)
- [x] Interact/Auto 각각 현재 phase의 transitions 를 순서대로 평가, 첫 매칭만 실행
- [x] `to` 필드가 phase 전환을 담당 (AdvancePhase action 불필요)
- [x] `QuestAction` 에서 `Branch` 와 `AdvancePhase` 제거
- [x] `AutoAdvance` struct 제거
- [x] Auto trigger의 `actions` 는 DespawnWorldItem, RemoveItem, SetFlag 만 허용 (기존 동일)
- [x] terminal phase = 해당 phase 에서 시작하는 transition 이 없는 phase

## 구현 완료 (영향 파일)

- [x] `src/modules/quest/mod.rs` — 구조체 + 실행 로직(`check_auto_advance`, `execute_actions`) + 검증(`validate_quest_def`) + 테스트
- [x] `src/modules/villager/mod.rs` — NPC 상호작용(첫 매칭 Interact transition 실행), `interact_can_advance`, terminal 판정, 글리프
- [x] `src/modules/ui/quest_panel.rs` — 위치 힌트(Auto transition), giver 대화 힌트
- [x] `assets/quests/*.ron` — 전 퀘스트 파일을 새 포맷으로 마이그레이션 (이후 추가된 퀘스트도 동일 포맷 사용)
- [x] `specs/quest.md`, `specs/villager.md` — 문서 갱신

## 엣지 케이스

- `to == from` 인 transition: 같은 phase 에 머무름 (Log 전용 등)
- `when` 조건 불충족 시 다음 transition 으로 넘어감
- 모든 transition 이 불충족일 때: Interact 는 아무것도 하지 않음, Auto 는 매 프레임 재평가

## 새 RON 포맷 예시

```ron
QuestDef(
    id: "gem_quest",
    title: "잃어버린 보석",
    giver_npc: "elder",
    initial_phase: "not_started",

    phases: {
        "not_started": QuestPhaseDef(dialog: [...], objective: Some("...")),
        "active":      QuestPhaseDef(dialog: [...], objective: Some("...")),
        "ready":       QuestPhaseDef(dialog: [...], objective: Some("...")),
        "done":        QuestPhaseDef(dialog: [...], objective: Some("...")),
    },

    transitions: [
        Transition(from: "not_started", trigger: Interact, to: "active"),
        Transition(from: "active", trigger: Auto, when: HasItem("eternal_gem"), to: "ready"),
        Transition(from: "ready", trigger: Interact,
            actions: [RemoveItem("eternal_gem"), GiveItem("philosophers_stone")],
            to: "done"),
    ],

    spawns: [...],
)
```

## 영향 범위

- `src/modules/quest/mod.rs` — 구조체 + 실행 로직 + 검증
- `src/modules/villager/mod.rs` — NPC 상호작용
- `src/modules/ui/quest_panel.rs` — 퀘스트 패널 + 글리프
- `assets/quests/*.ron` — 전 퀘스트 파일 마이그레이션
