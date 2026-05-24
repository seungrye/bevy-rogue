# 방향 시야 + 잠입(스텔스) 퀘스트

## 목적
1. 플레이어/적의 시야를 **방향(facing) 기반 두-반원**으로 바꿔(앞 큰 반원, 뒤 작은 반원) 현재 원형 시야의 어색함을 해소.
2. 그 위에 **잠입 퀘스트**: 적에게 탐지되지 않고 특정 구역을 통과. 탐지되면 (조금 더 강한) 가드와 전투로 전환되고 무탐지 보너스를 잃지만 클리어는 가능.

---

## Phase 1: 방향 시야 (facing-based FOV)

### Facing
- 플레이어·몬스터에 **`Facing(IVec2)`** 컴포넌트. 이동할 때마다 마지막 이동 방향으로 갱신. 정지 시 유지. 초기 기본값 `(0,-1)`(아래) 등.
- 몬스터도 이동(배회/추적) 시 facing 갱신.

### 두-반원 가시성
타일 `t`와 시야 주체(위치 `p`, facing `f`)에 대해:
- `front = dot(t - p, f) >= 0` (수직이면 front 로 간주, 관대).
- 가시 반경: front 면 `FOV_FRONT`(기본 8), back 면 `FOV_BACK`(기본 3).
- `dist² <= radius²` **그리고** `is_line_of_sight_clear` 면 가시.
- 상수 `FOV_FRONT`/`FOV_BACK` 분리(튜닝). f 가 0 벡터면(초기) 전방향 원형으로 폴백.

### 적용처
- 플레이어 `update_fov`: 위 모델로 visible/revealed 계산.
- 몬스터 `can_see_player`: monster facing 기준 두-반원 + LoS. (지금은 반경 원형) → 등 뒤의 플레이어는 안 보임.

### 테스트
- `is_in_view(p, facing, t, front_r, back_r, map)` 순수 함수로 분리해 경계 전수: 정면/측면(dot=0)/후면, front_r/back_r 경계, LoS 차단, facing 0 폴백. 4/8방향 facing.
- update_fov/can_see_player App 하네스로 방향별 가시 검증.

---

## Phase 2: 잠입 퀘스트

### 탐지 → 전투 (B안)
- 가드(잠입 구역 몬스터)가 방향 시야로 플레이어를 보면: 기존 alert/추적/전투가 발동(이미 구현된 monster_turn 흐름) **+** 퀘스트 플래그 `stealth_blown`(해당 퀘스트 활성 중) set.
- 탐지돼도 목표 도달로 클리어 가능. 단 `stealth_blown` 이면 **무탐지 보너스 미지급**.

### 가드 (조금 더 강함)
- 잠입 구역에 가드 몬스터 배치. 스탯 = **플레이어 현재 effective ATK/DEF + HP 기반 ×1.2**(상수 `GUARD_POWER_MULT` 튜닝). 레벨 무관하게 늘 "조금 더 셈" → 정면 돌파보다 잠입 유도.
- 가드 배치 방법(택1, 구현 시 결정): (a) 신규 quest action `SpawnGuards{zone, count}` (b) 잠입 전용 zone 의 몬스터를 가드 타입으로. → 데이터 주도 위해 (a) 선호.

### 퀘스트 정의 (RON, 기존 시스템 활용)
- 신규 플래그 조건/액션은 기존 `QuestCondition::Flag`/`QuestAction::SetFlag`/`ClearFlag` 재사용.
- 흐름 예: 퀘스트 시작 → 가드 스폰 → 플레이어가 목표 zone 도달(`InZone`) → 완료. 완료 액션에서 `stealth_blown` 없으면 보너스(추가 골드/아이템/XP) 지급.
- 탐지 시 `stealth_blown` set 은 시스템(몬스터 탐지)이 담당 — 활성 잠입 퀘스트가 있을 때만.

### 테스트
- 탐지 시 플래그 set / 미탐지 시 미set, 보너스 지급 분기, 가드 스탯 스케일(순수 함수 `guard_stats(player_power) -> CombatStats`), 가드 스폰 액션.

---

## 단계 진행
Phase 1(방향 시야)을 먼저 구현·검증(단독 가치) → Phase 2(잠입 퀘스트). 각 단계 spec→test→구현, nightly 100% 커버리지 유지. 헤드리스라 시야 feel 은 수동 플레이테스트 권장.
