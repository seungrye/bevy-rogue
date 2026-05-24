# 맵 생성기 확장 (v2): 물 타일 + 신규 생성기

## 목적
던전·마을·숲에 더해 **바다·섬·해안·바이옴**과 **미로/도시/고급(WFC)** 생성기를 추가한다.
각 생성기는 `src/modules/map/generators/<name>.rs` 단일 모듈로 분리하고, `MapGenerator`
트레이트만 구현해 `MapGeneratorRegistry`에 한 줄로 등록한다(쉬운 적용).

## 1. TileKind 확장

```rust
pub enum TileKind { Wall, Floor, Water, Sand }   // 기존 Wall/Floor + 신규
```

| 타일 | 이동 | FOV(시야 차단) | 렌더(예시 색) | 글리프(ascii) |
|------|------|----------------|----------------|----------------|
| `Wall` | 불가 | 차단 | 회색 | `#` |
| `Floor` | 가능 | 통과 | 어두운 회색 | `.` |
| `Water` | **불가** | **통과**(물 너머가 보임) | 파랑 | `~` |
| `Sand` | 가능 | 통과 | 모래색 | `,` |

### 동작 명세
- [ ] `TileKind::is_walkable()` → `Floor|Sand`는 true, `Wall|Water`는 false. 플레이어·몬스터 이동과 경로탐색이 이 함수를 쓴다.
- [ ] `TileKind::blocks_sight()` → `Wall`만 true. `Water`는 시야를 막지 않는다.
- [ ] 렌더링(타일 색/글리프), FOV, 이동 충돌, 경로탐색(`pathfinding`)이 위 두 술어를 사용하도록 갱신.
- [ ] 기존 `== TileKind::Wall` / `== TileKind::Floor` 로 walkable/sight를 판단하던 코드는 술어로 교체(누락 시 Water를 바닥처럼 취급하는 버그).

## 2. 생성 계약 (물 맵을 위한 조정)
- [ ] **지상 맵**(던전/마을/숲/미로): 기존 계약 유지 — 테두리 `Wall`, 통과타일 ≥ 10%.
- [ ] **수상 맵**(island/ocean/coastal/archipelago/biome): 테두리는 `Water`(플레이어가 맵 밖으로 못 나감), 통과타일(`Floor|Sand`) ≥ 10%, 스폰은 통과타일 위. `ensure_connectivity`는 통과타일 기준으로 동작하도록 일반화(또는 수상맵 전용 변형).
- [ ] 모든 생성기는 `StdRng::seed_from_u64(seed)` 사용(동일 시드 동일 맵), `thread_rng()` 금지.

## 3. 신규 생성기

### 던전/미로 (Wall/Floor)
- [ ] `maze` — **recursive backtracker**(DFS+스택). 격자 셀을 벽으로 분리, 무작위 미연결 이웃으로 통로 뚫기. 완전 미로(루프 없음).
- [ ] `maze_prim` — **Prim's**. 프런티어 셀 무작위 선택으로 분기 많은 미로.
- [ ] `recursive_division` — 빈 방을 벽으로 재귀 분할하고 각 벽에 통로 한 칸.
- [ ] `voronoi_rooms` — 무작위 시드점의 **Voronoi 셀**을 방으로 카브, 인접 셀을 통로로 연결.

### 마을/도시 (Wall/Floor)
- [ ] `walled_town` — 둘레 성벽 + 성문 1~2개, 내부에 도로망과 건물 블록.
- [ ] `voronoi_districts` — Voronoi로 구역 분할, 셀 경계를 도로로, 셀 내부에 건물.

### 바다/섬/바이옴 (Water/Sand 필요)
- [ ] `island` — **방사형 falloff(중심 1→가장자리 0) × 멀티옥타브 펄린/값노이즈** → 임계값 위는 땅(`Floor`), 물가 한 칸은 `Sand`, 나머지 `Water`. 바다로 둘러싸인 단일 섬.
- [ ] `archipelago` — 여러 노이즈 블롭/저(低)falloff → 흩어진 다도(多島).
- [ ] `coastal` — 한쪽 그라디언트 → 절반은 땅, 절반은 바다, 그 사이 해안선(`Sand`).
- [ ] `ocean` — 대부분 `Water` + 드문드문 작은 섬/암초.
- [ ] `biome_world` — 고도 노이즈로 `Water`(저)→`Sand`(해안)→`Floor`(평원)→`Wall`(산), 습도 노이즈로 평원/숲 변형. (Amit Patel식 Voronoi 폴리곤·강은 후속 확장 여지로 남김.)

### 고급
- [ ] `wfc` — **Wave Function Collapse(타일드 모델)**. 소규모 타일셋(벽/바닥 인접 제약)으로 셀 가능상태를 제약전파+붕괴로 풀어 구조적 맵 생성. 모순 시 시드 기반 재시작(제한 횟수).

## 4. 등록·전환·문서
- [ ] 각 생성기를 `generators/mod.rs`에 `pub mod`, `map/mod.rs` 셋업에 `registry.register(Box::new(...))` 한 줄 추가.
- [ ] `F1` 순환·`--algorithm <name>`·`--help` 목록에 자동 포함(레지스트리 기반이라 자동).
- [ ] `docs/map.md`의 생성기 표 갱신.

## 5. 테스트 (각 생성기 모듈 하단 `#[cfg(test)]`, 한글 서술형)
- [ ] 결정성: 같은 시드는 같은 맵을 만든다.
- [ ] 계약: 해당 유형의 테두리·통과타일 비율·연결성 충족.
- [ ] 스폰 가능: 최소 한 통과타일 존재(+rooms 또는 스폰 좌표 수단).
- [ ] 타일 술어: `is_walkable`/`blocks_sight`가 Water/Sand에 대해 올바름.
- [ ] 수상 생성기: Water/Sand가 실제로 생성됨, 땅이 바다로 둘러싸임.
- [ ] 100% branch+function 커버리지(nightly) 목표 — `docs/testing.md` 워크플로 적용.
