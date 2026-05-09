# 퀘스트 포털 배치 옵션

## 배경
현재 `OpenPortal` 은 항상 `PortalDirection::StairDown` (마지막 방 랜덤
Floor) 에 포털을 배치한다. 마을 같은 outdoor 맵에선 어색하고,
퀘스트 작성자가 위치를 지정할 수단이 없다.

## 동작 명세

### `OpenPortal` 액션 확장
```rust
OpenPortal {
    zone: String,
    generator: String,
    #[serde(default)]
    placement: PortalPlacement,
}
```

### `PortalPlacement` 옵션 (RON 표기)
- `InsideRoom` — **기본값** (현재 동작 유지 — RON 호환).
  맵의 랜덤 방 floor 한 칸. `map.rooms` 비어 있으면 Random 으로 fallback.
- `Border` — 맵 외곽선에 *가장 가까운* floor 타일.
  스캔 순서: 가장 바깥 ring 부터 안쪽으로, 첫 floor 발견 시 사용.
- `Random` — 맵 전체에서 랜덤 floor.
- `NearGiver { radius: usize }` — 퀘스트 `giver_npc` 위치 기준
  반경 `radius` 안 floor. giver 위치를 못 찾으면 InsideRoom fallback.

### RON 사용 예
```ron
OpenPortal(zone: "herb_glade", generator: "forest", placement: Border)
OpenPortal(zone: "demon_cave", generator: "cellular_automata") // 기본 InsideRoom
OpenPortal(zone: "elder_keep", generator: "town",
           placement: NearGiver(radius: 5))
```

## 구현 메모

### 데이터
- `PortalPlacement` enum 신설 (`Default = InsideRoom`).
- `QuestAction::OpenPortal` 분리: `placement: PortalPlacement` 추가.
- `SpawnQuestPortalEvent` 에 `placement` 전달.

### 위치 계산 (`zone::mod`)
- `compute_portal_pos(map, placement, ctx, used, rng) -> Option<(usize, usize)>`
  - `InsideRoom` → 랜덤 방의 random_floor_tile_in_room
  - `Border` → 외곽 ring 부터 BFS, 첫 Floor
  - `Random` → random_floor_tile_anywhere
  - `NearGiver { radius }` → giver 위치 ± radius 의 Floor 후보 중 랜덤
- `ctx` 는 giver 위치 (Option<(usize, usize)>) 를 포함.

### giver 위치 조회
`handle_spawn_quest_portal` 에서 quest registry 로 giver_npc 이름 얻고,
villager query 로 같은 zone 의 그 NPC entity 의 transform → tile coord.

### 마을 퀘스트 RON 갱신
- `herb_quest.ron`: `placement: Border` 명시
- `demonsword_quest.ron`, `parry_quest.ron`: 기존 동작 유지 (생략)

## 테스트
- `compute_portal_pos` 에 대해 InsideRoom / Border / Random 단위 테스트.
- RON 직렬화: `OpenPortal` 가 placement 없이도 파싱되는지 (close_portal_action_parses_from_ron 패턴).
- `placement: Border` 명시 RON 파싱 테스트.
- NearGiver 는 ECS 의존이라 단위 테스트 어려움 — fallback 경로만 단위 테스트.
