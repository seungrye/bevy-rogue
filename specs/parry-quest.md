# 파리 고수와 기계 공학자 — 퀘스트

원작: 浦島太郎なおっさん、最新ダンジョンでパリィ無双 (詩永あえし)

## 스토리 요약 (한글 번역·각색)

천재 기계공학자 그레체가 자신이 개발한 시제 무기 '파암추'를 테스트할
파일럿을 구하고 있다. 플레이어는 D급 던전에서 그 무기로 강철 갑주 보스를
패리로 격파하고, 그레체의 전속 테스트 파일럿으로 채용된다.

## 등장인물

| NPC    | 위치   | 역할 |
|--------|--------|------|
| 그레체 | 마을   | 퀘스트 수여자 (신대중공 천재 기계공학자) |

플레이어 = 가지 류이치 역할 (8년 의식불명 후 깨어난 전직 건설 노동자, 스킬 없음)

## 퀘스트 아이템

| item_id          | 표시 이름          | 획득 방법          | ASCII | 역할 |
|------------------|--------------------|--------------------|-------|------|
| `prototype_hammer` | 시제 6식 파암추  | 그레체가 직접 지급 | `H`   | 보스 격파 무기 |
| `steel_core`     | 강철 갑주 심장     | Named("d_rank_dungeon") 스폰 | `#` | 보스 격파 증거 |
| `pilot_badge`    | 전속 파일럿 인증서 | 그레체 퀘스트 보상 | `P`   | 퀘스트 완료 보상 |

## 관련 존

| zone_id          | 생성기 | 설명 |
|------------------|--------|------|
| `d_rank_dungeon` | `bsp`  | D급 던전 |

## 페이즈 흐름

```
not_started
  └─(그레체 대화)→ dungeon_ready
                  + GiveItem("prototype_hammer")
                  + OpenPortal("d_rank_dungeon", "bsp")
                  + 아이템 스폰: steel_core in Named("d_rank_dungeon")

dungeon_ready
  └─(HasItem("steel_core"))→ boss_defeated  [auto_advance]

boss_defeated
  └─(그레체 대화)→ done
                  + RemoveItem("prototype_hammer")
                  + RemoveItem("steel_core")
                  + GiveItem("pilot_badge")

done  [terminal]
```

## 동작 명세 체크리스트

- [ ] QuestItemKind 추가: PrototypeHammer, SteelCore, PilotBadge
- [ ] item_id_to_kind 매핑: "prototype_hammer", "steel_core", "pilot_badge"
- [ ] VILLAGER_DATA에 그레체 추가 (quest_id: Some("parry_quest"))
- [ ] assets/quests/parry_quest.ron 작성
- [ ] 던전 내 Log 메시지로 패리 전투 장면 서술

## 설계 결정

- 패리 게임플레이 메카닉은 추가하지 않음 — 기존 턴제 전투 유지, Log 메시지로 묘사
- 보스 격파 조건 = steel_core 아이템 습득 (QuestSpawn → HasItem auto_advance)
- 그레체는 마을 NPC — Named 존에 NPC 스폰 불가한 시스템 한계 우회
- 기존 시스템 추가 없이 완전 구현 가능
