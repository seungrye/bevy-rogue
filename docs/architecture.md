# 아키텍처

**Bevy 0.13** 기반 로그라이크 게임. 진입점은 `src/main.rs`이며, 고정 크기 창(1280×(맵+UI)px)을 설정하고 다섯 개의 플러그인을 순서대로 등록한다:

```
CorePlugin → MapPlugin → PlayerPlugin → TriggerPlugin → GameUiPlugin
```

모든 게임 코드는 `src/modules/` 아래에 위치한다:

| 모듈 | 역할 |
|------|------|
| `core` | 2D 카메라 스폰 |
| `map` | 맵 리소스, 타일 렌더링, 가시성 업데이트, 3가지 생성 알고리즘 |
| `player` | 플레이어 엔티티, WASD/방향키 이동(LERP 애니메이션), FOV 계산, 카메라 추적 |
| `trigger` | 위치 기반 트리거 감지(상자, 출구), 이벤트 디스패치, 입력으로 UI 닫기 |
| `ui` | 스탯 패널, 다이얼로그 로그, 미니맵 동적 텍스처 |

## 좌표 체계

두 가지 좌표 공간이 혼용된다:
- **타일 그리드:** `0..160` × `0..100`, `Map.tiles`에 `y * width + x` 인덱스로 저장
- **Bevy 월드(픽셀):** 화면 중심 기준; `TILE_SIZE`(16px)와 오프셋 보정값으로 상호 변환

## 프레임당 시스템 실행 순서

1. `trigger::close_ui_on_input` — 입력을 소비해 활성 UI 닫기
2. `trigger::check_triggers` — 플레이어-트리거 위치 겹침 감지
3. `trigger::handle_trigger_events` — 트리거 효과 실행 (상자 UI 스폰, 로그 메시지)
4. `player::player_movement` — 이동 유효성 검사 및 시작 (`MovingTo` 중에는 차단)
5. `player::smooth_player_lerp` — 7.5 타일/초 LERP 애니메이션, 도착 시 `MovingTo` 제거
6. `player::update_fov` — Bresenham 시선 계산, 반경 8, 타일 경계 통과 시에만 재계산
7. `player::camera_follow_player` — 플레이어 중심으로 카메라 이동
8. `map::update_tile_visibility` — 타일 색상 갱신 (흰색=가시, 회색=탐색됨, 숨김=미탐색)
9. `ui::update_dialog_box` — 큐에 쌓인 `LogMessage` 이벤트 표시 (`MessageLog`에 최근 5개 유지)
10. `ui::minimap::update_minimap` — 미니맵 RGBA 텍스처 재빌드, 위치 변화 없으면 스킵

## 이벤트 흐름 예시

```
플레이어가 트리거 타일로 이동
  → check_triggers 가 TriggerEvent 발행
    → handle_trigger_events 가 상자 UI를 스폰하거나 LogMessage 게시
      → close_ui_on_input 이 키 입력을 기다려 UI 노드 제거
```
