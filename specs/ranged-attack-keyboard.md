# 키보드 원격 공격

## 배경
현재 활(BOW) 장착 시 마우스 클릭으로만 화살을 쏠 수 있다.
키보드만으로도 타겟을 잡고 발사할 수 있도록 한다.

`FireProjectileEvent`, `BOW_RANGE`, `is_line_of_sight_clear`,
`weapon_attack` 은 이미 존재한다. 새 시스템은 입력→이벤트 발행만 담당.

## 키매핑
- `F` — 원격 모드 진입 (활 장착 시에만). 장착 무기가 활이 아니면 무시.
- `Tab` / `Shift+Tab` — 다음 / 이전 적 타겟. FOV 안 + 사거리 안 적 순환.
- `←→↑↓` 또는 `WASD` — 자유 커서 한 칸 이동 (대각 입력 가능).
- `Enter` — 발사. 1턴 소비. 모드 종료.
- `Esc` — 취소. 턴 소비 안 함. 모드 종료.

다른 패널 (인벤토리, 상점, 도움말) 열린 동안에는 F 무시.

## 발사 판정
- 사거리: `BOW_RANGE` (현재 8). 초과 시 발사 무시 + 로그 "사거리를 벗어났다."
- LoS: `is_line_of_sight_clear` 통과 안 하면 발사 무시 + 로그
  "장애물에 막혔다."
- 타겟 타일에 적이 있으면 적중 → 데미지 (기존 projectile 시스템).
  적이 없으면 그냥 화살이 날아가서 사라짐 (기존 동작).
- 발사 성공 시 `PlayerActedEvent` 발행.

## 시각 표시
- 모드 진입 시 커서 entity 1개 spawn (Text2d, glyph: `+`, 색상: 노란).
- 커서 위치는 매 프레임 cursor 타일을 따라 갱신.
- 사거리 밖이거나 LoS 차단된 위치면 커서 색을 빨강(불가)·노랑(가능).
- 모드 종료 시 entity despawn.

## 자동 타겟 순환
- 진입 시 가장 가까운 적 자동 타겟. 없으면 player 위치.
- Tab 순환 순서: FOV 안 + 사거리 안 적을 (거리 오름차순)으로 정렬.
  거리 같으면 (tile_x, tile_y) tie-break.
- 자유 커서 이동 후에도 Tab 누르면 다시 적 목록으로 복귀.

## 데이터 구조
```rust
#[derive(Resource, Default)]
pub struct RangedTargeting {
    pub active: bool,
    pub cursor: (usize, usize),
}

#[derive(Component)]
pub struct RangedCursor;
```

별도 `targets` 캐시는 두지 않고, Tab 누를 때마다 monster_tiles + map FOV
에서 즉석에서 후보 목록 계산 (적 수 적어 부담 없음).

## 모듈 구성
- 새 모듈 `src/modules/ranged/mod.rs` + `RangedPlugin`.
- `main.rs` 에서 등록.
- `player_input` 류 시스템에서 모드 활성 시 일반 이동 입력 차단
  — `RangedTargeting.active` 를 봐서 이동 시스템이 무시.

## 도움말
help.rs 의 "전투와 탐험" 섹션에 추가:
- `F`: 원격 공격 시작 (활 장착 시)
- `Tab / Shift+Tab`: 다음/이전 타겟
- `방향키 (원격 중)`: 자유 커서
- `Enter`: 발사
- `Esc (원격 중)`: 취소

## MVP 범위 (이후 확장 분리)
- 화살 자원: 무한 (TODO 주석으로 마킹).
- 데미지 공식: 기존 마우스 클릭과 동일 (`weapon_attack(BOW, &items)`,
  Lightning 원소).
- 트레일 / 곡사 trajectory: 기존 projectile 시스템 그대로 사용.
- 사거리 별 명중률 보정: 없음.

## 테스트
- 단위 테스트: `next_target_index` 가 거리순으로 후보를 정렬하고
  index 가 정확히 wrap-around 하는지.
- 단위 테스트: 활 미장착 시 F 입력이 무시되는지 (Resource 상태 검사).
- 시각 검증: 모드 진입/탈출, Tab 순환, 발사 성공/실패 로그.
