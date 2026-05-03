# 마우스 클릭 자동 이동

## 목적

좌클릭으로 목적지를 지정하면 플레이어가 BFS 최단 경로로 자동 이동한다.

## 동작 명세

- [x] 좌클릭 → 화면 좌표를 월드/타일 좌표로 변환 후 경로 계산
- [x] 목적지가 Floor 타일이 아니면 무시
- [x] BFS로 플레이어 현재 타일 → 목적지까지 경로 계산 (`PlayerPath` 리소스)
- [x] 경로를 따라 매 턴 1칸씩 이동 (일반 키 이동과 동일한 tick_hold 속도)
- [x] 이동 키(방향키/WASD) 누르면 즉시 경로 취소
- [x] 경로 이동 중 몬스터 타일에 도달하면 공격 후 경로 취소
- [x] 경로 이동 중 빌리저/장애물 타일이면 경로 취소
- [x] 도달 불가능한 목적지는 경로가 빈 채로 무시

## 자료 구조

| 항목 | 설명 |
|------|------|
| `PlayerPath(VecDeque<(usize,usize)>)` | 남은 이동 경로 (목적지 포함, 현재 위치 제외) |
| `pathfinding::find_path(map, from, to)` | BFS 반환값: `Vec<(usize,usize)>` |

## 경로 소비 흐름

```
마우스 클릭
  └─ on_mouse_click: 화면→타일, BFS, PlayerPath 설정
       └─ player_movement (매 프레임):
            └─ 키 입력 없고 PlayerPath 비어있지 않으면
                 └─ path.front() → next_tile → MovingTo 삽입 + PlayerActedEvent
```
