# 저장 데이터에 active 퀘스트 보존

## 목적

저장된 게임을 로드했을 때, 진행 중이던 퀘스트가 spawn_chance 재롤로 사라지지
않도록 한다.

## 현재 동작

- `SaveData.quest_state`: 저장 ✅ (각 quest 의 phase, flags 등)
- `QuestRegistry.active`: **저장 안 됨** ❌
- 로드 시:
  - PostStartup `load_if_save_exists` 가 `quest_state` 복원
  - 하지만 Startup 단계에서 이미 `load_quests` 가 실행되어 `spawn_chance` 로
    `QuestRegistry.active` 를 재롤
  - 결과: 진행 중이던 퀘스트가 비활성화된 상태로 로드되면 `check_auto_advance`,
    `spawn_quest_items` 가 그 퀘스트를 무시 → 사실상 진행 불가

## 동작 명세

- [ ] `SaveData` 에 `active_quests: HashSet<String>` 필드 추가
      (`#[serde(default)]` 로 기존 저장 파일 호환)
- [ ] 저장 시 `QuestRegistry.active` 를 `active_quests` 에 클론
- [ ] 로드 시 `QuestRegistry.active` 를 `save.active_quests` 로 덮어쓰기
      (load_quests 가 spawn_chance 로 롤한 값을 무시하고 saved 사용)

## 테스트 전략

- **유닛**: SaveData serde roundtrip 에 active_quests 보존
- **유닛**: 기존 저장 파일 (active_quests 필드 없음) 도 #[serde(default)] 로
  파싱 가능
