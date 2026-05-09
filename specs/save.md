# 저장·로드 시스템

로그라이크 특성에 맞게 매 턴 자동 저장, 앱 재시작 시 자동 로드. 별도 저장
조작 없이 항상 직전 턴까지의 상태가 보존된다.

## 자동 저장

- `PlayerActedEvent` 발생마다 `save/progress.ron` 에 저장.
- 원자적 쓰기: `save/progress.ron.tmp` 에 쓴 뒤 rename — 쓰기 중 충돌 방지.
- `save/` 디렉터리 없으면 자동 생성.
- 저장 실패 시 `error!` 로그, 게임 진행은 계속.

## 자동 로드

- 앱 시작(`PostStartup`) 에 `save/progress.ron` 존재 여부 확인.
- 파일 있으면 파싱 후 모든 리소스 복원 → `ApplyMapEvent` 발행.
- 파일 없거나 파싱 실패·버전 불일치 시 신규 게임 시작 (`warn!`).

## 저장 데이터 (`SaveData`)

| 필드 | 타입 | 설명 |
|------|------|------|
| `version` | `u32` | 형식 버전 (현재 5) |
| `global_seed` | `u64` | 전역 시드 — 모든 존 맵을 결정론적 재생성 |
| `global_turn` | `u64` | 누적 턴 |
| `player_tile` | `[usize; 2]` | 플레이어 타일 |
| `player_hp/max_hp/mp/max_mp/attack/defense` | `i32` | 스탯 |
| `inventory` | `PlayerInventory` | 아이템·소모품·금화 |
| `equipment` | `PlayerEquipment` | 장착 |
| `quest_state` | `QuestState` | 퀘스트 진행 |
| `current_zone` | `ZoneId` | 현재 존 |
| `zone_revealed` | `HashMap<ZoneId, String>` | 존별 탐험 (비트팩 → Base64) |
| `zone_persistence` | `HashMap<ZoneId, ZoneSnapshot>` | 혈흔·몬스터 슬롯 |
| `discovered_markers` | `DiscoveredMarkers` | 미니맵 마커 |
| `named_zones` | `NamedZoneConfig` | 동적 Named 존의 생성기·원점 존 |

### 존 시드 파생

```
zone_seed(global_seed, zone_id) = splitmix64(global_seed + zone_idx)
```

| ZoneId | zone_idx |
|--------|---------|
| `Town` | 0 |
| `Forest` | 1 |
| `Dungeon(n)` | 100 + n |
| `Named(s)` | FNV-1a hash(s) |

- 맵 타일 배열은 저장하지 않고 로드 시 `zone_seed` 로 결정론적 재생성.
- `zone_revealed` 인코딩: `MapTile.revealed` → `Vec<bool>` → 비트팩(1bit/tile)
  → Base64. 80×50 = 4000 tiles → 668 chars (RON 배열 대비 ~3× 압축).
- `MapTile.visible` 은 로드 시 `false` 초기화 (FOV 가 첫 프레임에 재계산).

## 맵 생성기 규약

- 모든 `MapGenerator` 구현체는 `seed: u64` 인자를 받아
  `StdRng::seed_from_u64(seed)` 사용 — `thread_rng()` 금지.
- `Map` 구조체에 `seed: u64`, `algorithm: String` 기록.
- 헬퍼 함수도 `&mut impl Rng` 를 받아 결정론적 재현 보장.
- `GlobalSeed` 리소스: 시작 시 `rand::random()` 으로 초기화, 로드 시 복원.

## 사망 상태 저장·로드 방어

### 증상
사망 후 종료·재시작 시 게임 오버 팝업이 뜨지 않고 HP=0 으로 조작 가능.

### 원인
- `auto_save` 가 `PlayerActedEvent` 마다 저장 — 사망 직전 이동 후 같은
  프레임에 monster damage 가 HP=0 으로 만들면 그 상태가 저장될 수 있다.
- `Defeated` 컴포넌트는 ECS 컴포넌트라 세이브에 미포함 — 로딩 시 UI 가
  사망 인지 못 한다.

### 수정
- `auto_save` 의 `player_q` 시그니처에 `Without<Defeated>` 추가 — 사망
  시점 저장 skip, 마지막 정상 턴 보존.
- `load_if_save_exists` 에서 `stats.hp <= 0` 이면 player entity 에
  `Defeated` 부여 — 손상된 세이브 / race 방어로 게임 오버 UI 즉시 트리거.

## 구현 위치
- `src/modules/save/mod.rs` — `SavePlugin`, `auto_save`, `load_if_save_exists`
- `PostStartup` 스케줄: 모든 `Startup` 시스템 이후 로드 실행
- 로드 후 `ApplyMapEvent` 발행 → 맵 재생성·플레이어/주민/몬스터 리스폰 자동.
- 세이브 버전 불일치 시 구 파일 무시 (마이그레이션 없음, 신규 게임).
