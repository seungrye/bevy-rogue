# 전체화면 미니맵

## 목적

플레이어 중심 미니맵 외에, M 키로 전체 맵을 한 화면에 펼쳐 볼 수 있는
"전체화면 미니맵" 모드를 추가한다. 던전 구조 파악, 미탐험 영역 추적에
유용하다.

## 동작 명세

- [x] L 키 토글 (Large map) — `FullMapOpen` Resource (bool).
      M 키는 작은 미니맵 toggle 에 이미 사용되어 충돌, Tab 은 패널 내
      탭 전환과 충돌하므로 L 선택.
- [ ] 열림 상태:
  - 화면 전체를 덮는 반투명 패널 + 중앙에 큰 맵 이미지
  - 이미지 크기: `MAP_WIDTH x MAP_HEIGHT` (80×50) → 화면에 비율 유지로 확대
  - 발견된(`revealed`) 타일만 표시 — 미탐험 영역은 검정
  - 플레이어 위치 표시 (별 모양 또는 빨간 점)
  - 미니맵 마커(quest, portal, stair) 동일 색상으로 단일 픽셀 표시
- [ ] 닫힘 상태: hidden, 일반 미니맵만 표시
- [ ] 줌 기능은 추가하지 않음 (지금도 존재하는 작은 미니맵 줌은 그대로 유지)

## 아키텍처

```
새 리소스/컴포넌트:
  FullMapImage(Handle<Image>)      — 80x50 픽셀 이미지
  FullMapOpen(bool)                — 토글 상태
  FullMapPanel                     — 전체화면 NodeBundle 마커
  FullMapImageNode                 — 안쪽 ImageBundle 마커

시스템 (Update):
  toggle_full_map                  — M 키 입력 처리
  update_full_map_image            — 열렸을 때 이미지 갱신 (탐험 영역, 마커 등)
  update_full_map_visibility       — 토글 상태에 따라 visibility 동기화
```

## 테스트 전략

- 픽셀 그리기는 Bevy 종속 — 단위 테스트 어려움
- 좌표 변환 / revealed 판정 등 순수 함수만 테스트
- 시각 검증
