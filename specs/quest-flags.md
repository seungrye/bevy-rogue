# 퀘스트 플래그 시스템 (Quest Flags)

## 목적

퀘스트 RON 파일 안에서 임의의 이름 있는 상태값을 읽고 쓸 수 있도록 하여,
NPC 간 관계 변화·감정 상태·세계 변형(마을 소각, NPC 사망 등)을 표현한다.
이를 통해 소설 수준의 서사 구조를 가진 비선형 퀘스트를 작성할 수 있다.

## 새로운 QuestCondition 변형

| 변형 | 의미 |
|------|------|
| `FlagIs(flag: "...", value: "...")` | 플래그가 정확히 해당 값인지 확인 |
| `HasFlag("...")` | 플래그가 존재하는지 확인 (값 무관) |

기존 `And` / `Or` / `Not` 조합자와 자유롭게 중첩 가능.

## 새로운 QuestAction 변형

| 변형 | 의미 |
|------|------|
| `SetFlag(flag: "...", value: "...")` | 플래그를 설정 |
| `ClearFlag("...")` | 플래그를 삭제 |
| `KillNpc("NPC이름")` | 해당 이름의 빌리저를 즉시 퇴장시키고 사망 로그 출력 |

## QuestState 변경

`QuestState.flags: HashMap<String, String>` 추가.
플래그 값은 자유로운 문자열이므로 "high"/"low", "alive"/"dead", "true", 숫자 등 어떤 값이든 사용 가능.

## 사용 예시 (RON 내)

```ron
// 신뢰도 설정
SetFlag(flag: "elara_trust", value: "betrayed")

// 세계 변형
SetFlag(flag: "village_burned", value: "true")

// NPC 사망
KillNpc("엘라라")

// 조건 분기
Branch(
    condition: FlagIs(flag: "elara_trust", value: "high"),
    if_true: [Log("엘라라: 당신을 믿어요.")],
    if_false: [Log("엘라라: 더 이상 당신을 믿지 않아요.")],
)
```

## 동작 규칙

- [x] `SetFlag` / `ClearFlag` 는 `on_interact` 액션 체인 안에서 실행
- [x] `FlagIs` / `HasFlag` 는 `auto_advance` 조건과 `Branch` 조건 모두에서 평가 가능
- [x] `KillNpc` 는 `KillNpcEvent` 를 발행 → `handle_kill_npc` 시스템이 같은 프레임에 처리
- [x] 같은 플래그를 여러 번 `SetFlag` 하면 마지막 값으로 덮어씌움
- [x] 존재하지 않는 플래그를 `ClearFlag` 하면 아무 일도 일어나지 않음

## NPC 사망 흐름

```
QuestAction::KillNpc("이름")
  → execute_actions: kill_npc.send(KillNpcEvent("이름"))
  → handle_kill_npc (villager 시스템):
      Query<(Entity, &Villager)>에서 name 일치 찾기
      → commands.entity(e).despawn_recursive()
      → LogMessage("{이름}이(가) 쓰러졌다...")
```

`Villager.name: String` 필드가 스폰 시 VILLAGER_DATA 첫 번째 요소로 설정됨.
