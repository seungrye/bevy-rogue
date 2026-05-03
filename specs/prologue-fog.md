# 안개 속의 발자국 — 프롤로그 퀘스트

## 목적

플레이어가 기억을 잃은 채 시작하는 프롤로그 퀘스트.
두 가지 선택(무기 + 가치관)의 조합으로 세 가지 각성 루트 중 하나로 귀결되며,
이후 퀘스트와 세계관에 지속적 영향을 준다.

## NPC

- **부상당한 병사** — 퀘스트 진행 안내자. 각성이 완료되면 KillNpc로 소멸 (사명 완료).

## 1단계: 본능적인 선택 (무기)

던전1에 세 가지 무기가 스폰됨. 플레이어가 하나를 집는 순간 나머지 둘은
**즉시 월드에서 제거**되어 중복 취득이 불가능하다.
→ `AutoAdvance.actions: [DespawnWorldItem("..."), ...]` 으로 구현.

| 아이템 | item_id | 의미 |
|--------|---------|------|
| 대검 | `prologue_greatsword` | 근력·명예 중시 → 스타크 후보 |
| 단검과 투척물 | `prologue_daggers` | 민첩·실리 중시 → 나이트워치 후보 |
| 부러진 활과 횃불 | `prologue_bowtorch` | 원거리·생존 중시 → 타르가르옌 후보 |

## 2단계: 가치관 판정 (병사 대화)

무기를 집은 뒤 부상당한 병사에게 돌아오면, 병사가 **무기 선택에서 플레이어의 가치관을 읽어냄**.
플레이어가 명시적으로 선택하는 것이 아니라, 무기가 곧 본능·가치관임을 서사로 표현.

| 무기 | 가치관 플래그 | 의미 |
|------|-------------|------|
| 대검 | `values = "honor"` | 자비와 명예 |
| 단검 | `values = "pragmatism"` | 실리와 계산 |
| 활+횃불 | `values = "survival"` | 생존과 냉정 |

## 각성 조건 (3루트)

| 루트 | 조건 | 각성 NPC | 보상 |
|------|------|----------|------|
| **스타크** | weapon=greatsword AND values=honor | 에다드 스타크 | `ice_sword` |
| **타르가르옌** | weapon=bowtorch AND values=survival | 대너리스 타르가르옌 | `dragon_egg` |
| **나이트워치** | 그 외 모든 조합 | 존 스노우 | `ghost_wolf` |

## 페이즈 흐름 (13단계)

```
dormant → weapon_hunt → soldier_test_{sword/daggers/bowtorch}
       → crest_hunt → awakening_ready
       → {stark/targaryen/nightswatch}_dawn
       → {stark/targaryen/nightswatch}_end (terminal)
```

## 각성 후 세계관 변화

- `flags["character"]` = "stark" / "targaryen" / "jon_snow" 설정
- 이후 퀘스트에서 `FlagIs(flag: "character", value: "stark")` 등으로 분기 가능
- 부상당한 병사 NPC가 소멸 (`KillNpcEvent`) → 세계에서 영구 제거

## 스폰 아이템

| phase | item_id | zone |
|-------|---------|------|
| weapon_hunt | prologue_greatsword | Dungeon(1) |
| weapon_hunt | prologue_daggers | Dungeon(1) |
| weapon_hunt | prologue_bowtorch | Dungeon(1) |
| crest_hunt | family_crest | Dungeon(1) |

## 설계 원칙

- 플레이어는 명시적 선택지를 클릭하지 않는다 — **행동이 곧 선택**이다
- 무기를 집는 순간 본능이 드러나고, 병사는 그것을 서사로 확인한다
- 각성 연출은 `Log` 액션 3연속으로 감정적 고조를 만든다
- `KillNpc`로 병사가 사라지며 프롤로그가 완전히 닫힌다
