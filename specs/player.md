# 플레이어

플레이어 캐릭터의 이동, 장비, 성장, 상태 표시, 입력 처리, 사망/리스타트.

## 부드러운 이동

격자 단위 이동이지만 lerp 애니메이션으로 자연스럽게 보인다.

### 키보드
- 한 번 누르면 즉시 한 칸.
- 누르고 있으면 초기 지연(`INITIAL_HOLD_DELAY = 120ms`) 후 연속 이동.
- 연속 이동 중 lerp 완료 직후 다음 이동.
- 연속 중 방향 변경 시 즉시 새 방향으로.
- 초기 지연 중 방향 변경 시 타이머 리셋, 이동 안 함.
- 키 떼면 즉시 정지.

### NPC/몬스터 다중 이동
Speed > 1.0 엔티티가 한 턴에 여러 칸 이동 시 각 칸 순차 애니메이션.
- `MoveQueue(VecDeque<Vec3>)` 컴포넌트 — 이동할 목적지 큐.
- 논리 위치 (tile_x/y) 는 턴 처리 시 즉시 갱신, 시각 위치는 큐로 지연.
- 각 큐 스텝은 `LERP_SPEED * speed.value` 속도.
- 이전 턴 애니메이션 미완료 시 다음 턴 이동을 큐에 이어붙임.

### 구현
- `MoveHoldState` 리소스 — 현재 dir, 누적 elapsed 관리.
- `tick_hold(state, dir, just_pressed, dt) → bool` — 이동 가능 여부:
  - `just_pressed=true` → 즉시 true (첫 탭).
  - 연속 중 방향 변경 → `elapsed = INITIAL_HOLD_DELAY` 유지, true.
  - 초기 지연 중 방향 변경 → 0 리셋, false.
  - 동일 방향 → elapsed 누적, 초과 시 true.
- `Without<MovingTo>` 필터로 lerp 완료 전까지 이동 차단.

## 마우스 자동 경로

좌클릭으로 목적지 지정 → BFS 최단 경로 자동 이동.

### 동작
- 좌클릭 → 화면→타일 변환 후 BFS.
- 목적지가 Floor 아니면 무시.
- `PlayerPath(VecDeque<(usize,usize)>)` 에 경로 저장 (목적지 포함, 현재
  위치 제외).
- 매 턴 1 칸씩 소비 (일반 키 이동과 동일 속도).
- 키 입력 시 즉시 취소.
- 몬스터 타일 도달 시 공격 후 취소.
- 빌리저/장애물이면 취소.
- 도달 불가능하면 빈 채로 무시.
- 원격 모드 활성 중 (`RangedTargeting.active`) 또는 사망 (`Defeated`) 시
  마우스 클릭 무시.

## 장비 시스템

### 인벤토리
- 무기·방어구는 개별 아이템으로 (검 2 개 = 검, 검).
- 소모품은 동일 종류끼리 수량 누적 (x2, x3 …).
- 획득 시 로그.

### 장비 패널 (`E` 토글, Cogmind 스타일 우측 패널)
- 화면 우측 고정 너비 260px, 녹색 계열 테마.
- 패널 열린 동안 이동 차단.
- `↑↓` 항목 선택 (커서 노랑), `Enter` 장착/해제/사용, `Esc`/`E` 닫기.

레이아웃:
```
/ P A R T S /
─────────────────────
 W e a p o n
  [아이콘] 검  (|||||||)  ATK 7
 A r m o r
  [아이콘] 없음

/ I N V E N T O R Y /
─────────────────────
 >1 [아이콘] 검  (ATK 7)   [장착]
  2 [아이콘] 창  (ATK 9)
  3 [아이콘] 체력 물약  x2
─────────────────────
↑↓ 이동  Enter 장착/사용  Esc·E 닫기
```

PARTS 섹션은 장착 슬롯 (아이콘+이름+상태바). 미장착은 어두운 "없음".
INVENTORY 섹션은 번호+아이콘+이름+수치, 장착 항목에 `[장착]` 태그
(밝은 녹색). 아이콘(rpg-awesome) 과 한글 (NanumSquareNeo) 은 별도
TextSection 으로 혼합 렌더링.

