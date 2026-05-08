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

## 생성 알고리즘

`MapGeneratorRegistry` 리소스에 11종의 생성기가 등록된다.
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
