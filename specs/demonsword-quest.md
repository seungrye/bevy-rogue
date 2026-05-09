# 성기사가 마검을 들다 — 퀘스트

## 스토리 요약

노기사 바스티안이 플레이어에게 봉인이 풀리기 시작한 마검의 위험을 알린다.
플레이어는 마귀 동굴에서 마검과 포로 엘레나의 메모를 찾고,
폐허 요새에서 고대 의식서를 입수해 바스티안과 함께 봉인 의식을 완수한다.

## 등장인물

| NPC      | 위치   | 역할 |
|----------|--------|------|
| 바스티안 | 마을   | 퀘스트 수여자 (성기사단 노기사) |
| 엘레나   | 동굴   | 의식서 위치를 메모로 남긴 포로 (아이템으로 표현) |

## 퀘스트 아이템

| item_id              | 표시 이름        | 획득 위치                      | ASCII | 역할 |
|----------------------|------------------|-------------------------------|-------|------|
| `demon_sword`        | 마검             | `Named("demon_cave")`         | `D`   | 봉인 의식 재료 |
| `elenas_memo`        | 엘레나의 메모    | `Named("demon_cave")`         | `e`   | 요새 위치 단서 |
| `ancient_ritual_book`| 고대 의식서      | `Named("ruined_fortress")`    | `R`   | 봉인 의식 재료 |

## 관련 존

| zone_id           | 생성기             | 설명 |
|-------------------|--------------------|------|
| `demon_cave`      | `cellular_automata`| 마귀 동굴 |
| `ruined_fortress` | `bsp_indoor`       | 폐허 요새 |

## 페이즈 흐름

```
not_started
  └─(바스티안 대화)→ awaiting_cave
                    + OpenPortal("demon_cave", "cellular_automata")
                    + 아이템 스폰: demon_sword, elenas_memo in demon_cave

awaiting_cave
  └─(HasItem(demon_sword) AND HasItem(elenas_memo))→ cave_done  [auto_advance]

cave_done
  └─(바스티안 대화)→ awaiting_fortress
                    + OpenPortal("ruined_fortress", "bsp_indoor")
                    + 아이템 스폰: ancient_ritual_book in ruined_fortress

awaiting_fortress
  └─(HasItem(ancient_ritual_book))→ ritual_ready  [auto_advance]

ritual_ready
  └─(바스티안 대화 + Branch)
      ├─ 조건 충족(demon_sword + ancient_ritual_book 보유):
      │    RemoveItem × 2, Log(희생 묘사), AdvancePhase("done")
      └─ 조건 미충족:
           Log("아직 준비 안됨")

done  [terminal]
```

## 동작 명세 체크리스트

- [ ] QuestItemKind 추가: DemonSword, ElenasMemo, AncientRitualBook
- [ ] item_id_to_kind 매핑: "demon_sword", "elenas_memo", "ancient_ritual_book"
- [ ] VILLAGER_DATA에 바스티안 추가 (quest_id: Some("demonsword_quest"))
- [ ] assets/quests/demonsword_quest.ron 작성
- [ ] 페이즈 전환 시 Log 메시지로 스토리 전달
- [ ] 봉인 의식 완료 시 아이템 제거 + 엔딩 메시지 출력

## 설계 결정

- 엘레나·루시퍼는 NPC 대신 **아이템**으로 표현 — 현재 시스템에서 Named 존에 NPC 스폰 불가
- ZoneKillsAtLeast 조건 없이 **HasItem 조건만** 사용 — 기존 시스템 범위 내 완전 구현
- HeroicSacrifice 전용 액션 없이 **Branch + RemoveItem + Log + AdvancePhase** 조합으로 희생 표현
