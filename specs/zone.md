# 존 시스템

여러 독립 구역(마을·숲·던전 층 + 동적 Named 존) 간 이동, 맵 메모리,
포털 영속성.

## 존 구성

| ZoneId | 알고리즘 | 설명 |
|--------|---------|------|
| `Town` | `organic_village` | 퀘스트 수락·완료 장소 |
| `Forest` | `forest` | 마을과 던전을 잇는 통로 |
| `Dungeon(1)` | `bsp` | 던전 1층 |
| `Dungeon(2)` | `bsp` | 보석이 있는 던전 2층 |
| `Named(s)` | (퀘스트가 지정) | 동적 생성 — 포털과 함께 등록 |

### 연결
```
Town ←→ Forest ←→ Dungeon(1) ←→ Dungeon(2)
```
- Town 남쪽 경계 → Forest 북쪽 진입
- Forest 남쪽 경계 → Dungeon(1) 북쪽 진입
- Dungeon(1)/(2) 사이는 방의 계단(↓/↑) 글리프
- Named 존은 퀘스트의 `OpenPortal` 액션으로 동적 생성

## 포털

- `ZonePortal { target: ZoneId, arrive_from: PortalDirection }` 엔티티.
- 플레이어가 포털 타일에 이동하면 `ZoneTransitionEvent` 발행.
- 포털 충돌은 `MovingTo` 목적지 기준 — `Transform` 은 lerp 중간값이라
  미사용.
- 게임 시작 존: `Town`.

## 맵 메모리

- `WorldState.maps: HashMap<ZoneId, Map>` 에 방문한 맵 캐시.
- 첫 방문: `zone_seed(global_seed, zone_id)` 로 시드 파생 → 알고리즘으로
  생성 → 캐시.
- 재방문: 캐시에서 복원 (동일 시드라 타일 배치 항상 동일).
- 저장 시: `GlobalSeed` + `MapTile.revealed` 비트팩(Base64) 만 보존 —
  타일 배열은 저장 안 함. 로드 시 `zone_seed` 로 재생성.
- 엔티티 (몬스터·낙하 아이템) 는 재방문 시 재스폰 (몬스터 리젠은
  로그라이크 표준).
- 퀘스트 아이템 (보석 등) 은 퀘스트 상태로 추적 — 수집 후 재스폰 X.

## 영속성 (`ZonePersistence`)

존을 떠났다 돌아올 때 시간 민감 상태가 자연스럽게 유지되도록 통합 리소스
하나로 관리.

```
ZonePersistence(HashMap<ZoneId, ZoneSnapshot>)
  └─ ZoneSnapshot {
       blood_stains: Vec<SavedBloodStain>,   // 이탈 시 저장, 복귀 시 복원 후 비움
       monster_slots: Vec<MonsterSlot>,       // 항상 유지 (alive/dead 추적)
       portals: Vec<SavedPortal>,             // 포털 위치/타깃 영속화
       last_visited_turn: u64,                // 이탈 시각 기록
     }
```

`ZoneMonsterState`, `WorldState.zone_state` 는 이 리소스로 통합.

### 혈흔 존 간 유지
- 다른 존으로 이동 시 현재 존의 혈흔을 스냅샷에 저장 후 엔티티 제거.
- `last_visited_turn` 갱신.
- 복귀 시 `turns_passed = global_turn - last_visited_turn` 만큼 알파 감소
  복원. 알파 ≤ 0 이면 복원 안 함 (자연 소멸). 복원된 혈흔도 이후 정상
  페이드.

### 몬스터 리스폰
- 사망 즉시 슬롯에 `respawn_at_turn = global_turn + rand(30..=120)` 기록.
- 같은 존에서는 죽은 몬스터 리스폰 없음.
- 다른 존 이동 후 복귀 시 `respawn_at_turn <= global_turn` 슬롯만 리스폰,
  나머지는 빈 슬롯 유지.
- 첫 입장 시 모든 슬롯 `alive`(=None) 초기화.

### 포털 위치 영속화

#### 증상
존을 떠났다 돌아오면 포털이 매번 다른 위치에 스폰되는 버그.

#### 원인
`handle_zone_transition` 가 떠날 때 포털 엔티티를 despawn 만 하고 위치
저장 안 함. `spawn_portals_after_apply` 가 매번 `portal_tile()` 로 랜덤
재결정.

#### 수정
- `ZoneSnapshot.portals: Vec<SavedPortal>` 추가.
  `SavedPortal { tile_x, tile_y, target, arrive_from }` —
  `PortalDirection` 에 serde derive 추가.
- 존을 떠날 때 포털을 모두 `portals` 에 직렬화 후 despawn.
- 진입 시 저장된 포털이 있으면 정확히 복원, 없으면 기존처럼 랜덤 생성.
- 퀘스트 포털 (`handle_spawn_quest_portal`) 도 첫 생성 후 떠날 때 자동
  영속화 — 별도 로직 불필요.
- **첫 방문 시에도 player 도착 위치를 portal 위치에 일치**:
  `handle_zone_transition` 이 도착 위치 결정 직전 `ensure_zone_portals_persisted`
  헬퍼로 destination zone 의 portal 을 미리 배치·저장 → persistence 가
  비어있는 첫 진입에서도 return portal 위치를 알 수 있다.

### 글로벌 턴
`GlobalTurn(u64)` 리소스가 `PlayerActedEvent` 발생마다 +1.
혈흔 페이드·몬스터 리스폰 catch-up 에 사용.

## 동작 명세
- `WorldState` 리소스 — 현재 존 + 맵 캐시.
- 존 전환 시 이전 맵 캐시 후 새 맵 교체.
- `ApplyMapEvent` 발행 → 몬스터·아이템·빌리저 재스폰.
- 포털 진입 시 플레이어 위치를 도착 존의 적절한 지점으로 이동.
- 계단 포털은 `>` (내려가기) / `<` (올라가기) 글리프.
- 마을·던전 구분: `MapType::Village` vs `MapType::Dungeon` 유지.

## 모듈 의존
- `zone/mod.rs`: `ZonePersistence`, `ZoneSnapshot`, `MonsterSlot`,
  `SavedBloodStain`, `SavedPortal` 정의·관리.
- `monster/mod.rs`: `ZonePersistence` 읽기/쓰기 (spawn/respawn/cleanup).
- 몬스터 리스폰 대기: `30~120 턴` 랜덤 (개체별).

## 테스트 전략
- 동일 `ZoneSnapshot` 으로 두 번 spawn 시 같은 위치 포털 (결정적).
- 두 번째 진입 시 persistence 가 있으면 `portal_tile` 미호출 (mock/spy).
