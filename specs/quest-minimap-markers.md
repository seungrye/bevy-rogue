# 퀘스트 NPC·포털 미니맵 표시 강화

## 목적

퀘스트가 발생/시작되면 NPC 와 관련 포털 위치를 미니맵에서 즉시 찾을 수
있게 한다. 현재는 시점 한정·FOV 기반이라 사용자가 위치를 잊기 쉽다.

## 현재 동작

| 항목 | 마커 추가 시점 | 문제 |
|---|---|---|
| 퀘스트 NPC | `handle_bump` 에서 phase 가 `not_started→active` 로 전환될 때 1 회 | 그 전엔 NPC 위치 모름 |
| 퀘스트 포털 (`Named` zone 진입) | `discover_portals_in_fov` (FOV 안 들어와야) | quest 받은 직후 멀면 못 봄 |
| 일반 portal (계단 등) | `discover_portals_in_fov` | OK |

## 동작 명세

- [ ] 퀘스트 NPC 가 player FOV 에 들어오면 `MarkerKind::QuestGiver` 마커
      추가 — 새 시스템 `discover_quest_npcs_in_fov`
- [ ] `handle_spawn_quest_portal` 이 새 portal 생성 시 즉시 미니맵 마커
      (`MarkerKind::Portal`) 추가 — FOV 검사 없이 발견된 것으로 처리
- [ ] 기존 `handle_bump` 의 active 전환 시 마커 추가는 유지 (이미 NPC 와
      대화한 직후엔 보장)

## 사용자 흐름 개선

1. 마을에 들어가 NPC 가 시야에 들어옴 → 퀘스트 NPC 위치 미니맵 노란 점
2. NPC 와 대화해 퀘스트 시작 → portal 생성 즉시 보라색 점 (멀어도 표시)
3. 어디로 가야 할지 미니맵에서 한눈에 파악

## 테스트 전략

- **유닛**: `discover_quest_npcs_in_fov` 가 quest_id 있는 villager 만,
  visible 타일에서만 마커 추가
- **유닛**: `handle_spawn_quest_portal` 이 마커를 즉시 추가
- **통합 시나리오**: 퀘스트 시작 후 미니맵에 portal 마커가 나타남 (FOV 와
  무관)
