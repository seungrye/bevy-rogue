# 동적/파괴 가능 지형

## 목적
런타임에 지형을 바꾼다.
- **(A) 퀘스트 스크립트형**: 주요 NPC 폭발 → 주변 지형 변형(잔해) → 숨겨진 던전 개방 → 탐색.
- **(B) 전투/마법 폭발형**: 파이어볼·폭탄 등이 터지면 영향 범위의 파괴 가능 지형이 부서지고 범위 내 엔티티가 피해.

두 형태 모두 같은 **폭발 코어**를 공유한다.

## 1. 타일 인프라 (파괴 모델 B+C)
- `TileKind::DestructibleWall` 추가: **벽처럼 동작**(`is_walkable()=false`, `blocks_sight()=true`)하지만 **파괴 가능**. 집/건물 벽 등 "부술 수 있는 구조물". 글리프/색은 일반 Wall 과 살짝 구분(예 ascii `▒`/약간 밝은 회색).
- `TileKind::Rubble` 추가: **통행 가능**(`is_walkable()=true`), **시야 통과**(`blocks_sight()=false`), 부서진 잔해(예 ascii `%`, 칙칙한 회갈색).
- 렌더(`tile_glyph`/`tile_base_color`)·미니맵 색에 두 타일 매핑 추가.
- `TileKind::is_destructible()` — **`DestructibleWall` 만 true**. 일반 `Wall`(테두리·자연 암벽)은 파괴 불가(C).
- `fn destroy_tile(map, x, y) -> bool`: (x,y)가 `DestructibleWall` 이고 맵 테두리가 아니면 → `Rubble`(B) 로 바꾸고 true. 그 외 false(경계·일반벽 보존).
- **건물 생성기가 DestructibleWall 사용**: 마을/실내 생성기(`walled_town`, `voronoi_districts`, `grid_village`, `organic_village`, `bsp_indoor`, `prefab`)의 **건물 벽**을 `DestructibleWall` 로 생성(맵 테두리/외곽은 일반 Wall 유지). → "마을에서 폭탄 터지면 집 일부 파괴" 가 성립. 던전/동굴 자연벽은 그대로 Wall(파괴 불가). (전부 한 번에 어려우면 walled_town/grid_village/bsp_indoor 우선.)

## 2. 국소 재렌더
- 지금은 맵 교체 시 전체 재렌더만 있음. 신규 이벤트 `TilesChangedEvent { tiles: Vec<(usize,usize)> }` + 시스템: 해당 좌표의 타일 엔티티 글리프/색만 갱신(없으면 전체 재렌더 재사용도 허용하되 국소 선호).

## 3. 폭발 코어 (공통)
- 이벤트 `ExplosionEvent { center: (usize,usize), radius: i32, terrain: bool, entity_damage: i32 }`.
- 시스템 `handle_explosion`:
  - `terrain` 이면 반경 내(원형, dist²≤r²) 각 타일 `destroy_tile` → 바뀐 좌표 모아 `TilesChangedEvent` 발행.
  - `entity_damage>0` 이면 반경 내 몬스터/플레이어 `CombatStats.hp -= entity_damage`(사망 처리 기존 흐름 재사용). 플레이어 포함 여부는 가능.
  - 폭발 비주얼은 `combat_feedback`(핏자국/파티클) 또는 간단한 이펙트 재사용.
- 순수 함수로 `tiles_in_radius(center, radius, w, h) -> Vec<(usize,usize)>` 분리(테스트 용이).

## 3.5 엔티티 HP 회복 (폭발/전투 생존자)
- 폭발·전투로 피해를 입었지만 죽지 않은 엔티티는 **시간이 지나며 HP 회복**.
- 신규 시스템 `regenerate_health`(매 턴 `PlayerActedEvent` 기준): 플레이어·몬스터의 `CombatStats.hp < max_hp` 면 일정량 회복(예 상수 `REGEN_PER_TURN` 또는 `REGEN_INTERVAL` 턴마다 +1), `max_hp` 초과 금지. 사망(hp<=0/Defeated)은 회복 안 함.
- 순수 함수 `regen_hp(hp, max_hp, amount) -> i32` 로 분리(경계 테스트). 튜닝 상수 분리.
- (선택) 최근 피격 후 일정 턴 지연 후 회복 시작 — 단순화하려면 매 턴 소량 회복으로 시작.

## 4. (A) 퀘스트 스크립트형
- 신규 `QuestAction::Explode { radius: i32, terrain: bool, entity_damage: i32 }` — 현재 트리거 위치(또는 지정 좌표) 기준 `ExplosionEvent` 발행. 시나리오: NPC 폭발 액션 → 지형 변형 + (이어서) `OpenPortal` 로 숨겨진 던전 개방.
- (숨겨진 던전 "탐색" 자체는 기존 OpenPortal + zone 시스템으로 충족.)

## 5. (B) 전투/마법 폭발형
- 폭발성 소스가 `ExplosionEvent` 발행: 예) 신규 소비아이템 "폭탄"(사용 시 플레이어 위치 폭발) 또는 파이어볼 마법/투사체 명중 시 폭발. (기존 projectile/elemental/consumable 과 연계.)
- 최소 구현: 소비아이템 "폭탄" 1종(범위 파괴+피해)로 B 시연.

## 6. 결정 (기본값 — 변경 가능)
- 파괴: Wall→Rubble(통행 가능), **테두리 파괴 불가**.
- 폭발은 범위 내 **엔티티 피해 포함**(몬스터+플레이어).
- A·B 모두. 우선순위: 코어 인프라 + A(퀘스트/던전) 먼저 → B(전투 폭탄) 후속.

## 7. 테스트 (100% 커버리지, 한글 의도서술형)
- `is_destructible`/`destroy_tile`(테두리 보존·Wall만), `tiles_in_radius` 경계, `handle_explosion`(지형 파괴+엔티티 피해+TilesChangedEvent), 국소 재렌더, `QuestAction::Explode`, Rubble 술어/렌더, 폭탄 아이템(B).
