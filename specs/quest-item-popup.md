# 퀘스트 아이템 획득 팝업

## 목적

퀘스트 아이템을 획득했을 때 이미지 팝업을 출력하여 중요한 획득 순간을 강조한다.

## 동작

- [x] chest "?" 트리거 심볼 제거 (trigger 모듈 삭제)
- [x] 플레이어가 퀘스트 아이템 위를 지나면 자동 획득 + `QuestItemAcquiredEvent` 발행 — 픽업은 이동 애니메이션 완료(PlayerSystemSet::MovementComplete) 이후에 처리된다
- [x] 이미지 팝업이 화면 중앙에 표시된다 (z-index 100, 다른 UI 위에 렌더링)
- [x] 플레이어가 아이템을 집은 타일을 벗어나면 팝업 닫기 — `QuestItemPopup`에 픽업 타일 `(tile_x, tile_y)` 저장, 매 프레임 플레이어 위치와 비교
- [x] Escape 키로 즉시 닫기
- [x] 팝업이 이미 열려 있으면 중복 스폰하지 않는다 — 이벤트를 루프 전에 전부 드레인 후 첫 번째만 처리, 팝업 존재 여부는 루프 외부에서 한 번만 확인
- [x] 팝업 닫기는 `iter()` 로 순회하여 복수 팝업 엔티티도 모두 제거한다 (get_single 실패 방지)

## 이미지 매핑

각 퀘스트 아이템 종류별로 이미지 경로를 `quest_item_image_path()` 함수에서 관리한다.
현재는 `scene/open-chest.png`를 공통으로 사용하며, 이후 아이템별 이미지로 교체한다.

| 아이템 | 경로 |
|--------|------|
| 영원의 보석 | scene/open-chest.png (placeholder) |
| 현자의 돌   | scene/open-chest.png (placeholder) |
| 용비늘      | scene/open-chest.png (placeholder) |
| 고대 주문서 | scene/open-chest.png (placeholder) |

## 제거된 기능

- `trigger` 모듈 전체 삭제
- `TriggerRespawnEvent` 삭제 (map 모듈에서 발행 제거)
- 맵 방 중앙의 "?" 랜덤 배치 제거
