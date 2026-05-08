# 코드 주석 언어 정책

## 목적

프로젝트의 설명 주석을 한국어 중심으로 유지해 코드 의도가 팀 문서와 같은 언어로 읽히게 한다.
식별자, 타입명, 키 이름처럼 코드와 직접 연결되는 용어는 그대로 두되, 설명 문장은 한국어로 작성한다.

## 작성 규칙

- [x] 새로 작성하는 함수 설명 주석은 한국어로 작성한다
- [x] 구현 의도를 설명하는 `//` 주석도 한국어로 작성한다
- [x] `HUD`, `XP`, `Map`, `QuestState`, `PlayerActedEvent` 처럼 코드 식별자와 연결되는 용어는 유지할 수 있다
- [x] 알고리즘 약어와 고유명사는 필요하면 원문을 유지하되 한국어 설명을 붙인다
- [x] 테스트용 도식이나 ASCII 레이아웃은 의미가 깨지지 않는 선에서 한국어 설명을 덧붙인다

## 정리된 범위

- [x] 최근 추가된 성장 루프 함수 설명 주석
- [x] 최근 추가된 이동/도움말 차단 설명 주석
- [x] 아이템 글리프 설명 주석
- [x] 맵 생성기 문서 주석의 `Arguments`, `Returns` 표기
- [x] 일부 섹션 제목성 주석과 테스트 설명 주석

## 구현 위치

- `src/modules/player/mod.rs`
- `src/modules/item/mod.rs`
- `src/modules/map/*.rs`
- `src/modules/map/generators/prefab.rs`
- `src/modules/monster/mod.rs`
- `src/modules/villager/mod.rs`

## 테스트

- [x] 주석 변경 후 전체 회귀: `cargo test`
- [x] 공백 검사: `git diff --check`

## 남은 개선 후보

- `assets/quests/*.ron` 상단 설명의 타입명 나열을 어느 수준까지 번역할지 기준화
- 공개 API 문서 주석 형식을 `목적/인수/반환값`으로 통일
- CI에서 주석 언어 규칙을 자동 검사할지 검토
