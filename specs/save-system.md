# 저장·로드 시스템

## 목적

로그라이크 특성에 맞게 **매 턴 자동 저장**, 앱 재시작 시 **자동 로드**한다.
별도 저장 조작 없이 항상 직전 턴까지의 상태가 보존된다.

## 동작 명세

### 자동 저장

- [x] `PlayerActedEvent` 발생마다 `save/progress.ron` 에 자동 저장
- [x] 원자적 쓰기: `save/progress.ron.tmp` 에 쓴 뒤 파일명 교체 (쓰기 중 충돌 방지)
- [x] `save/` 디렉터리 없으면 자동 생성
- [x] 저장 실패 시 `error!` 로그 출력, 게임 진행은 계속

### 자동 로드

- [x] 앱 시작(`PostStartup`)에 `save/progress.ron` 존재 여부 확인
- [x] 파일 있으면 파싱 후 모든 리소스 복원 → `ApplyMapEvent` 발행
- [x] 파일 없거나 파싱 실패·버전 불일치 시 신규 게임 시작 (`warn!` 로그)
- [x] 로드 후 플레이어 위치·HP 등 모든 상태 직전 턴 기준으로 복원

## 저장 데이터 (`SaveData`)

| 필드 | 타입 | 설명 |
|------|------|------|
| `version` | `u32` | 형식 버전 (현재 3) |
| `global_seed` | `u64` | 전역 시드 — 모든 존 맵을 결정론적으로 재생성 |
| `global_turn` | `u64` | 누적 턴 수 |
| `player_tile` | `[usize; 2]` | 플레이어 타일 좌표 |
| `player_hp/max_hp` | `i32` | 현재·최대 HP |
| `player_mp/max_mp` | `i32` | 현재·최대 MP |
| `player_attack/defense` | `i32` | 공격·방어력 |
| `inventory` | `PlayerInventory` | 아이템·소모품·금화 |
| `equipment` | `PlayerEquipment` | 장착 무기·방어구 |
| `quest_state` | `QuestState` | 퀘스트 진행(phases, spawned, flags) |
| `current_zone` | `ZoneId` | 현재 존 ID |
| `zone_revealed` | `HashMap<ZoneId, String>` | 존별 탐험 기록 (비트팩 → Base64) |
| `zone_persistence` | `HashMap<ZoneId, ZoneSnapshot>` | 혈흔·몬스터 슬롯 |
| `discovered_markers` | `DiscoveredMarkers` | 미니맵 마커 |

### 존 시드 파생 방식

```
zone_seed(global_seed, zone_id) = splitmix64(global_seed + zone_idx)
```

| ZoneId | zone_idx |
|--------|---------|
| `Town` | 0 |
| `Forest` | 1 |
| `Dungeon(n)` | 100 + n |
| `Named(s)` | FNV-1a hash of s |

- 맵 타일 배열은 저장하지 않고, 로드 시 `zone_seed` 로 결정론적 재생성
- `zone_revealed` 인코딩 파이프라인: `Vec<bool>` → 비트팩(1bit/tile) → Base64 → `String`
  - 80×50=4000 tiles: 4000 bytes(bool) → 500 bytes(bitpack) → **668 chars(base64)**
  - RON 배열(`[0, 255, ...]`) 대비 약 3× 압축 — 존 10개 방문 시 ~7KB
- `visible_tiles` 는 로드 시 `vec![false; w*h]` 로 초기화 (FOV가 첫 프레임에 재계산)

## 맵 생성기 규약

- [x] 모든 `MapGenerator` 구현체는 `seed: u64` 인자를 받아 `StdRng::seed_from_u64(seed)` 사용
- [x] `Map` 구조체에 `seed: u64`, `algorithm: String` 필드 추가 — 생성 시 기록
- [x] `thread_rng()` 사용 금지 — 동일 시드로 항상 동일한 맵 재현 보장
- [x] 내부 헬퍼 함수(`split_rect`, `connect_rooms`, `carve_path` 등)도 `&mut impl Rng` 로 rng 전달받음
- [x] `Map::new()` 는 `seed: 0, algorithm: String::new()` 로 초기화 (추후 덮어씀)
- [x] `GlobalSeed` 리소스: 게임 시작 시 `rand::random()` 으로 초기화, 로드 시 복원

## 구현 세부사항

- `src/modules/save/mod.rs` — `SavePlugin`, `auto_save`, `load_if_save_exists`
- `PostStartup` 스케줄: 모든 `Startup` 시스템 이후 로드 실행
- 로드 후 `ApplyMapEvent` 발행 → 맵 타일 재생성·플레이어·주민·몬스터 리스폰 자동 처리
- 세이브 버전 불일치 시 구 파일 무시 (마이그레이션 없음, 신규 게임)
