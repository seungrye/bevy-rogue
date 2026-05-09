# 사망 시 모든 액션 입력 차단

## 배경
플레이어가 사망 (`Defeated` 컴포넌트 부여) 상태에서도 마우스 클릭으로
자동 경로 이동, 활 원격 공격 등이 가능한 버그. `player_movement` 는
`Without<Defeated>` 로 이미 막혀있지만, 다른 입력 시스템들이 누락.

## 차단 대상
사망 시 게임 오버 팝업 (R/N/Esc 안내) 만 활성. 다른 모든 입력 — 액션
+ UI 토글 — 차단. 게임 오버 팝업 위에 다른 패널이 떠 어색해지는 것을
방지.

게임 오버 시스템 자체 (`handle_game_over_exit`, `handle_new_game_input`)
는 정상 동작.

차단 시스템:
- `player/mod.rs::on_mouse_click` — path 이동.
- `ranged/mod.rs::handle_ranged_input` / `handle_ranged_mouse` /
  `update_ranged_cursor` — 원격 모드.
- `ui/help.rs::toggle_help_overlay` — H/?.
- `ui/minimap.rs::toggle_full_map` — M.
- `ui/equipment.rs::toggle_equipment_panel` — E.
- `ui/quest_panel.rs::toggle_quest_panel` — Q.
- `map/mod.rs::cycle_map_generator` — F1.
- `item/mod.rs::cycle_glyph_style` — G.

방식: 각 시스템에 `defeated_q: Query<(), With<Defeated>>` 추가하고
`if !defeated_q.is_empty() { return; }`. (또는 player Query 시그니처에
`Without<Defeated>` — query 가 비면 early return.)

## 모드 정리
사망 시점에 원격 모드가 활성 중이면 cursor entity 가 남는다.
`Defeated` 가 붙는 시점 (combat 시스템) 에 모드 강제 종료 — 별도 시스템
또는 game_over 진입 시 RangedTargeting reset + cursor despawn.

이 spec 에서는 단순화: cursor entity 는 game_over 의 `reset_to_new_game`
에서 다른 entity 들과 함께 despawn 되도록 처리하거나, 새 게임 시작 시
RangedTargeting 도 reset. 사망 → 게임 오버 화면이 즉시 뜨므로 잠시 cursor
가 남는 시각적 잔존은 무시 가능.

## 수정
- player_q 시그니처에 `Without<Defeated>` 추가.
- `Defeated` 임포트 추가 (해당 모듈).
- 사망 후 새 게임 시작 시 RangedTargeting / cursor 정리 — game_over
  reset 코드에 추가.

## 테스트
- ECS 의존이라 단위 테스트 어려움 — 코드 단위로 query 시그니처 변경
  검증.
- 시각: 사망 후 마우스 클릭 / F / Tab / Enter 모두 무시되는지.