### 전투 스탯
- 무기 장착 시 공격력 = 무기 값 (대체).
- 방어구 장착 시 기본 방어력 + 보너스.
- 해제 시 기본 스탯 복원.
- 비무장 기본: `PLAYER_ATK = 5`, `PLAYER_DEF = 1`.

## 글리프 스타일

아이템 글리프를 세 가지 스타일 중 하나로 표시. CLI 옵션 + `G` 키 순환.

| 스타일 ID | 폰트 |
|-----------|------|
| `ascii` | FiraMono-Medium.ttf |
| `unicode` | NotoSansSymbols2-Regular.ttf |
| `icon` | rpg-awesome.ttf |

| 아이템 | ASCII | Unicode (U+) | GameIcon (U+) |
|--------|-------|--------------|--------------|
| 검 | `/` | 🗡 1F5E1 | E946 (ra-broadsword) |
| 창 | `\|` | ⬆ 2B06 | EAAC (ra-spear-head) |
| 활 | `)` | ➤ 27A4 | E978 (ra-crossbow) |
| 가죽 갑옷 | `]` | 🛡 1F6E1 | EA96 (ra-shield) |
| 체력 물약 | `!` | ❤ 2764 | EA72 (ra-potion) |

- CLI `--glyph-style <ascii|unicode|icon>` 으로 초기 지정 (기본 `ascii`).
- `G` → ascii → unicode → icon → ascii 순환.
- 전환 시 모든 아이템 엔티티 글리프·폰트 즉시 갱신 + 로그.
- 신규 드롭 아이템은 현재 스타일로 스폰.

## XP 성장

장비/퀘스트 보상 외 전투 반복의 동기. 즉시 보상 + 레벨업으로 생존력
증가.

### 성장 상태 (`PlayerProgress`)
- 레벨, 현재 XP, 다음 레벨 필요 XP, 처치 수.
- 기본 Lv.1, XP 0.
- `xp_to_next_level(level)` 로 필요 XP 계산.
- 자동 저장/로드 포함, 새 게임 시 기본값 초기화.

### XP 획득
- 몬스터 처치 시 이름별 XP: 고블린 8, 오크 14, 트롤 24. 알 수 없으면 10.
- 처치 로그에 획득 XP 표시.
- 이미 HP ≤ 0 인 몬스터 재공격 시 중복 지급 방지.

### 레벨업
- 누적 XP ≥ 다음 레벨 필요량이면 레벨업. 한 번에 여러 레벨 가능 (반복).
- 레벨업 시 max HP +5, max MP +2, HP/MP 최대치 회복.
- 로그에 새 레벨 + 회복량 표시.
- 공격/방어 성장은 장비 스탯 갱신 흐름과 충돌 가능성 → 미적용.

### 남은 개선 후보
- 레벨업 시 능력치 선택 UI.
- 퀘스트 완료 XP 보상.
- Game Over summary 에 레벨/처치 수/XP 표시.
- 장비 보정과 분리된 기본 스탯 모델.

## 상태바 (player-status-bars)

플레이어 스프라이트 위에 HP/MP 프로그레스바.

### 동작
- HP 바는 스프라이트 바로 위, MP 바는 HP 위 (간격 없음).
- 왼쪽→오른쪽으로 채워짐 (`Anchor::CenterLeft`).
- HP 비율 색상: >50% 녹색, 25~50% 노랑, ≤25% 빨강.
- MP 바는 파랑.
- 모든 바 (전경·배경) 반투명 alpha 0.7.
- HP 배경 어두운 빨강 `rgba(0.6, 0, 0, 0.7)`, MP 배경 회색
  `rgba(0.35, 0.35, 0.35, 0.7)`.
- 플레이어 이동 (lerp 포함) 시 함께 이동 (부모-자식 entity).
- HP/MP 변경 프레임에 즉시 갱신 (`Changed<CombatStats>`).

### 레이아웃
```
MP 바  ████░░░░  y = +13px
HP 바  ██████░░  y = +11px
  '@'            y =   0px
```
- 너비 14px (TILE_SIZE = 16px, 좌우 1px 여백), 높이 2px.

### 구현
- `CombatStats` 에 `mp`, `max_mp` 필드 (`combat` 모듈).
- `HpBarFill`, `MpBarFill` 마커 컴포넌트.
- `hp_color(ratio: f32) -> Color` 순수 함수.
- `spawn_player` 가 배경+전경 4 entity 를 자식으로 스폰.
- `update_player_bars` 시스템 — `Changed<CombatStats>` 감지.

