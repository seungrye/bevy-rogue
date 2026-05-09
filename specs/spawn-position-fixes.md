# 아이템·몬스터 스폰 위치 버그 수정

## 목적

퀘스트 아이템과 몬스터가 잘못된 위치(맵 영역 밖, wall 위, 단일 room 집중)에
스폰되는 세 가지 버그를 한꺼번에 고친다.

## 버그 #3: 퀘스트 아이템이 맵 영역 밖

### 원인

`quest/mod.rs::spawn_quest_items` 의 fallback:

```rust
.unwrap_or_else(|| map.rooms.last().map(|r| r.center())
    .unwrap_or((map.width / 2, map.height / 2)))
```

- `map.rooms.last().center()` 가 맵 경계 밖 좌표일 수 있다 (room 생성기 버그
  여지)
- `(map.width / 2, map.height / 2)` 가 Wall 타일일 수 있어 vsisible 영역 밖
  처럼 보일 수 있다
- 어떤 경우든 Floor 검증 없이 좌표를 반환

### 수정

- [ ] `random_floor_tile_in_room` 이 모두 실패하면 맵 전체에서 Floor 타일을
      선형 검색하는 견고한 fallback 사용
- [ ] 그것도 실패하면 (맵에 Floor 가 없는 비정상 맵) `info!` 로깅 후 스폰
      포기 (잘못된 좌표에 스폰하느니 안 하는 게 낫다)

## 버그 #4: count > 1 일 때 단일 room 집중

### 원인

```rust
for _ in 0..spawn.count {
    let (tx, ty) = candidate_rooms.iter()
        .flat_map(|r| random_floor_tile_in_room(r, ...))
        .next()  // ← 첫 room 의 첫 가능 타일
        ...
}
```

`flat_map().next()` 패턴이 항상 첫 room 부터 시도하므로, 첫 room 에 충분한
floor 가 있으면 N 개 모두 첫 room 에 분포된다.

### 수정

- [ ] 매 iteration 마다 `candidate_rooms.choose(rng)` 로 무작위 room 선택
- [ ] 선택된 room 에 빈 자리 없으면 다른 room 시도 (전체 fallback 까지)

## 버그 #5: 적이 wall 또는 영역 밖

### 원인

`monster/mod.rs::spawn_from_slots`:

```rust
let (tx, ty) = {
    let mut tile = room.center();
    for _ in 0..10 {
        let x = rng.gen_range(room.x1..room.x2);
        let y = rng.gen_range(room.y1..room.y2);
        tile = (x, y);
        break;  // ← 즉시 break, Floor 검사 없음
    }
    tile
};
```

`for _ in 0..10` 루프지만 첫 회에서 `break` — Floor 검사 없이 random tile 1개
픽. cellular_automata 같은 맵에서 room 안에 wall 이 있으면 monster 가 wall
위에 스폰. room boundary 가 맵 영역 넘으면 영역 밖에 스폰.

### 수정

- [ ] `random_floor_tile_in_room` 사용으로 통일 (퀘스트 아이템과 같은 헬퍼)
- [ ] 실패 시 #3 과 동일한 견고한 fallback

## 공통 헬퍼

세 버그 모두 "room 들에서 Floor 타일을 무작위로 N 개 고르기" 패턴이다.
공통 헬퍼를 `map` 모듈에 추가:

```rust
/// rooms 중에서 무작위 room 을 골라 무작위 Floor 타일 반환.
/// 모든 room 시도 후 실패하면 맵 전체에서 fallback.
pub fn random_floor_tile_anywhere(
    rooms: &[Rect],
    map: &Map,
    used: &mut HashSet<(usize, usize)>,
    rng: &mut impl Rng,
) -> Option<(usize, usize)>;
```

## 테스트 전략

- **유닛**: room 들에서 Floor 만 반환 검증 (모든 후보가 Floor)
- **유닛**: count=N 호출 시 결과가 단일 room 에 집중되지 않음 (다른 room 도
  포함됨) — 무작위라 시드 고정 후 검증
- **유닛**: room 안 모든 타일이 wall 인 경우 다른 room 으로 fallback
- **통합**: spawn_from_slots 가 항상 Floor 위에 monster 를 놓음
