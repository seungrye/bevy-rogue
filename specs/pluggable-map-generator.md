# 플러그어블 맵 생성기 (Pluggable Map Generator)

## 목적

현재 `MapAlgorithm` 열거형과 `match` 분기로 구현된 맵 생성 방식은 알고리즘을 추가할 때마다
열거형과 `create_and_store_map` 시스템을 함께 수정해야 한다. 새 알고리즘을 기존 코드 변경 없이
추가할 수 있도록 트레이트 기반의 플러그어블 구조로 전환하고, 런타임에 키 입력 또는 시작 옵션으로
생성기를 바꿔 끼울 수 있게 한다.

---

## 인터페이스 명세

### MapGenerator 트레이트

```rust
pub trait MapGenerator: Send + Sync {
    /// 맵을 생성하여 반환한다.
    fn generate(&self, width: usize, height: usize) -> Map;

    /// UI·로그에 표시할 이름을 반환한다.
    fn name(&self) -> &str;
}
```

- `generate`는 순수 함수처럼 동작해야 한다 — 같은 시드를 주면 같은 결과를 낸다 (결정적 생성은 선택적으로 지원).
- 기존 세 알고리즘(BSP, SimpleRooms, DrunkardWalk)은 이 트레이트를 구현하는 구조체로 각각 분리된다.
- 새 알고리즘은 이 트레이트를 구현하고 레지스트리에 등록하는 것만으로 추가된다.

### MapGeneratorRegistry 리소스

```rust
#[derive(Resource)]
pub struct MapGeneratorRegistry {
    generators: Vec<Box<dyn MapGenerator>>,
    current: usize,
}
```

- [ ] `current()`는 현재 선택된 생성기를 반환한다.
- [ ] `next()`는 다음 생성기로 순환한다 (마지막이면 첫 번째로 돌아온다).
- [ ] `select_by_name(name)`은 이름으로 특정 생성기를 선택한다. 없으면 선택을 변경하지 않는다.
- [ ] 생성기가 한 개도 등록되지 않은 상태에서 `current()`를 호출하면 패닉 대신 에러를 반환한다.

---

## 동작 명세

### 맵 생성 계약

모든 `MapGenerator` 구현체는 다음을 보장해야 한다:

- [ ] 생성된 맵의 Floor 타일이 전체의 10% 이상이다.
- [ ] 생성된 맵의 테두리(x=0, x=width-1, y=0, y=height-1)는 모두 Wall이다.
- [ ] `map.rooms`가 비어있지 않거나, 최소 하나의 Floor 타일 좌표를 반환하는 수단이 존재한다
  (플레이어 스폰 지점 결정에 사용).

> DrunkardWalk처럼 방(Rect) 개념이 없는 알고리즘은 `rooms`에 Floor 타일 중심을 담은
> 단일 `Rect`를 하나 넣는 것으로 계약을 충족한다.

### 런타임 전환

- [ ] `Tab` 키를 누르면 다음 생성기로 순환하며 맵을 즉시 재생성한다.
- [ ] 재생성 시 기존 타일 엔티티는 모두 제거(despawn)되고 새 타일이 스폰된다.
- [ ] 재생성 시 플레이어는 새 맵의 첫 번째 방 중앙으로 이동한다.
- [ ] 재생성 시 트리거(chest, exit)도 새 맵 기준으로 재배치된다.
- [ ] 현재 생성기 이름이 UI 스탯 패널에 표시된다.

### 시작 옵션

- [ ] `--algorithm <name>` 커맨드라인 인수로 시작 시 사용할 생성기를 지정할 수 있다.
  - 유효한 이름: `bsp`, `simple_rooms`, `drunkard_walk`
  - 지정하지 않으면 기본값은 `bsp`이다.
  - 알 수 없는 이름이 전달되면 경고 메시지를 출력하고 기본값을 사용한다.

---

## 엣지 케이스

- 맵 재생성 중(`Tab` 처리 중) 추가 `Tab` 입력은 무시한다.
- DrunkardWalk는 `rooms`를 생성하지 않으므로 스폰 시스템이 `rooms`가 비어있을 때
  Floor 타일 중 임의의 위치를 선택하는 폴백이 있어야 한다.
- 생성기가 하나뿐일 때 `Tab`을 눌러도 불필요한 재생성을 하지 않는다.

---

## 구현 힌트

### 디렉터리 구조 (변경 후)

```
src/modules/map/
  mod.rs              — MapGenerator 트레이트, MapGeneratorRegistry, 시스템
  generators/
    bsp.rs            — BspGenerator 구조체
    rooms.rs          — SimpleRoomsGenerator 구조체
    drunkard.rs       — DrunkardWalkGenerator 구조체
```

### Bevy 통합 패턴

```rust
// 각 생성기를 플러그인에서 등록
impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        let mut registry = MapGeneratorRegistry::new();
        registry.register(Box::new(BspGenerator));
        registry.register(Box::new(SimpleRoomsGenerator));
        registry.register(Box::new(DrunkardWalkGenerator));
        // 시작 옵션 반영
        registry.select_by_name(&self.initial_algorithm);

        app.insert_resource(registry)
           .add_systems(Startup, (create_and_store_map, draw_map.after(create_and_store_map)))
           .add_systems(Update, (cycle_map_generator, update_tile_visibility));
    }
}
```

