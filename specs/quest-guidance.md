# 퀘스트 목표 안내

## 목적

퀘스트 패널이 단순히 objective 문장만 보여주는 수준을 넘어, 플레이어가 다음 행동을 판단할 수 있는 힌트를 제공하게 한다.
특히 목표 존, 수집 진행도, NPC 대화 필요 여부를 외부 문서 없이 확인할 수 있어야 한다.

## 동작 명세

### 목표 존 안내

- [x] 현재 phase에 연결된 퀘스트 스폰의 `zone` 값을 위치 힌트로 표시한다
- [x] `auto_advance` 조건에 포함된 `InZone` 값을 위치 힌트로 표시한다
- [x] 중첩된 `And`, `Or`, `Not` 조건 안의 `InZone` 도 탐색한다
- [x] 같은 존은 한 번만 표시한다
- [x] 목표 존이 현재 `WorldState.current` 와 같으면 `현재 위치` 힌트를 함께 표시한다

### 수집 진행도 안내

- [x] 현재 phase에 연결된 퀘스트 스폰을 기준으로 목표 아이템을 표시한다
- [x] 인벤토리의 같은 `ItemKind` 개수를 세어 보유/필요 수량을 표시한다
- [x] 필요 수량을 채우지 못했으면 `진행`, 채웠으면 `완료`로 표시한다
- [x] 이미 스폰 완료로 기록된 항목은 진행 힌트에서 제외한다

### NPC 대화 안내

- [x] 현재 phase의 `on_interact` 액션이 비어 있지 않으면 퀘스트 제공자와 대화 힌트를 표시한다
- [x] 완료 phase에서는 대화 힌트를 표시하지 않는다
- [x] 현재 존에 발견된 퀘스트 제공자 마커가 있으면 `현재 존 / 미니맵 표시` 힌트를 덧붙인다

### 미니맵 목표 마커

- [x] 퀘스트 목표 아이템용 `QuestTarget` 마커 종류를 추가한다
- [x] 퀘스트 아이템이 월드에 스폰될 때 같은 타일에 `QuestTarget` 마커를 등록한다
- [x] `QuestTarget` 마커는 기존 퀘스트 제공자 마커와 다른 색을 사용한다
- [x] 기존 `DiscoveredMarkers` 중복 방지 규칙을 그대로 따른다

## 구현 위치

- `src/modules/ui/quest_panel.rs`
- `src/modules/ui/minimap.rs`
- `src/modules/quest/mod.rs`
- `docs/roguelike-feature-checklist.md`

## 테스트

- [x] `active_quest_shows_target_zone_and_progress`
- [x] `active_quest_marks_current_zone_when_target_matches_world`
- [x] `active_quest_progress_counts_inventory_items`
- [x] `ready_quest_hints_giver_dialogue`
- [x] `nested_zone_conditions_are_collected_once`
- [x] `auto_advance_zone_conditions_become_location_hints`
- [x] `ready_quest_hints_current_zone_marker_when_giver_discovered`
- [x] `quest_target_marker_uses_distinct_color`
- [x] 전체 회귀: `cargo test`

## 남은 개선 후보

- 완료 가능한 NPC의 방향 표시
- 발견되지 않은 퀘스트 제공자 NPC의 현재 존 추론
- Named 존 포탈이 아직 열리지 않았을 때 입구 안내
- 여러 목표가 있는 phase에서 우선순위와 완료 상태를 더 세밀하게 표시
