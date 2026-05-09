# 퀘스트 등장 확률 시스템

## 목적

매 게임 시작 시 각 퀘스트를 `spawn_chance` 확률로 활성화하여,
런마다 다른 퀘스트 조합이 등장하게 해 재플레이 가치를 높인다.

## 동작 명세

- [ ] `QuestDef`에 `spawn_chance: f32` 필드 추가 (0.0 ~ 1.0, 기본값 1.0)
- [ ] 게임 시작(Startup) 시 각 퀘스트를 `spawn_chance` 확률로 활성화
- [ ] 비활성 퀘스트의 NPC는 마을에 스폰되지 않음
- [ ] 비활성 퀘스트의 아이템은 맵에 스폰되지 않음
- [ ] 비활성 퀘스트의 auto_advance 조건은 평가하지 않음
- [ ] `spawn_chance` 미지정 시 1.0으로 간주 (기존 RON 파일 호환)

## 상수 (퀘스트별 spawn_chance)

| 퀘스트           | spawn_chance | 이유 |
|------------------|:---:|------|
| `prologue_fog`   | 1.0 | 항상 등장하는 프롤로그 |
| `gem_quest`      | 0.8 | 기본 의뢰 퀘스트 |
| `herb_quest`     | 0.8 | 기본 수집 퀘스트 |
| `alchemist_quest`| 0.7 | 중급 의뢰 퀘스트 |
| `parry_quest`    | 0.75| 모험 퀘스트 |
| `demonsword_quest`| 0.7| 모험 퀘스트 |
| `stark_quest`    | 0.6 | 스토리 이벤트 |
| `targaryen_quest`| 0.6 | 스토리 이벤트 |
| `jon_snow_quest` | 0.6 | 스토리 이벤트 |
| `world_fracture` | 0.5 | 희귀 엔드게임 퀘스트 |

## 구현 방식

```
게임 시작
  → load_quests (Startup, QuestSystemSet::Load)
      ├─ RON 파일 파싱
      ├─ 검증
      └─ 각 퀘스트: rand() < spawn_chance → QuestRegistry.active에 추가

spawn_on_startup (Startup, after QuestSystemSet::Load)
  → do_spawn이 registry.active 체크
  → 비활성 퀘스트 NPC는 스폰 안 함
```

## 아키텍처 변경

- `QuestDef`: `spawn_chance: f32` 추가
- `QuestRegistry`: `active: HashSet<String>` + `is_quest_active()` 추가
- `QuestSystemSet::Load` 추가 — villager 스폰 순서 보장
- `check_auto_advance` / `spawn_quest_items`: 비활성 퀘스트 스킵
- `do_spawn`: `&QuestRegistry` 파라미터 추가, active 체크
