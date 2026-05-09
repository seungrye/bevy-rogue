# 미니맵 정리 + 게임 화면 letterbox 제거

## 변경 1: 미니맵 줌 기능 제거

### 사유
사용자 요구로 줌 기능이 불필요. 코드 단순화.

### 동작 명세
- [x] `zoom_minimap` 시스템 제거
- [x] `MinimapConfig` Resource 제거 (view_radius 더 이상 필요 없음)
- [x] `MINIMAP_VIEW_RADIUS_MIN/MAX`, `MINIMAP_ZOOM_STEP` 상수 제거
- [x] `apply_zoom` 함수 제거
- [x] `update_minimap` 의 `scale = config.view_radius / MINIMAP_RADIUS` 계산
      제거 — 항상 1.0 (1:1 매핑)
- [x] 도움말의 "Ctrl + +/- 미니맵 줌 조절" 항목 제거
- [x] zoom 관련 테스트 제거

## 변경 2: 게임 메인 화면 letterbox 제거

### 증상
사용자가 윈도우를 늘리면 화면 위쪽/주변에 큰 검은 letterbox 가 생긴다.
미니맵에는 그 영역이 맵 안 (revealed) 으로 표시되는데도 메인 화면엔
타일이 그려지지 않는다.

### 원인
`tile_in_viewport` 가 viewport 를 **44×30 타일로 하드코딩** (`HALF_W=22`,
`HALF_H=15`). 윈도우/카메라 viewport 가 그보다 커도 그 영역 너머의
타일은 `Visibility::Hidden` 으로 강제되어 검정으로 보인다.
미니맵은 이 제한을 받지 않으므로 정상 표시된다.

`camera_follow_player` 도 viewport 크기를 40×25 로 가정한 정적 clamp 라
윈도우가 더 크면 맵 외부가 그대로 노출된다.

### 수정
- `update_tile_visibility` 에서 viewport 체크 제거. revealed/visible
  타일은 항상 `Visible` 로 두고 카메라가 자동 컬링하도록 한다.
  (맵 타일 수 80×50 = 4000 — 부담 없음)
- `tile_in_viewport` 함수와 관련 테스트 제거.
- `camera_follow_player` clamp 를 `OrthographicProjection.area`
  기준 동적으로 변경. viewport 가 맵보다 크면 중앙 (0, 0) 에 고정.

### 제약
- UI 요소(미니맵, HUD 등) 위치는 유지되어야 함
- 타일 비율은 변형되지 않아야 함 (정사각형 타일)

## 변경 3: 전체 미니맵 상하 반전 수정

### 증상
M 키로 여는 전체 미니맵이 상하 반전되어 표시됨.

### 원인
게임 좌표는 좌하단 원점 (y 증가 = 위쪽), bevy UI 이미지는 좌상단 원점.
`update_full_map_image` 가 `image[y*W+x] ← tile[x, y]` 로 직접 매핑해
y 축이 뒤집힘. 작은 미니맵은 `MINIMAP_RADIUS - ty` 로 이미 뒤집고 있다.

### 수정
타일 픽셀과 마커 모두 `pixel_y = FULL_MAP_H - 1 - tile_y` 로 변환해
이미지에 기록한다.

## 테스트 전략

- 줌 제거: 컴파일 통과 + 기존 테스트 회귀 없음
- letterbox / 전체 미니맵 반전: 시각 검증 (단위 테스트 어려움)
