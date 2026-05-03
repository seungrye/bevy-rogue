# 미니맵 마커

## 목적

미니맵에 퀘스트 제공자, 존 포털(맵 경계), 계단(내려가기/올라가기) 위치를
발견 후 지속 표시한다.

## 마커 종류

| MarkerKind | 색상 | 표시 조건 |
|------------|------|-----------|
| `QuestGiver` | 노란색 (1.0, 1.0, 0.0) | 퀘스트 수락 시 |
| `Portal` | 청록색 (0.0, 1.0, 1.0) | 포털 타일이 FOV에 들어왔을 때 |
| `StairDown` | 주황색 (1.0, 0.6, 0.0) | 계단(↓) 타일이 FOV에 들어왔을 때 |
| `StairUp` | 연두색 (0.5, 1.0, 0.5) | 계단(↑) 타일이 FOV에 들어왔을 때 |

## 자료 구조

```rust
pub enum MarkerKind { QuestGiver, Portal, StairDown, StairUp }

pub struct MapMarker {
    pub tile_x: usize,
    pub tile_y: usize,
    pub kind: MarkerKind,
    pub zone: ZoneId,
}

#[derive(Resource, Default)]
pub struct DiscoveredMarkers(pub Vec<MapMarker>);
```

## 발견 조건

- `QuestGiver`: 퀘스트가 active 단계로 전환될 때 (`show_quest_dialog`에서 `AdvancePhase` 후)
- `Portal` / `StairDown` / `StairUp`: `map.visible_tiles[idx]` 가 true 인 포털 엔티티 타일

## 미니맵 렌더링

- 기존 타일 렌더링 패스 이후 마커 픽셀 오버레이 패스를 추가한다
- 마커 픽셀 좌표 계산:
  ```
  minimap_tx = MINIMAP_RADIUS + round((mx - px) / scale)
  minimap_ty = MINIMAP_RADIUS - round((my - py) / scale)
  ```
- 현재 존(`WorldState.current`)의 마커만 렌더링한다
- 미니맵 범위 밖(0~MINIMAP_SIDE) 마커는 클리핑한다

## 동작 명세

- [x] `DiscoveredMarkers` 리소스가 존재한다
- [x] 포털 타일이 FOV에 들어오면 해당 마커가 `DiscoveredMarkers`에 추가된다 (중복 추가 안 함)
- [x] 퀘스트 수락 시 빌리저 위치가 `QuestGiver` 마커로 추가된다 (중복 추가 안 함)
- [x] 미니맵 렌더링 시 현재 존의 마커만 픽셀로 표시된다
- [x] 맵 재생성 시 이전 존의 마커는 `DiscoveredMarkers`에 유지된다 (재방문 시 표시 위해)

## 테스트 체크리스트

- [x] `DiscoveredMarkers`에 동일 위치/종류의 마커를 두 번 추가해도 중복이 없다
- [x] 마커 픽셀 좌표가 미니맵 범위 안에 있을 때만 렌더링된다
