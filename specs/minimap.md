# 미니맵

플레이어 중심 작은 미니맵 (항상 표시) + 전체 미니맵 (M 키 토글) +
마커 시스템.

## 작은 미니맵 (오버레이)

게임 화면 우측 상단에 반투명 다이아몬드 오버레이로 현재 위치·주변 지형을
즉시 파악.

### 동작
- 우측 상단 반투명 오버레이 (우측 5px, 상단 10px 여백).
- 다이아몬드 형태 테두리.
- 탐험 타일 밝기 구분 (시야 내 > 탐험됨 > 미탐험).
- 플레이어 위치는 노란 점.
- 보간 없이 픽셀 = 대표 타일 1 개 (nearest 샘플링) 으로 또렷한 픽셀 스타일.
- 미니맵 아래 현재 생성기 이름 (청록), `[F1] 맵 전환` 힌트 (회색).
- 항상 표시 (히든 토글 없음).

### 상수
- 표시 크기: `MINIMAP_DISPLAY_SIZE = 180.0` px.
- view_radius: `MINIMAP_RADIUS = 20` (타일). 줌 기능 없음 — 1:1 매핑 고정.

## 전체 미니맵 (`M` 토글)

전체 맵을 한 화면에 펼쳐 던전 구조·미탐험 영역 파악.

### 동작
- `M` 키 토글 — `FullMapOpen` Resource (bool). 기본 닫힘.
- 열림: 화면 전체 반투명 패널 + 중앙에 큰 맵 이미지 (`MAP_WIDTH x
  MAP_HEIGHT` = 80×50 → 비율 유지 확대).
- 발견된 (`revealed`) 타일만 표시. 미탐험은 검정.
- 플레이어 위치 + 마커 (quest, portal, stair) 동일 색상 단일 픽셀.

### 아키텍처
```
리소스/컴포넌트:
  FullMapImage(Handle<Image>)   80x50 픽셀 이미지
  FullMapOpen(bool)             토글 상태
  FullMapPanel                  전체화면 NodeBundle 마커
  FullMapImageNode              안쪽 ImageBundle 마커

시스템 (Update):
  toggle_full_map               M 키 입력
  update_full_map_image         열렸을 때 이미지 갱신
  update_full_map_visibility    토글 상태 동기화
```

### 상하 반전 보정
게임 좌표는 좌하단 원점, bevy UI 이미지는 좌상단 원점. 직접 매핑하면
y 축이 뒤집힌다. 타일·마커 모두 `pixel_y = FULL_MAP_H - 1 - tile_y` 로
변환해 이미지에 기록.

## 마커 시스템

탐험 진행 시 발견되는 위치를 미니맵·전체 맵에 지속 표시.

### 종류

| MarkerKind | 색상 | 표시 조건 |
|------------|------|-----------|
| `QuestGiver` | 노랑 (1.0, 1.0, 0.0) | 퀘스트 수락 시 / FOV 진입 시 |
| `QuestTarget` | 자홍 (1.0, 0.0, 1.0) | 퀘스트 목표 배정 시 |
| `Portal` | 청록 (0.0, 1.0, 1.0) | 포털 타일 FOV 진입 / 퀘스트 포털 즉시 |
| `StairDown` | 주황 (1.0, 0.6, 0.0) | 계단(↓) FOV 진입 |
| `StairUp` | 연두 (0.5, 1.0, 0.5) | 계단(↑) FOV 진입 |

### 자료 구조
```rust
pub enum MarkerKind { QuestGiver, QuestTarget, Portal, StairDown, StairUp }

pub struct MapMarker {
    pub tile_x: usize,
    pub tile_y: usize,
    pub kind: MarkerKind,
    pub zone: ZoneId,
    pub actor: Option<String>,   // 동적 actor (NPC) 식별자
}

#[derive(Resource, Default)]
pub struct DiscoveredMarkers(pub Vec<MapMarker>);
```

### 메서드
- `add(tile_x, tile_y, kind, zone)` — 정적 마커. 같은 위치/종류/존이
  이미 있으면 추가 안 함 (idempotent).
- `remove_at(tile_x, tile_y, kind, zone)` — 위치+종류로 제거.
- `remove_actor(actor, kind, zone)` — actor 식별자로 제거.
- `update_actor_position(actor, kind, zone, x, y)` — 같은 actor 있으면
  위치 갱신, 없으면 새로 추가.

