# 플레이어 XP 성장 루프

## 목적

장비와 퀘스트 보상 외에 전투 반복의 동기를 제공한다.
몬스터 처치가 즉시 보상으로 이어지고, 레벨업을 통해 생존력이 증가해야 한다.

## 동작 명세

### 성장 상태

- [x] `PlayerProgress` 리소스로 레벨, 현재 XP, 다음 레벨 필요 XP, 처치 수를 관리한다
- [x] 기본값은 레벨 1, XP 0, 처치 수 0이다
- [x] 다음 레벨 필요 XP는 `xp_to_next_level(level)` 로 계산한다
- [x] 성장 상태는 자동 저장/로드에 포함된다
- [x] 새 게임 시작 시 성장 상태를 기본값으로 초기화한다

### XP 획득

- [x] 몬스터를 처치하면 몬스터 이름에 따라 XP를 획득한다
- [x] 고블린은 8 XP, 오크는 14 XP, 트롤은 24 XP를 지급한다
- [x] 알 수 없는 몬스터 이름은 기본 10 XP를 지급한다
- [x] 처치 로그에 획득 XP를 표시한다
- [x] 이미 HP가 0 이하인 몬스터를 다시 공격해 XP가 중복 지급되지 않게 한다

### 레벨업

- [x] 누적 XP가 다음 레벨 필요량 이상이면 레벨업한다
- [x] 한 번에 여러 레벨을 올릴 수 있도록 반복 처리한다
- [x] 레벨업 시 최대 HP +5, 최대 MP +2를 적용한다
- [x] 레벨업 시 HP와 MP를 최대치까지 회복한다
- [x] 레벨업 로그에 새 레벨과 회복된 HP/MP를 표시한다
- [x] 공격/방어 성장은 장비 스탯 갱신 흐름과 충돌하지 않도록 아직 적용하지 않는다

### HUD 연동

- [x] 상단 HUD에 `Lv.N XP 현재/필요` 형식으로 표시한다
- [x] 성장 상태가 바뀌면 HUD를 갱신한다

## 구현 위치

- `src/modules/player/mod.rs`
- `src/modules/monster/mod.rs`
- `src/modules/save/mod.rs`
- `src/modules/ui/hud.rs`
- `src/modules/ui/game_over.rs`
- `docs/roguelike-feature-checklist.md`

## 테스트

- [x] `xp_curve_increases_after_first_level`
- [x] `monster_xp_rewards_match_first_balance_pass`
- [x] `grant_xp_levels_up_and_refills_resources`
- [x] `save_data_roundtrip_ron` 에서 성장 상태 라운드트립 검증
- [x] `status_hud_text_contains_core_summary` 에서 레벨/XP 표시 검증
- [x] 전체 회귀: `cargo test`

## 남은 개선 후보

- 레벨업 시 공격/방어/속도 중 하나를 고르는 선택 UI
- 퀘스트 완료 XP 보상
- Game Over summary에 레벨, 처치 수, XP 표시
- 장비 보정과 분리된 기본 공격/방어 스탯 모델 추가
