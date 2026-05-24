# 아키텍처

**Bevy 0.13** 기반 로그라이크 게임. 진입점은 `src/main.rs`이며, 고정 크기 창을 설정하고 플러그인을 순서대로 등록한다:

```
RapierPhysicsPlugin → MapPlugin → PlayerPlugin → CombatPlugin → MonsterPlugin
  → CombatFeedbackPlugin → ElementalPlugin → ProjectilePlugin → RangedPlugin
  → ItemPlugin → GameUiPlugin → VillagerPlugin → ZonePlugin → QuestPlugin → SavePlugin
```

모든 게임 코드는 `src/modules/` 아래에 위치한다:

| 모듈 | 역할 |
|------|------|
| `core` | 2D 카메라 스폰 |
| `map` | 맵 리소스·타일 렌더링·가시성·방향 시야(FOV), 23종 절차 생성 알고리즘, GlobalSeed |
| `player` | 플레이어 엔티티, 방향키/WASD 이동(LERP 애니메이션), Facing 기반 FOV 계산, 카메라 추적 |
| `combat` | 전투 판정(`calc_damage`), CombatStats 컴포넌트, 피해·사망 처리 |
| `monster` | 몬스터 스폰·AI·리스폰 타이머, 방향 시야 기반 추적, 잠입 가드(`guard_stats`/`SpawnGuards`), 플레이어 광량 반영 탐지 보정 |
| `lighting` | 조명/그림자 — 광원(`LightSource`) 반경 기반 타일 광량(`LightMap`), 어둠 디밍 + 어둠 은신(가드 탐지 반경 감소) |
| `combat_feedback` | 피격 시 핏자국(BloodStain)·핏방울 파티클·피격 섬광 이펙트 |
| `elemental` | 무기/몬스터 원소 속성, 원소 반응(융해·파쇄 등), 지속피해·기절·둔화 상태 |
| `projectile` | 화살 등 투사체 발사·비행·충돌(Rapier 센서), 명중 시 피해·원소 부여 |
| `ranged` | 활 원거리 조준 모드(키보드/마우스 타겟팅), 사거리·시야 판정 후 발사 |
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

## 시야(방향 FOV)

플레이어·몬스터는 `Facing(IVec2)` 컴포넌트로 마지막 이동 방향을 추적하고,
시야는 **방향 기반 두-반원**이다(앞 큰 반원 `FOV_FRONT=8`, 뒤 작은 반원 `FOV_BACK=3`).
순수 함수 `is_in_view(...)`가 거리·반원·LoS(Bresenham)를 판정하며, 플레이어 `update_fov`와
몬스터 탐지가 공유한다. `Facing`이 0 벡터면 전방향 원형으로 폴백.
설계 상세는 [`specs/stealth-and-directional-fov.md`](../specs/stealth-and-directional-fov.md).

## 조명/그림자

광원(`LightSource { radius }`) 반경 기반의 2단계 광량(`LightLevel::Bright/Dark`)을
`lighting` 모듈이 단일 정본 `LightMap` 리소스에 매 프레임 계산한다(`update_light_map`).
플레이어는 기본 시야광(`PLAYER_LIGHT_RADIUS`)을 자동 보유하고, 횃불(`SpawnTorchEvent`)도
같은 컴포넌트로 표현된다. 순수 함수 `light_level(tile, sources)`(반경 경계 포함)로 광량을
계산하고, 같은 `LightMap` 을 **렌더(어둠 디밍, `apply_light_dimming`/`tile_render_color`)** 와
**탐지(가드 시야)** 양쪽이 공유한다. 잠입 연계: 어둠 타일은 `effective_vision_radius` 로
가드 탐지 반경이 `DARK_VISION_PENALTY` 만큼 줄어(은신 보너스), `monster_turn`·`danger_tiles`
(위험 오버레이)에 일관 반영된다. 설계는 [`specs/additional-systems.md`](../specs/additional-systems.md) §C.

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
