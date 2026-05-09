# 사망 상태 저장·로드 방어

## 증상
사망 후 게임을 종료하고 다시 실행하면 게임 오버 팝업 없이 HP=0 상태로
이동/조작이 가능하다.

## 원인
`auto_save` 가 `PlayerActedEvent` 마다 저장한다. 플레이어가 사망 직전
이동을 했고 같은 프레임 내에 monster damage 가 HP 를 0 으로 만들면
다음 auto_save 가 HP=0 상태로 저장될 수 있다. 또한 `Defeated` 컴포넌트
는 ECS 컴포넌트라 세이브에 포함되지 않으므로 로딩 시 누락 — UI 가 사망
인지 못 한다.

## 수정

### auto_save: 사망 상태 저장 skip
`player_q` 시그니처에 `Without<Defeated>` 추가. 사망 시점에 query 가
비어 early return 하므로 사망 상태가 저장되지 않는다. 마지막 정상
턴의 스냅샷이 보존된다.

### load_if_save_exists: HP<=0 방어
세이브 파일에 HP<=0 이 들어와 있으면 (예: 이전 버그로 저장됨) 로딩
직후 player entity 에 `Defeated` 부여. 게임 오버 UI 가 정상 표시되어
R/N/Esc 만 받게 된다.

## 테스트
- 단위: 어려움 (ECS · 파일 IO 의존). 시각 검증.
- 시각: 사망 후 종료 → 재실행 시 게임 오버 팝업이 즉시 뜨는지.
