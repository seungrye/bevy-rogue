# 화살 직사 궤적 (탑다운)

## 배경
현재 화살이 Rapier `GravityScale` + `vy` 중력 보정으로 포물선
(곡사) 으로 날아간다. 탑다운 시점이라 중력은 부자연스럽다.
직사 — origin → target 직선 — 으로 변경한다.

## 수정
`fire_projectile`:
- `GravityScale(0.0)` 으로 spawn (또는 `GravityScale` 컴포넌트 제거).
- 속도 계산에서 중력 보정 제거. `flight_time` 은 표시 시간이 아닌 단순
  속도 스케일링용으로 유지: `velocity = delta / flight_time`.
- 초기 회전각 = `delta.y.atan2(delta.x)` (이미 거의 같지만 vy 보정 빠짐).

기존 상수 `GRAVITY_SCALE`, `RAPIER_GRAVITY`, `PIXELS_PER_METER`,
`fn rotate_arrow` 는 그대로 둔다 — 직사여도 회전은 자연스럽다 (어차피
방향이 일정).

## 영향
- `update_projectiles` 의 lifetime / 충돌 검사는 그대로 유효.
- 마우스 클릭 / 키보드 원격 모드 모두 동일 효과.

## 테스트
- `fire_projectile` 직접 단위 테스트는 ECS 의존이라 어렵다 — 상수만
  검증.
- 시각 검증: 화살이 직선으로 날아가는지.
