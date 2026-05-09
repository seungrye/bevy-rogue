# 퀘스트 아이템이 Wall 위에 spawn 되는 회귀

## 증상

연금술사의 비전 등 던전 퀘스트 아이템(dragon_scale, ancient_scroll 등)이
가끔 Wall 타일 위에 겹쳐 표시된다.

## 원인 (race condition)

- `execute_apply` (맵 교체) 는 `MapSystemSet::ExecuteRegen` 에 속함
- `spawn_quest_items` 는 `Update` 에 ordering 없이 등록됨
- 같은 frame 에 두 시스템이 실행되면 `spawn_quest_items` 가 옛 map 의
  `rooms`/`tiles` 를 보고 좌표를 고를 수 있다
- 새 map 이 적용된 후 그 좌표는 다른 map 의 wall 일 수 있다

`is_changed()` 는 한 frame 만 true 라 spawn 시도가 1 회만 일어나는데, 그
1 회가 **새 map 적용 전** 의 옛 map 데이터를 본다.

## 수정

- [ ] `spawn_quest_items` 에 `.after(MapSystemSet::ExecuteRegen)` ordering
      추가 — 새 map 이 완전히 적용된 후에만 좌표 결정
- [ ] 안전망: spawn 직전 `map.get_tile(tx, ty) == Floor` 검증, 아니면 스킵 +
      error 로그

## 테스트 전략

- 단위 테스트로 race condition 자체는 재현 어려움 (Bevy schedule 필요)
- 로직 테스트: Floor 검사 가드가 wall 좌표를 거절하는지
