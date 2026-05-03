# 부드러운 이동 (Smooth Movement)

## 목적

키를 누르면 즉시 한 칸 이동하고, 계속 누르고 있으면 연속으로 부드럽게 이동한다.
격자 단위 이동이지만 lerp 애니메이션으로 타일 간 전환이 자연스럽게 보인다.

## 동작 명세

- [x] 키를 한 번 누르면 즉시 한 칸 이동한다
- [x] 키를 누른 채로 있으면 초기 지연(120ms) 후 연속 이동이 시작된다
- [x] 연속 이동 중에는 이전 이동의 lerp가 완료된 즉시 다음 이동이 시작된다
- [x] 이동 방향이 바뀌면 지연 타이머가 리셋된다 (새 방향으로 즉시 이동은 없음)
- [x] 키를 떼면 이동이 즉시 멈춘다

## 구현 메모

- `MoveHoldState` 리소스 — 현재 방향(dir)과 누적 시간(elapsed) 관리
- `tick_hold(state, dir, just_pressed, dt) → bool` — 이동 가능 여부 반환
  - `just_pressed=true`: 즉시 true 반환 (첫 탭)
  - 방향 변경 시: 타이머 리셋, false 반환
  - 동일 방향 유지: elapsed 누적, INITIAL_HOLD_DELAY 초과 시 true
- `Without<MovingTo>` 필터로 lerp 완료 전까지 이동 차단 (자연스러운 속도 조절)
