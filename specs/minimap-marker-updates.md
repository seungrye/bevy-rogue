# 미니맵 마커 동적 갱신

## 목적

1. 퀘스트 아이템을 획득하면 해당 마커가 미니맵에서 즉시 제거되어야 한다.
2. 시야에 있는 퀘스트 NPC 가 이동하면 미니맵 마커도 따라 갱신되어야 한다
   (시야 밖이면 마지막 본 위치 유지).

## 현재 동작

- `DiscoveredMarkers::add` 는 같은 (tile_x, tile_y, kind, zone) 가 있으면
  중복 안 되지만 그 외엔 무한 누적
- 퀘스트 아이템 픽업 시 마커 제거 로직 없음 → 획득 후에도 마커 그대로
- NPC 이동 시 새 위치에 마커 추가되고 옛 위치 마커 제거 안 됨 → NPC 가
  거쳐간 모든 위치에 마커가 남음

## 동작 명세

- [x] `MapMarker` 에 `actor: Option<String>` 필드 추가
      (`#[serde(default)]` 로 기존 저장 데이터 호환)
- [x] `DiscoveredMarkers::remove_at(tile_x, tile_y, kind, zone)` — 위치+종류로
      마커 제거
- [x] `DiscoveredMarkers::remove_actor(actor, kind, zone)` — actor 식별자로
      마커 제거 (퀘스트 종료/시작 전 NPC 마커 정리)
- [x] `DiscoveredMarkers::update_actor_position(actor, kind, zone, x, y)` —
      같은 actor 가 있으면 위치만 갱신, 없으면 새 마커 추가
- [x] `pickup_items` 에서 QuestItem 픽업 시 그 위치의 QuestTarget 마커 제거
- [x] `discover_quest_npcs_in_fov` 가 quest 상태 기반으로 갱신:
  - quest 시작 전 (initial_phase) → 마커 제거
  - quest 종료 (terminal phase) → 마커 제거
  - active + FOV 안 → 위치 갱신 (NPC 이름을 actor 로 사용)
  - active + FOV 밖 → 마지막 본 위치 유지

## 테스트 전략

- **유닛**: `update_actor_position` 이 같은 actor 의 위치를 갱신
- **유닛**: `remove_at` 이 정확한 위치/종류만 제거
- **유닛**: legacy MapMarker (actor 없음) serde 호환
