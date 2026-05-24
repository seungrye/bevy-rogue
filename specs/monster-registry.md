# 몬스터 데이터 주도화 (RON 레지스트리) + site 카탈로그

## 목적
지금 몬스터만 데이터 주도가 아니다(코드 const 테이블 + 이름 문자열 매칭). 아이템/퀘스트/주민처럼
**`assets/monsters/*.ron` 레지스트리**로 통일해 일관성 확보 + 새 몬스터 추가를 데이터만으로.
또한 site/webapp 퀘스트 편집기에 **몬스터 카탈로그 메뉴**를 추가(villagers/items/zones 카탈로그와 동형).

## 현재 (제거 대상)
- `src/modules/monster/mod.rs`: const 테이블 `("고블린","g",[0.2,0.8,0.2],6,3,0,6,1.5), ("오크",...), ("트롤",...)` = (이름, 글리프, 색[3], hp, atk, def, 시야, 속도).
- `src/modules/elemental/mod.rs::monster_element(name)`: 이름 문자열로 원소 매칭(고블린=독/오크=불/트롤=얼음).
→ 한 몬스터 정체성이 이름 문자열로 두 곳에 흩어짐.

## MonsterDef (RON 스키마 — bevy-rogue·site 공통 정본)
```ron
// assets/monsters/monsters.ron — Vec<MonsterDef>
MonsterDef(
    id: "goblin",            // 영문 안정 식별자
    display_name: "고블린",
    glyph: "g",              // (MVP: 단일 글리프. 추후 ascii/unicode/game_icon 3종 확장 여지)
    color: (0.2, 0.8, 0.2),  // RGB
    hp: 6, attack: 3, defense: 0,
    vision_radius: 6,
    speed: 1.5,
    element: Some("poison"), // "fire"/"ice"/"poison"/"lightning" 또는 None
    spawn_weight: 1.0,       // (선택) 자연 스폰 가중치. 기본 1.0

    // ── 스폰 규칙 (존/조건) ──
    zones: [],               // 나오는 존 목록. 비어있으면 모든 일반 존(예: [Dungeon, Forest] / Named). 비면 제한 없음.
    spawn_condition: None,   // Option<QuestCondition> — 이 조건이 참일 때만 자연 스폰.
                             //   퀘스트 전용/조건부에 사용: 예) HasFlag("dragon_awakened"),
                             //   PhaseIs(quest:"x", phase:"active"), Not(HasFlag("boss_slain")), InZone(...).
    quest_only: false,       // true 면 자연 스폰 안 됨(오직 QuestAction::SpawnMonster 로만 등장 — 보스/퀘스트 전용).
)
```
초기 데이터: goblin(고블린, 독), orc(오크, 불), troll(트롤, 얼음) — 기존 수치 그대로. 신규 예: 특정 던전 존에만 나오는 몬스터, 퀘스트 플래그가 켜져야 나오는 몬스터, 보스(quest_only).

## 스폰 규칙 동작 (퀘스트 아이템 스폰과 동형)
- 자연 스폰 시스템은 후보 MonsterDef 중 **현재 존이 `zones` 에 포함**(또는 zones 비어있음)되고 **`spawn_condition` 이 참**(없으면 항상)이며 `quest_only==false` 인 것만 가중치로 선택. → "특정 존에만", "특정 조건에만 나옴/안 나옴(Not)"이 데이터로 표현됨.
- **퀘스트 전용/보스**: `quest_only: true` + 신규 **`QuestAction::SpawnMonster { id: String, count: u32 }`** 로 퀘스트가 명시적으로 스폰(존/위치 = 현재). (기존 `SpawnGuards{count}` 는 플레이어×1.2 스케일 가드 전용으로 유지; SpawnMonster 는 임의 MonsterDef 를 기본 스탯으로.)
- 레벨 게이팅이 필요하면 `spawn_condition` 에 레벨 조건을 두거나, 기존 드롭의 tier 가중처럼 MonsterDef 에 `tier` 추가 여지(선택).

## bevy-rogue 구현
- `MonsterRegistry`(Resource) + RON 로드 시스템(item/villager 레지스트리 패턴 그대로, Box::leak 등 동일 관용).
- 몬스터 스폰이 const 테이블 대신 레지스트리에서 읽도록 교체.
- `monster_element` 제거 → 원소는 MonsterDef.element 에서. elemental 의 weapon_element 처럼 문자열→Element 매핑 재사용.
- 가드(SpawnGuards)도 레지스트리 기반으로(가드용 MonsterDef 또는 동적 스탯 — 기존 guard_stats 유지하되 글리프/이름은 정의에서).
- 기존 테스트(이름 매칭 단언 등) 갱신. nightly 100% 커버리지.

## site/webapp 카탈로그 (bevy-rogue 리팩터 후, 실제 RON 미러링)
- 기존 villagers/items/zones 카탈로그(`/api/quests/{villagers,items,zones}` + 타입 + UI)와 **동형**으로 `monsters` 추가:
  - `src/types/` 에 MonsterDef 타입, RON 파서/직렬화에 MonsterDef 지원(필요시), `/api/quests/monsters` (import/export), 카탈로그 UI 메뉴/페이지.
  - 퀘스트가 향후 몬스터 id 를 참조할 수 있게 카탈로그로 노출(현재 SpawnGuards 는 count 만; 확장 여지).
- npm test/build 통과.

## 순서
지형 코어 완료 → bevy-rogue 몬스터 레지스트리 리팩터(monster/elemental 같은 파일 편집이라 지형 후) → site 몬스터 카탈로그(실제 RON 스키마 미러링).
