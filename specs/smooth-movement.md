# 부드러운 이동 (Smooth Movement)

## 목적

키를 누르면 즉시 한 칸 이동하고, 계속 누르고 있으면 연속으로 부드럽게 이동한다.
격자 단위 이동이지만 lerp 애니메이션으로 타일 간 전환이 자연스럽게 보인다.

## 동작 명세

- [x] 키를 한 번 누르면 즉시 한 칸 이동한다
- [x] 키를 누른 채로 있으면 초기 지연(120ms) 후 연속 이동이 시작된다
- [x] 연속 이동 중에는 이전 이동의 lerp가 완료된 즉시 다음 이동이 시작된다
- [x] 연속 이동 중(elapsed ≥ INITIAL_HOLD_DELAY) 방향이 바뀌면 즉시 새 방향으로 이동한다
- [x] 초기 지연 중(연속 이동 미시작) 방향이 바뀌면 지연 타이머가 리셋되고 이동하지 않는다
- [x] 키를 떼면 이동이 즉시 멈춘다

## NPC/몬스터 다중 이동 애니메이션

- [x] Speed > 1.0인 엔티티가 한 턴에 여러 칸을 이동할 때 각 칸을 순차적으로 애니메이션한다
- [x] `MoveQueue(VecDeque<Vec3>)` 컴포넌트 — 이동할 목적지 좌표를 순서대로 보관
- [x] 논리 위치(tile_x/y)는 턴 처리 시 즉시 갱신, 시각적 위치는 큐를 통해 지연 반영
- [x] 각 큐 스텝은 `LERP_SPEED * speed.value` 속도로 애니메이션 (빠른 엔티티는 빠르게 이동)
- [x] 이전 턴 애니메이션이 완료되기 전에 다음 턴 이동이 오면 큐에 이어붙임

## 구현 메모

- `MoveHoldState` 리소스 — 현재 방향(dir)과 누적 시간(elapsed) 관리
- `tick_hold(state, dir, just_pressed, dt) → bool` — 이동 가능 여부 반환
  - `just_pressed=true`: 즉시 true 반환 (첫 탭)
  - 연속 이동 중 방향 변경: elapsed를 INITIAL_HOLD_DELAY로 유지, true 반환 (즉시 이동)
  - 초기 지연 중 방향 변경: 타이머 리셋(0), false 반환
  - 동일 방향 유지: elapsed 누적, INITIAL_HOLD_DELAY 초과 시 true
- `Without<MovingTo>` 필터로 lerp 완료 전까지 이동 차단 (자연스러운 속도 조절)
