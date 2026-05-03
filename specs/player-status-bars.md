# 플레이어 상태바 (Player Status Bars)

## 목적

플레이어 스프라이트('@') 바로 위에 HP·MP 프로그레스바를 표시하여
전투 중 상태를 직관적으로 확인할 수 있게 한다.

## 동작 명세

- [x] HP 바는 플레이어 스프라이트 바로 위에, MP 바는 HP 바 위에 표시된다
- [x] 바는 왼쪽에서 오른쪽으로 채워진다 (Anchor::CenterLeft)
- [x] HP 비율에 따라 색상이 변한다 (>50%: 녹색, 25~50%: 노란색, ≤25%: 빨간색)
- [x] MP 바는 파란색이다
- [x] 모든 바(전경·배경)는 반투명(alpha 0.7)으로 표시된다
- [x] HP 배경은 어두운 빨간색, MP 배경은 회색이다
- [x] HP 바와 MP 바 사이에 간격이 없다 (바로 붙어 있음)
- [x] 플레이어가 이동(lerp 포함)할 때 바도 함께 이동한다 (부모-자식 엔티티)
- [x] HP/MP 값이 변경된 프레임에 바가 즉시 업데이트된다 (Changed<CombatStats>)

## 레이아웃

```
MP 바  ████░░░░  y = +13px (HP 바 바로 위, 간격 없음)
HP 바  ██████░░  y = +11px
  '@'            y =   0px
```

- 바 너비: 14px (TILE_SIZE = 16px, 좌우 1px 여백)
- 바 높이: 2px
- HP 배경: 어두운 빨간색 (rgba 0.6, 0, 0, 0.7)
- MP 배경: 회색 (rgba 0.35, 0.35, 0.35, 0.7)

## 구현 범위

- `CombatStats`에 `mp`, `max_mp` 필드 추가 (`combat` 모듈)
- `HpBarFill`, `MpBarFill` 마커 컴포넌트
- `hp_color(ratio: f32) -> Color` 순수 함수 (테스트 가능)
- `spawn_player`에서 배경 + 전경 바 엔티티 4개를 플레이어의 자식으로 스폰
- `update_player_bars` 시스템 — `Changed<CombatStats>` 감지 후 스프라이트 갱신
