# 퀘스트 종료 시 포털 정리

## 목적

퀘스트 완료 후 더 이상 필요 없는 Named zone 포털을 명시적으로 닫는 액션을
추가한다. 현재는 OpenPortal 만 있어 종료된 퀘스트의 포털이 영구히 남는다.

## 동작 명세

- [x] `QuestAction::ClosePortal(zone: String)` 추가
- [x] `CloseQuestPortalEvent { zone: String }` 추가 (zone 모듈)
- [x] `handle_close_quest_portal` 시스템이 다음 정리 수행:
  1. 현재 활성 `ZonePortal` entity 중 `target == ZoneId::Named(zone)` 인 것
     모두 despawn
  2. 모든 `ZonePersistence[*].portals` 에서 동일 target 의 saved portal 제거
     (zone 재방문 시 재 spawn 방지)
  3. `NamedZoneConfig.zones` 에서 해당 zone 제거
  4. `DiscoveredMarkers` 에서 해당 portal 위치의 Portal/Stair 마커 제거
- [x] `parry_quest.ron` 의 boss_defeated → done 전환 시 ClosePortal 호출
- [x] auto_advance 에서는 미지원 (on_interact 만 OK) — 일관성

## 구현 위치

`QuestAction::ClosePortal` 처리는 SpawnQuestPortalEvent 와 짝이 되는
`CloseQuestPortalEvent` 를 새로 만들어 zone 모듈에서 처리한다.
execute_actions 에서 이벤트 발행 → zone 시스템이 정리.

## 테스트 전략

- 유닛: ClosePortal 액션 실행 시 이벤트 발행
- 유닛: zone 시스템이 portal entity, persistence, named_config, marker 모두
  정리
