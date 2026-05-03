# 존 시스템

## 목적

여러 독립 구역(마을·숲·던전 층)을 이동하며 각 구역의 맵을
첫 방문 시 생성하고 이후에는 동일하게 복원한다.

## 존 구성

| ZoneId | 알고리즘 | 설명 |
|--------|---------|------|
| `Town` | `organic_village` | 퀘스트 수락·완료 장소 |
| `Forest` | `forest` | 마을과 던전을 잇는 통로 |
| `Dungeon(1)` | `bsp` | 던전 1층 |
| `Dungeon(2)` | `bsp` | 보석이 있는 던전 2층 |

## 존 연결

```
Town ←→ Forest ←→ Dungeon(1) ←→ Dungeon(2)
```

- Town  남쪽 경계 → Forest 북쪽 진입
- Forest 남쪽 경계 → Dungeon(1) 북쪽 진입
- Dungeon(1) 특정 방에 계단(↓) → Dungeon(2)
- Dungeon(2) 특정 방에 계단(↑) → Dungeon(1)

## 포털 엔티티

- `ZonePortal { target: ZoneId }` 컴포넌트를 가진 엔티티
- 플레이어가 포털 타일에 이동하면 `ZoneTransitionEvent` 발행
- 맵 경계 포털: 맵 가장자리의 Floor 타일 (중앙부)
- 계단 포털: 방 중앙에 `>` / `<` 글리프로 표시

## 맵 메모리

- `WorldState.maps: HashMap<ZoneId, Map>` 에 방문한 맵을 캐시
- 첫 방문: `zone_seed(global_seed, zone_id)` 로 시드 파생 → 알고리즘으로 생성 → 캐시
- 재방문: 캐시에서 복원 (동일 시드이므로 타일 배치 항상 동일)
- 저장 시: `GlobalSeed` + `revealed_tiles`(비트팩 Base64)만 보존 — 전체 타일 배열 저장 안 함
- 로드 시: `zone_seed` 로 맵 재생성 후 `revealed_tiles` 복원
- 엔티티(몬스터·낙하 아이템)는 재방문 시 재스폰 (몬스터 리젠은 로그라이크 표준)
- 퀘스트 아이템(보석 등)은 퀘스트 상태로 추적 — 수집 후엔 재스폰되지 않음

## 동작 명세

- [x] `WorldState` 리소스: 현재 존 + 맵 캐시
- [x] 존 전환 시 이전 맵을 캐시에 저장하고 새 맵으로 교체
- [x] `ApplyMapEvent` 발행 → 몬스터·아이템·빌리저 재스폰 트리거
- [x] 포털 진입 시 플레이어 위치를 도착 존의 적절한 지점으로 이동
- [x] 계단 포털은 `>` (내려가기) / `<` (올라가기) 글리프로 맵에 표시
- [x] 마을·숲 구분: `MapType::Village` vs `MapType::Dungeon` 유지
- [x] 포털 충돌은 `MovingTo` 목적지 기준 — Transform 은 lerp 중간값이므로 미사용
- [x] `TriggerEffect::ShowMessage` 제거 — ZonePortal 로그 메시지로 대체됨

## 시작 존

게임 시작 시 `Town` 에서 시작.
