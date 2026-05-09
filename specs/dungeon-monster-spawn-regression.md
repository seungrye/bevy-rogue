# 던전 몬스터 스폰 회귀 버그

## 증상

새로 들어가는 던전에 몬스터가 한 마리도 없다.

## 원인

portal-position-persistence 작업에서 `handle_zone_transition` 이
`ensure_zone_portals_persisted` 를 호출하여 도착 zone 의 persistence entry
를 미리 만든다 (portal 저장용).

monster `respawn_on_regen` 은 첫 방문을 다음 조건으로 판정:
```rust
if !persistence.0.contains_key(&zone_id) {
    persistence.0.entry(...).or_default().monster_slots = init_zone_monster_slots(&rooms);
}
```

portal 저장이 entry 를 먼저 만들었으므로 `contains_key` 가 true → monster
init 스킵 → `monster_slots` 가 비어 있는 채로 `spawn_from_slots` 호출 →
한 마리도 spawn 안 됨.

ZonePersistence resource 가 portals 와 monster_slots 를 공유하는데, 두
초기화 경로가 서로의 entry 존재 가정에 의존하던 것이 충돌.

## 수정

- [ ] monster_slots 초기화 조건을 `entry 존재` 에서 `monster_slots 비어있음`
      으로 변경 — portal 이 먼저 entry 를 만들어도 monster_slots 가 비었으면
      여전히 첫 방문으로 인식

## 테스트 전략

- **유닛**: persistence entry 가 portal 만 있고 monster_slots 가 비었을 때
  init 이 호출되는지
