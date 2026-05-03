# 존 영속성 (Zone Persistence)

## 목적

맵을 떠났다가 돌아올 때 **모든 시간 민감 상태**가 자연스럽게 유지되도록 한다.
혈흔뿐 아니라 몬스터 상태 등 향후 추가될 시간 기반 상태도 동일 구조를 사용한다.

## 핵심 개념

- 존을 떠날 때: 현재 상태를 **그대로** 스냅샷에 저장하고 엔티티 제거
- 존에 돌아올 때: `turns_passed = global_turn - last_visited_turn` 계산 후 catch-up 처리
- 동일 존에서는 실시간 처리, 다른 존에 있는 동안은 시간이 멈춘 것처럼 관리

## 통합 리소스: ZonePersistence

모든 존의 영속 상태를 단일 리소스로 관리한다.

```
ZonePersistence(HashMap<ZoneId, ZoneSnapshot>)
  └─ ZoneSnapshot {
       blood_stains: Vec<SavedBloodStain>,   // 존 이탈 시 저장, 복귀 시 복원 후 비움
       monster_slots: Vec<MonsterSlot>,       // 항상 유지 (alive/dead 상태 추적)
       last_visited_turn: u64,                // 존 이탈 시각 기록
     }
```

`ZoneMonsterState`와 `WorldState.zone_state`는 이 리소스로 통합되었다.

## 혈흔 존 간 유지

- [x] 다른 존으로 이동하면 현재 존의 혈흔 엔티티를 모두 스냅샷에 저장 후 제거
- [x] `last_visited_turn` 갱신 (ZoneSnapshot)
- [x] 원래 존으로 돌아오면 `turns_passed` 만큼 알파값 감소하여 복원
- [x] 알파값이 0 이하가 된 혈흔은 복원하지 않는다 (자연 소멸)
- [x] 복원된 혈흔은 이후 정상적으로 페이드 아웃된다

## 몬스터 리스폰 타이머

- [x] 몬스터가 죽으면 즉시 슬롯에 `respawn_at_turn = global_turn + rand(30..=120)` 기록
- [x] 같은 존에서는 죽은 몬스터가 리스폰되지 않는다
- [x] 다른 존으로 이동 후 돌아올 때 `respawn_at_turn <= global_turn`이면 해당 슬롯 몬스터 리스폰
- [x] 아직 타이머가 남아 있으면 리스폰하지 않는다 (빈 슬롯 유지)
- [x] 존에 처음 입장하면 모든 슬롯이 `alive`(=None) 상태로 초기화

## 글로벌 턴 카운터

- [x] `GlobalTurn(u64)` 리소스: `PlayerActedEvent` 발생마다 1씩 증가
- [x] 혈흔 페이드 계산 및 몬스터 리스폰 타이머 catch-up에 사용

## 모듈 의존 관계

- `zone/mod.rs`: `ZonePersistence`, `ZoneSnapshot`, `MonsterSlot`, `SavedBloodStain` 정의 및 관리
- `monster/mod.rs`: `ZonePersistence` 읽기/쓰기 (spawn, respawn, cleanup)
- `ZoneMonsterState` 제거됨 → `ZonePersistence.monster_slots`로 통합

## 상수

- 몬스터 리스폰 대기 턴: `30~120 턴` (랜덤, 개체마다 다름)
