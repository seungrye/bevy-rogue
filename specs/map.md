# 맵·생성기·시야

## 맵 크기

`MAP_WIDTH = 80`, `MAP_HEIGHT = 50` (타일 단위). 컨텐츠 밀도를 위해
과도하게 넓은 크기는 피한다.

## 플러그어블 맵 생성기

### `MapGenerator` 트레이트
```rust
pub trait MapGenerator: Send + Sync {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map;
    fn name(&self) -> &str;
}
```

- 모든 구현체는 `StdRng::seed_from_u64(seed)` 사용 — `thread_rng()` 금지.
- 내부 헬퍼 함수도 `&mut impl Rng` 로 rng 전달 — 동일 시드 동일 맵 보장.

### `MapGeneratorRegistry` 리소스
- `current()` — 현재 생성기. 빈 레지스트리면 `None`.
- `next()` — 다음 생성기 순환 (마지막이면 첫 번째).
- `select_by_name(name)` — 이름으로 선택. 없으면 변경 안 함.

### 생성 계약
모든 구현체가 다음을 보장:
- Floor 타일 비율 ≥ 10%.
- 테두리 (x=0/x=width-1/y=0/y=height-1) 모두 Wall.
- `map.rooms` 비어있지 않거나, 최소 한 Floor 좌표 반환 수단 존재
  (플레이어 스폰).

### `ensure_connectivity`
단일 연결 구역만 남기는 후처리.
- 맵 중앙 (`width/2, height/2`) 이 Floor 면 거기서 flood fill 시작.
- 중앙이 Wall 이면 좌상단부터 첫 Floor 에서 시작 (fallback).
- 시작 타일에 연결 안 된 모든 Floor → Wall.

### 런타임 전환
- `F1` — 다음 생성기 순환 + 맵 즉시 재생성.
  타일 엔티티 모두 despawn 후 재스폰. 플레이어는 첫 방 중앙으로,
  트리거는 새 맵 기준 재배치.
- 미니맵 아래에 현재 생성기 이름 표시.
- `--algorithm <name>` 커맨드라인 인수로 시작 시 지정 가능.
  `--help`/`-h` 로 사용 가능 목록 출력.

### 구현된 생성기

총 **23종**이 `MapGeneratorRegistry` 에 등록된다. 등록 이름·유형·느낌 전체 표는
[`docs/map.md`](../docs/map.md#생성-알고리즘) 가 정본. 물 타일(Water/Sand)을 쓰는
수상 생성기와 v2 확장(미로/도시/바다/WFC) 설계는 [`map-generation-v2.md`](map-generation-v2.md) 참고.

### DLA 성능
DLA 는 매 파티클마다 전체 타일 (16,000) 선형 스캔 시 5,600 파티클 ×
O(16,000) ≈ 9천만 반복으로 수 초 지연. 해결:
- Floor 타일 목록을 `Vec<(usize, usize)>` 별도 유지 → O(1) 랜덤 선택.
- 새 타일 추가 시 `push()` 갱신.
- 파티클 최대 스텝 800 → 400 (반경 ≈ √400 = 20 충분).

## 스폰 위치

세 가지 버그 (퀘스트 아이템·몬스터·count > 1) 의 공통 원인은 "room 들에서
Floor 타일을 무작위 N 개 고르기" 패턴이 부재한 점.

### 공통 헬퍼 (`map` 모듈)
```rust
/// rooms 중 무작위 room 의 무작위 Floor 반환.
/// 모든 room 실패 시 맵 전체에서 fallback.
pub fn random_floor_tile_anywhere(
    rooms: &[Rect],
    map: &Map,
    used: &mut HashSet<(usize, usize)>,
    rng: &mut impl Rng,
) -> Option<(usize, usize)>;
```

### 수정 대상
- 퀘스트 아이템 fallback (`spawn_quest_items`): `room.center()` 가 wall/맵
  밖일 수 있어 Floor 검증 없이 좌표 반환 → 헬퍼로 통일, 실패 시 `info!`
  로깅 후 스폰 포기.
- count > 1 단일 room 집중 (`flat_map().next()`): 매 iteration
  `candidate_rooms.choose(rng)` 로 무작위 room 선택, 빈자리 없으면 다른
  room 시도.
- 몬스터 wall 스폰 (`spawn_from_slots`): 첫 회 `break` 로 Floor 검증 없이
  타일 반환 → 헬퍼로 통일, 실패 시 동일 fallback.

### 테스트 전략
- 유닛: room 들에서 Floor 만 반환.
- 유닛: count=N 호출 결과가 단일 room 에 집중되지 않음 (시드 고정 후).
- 유닛: room 안 모두 wall 인 경우 다른 room 으로 fallback.
- 통합: `spawn_from_slots` 가 항상 Floor 위 monster 배치.

## 몬스터 시야 (FOV)

몬스터가 무조건 추적 대신 **방향 기반 두-반원 시야** + LOS 로 탐지.
방향 FOV 모델(`Facing`, `FOV_FRONT`/`FOV_BACK`, `is_in_view`)은
[`stealth-and-directional-fov.md`](stealth-and-directional-fov.md) 참고.

### 시야 판정
`is_in_view(monster_pos, facing, tile, vision_radius, FOV_BACK, map)` —
정면 반경 `vision_radius`, 후면 반경 `FOV_BACK`, Bresenham LOS 통과
(`is_line_of_sight_clear` 공유). 등 뒤·벽 너머 플레이어는 미탐지.

### AI 상태
- `Idle` — 미인지. 무작위 배회 (30% 제자리).
- `Alerted` — 발견. 플레이어 방향 이동 또는 인접 시 공격.
- 시야 진입 시 `Idle → Alerted`, `alert_turns = MAX_ALERT_TURNS` 리셋.
- `Alerted` 중 시야 이탈해도 `alert_turns` 남으면 추적.
- `alert_turns = 0` → `Alerted → Idle`.

### 몬스터별 시야

| 몬스터 | vision_radius | 특성 |
|--------|--------------|------|
| 고블린 | 6 | 좁은 시야 |
| 오크   | 8 | 넓은 시야 |
| 트롤   | 5 | 가장 좁음 |

상수: `MAX_ALERT_TURNS = 5`.

### 엣지
- 벽 뒤 거리 내라도 LOS 차단 → 미탐지.
- 이미 Alerted 중 재진입 시 `alert_turns` 리셋.
- Idle 도 인접 시 공격 (우발적 접촉).

## 던전 몬스터 스폰 회귀 (해결)

### 증상
새 던전 진입 시 몬스터 0 마리.

### 원인
포털 위치 영속화 작업에서 `handle_zone_transition` 의
`ensure_zone_portals_persisted` 가 도착 zone 의 persistence entry 를 미리
생성. monster `respawn_on_regen` 의 첫 방문 판정이
`!persistence.contains_key(zone_id)` 였으므로 portal entry 가 먼저
생기면 `contains_key` true → monster init 스킵 → `monster_slots` 빈 채
`spawn_from_slots` 호출.

### 수정
첫 방문 판정을 `entry 존재` 에서 `monster_slots 비어있음` 으로 변경.
portal 이 entry 를 먼저 만들어도 `monster_slots` 가 비었으면 첫 방문으로
인식.
