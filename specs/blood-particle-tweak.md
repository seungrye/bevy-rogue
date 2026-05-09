# Blood 파티클 효과 자연화

## 목적

탑다운 시점 게임에서 blood 파티클이 중력으로 아래로 떨어지는 어색함을
없애고, 피격 위치 주변에 짧게 튀는 효과로 정리한다.

## 현재 동작

`combat_feedback/mod.rs::spawn_blood_particles`:

```rust
GravityScale(2.0),  // ← 탑다운 시점에선 부적절
```

- 360° 모든 방향으로 튀지만 중력으로 모두 아래로 휘어짐
- 측면 게임의 외관처럼 보임 (탑다운에선 어색)

## 동작 명세

- [ ] `GravityScale(2.0)` → `GravityScale(0.0)` — 중력 제거
- [ ] `LinearDamping` 추가 — 속도가 시간에 따라 감소 (자연스러운 정지)
- [ ] `PARTICLE_LIFETIME` 0.45 → 0.3 — 짧게 보이고 사라짐
- [ ] 향후 개선 여지: 공격 방향(attacker → target)을 받아 cone 분포로
      튀게 하면 더 자연스러움 (CombatFeedbackEvent 시그니처 변경 필요라
      별도 작업)

## 테스트 전략

- 현재 spawn_blood_particles 는 Bevy world 종속 — 단위 테스트 어려움
- 시각 검증으로 충분