## 상단 상태 HUD

매 턴 판단할 핵심 상태를 한 줄로 항상 확인.

### 표시 위치
- 맵 상단 고정 높이.
- 패널 (장비/퀘스트/상점) 보다 낮은 z-index — 패널이 위에 깔림.
- 맵 너비 기준.

### 표시 내용
- 현재 존 이름, 누적 턴.
- 레벨, XP/필요 XP.
- HP/MP 숫자, 공격/방어, 골드.
- 장착 무기/방어구, 현재 맵 생성기 이름.

### 갱신 조건
- 존 / 턴 / 맵 / 인벤토리 / 장비 / 성장 / 플레이어 스탯 변경 시.
- HUD 문자열 생성은 순수 함수로 분리해 테스트.

### 남은 개선 후보
- 상태이상, 현재 목표, 위험도.
- 좁은 화면에서 줄바꿈/축약.
- Game Over summary 와 데이터 공유.

## Game Over · 새 게임

### Game Over 오버레이
- 플레이어에게 `Defeated` 컴포넌트 부여 시 중앙 오버레이.
- 표시: `GAME OVER`, 현재 존, 생존 턴, 조작 안내.
- `Esc` → 앱 종료 이벤트.
- 사망 안 한 상태에서는 게임 오버 입력 처리 안 함.

### 새 게임 (`R` / `N`)
Game Over 상태에서 `R` 또는 `N` 입력 시 새 run.
- `save/progress.ron` 삭제.
- `GlobalSeed` 를 새 random 값으로 교체.
- `GlobalTurn = 0` 초기화.
- `WorldState`, `ZonePersistence`, `NamedZoneConfig` 초기화.
- `QuestState`, `PlayerInventory`, `PlayerEquipment` 초기화 → 시작
  로드아웃 (`apply_start_loadout`) 적용.
- 장비/퀘스트/상점 패널 닫힌 상태.
- 이동 홀드 / 마우스 자동 경로 / 원격 타겟팅 (`RangedTargeting`) /
  ranged cursor 모두 정리.
- 아이템 / 몬스터 / 주민 / 포털 / 혈흔 entity 제거.
- 플레이어의 `Defeated` / `MovingTo` 컴포넌트 제거.
- HP/MP/공격/방어 스탯 기본값 복원.
- Town 맵을 새 시드로 생성하고 `ApplyMapEvent` 적용.

### 남은 개선 후보
- `R` = 마지막 체크포인트 재시도, `N` = 완전 새 게임 의미 분리.
- run summary: 처치 수, 획득 골드, 완료 퀘스트, 레벨.
- 사망 원인 / 마지막 피격 로그 오버레이 표시.

## 사망 시 입력 차단

플레이어가 `Defeated` 상태일 때 게임 오버 팝업 (`R/N/Esc`) 외 모든 입력
무시. 게임 오버 팝업 위에 다른 패널이 떠 어색해지는 것을 방지.

### 차단 시스템
액션을 발생시키거나 모달을 열 수 있는 모든 시스템:
- `player/mod.rs::on_mouse_click` — path 이동.
- `ranged/mod.rs::handle_ranged_input` / `handle_ranged_mouse` /
  `update_ranged_cursor`.
- `ui/help.rs::toggle_help_overlay` (H/?).
- `ui/minimap.rs::toggle_full_map` (M).
- `ui/equipment.rs::toggle_equipment_panel` (E).
- `ui/quest_panel.rs::toggle_quest_panel` (Q).
- `map/mod.rs::cycle_map_generator` (F1).
- `item/mod.rs::cycle_glyph_style` (G).

방식: 각 시스템에 `defeated_q: Query<(), With<Defeated>>` 추가하고
`if !defeated_q.is_empty() { return; }`. 또는 player Query 시그니처에
`Without<Defeated>` — query 가 비면 early return.

### 모드 정리
사망 시점에 원격 모드가 활성 중이면 cursor entity 가 남는다.
`game_over::reset_to_new_game` 에서 `RangedTargeting.active = false` 와
ranged cursor entity despawn 처리.
