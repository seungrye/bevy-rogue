# 맵 시스템

## Map 리소스

`Map` (`map/mod.rs`)이 전체 월드 상태를 보관한다:
- `tiles: Vec<MapTile>` — Wall 또는 Floor
- `revealed_tiles` / `visible_tiles` — 전장의 안개; revealed는 영구 기억, visible은 현재 FOV
- `rooms: Vec<Rect>` — 플레이어·트리거 배치에 사용되는 방 목록

## 생성 알고리즘

`MapAlgorithm` 리소스로 세 가지 알고리즘을 전환할 수 있다:

| 알고리즘 | 파일 | 특징 |
|----------|------|------|
| `Bsp` | `map/bsp.rs` | 160×100 맵을 재귀적으로 분할, L자형 복도 연결, 규칙적인 배치 |
| `SimpleRooms` | `map/rooms.rs` | 무작위 방 배치 + 복도 연결, 유기적인 느낌 |
| `DrunkardWalk` | `map/drunkard.rs` | 무작위 보행으로 동굴 형태 생성 |

## 에셋

게임은 `assets/` 디렉터리 아래의 파일들을 필요로 한다:
- `fonts/FiraMono-Medium.ttf` — 타일·스탯 렌더링용 고정폭 폰트
- `fonts/NanumSquareNeo-bRg.ttf` — 다이얼로그 텍스트용 한국어 폰트
- `scene/open-chest.png` — 상자 팝업 이미지
