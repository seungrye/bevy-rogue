# 아키텍처

**Bevy 0.13** 기반 로그라이크 게임. 진입점은 `src/main.rs`이며, 고정 크기 창을 설정하고 플러그인을 순서대로 등록한다:

```
MapPlugin → PlayerPlugin → MonsterPlugin → CombatFeedbackPlugin
  → ItemPlugin → GameUiPlugin → VillagerPlugin → ZonePlugin → QuestPlugin → SavePlugin
```

모든 게임 코드는 `src/modules/` 아래에 위치한다:

| 모듈 | 역할 |
|------|------|
| `core` | 2D 카메라 스폰 |
| `map` | 맵 리소스·타일 렌더링·가시성·FOV, 11종 절차 생성 알고리즘, GlobalSeed |
| `player` | 플레이어 엔티티, 방향키/WASD 이동(LERP 애니메이션), FOV 계산, 카메라 추적 |
| `monster` | 몬스터 스폰·AI·리스폰 타이머, FOV 기반 추적 |
| `combat` | 전투 판정, CombatStats 컴포넌트, 피해·사망 처리 |
| `combat_feedback` | 피격 시 화면 흔들림·핏자국(BloodStain) 이펙트 |
| `item` | 아이템 종류·인벤토리·장비·낙하 아이템 스폰, 글리프 스타일 |
| `villager` | 마을 NPC 스폰·턴제 이동·대화·퀘스트 NPC 글리프 |
| `zone` | 존 전환(ZoneTransitionEvent), WorldState 캐시, GlobalSeed 기반 결정론적 맵 재생성 |
| `quest` | RON 퀘스트 정의 로드·상태 머신, QuestRegistry, 퀘스트 아이템 스폰 |
| `save` | 매 턴 자동 저장/로드 (RON), GlobalSeed + zone_revealed(Base64) 방식 |
| `ui` | 스탯 패널·다이얼로그 로그·미니맵·장비창·상점·퀘스트 패널 |

## 좌표 체계

두 가지 좌표 공간이 혼용된다:
- **타일 그리드:** `0..80` × `0..50` (`MAP_WIDTH=80, MAP_HEIGHT=50`), `Map.tiles`에 `y * width + x` 인덱스로 저장
- **Bevy 월드(픽셀):** 화면 중심 기준; `TILE_SIZE`(16px)와 오프셋 보정값으로 상호 변환

## 맵 생성 및 시드 체계

- 게임 시작 시 `GlobalSeed(rand::random())` 리소스 생성
- 각 존의 맵 시드는 `zone_seed(global_seed, zone_id)` 로 결정론적 파생 (splitmix64)
- 저장 시 전체 타일 배열 대신 `global_seed` + `MapTile.revealed` 비트팩(Base64)만 보존
- 로드 시 동일 시드로 맵 재생성 → 항상 동일한 던전 레이아웃 보장

## 이벤트 흐름 예시

```
플레이어 이동 완료
  → PlayerActedEvent 발행
    → 주민·몬스터 턴 처리
    → GlobalTurn 증가
    → auto_save 트리거 (save/progress.ron 원자적 쓰기)

플레이어가 존 포탈 타일로 이동
  → ZoneTransitionEvent 발행
    → handle_zone_transition: 이전 맵 캐시·스냅샷 저장 → 새 맵 생성(zone_seed)
    → ApplyMapEvent: 타일 재렌더·플레이어·주민·몬스터 리스폰
```
