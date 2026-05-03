# 봉인의 각성 (World Fracture) — Giga 퀘스트

## 목적

비선형 멀티 엔딩 메인 퀘스트. 세계 봉인이 약해지고 있으며 플레이어가 4개의 성물을 모아 봉인을 강화해야 한다. gem_quest / alchemist_quest 진행 상태와 보유 아이템 조합에 따라 5가지 결말로 분기한다.

## NPC 연결

- **담당 NPC**: 노인 (`villager.rs` VILLAGER_DATA의 `quest_id: Some("world_fracture")`)
- **선행 퀘스트**: `gem_quest` 완료 필수 (dormant → awakened 전환 조건)

## 수집 아이템

| 아이템 | 획득 경로 |
|--------|-----------|
| 영원의 보석 (`eternal_gem`) | 던전 2층 스폰 |
| 현자의 돌 (`philosophers_stone`) | gem_quest 보상으로만 획득 (장로 교환) |
| 용비늘 (`dragon_scale`) | 던전 2층 스폰 |
| 고대 주문서 (`ancient_scroll`) | 던전 1층 스폰 |

## 페이즈 구조 (22단계)

```
dormant
  └─ [auto: gem_quest.done] → awakened
awakened
  └─ [on_interact] → need_alchemist | prologue_done
need_alchemist
  └─ [auto: alchemist 시작됨] → prologue_done
prologue_done
  └─ [on_interact] → gathering_all
gathering_all  ← 핵심 수집 단계
  ├─ [auto 1순위] 4성물 + gem_done + alchemist_legendary → legendary_ready
  ├─ [auto 2순위] 4성물 + alchemist_normal             → normal_ready
  ├─ [auto 3순위] 4성물                                → all_gathered
  ├─ [auto 4순위] 현자의 길 (gem+stone only)           → wisdom_alt_entry
  ├─ [auto 5순위] 전사의 길 (scale+scroll only)        → warrior_alt_entry
  ├─ [auto 6-9순위] 힌트 페이즈 4종                    → hint_*
  └─ [auto 10-11순위] 초기 힌트                        → hint_dungeon2 | hint_dungeon1
hint_* (4종)
  └─ [auto] gathering_all로 복귀
wisdom_alt_entry / warrior_alt_entry
  └─ [on_interact] → wisdom_alt_choice / warrior_alt_choice
wisdom_alt_choice / warrior_alt_choice
  └─ [on_interact] → wisdom_ending | warrior_ending | 거부 시 gathering_all 복귀
all_gathered
  └─ [on_interact] → ritual_now_or_wait
ritual_now_or_wait
  └─ [on_interact] → ritual_confirmation | 대기
ritual_confirmation
  └─ [auto] legendary_ready | normal_ready | incomplete_ending
legendary_ready / normal_ready
  └─ [on_interact] → legendary_ending | normal_ending
```

## 5가지 결말

| 결말 | 조건 |
|------|------|
| `legendary_ending` | 4성물 + gem_quest 완료 + alchemist_legendary 완료 |
| `normal_ending` | 4성물 + alchemist_normal 또는 legendary 완료 |
| `incomplete_ending` | 4성물, alchemist 미완료 (강행) |
| `wisdom_ending` | 영원의 보석 + 현자의 돌 (gem_quest 전용 경로) |
| `warrior_ending` | 용비늘 + 고대 주문서 (alchemist_legendary 전용 경로) |

## 비선형성 설계

- `auto_advance` 11단계 우선순위 체계 — 조건 충족 즉시 자동 전환
- `on_interact` 3단계 중첩 Branch 조건
- 교차 퀘스트 참조: `PhaseIs(quest: "gem_quest", ...)`, `PhaseIs(quest: "alchemist_quest", ...)`
- 대안 경로(현자/전사) — 4성물 없이도 완료 가능한 2성물 클리어 루트
- `alchemist_quest` 완료 수준(normal vs legendary)에 따른 결말 품질 차이

## 스폰 정보

| 아이템 | 존 | 스폰 키 |
|--------|-----|---------|
| `eternal_gem` | Dungeon(1) | `world_fracture_gem_d1` |
| `eternal_gem` | Dungeon(2) | `world_fracture_gem_d2` |
| `dragon_scale` | Dungeon(2) | `world_fracture_scale` |
| `ancient_scroll` | Dungeon(1) | `world_fracture_scroll` |
| `ancient_scroll` | Forest | `world_fracture_scroll_forest` |
