# 플러그어블 맵 생성기 (Pluggable Map Generator)

## 목적

새 알고리즘을 기존 코드 변경 없이 추가할 수 있도록 트레이트 기반의 플러그어블 구조로 구성하고,
런타임에 키 입력으로 생성기를 바꿔 끼울 수 있게 한다.

---

## 인터페이스 명세

### MapGenerator 트레이트

```rust
pub trait MapGenerator: Send + Sync {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map;
    fn name(&self) -> &str;
}
```

- 모든 구현체는 `StdRng::seed_from_u64(seed)` 를 사용해 결정론적으로 생성한다
- `thread_rng()` 는 사용하지 않는다 — 동일 시드로 항상 동일한 맵 재현 보장
- 내부 헬퍼 함수도 `&mut impl Rng` 로 rng 를 전달받는다

### MapGeneratorRegistry 리소스

- [x] `current()`는 현재 선택된 생성기를 반환한다.
- [x] `next()`는 다음 생성기로 순환한다 (마지막이면 첫 번째로 돌아온다).
- [x] `select_by_name(name)`은 이름으로 특정 생성기를 선택한다. 없으면 선택을 변경하지 않는다.
- [x] 생성기가 한 개도 등록되지 않은 상태에서 `current()`를 호출하면 패닉 대신 `None`을 반환한다.

---

## 맵 크기

- `MAP_WIDTH = 80`, `MAP_HEIGHT = 50` (타일 단위)
- 컨텐츠 밀도를 높이기 위해 과도하게 넓은 크기는 피한다

## 동작 명세

### 맵 생성 계약

모든 `MapGenerator` 구현체는 다음을 보장해야 한다:

- [x] 생성된 맵의 Floor 타일이 전체의 10% 이상이다.
- [x] 생성된 맵의 테두리(x=0, x=width-1, y=0, y=height-1)는 모두 Wall이다.
- [x] `map.rooms`가 비어있지 않거나, 최소 하나의 Floor 타일 좌표를 반환하는 수단이 존재한다
  (플레이어 스폰 지점 결정에 사용).

### ensure_connectivity 동작 명세

`ensure_connectivity(map)`는 단일 연결 구역만 남기는 후처리 함수다.

- [x] 맵 중앙(`width/2, height/2`)이 Floor면 거기서 flood fill을 시작한다
  (중앙 구역이 가장자리 고립 구역보다 우선 보존됨)
- [x] 중앙이 Wall이면 좌상단부터 스캔해 첫 번째 Floor 타일에서 시작한다 (fallback)
- [x] 시작 타일에서 연결되지 않은 모든 Floor 타일은 Wall로 변환된다

### 런타임 전환

- [x] `F1` 키를 누르면 다음 생성기로 순환하며 맵을 즉시 재생성한다.
- [x] 재생성 시 기존 타일 엔티티는 모두 제거(despawn)되고 새 타일이 스폰된다.
- [x] 재생성 시 플레이어는 새 맵의 첫 번째 방 중앙으로 이동한다.
- [x] 재생성 시 트리거(chest, exit)도 새 맵 기준으로 재배치된다.
- [x] 현재 생성기 이름이 미니맵 아래에 표시된다.

### 시작 옵션

- [x] `--algorithm <name>` 커맨드라인 인수로 시작 시 사용할 생성기를 지정할 수 있다.
- [x] `--help` / `-h`로 사용법과 사용 가능한 생성기 목록을 출력할 수 있다.

---

## 구현된 생성기 목록

| 생성기 | 등록 이름 | 유형 | 결과 느낌 |
|--------|-----------|------|-----------|
| `BspGenerator` | `bsp` | 던전 | 규칙적인 방 분할, 깔끔한 복도 (depth 6, 최소 8×8) |
| `SimpleRoomsGenerator` | `simple_rooms` | 던전 | 크기 다양한 방들이 랜덤 배치 |
| `DrunkardWalkGenerator` | `drunkard` | 동굴 | 취한 듯 굴곡진 유기적 통로 |
| `CellularAutomataGenerator` | `cellular_automata` | 동굴 | 자연 침식된 느낌의 불규칙 동굴 |
| `DlaGenerator` | `dla` | 동굴 | 중심에서 뻗어나가는 침식 구조 |
| `BspIndoorGenerator` | `bsp_indoor` | 실내 | BSP를 소규모에 적용한 건물 평면도 |
| `PrefabGenerator` | `prefab` | 실내 | 손제작 방 청사진 조합 |
| `OrganicVillageGenerator` | `organic_village` | 마을 | 유기적 배치의 건물군 |
| `GridVillageGenerator` | `grid_village` | 마을 | 격자 도로망 + 블록 건물 |
| `ForestGenerator` | `forest` | 숲 | 나무 군집 사이 좁은 길 |
| `PerlinNoiseGenerator` | `perlin` | 숲 | 펄린 노이즈 기반 자연 지형 |

---

## 테스트 체크리스트

- [x] `BspGenerator::generate()`가 `MapGenerator` 계약을 충족한다
- [x] `SimpleRoomsGenerator::generate()`가 `MapGenerator` 계약을 충족한다
- [x] `DrunkardWalkGenerator::generate()`가 `MapGenerator` 계약을 충족한다
- [x] `CellularAutomataGenerator::generate()`가 `MapGenerator` 계약을 충족한다
- [x] `DlaGenerator::generate()`가 `MapGenerator` 계약을 충족한다
- [x] `BspIndoorGenerator::generate()`가 `MapGenerator` 계약을 충족한다
- [x] `PrefabGenerator::generate()`가 `MapGenerator` 계약을 충족한다
- [x] `OrganicVillageGenerator::generate()`가 `MapGenerator` 계약을 충족한다
- [x] `GridVillageGenerator::generate()`가 `MapGenerator` 계약을 충족한다
- [x] `ForestGenerator::generate()`가 `MapGenerator` 계약을 충족한다
- [x] `PerlinNoiseGenerator::generate()`가 `MapGenerator` 계약을 충족한다
- [x] `MapGeneratorRegistry::next()`가 순환한다
- [x] `MapGeneratorRegistry::select_by_name()`이 올바르게 선택한다
- [x] 빈 레지스트리에서 `current()`가 패닉을 일으키지 않는다
- [x] 생성기 1개짜리 레지스트리에서 `next()` 후 동일 생성기가 유지된다
