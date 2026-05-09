# 포털 위치 영속화

## 목적

존을 떠났다 돌아왔을 때 포털이 매번 다른 위치에 스폰되는 버그를 고친다.
같은 존 내에서 포털 위치는 일관성을 유지해야 한다.

## 버그 재현

1. herb_quest NPC 와 대화해 herb_glade 진입 포털이 마을의 (x, y) 에 생성됨
2. 포털을 통해 herb_glade 로 이동 후 돌아옴
3. 마을에 돌아왔을 때 herb_glade 포털이 다른 위치 (x', y') 에 스폰됨
4. 다른 마을의 던전 진입 포털도 같은 문제

## 원인

`zone/mod.rs::handle_zone_transition` 가 존을 떠날 때 포털 엔티티를 모두
despawn 하지만, 포털 위치는 어디에도 저장하지 않는다.
`spawn_portals_after_apply` 가 존 진입 시 매번 `portal_tile()` 로 랜덤
위치를 새로 결정하므로 위치가 매번 바뀐다.

## 동작 명세

- [ ] `ZoneSnapshot` 에 `portals: Vec<SavedPortal>` 추가
- [ ] `SavedPortal { tile_x, tile_y, target: ZoneId, arrive_from: PortalDirection }`
- [ ] `PortalDirection` 에 serde derive 추가
- [ ] 존을 떠날 때 (handle_zone_transition) 모든 포털의 위치/타깃을
      ZonePersistence 에 저장
- [ ] 존 진입 시 (spawn_portals_after_apply) ZonePersistence 에 저장된 포털이
      있으면 그 위치에 정확히 복원, 없으면 기존처럼 랜덤 생성
- [ ] 퀘스트 포털(`handle_spawn_quest_portal`)도 첫 생성 후 떠날 때
      자동 영속화됨 (별도 로직 불필요)

## 아키텍처

```
존 진입 (spawn_portals_after_apply):
  ┌─ persistence.portals 비어 있음? ─┐
  ├─ Yes → 기본 포털 + Named 진입 포털을 portal_tile() 로 랜덤 배치
  └─ No  → 저장된 (tile_x, tile_y, target, arrive_from) 그대로 복원

존 이탈 (handle_zone_transition):
  Query<(&Transform, &ZonePortal)> → SavedPortal 벡터로 직렬화
  → ZonePersistence[from_zone].portals 에 저장
  → 엔티티 despawn
```

## 테스트 전략

- **유닛 테스트**: 동일한 ZoneSnapshot 으로부터 두 번 spawn 했을 때 같은
  위치에 포털이 생성되는지 검증 (랜덤 호출 없이 결정적)
- **통합**: 두 번째 진입 시 persistence 가 있으면 portal_tile 이 호출되지
  않음을 mock/spy 로 검증 — 또는 같은 좌표가 나오는지 직접 비교
