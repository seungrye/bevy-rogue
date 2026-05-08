# 게임 오버와 새 게임 흐름

## 목적

플레이어 사망이 단순한 조작 정지가 아니라 로그라이크의 반복 플레이 루프가 되도록 한다.
사망 후 현재 run의 종료 상태를 명확히 보여주고, 기존 세이브에 갇히지 않고 새 run을 시작할 수 있어야 한다.

## 동작 명세

### Game Over 오버레이

- [x] 플레이어에게 `Defeated` 컴포넌트가 붙으면 중앙 오버레이를 표시한다
- [x] 오버레이에는 `GAME OVER`, 현재 존, 생존 턴, 조작 안내를 표시한다
- [x] `Esc` 입력 시 앱 종료 이벤트를 발행한다
- [x] 사망하지 않은 상태에서는 Game Over 입력 처리를 하지 않는다

### 새 게임 시작

- [x] Game Over 상태에서 `R` 또는 `N` 입력 시 새 run을 시작한다
- [x] 기존 `save/progress.ron` 을 삭제한다
- [x] `GlobalSeed` 를 새 무작위 값으로 교체한다
- [x] `GlobalTurn` 을 0으로 초기화한다
- [x] `WorldState`, `ZonePersistence`, `NamedZoneConfig` 를 초기화한다
- [x] `QuestState`, `PlayerInventory`, `PlayerEquipment` 를 초기화한다
- [x] 장비 패널, 퀘스트 패널, 상점 패널 상태를 닫힌 상태로 초기화한다
- [x] 이동 홀드 상태와 마우스 자동 이동 경로를 초기화한다
- [x] 아이템, 몬스터, 주민, 포털, 혈흔 엔티티를 제거한다
- [x] 플레이어의 `Defeated`, `MovingTo` 컴포넌트를 제거한다
- [x] 플레이어 HP/MP/공격/방어 스탯을 기본값으로 되돌린다
- [x] Town 맵을 새 시드로 생성하고 `ApplyMapEvent` 로 적용한다

## 구현 위치

- `src/modules/ui/game_over.rs`
- `src/modules/ui/mod.rs`
- `src/modules/ui/quest_panel.rs`
- `src/modules/player/mod.rs`

## 테스트

- [x] `game_over_text_includes_actions_and_summary`
- [x] 전체 회귀: `cargo test`

## 남은 개선 후보

- `R`은 마지막 체크포인트 재시도, `N`은 완전 새 게임처럼 의미 분리
- 처치 수, 획득 골드, 완료 퀘스트, 레벨을 run summary에 표시
- 사망 원인과 마지막 피격 로그를 오버레이에 표시