### 재생성 이벤트 흐름

```
Tab 키 입력
  → cycle_map_generator 시스템
    → registry.next() 호출
    → RegenerateMapEvent 발행
      → despawn_map_tiles 시스템 (기존 TileEntity 제거)
      → create_and_store_map 시스템 (새 Map 생성)
      → draw_map 시스템 (새 타일 스폰)
      → respawn_player 시스템 (첫 방 중앙으로 이동)
      → respawn_triggers 시스템 (트리거 재배치)
```

---

## 지형 유형별 생성기 계획

플러그어블 구조 완성 후 지형 유형별로 2개씩 생성기를 추가한다.
각 생성기는 독립 스펙 파일을 별도로 작성한다.

### 던전 (Dungeon)

지하 구조물·석조 방과 복도로 이뤄진 전통적인 로그라이크 맵.

| 생성기 | 등록 이름 | 구현 상태 | 결과 느낌 |
|--------|-----------|-----------|-----------|
| **BSP** | `bsp` | ✅ 구현됨 | 규칙적인 방 분할, 깔끔한 복도 |
| **Simple Rooms** | `simple_rooms` | ✅ 구현됨 | 크기 다양한 방들이 랜덤 배치 |

---

### 동굴 / 자연 지하 (Cave)

인공적 설계 없이 자연 침식된 느낌의 유기적 지하 공간.

| 생성기 | 등록 이름 | 구현 상태 | 결과 느낌 |
|--------|-----------|-----------|-----------|
| **Cellular Automata** | `cellular_automata` | ⬜ 미구현 | 반복 규칙으로 동굴 벽이 자연스럽게 형성 |
| **Diffusion-Limited Aggregation (DLA)** | `dla` | ⬜ 미구현 | 중심에서 뻗어나가는 침식 구조, 가지치기 형태 |

참고: [ch.27 Cellular Automata](https://bfnightly.bracketproductions.com/chapter_27.html) · [ch.30 DLA](https://bfnightly.bracketproductions.com/chapter_30.html)

---

### 실내 / 건물 내부 (Indoor)

건물 한 채의 내부 공간. 방마다 용도(침실·거실·창고 등)가 있고 문으로 연결.

| 생성기 | 등록 이름 | 구현 상태 | 결과 느낌 |
|--------|-----------|-----------|-----------|
| **BSP Indoor** | `bsp_indoor` | ⬜ 미구현 | BSP를 소규모 공간에 적용, 방들이 복도(문) 로 연결된 실내 평면도 |
| **Prefab** | `prefab` | ⬜ 미구현 | 손제작 방 청사진(침실·부엌·계단 등)을 조합해 건물 내부 구성 |

> BSP Indoor는 기존 BSP 생성기를 파라미터(최소 방 크기, 분할 깊이)로 재구성하거나
> 별도 구조체로 분리한다.

참고: [ch.31 Prefabs](https://bfnightly.bracketproductions.com/chapter_31.html)

---

### 마을 / 야외 정착지 (Village)

도로·광장·건물 외벽으로 이뤄진 야외 공간. 집 내부는 별도 맵(Indoor)으로 전환.

| 생성기 | 등록 이름 | 구현 상태 | 결과 느낌 |
|--------|-----------|-----------|-----------|
| **Organic Village** | `organic_village` | ⬜ 미구현 | 중심 광장에서 방사형 도로 생성 후 도로변에 건물 배치. 중세 마을 느낌. |
| **Grid Village** | `grid_village` | ⬜ 미구현 | 격자 도로망 생성 후 블록마다 건물 배치. 계획 도시 느낌. |

두 생성기 모두 건물은 Prefab 청사진을 외벽(Wall)만 채워 배치하고,
입구(Floor 1칸)로 내부 진입 트리거를 붙인다.

---

### 숲 / 야외 경로 (Forest)

나무(Wall)가 대부분이고 좁은 길(Floor)이 뚫려있는 자연 지형.

| 생성기 | 등록 이름 | 구현 상태 | 결과 느낌 |
|--------|-----------|-----------|-----------|
| **Cellular Automata Forest** | `forest` | ⬜ 미구현 | 초기 70% Wall 밀도에 규칙 적용 → 자연스러운 나무 군집과 빈터(clearing) |
| **Perlin Noise** | `perlin` | ⬜ 미구현 | 노이즈 임계값으로 나무 밀도 결정, 저지대(낮은 값) 구간이 자연스러운 길이 됨 |

> Cellular Automata를 동굴과 숲 양쪽에 재사용할 수 있다.
> 초기 밀도와 반복 횟수, Wall/Floor 판정 임계값을 파라미터화하면
> 동굴(낮은 초기 밀도)과 숲(높은 초기 밀도) 두 가지를 하나의 구조체로 커버 가능.

참고: [Perlin Noise 지형 생성](https://docs.rs/noise/latest/noise/) (noise crate)

---

> **공통 레퍼런스**: Herbert Wolverson의 [Roguelike Tutorial in Rust](https://bfnightly.bracketproductions.com)
> — 각 알고리즘의 Rust 구현 예제와 설명이 챕터별로 정리되어 있다.
> 소스코드: [thebracket/rustrogueliketutorial](https://github.com/thebracket/rustrogueliketutorial)

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