### 단일 픽셀 마커
이전엔 십자 5 픽셀 스탬프였으나 미니맵 픽셀 밀도가 낮아 영역을 너무
차지. 색상 만으로 충분히 구분되므로 1 픽셀로 변경. `marker_stamp_pixels`
제거, 호출부에서 `write_minimap_pixel` 직접 호출.

### 동적 갱신
- 퀘스트 아이템 픽업 시 (`pickup_items`): 그 위치의 `QuestTarget` 마커
  `remove_at`.
- 퀘스트 NPC 위치 갱신 (`discover_quest_npcs_in_fov`):
  - quest 시작 전 (initial_phase) → 마커 제거.
  - quest 종료 (terminal phase) → 마커 제거.
  - active + FOV 안 → `update_actor_position` (NPC 이름을 actor 로).
  - active + FOV 밖 → 마지막 본 위치 유지.

### 발견 시점 강화 — 퀘스트 NPC·포털
사용자 흐름:
1. 마을 진입 → NPC 시야 들어오면 `QuestGiver` 마커 노란 점.
2. NPC 대화로 퀘스트 시작 → `OpenPortal` 액션 직후 `Portal` 마커 즉시
   추가 (FOV 검사 없이).
3. 미니맵에서 한눈에 목표 위치 파악.

구현:
- `discover_quest_npcs_in_fov` — quest_id 있는 villager 만, visible
  타일에서 마커 추가/갱신.
- `handle_spawn_quest_portal` 이 portal 생성 시 즉시 마커 추가.
- 기존 `handle_bump` 의 active 전환 시 마커 추가는 유지.

### 렌더링
- 작은 미니맵: 타일 패스 후 마커 오버레이 패스. 현재 존
  (`WorldState.current`) 마커만. 미니맵 범위 (0~MINIMAP_SIDE) 클리핑.
  좌표 변환:
  ```
  minimap_tx = MINIMAP_RADIUS + round((mx - px) / scale)
  minimap_ty = MINIMAP_RADIUS - round((my - py) / scale)
  ```
- 전체 미니맵: 발견된 모든 위치 마커 단일 픽셀.

## Letterbox 제거 (관련 카메라 픽스)

미니맵엔 정상 표시되는 영역이 메인 화면엔 검정으로 보이던 버그.

### 원인
`tile_in_viewport` 가 viewport 를 44×30 타일로 하드코딩 (`HALF_W=22`,
`HALF_H=15`). 윈도우/카메라 viewport 가 그보다 커도 그 영역 너머 타일은
`Visibility::Hidden` 강제. `camera_follow_player` 도 정적 clamp 라
윈도우가 더 크면 맵 외부 노출.

### 수정
- `update_tile_visibility` 에서 viewport 체크 제거. revealed/visible
  타일은 항상 `Visible` — 카메라가 자동 컬링 (4000 타일이라 부담 없음).
- `tile_in_viewport` 함수와 관련 테스트 제거.
- `camera_follow_player` clamp 를 `OrthographicProjection.area` 기준 동적.
  viewport 가 맵보다 크면 (0, 0) 중앙 고정.

## 동작 명세 요약
- `DiscoveredMarkers` 리소스 존재.
- 포털 타일 FOV 진입 시 마커 추가 (중복 X).
- 퀘스트 수락 시 빌리저 위치 `QuestGiver` 마커 (중복 X).
- 퀘스트 목표 위치 `QuestTarget` 마커 (중복 X).
- 미니맵 렌더 시 현재 존 마커만.
- 맵 재생성 시 다른 존 마커는 유지 (재방문 표시 위해).
- 미니맵 hidden 상태 시 텍스처 업데이트 skip.

## 테스트
- 동일 위치/종류 마커 두 번 추가해도 중복 없음.
- 마커 픽셀 좌표가 미니맵 범위 안일 때만 렌더링.
- `update_actor_position` 이 같은 actor 위치 갱신.
- `remove_at` 이 정확한 위치/종류만 제거.
- legacy MapMarker (actor 없음) serde 호환.
- 줌 제거 / letterbox / 전체 미니맵 반전: 시각 검증.
