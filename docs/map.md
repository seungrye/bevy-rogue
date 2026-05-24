# 맵 시스템

## Map 리소스

`Map` (`map/mod.rs`)이 전체 월드 상태를 보관한다:

| 필드 | 타입 | 설명 |
|------|------|------|
| `tiles` | `Vec<MapTile>` | 타일 종류(`kind`)와 탐험/시야 상태(`revealed`, `visible`)를 함께 보관 |
| `rooms` | `Vec<Rect>` | 플레이어·포털·아이템 배치에 사용되는 방 목록 |
| `map_type` | `MapType` | `Dungeon` 또는 `Village` |
| `seed` | `u64` | 이 맵을 생성한 시드 |
| `algorithm` | `String` | 사용된 생성기 이름 (예: `"bsp"`, `"organic_village"`) |
| `shop_vendor` | `Option<(usize, usize)>` | 마을 상점의 상인(vendor) 고정 위치(가판대 뒤 바닥). 마을 생성기가 한 건물을 상점으로 만들면 설정된다. |

## 타일 종류 (TileKind)

| 종류 | 글리프 | 통행 | 시야 차단 | 비고 |
|------|--------|------|-----------|------|
| `Wall` | `#` | ✕ | ○ | 테두리·자연 암벽(파괴 불가) |
| `Floor` | `.` | ○ | ✕ | 일반 바닥 |
| `Water` | `~` | ✕ | ✕ | 물(통행 불가, 시야는 통과) |
| `Sand` | `,` | ○ | ✕ | 모래(해변) |
| `DestructibleWall` | `▒` | ✕ | ○ | 건물 벽(폭발로 파괴 가능 → `Rubble`) |
| `Rubble` | `%` | ○ | ✕ | 부서진 잔해 |
| `Counter` | `=` | ✕ | ✕ | **상점 가판대.** 통행 불가지만 시야는 통과해 카운터 앞에서 그 너머 상인과 거래한다. |

`is_walkable`/`blocks_sight`/`is_destructible`/`is_interactable` 술어가 종류별 동작을
결정하며, 렌더·이동·FOV·경로탐색·미니맵이 모두 이 술어를 사용한다.
`Counter` 는 `is_interactable` 이 참이라 향해 이동하면 상점이 열린다(카운터 앞 보정).

## 생성 알고리즘

`MapGeneratorRegistry` 리소스에 23종의 생성기가 등록된다.
모든 생성기는 `MapGenerator` 트레이트를 구현하며 `seed: u64`를 받아 결정론적으로 맵을 생성한다:

```rust
pub trait MapGenerator: Send + Sync {
    fn generate(&self, width: usize, height: usize, seed: u64) -> Map;
    fn name(&self) -> &str;
}
```

| 등록 이름 | 유형 | 느낌 |
|-----------|------|------|
| `bsp` | 던전 | 규칙적인 방 분할, 깔끔한 복도 |
| `simple_rooms` | 던전 | 크기 다양한 방들이 랜덤 배치 |
| `drunkard` | 동굴 | 굴곡진 유기적 통로 |
| `cellular_automata` | 동굴 | 자연 침식된 불규칙 동굴 |
| `dla` | 동굴 | 중심에서 뻗어나가는 침식 구조 |
| `bsp_indoor` | 실내 | BSP 기반 건물 평면도 |
| `prefab` | 실내 | 손제작 방 청사진 조합 |
| `organic_village` | 마을 | 유기적 배치의 건물군 |
| `grid_village` | 마을 | 격자 도로망 + 블록 건물 |
| `forest` | 숲 | 나무 군집 사이 좁은 길 |
| `perlin` | 숲 | 펄린 노이즈 기반 자연 지형 |
| `maze` | 미로 | recursive backtracker, 루프 없는 완전 미로 |
| `maze_prim` | 미로 | Prim's 알고리즘, 분기 많은 미로 |
| `recursive_division` | 미로 | 빈 방을 벽으로 재귀 분할, 벽마다 통로 한 칸 |
| `voronoi_rooms` | 던전 | Voronoi 셀을 방으로 카브, 인접 셀 복도 연결 |
| `walled_town` | 도시 | 둘레 성벽 + 성문, 내부 도로망과 건물 블록 |
| `voronoi_districts` | 도시 | Voronoi 구역 분할, 셀 경계 도로 + 내부 건물 |
| `island` | 바다 | 방사형 falloff × 멀티옥타브 노이즈, 바다로 둘러싸인 단일 섬(해변 `Sand`) |
| `archipelago` | 바다 | 약한 falloff + 다중 노이즈, 흩어진 여러 섬(다도해) |
| `coastal` | 바다 | 한 축 그라디언트, 절반 땅·절반 바다·사이 해안선(`Sand`) |
| `ocean` | 바다 | 대부분 `Water` + 드문드문 작은 섬/암초 |
| `biome_world` | 바다 | 고도 노이즈로 `Water`→`Sand`→`Floor`→`Wall` 바이옴 대륙 |
| `wfc` | 고급 | Wave Function Collapse(타일드 모델), 인접 제약 전파·붕괴로 구조적 던전 생성 |

수상 생성기(island/archipelago/coastal/ocean/biome_world)는 지상과 계약이 다르다:
테두리가 전부 `Water`(맵 밖 이탈 방지), 통과타일(`Floor`/`Sand`) 비율 ≥ 5%,
스폰은 통과타일 위에서 이루어진다.

- `F1` 키로 런타임에 생성기를 순환할 수 있다 (개발/테스트용)
- `--algorithm <이름>` 커맨드라인 옵션으로 시작 생성기를 지정할 수 있다

## 시드 체계

- 게임 시작 시 `GlobalSeed(u64)` 리소스를 `rand::random()`으로 초기화
- 각 존의 맵 시드는 `zone_seed(global_seed, zone_id)` 로 파생 (splitmix64 방식)
- 동일 `GlobalSeed`로는 항상 동일한 맵이 생성됨 (로그라이크 시드 재현 가능)
- `thread_rng()` 는 맵 생성에 사용하지 않음

## 에셋

게임은 `assets/` 디렉터리 아래의 파일들을 필요로 한다:
- `fonts/FiraMono-Medium.ttf` — 타일·스탯 렌더링용 고정폭 폰트
- `fonts/NanumSquareNeo-bRg.ttf` — 다이얼로그·UI 텍스트용 한국어 폰트
- `fonts/NotoSansSymbols2-Regular.ttf` — 유니코드 아이템 글리프용 폰트
- `fonts/rpg-awesome.ttf` — RPG 아이콘 글리프용 폰트
- `fonts/kenney-icon-font.ttf` — 아이콘 UI 보조 폰트
- `scene/open-chest.png` — 퀘스트 아이템 팝업 이미지
- `quests/*.ron` — 퀘스트 정의 파일
