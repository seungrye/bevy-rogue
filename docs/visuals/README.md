# 인터랙티브 시각화

각 파일은 외부 의존 없는 **자체완결형 single HTML**(CSS·JS 인라인)이다. 브라우저에서 더블클릭으로 연다.
수치는 실제 코드와 대조되어 있다.

| 파일 | 내용 | 관련 문서/코드 |
|------|------|----------------|
| [directional-fov.html](directional-fov.html) | 방향 시야(앞 큰 반원/뒤 작은 반원 + LoS) 인터랙티브 — facing 변경·벽 토글 | `is_in_view`, `FOV_FRONT=8`/`FOV_BACK=3`, [specs/stealth-and-directional-fov.md](../../specs/stealth-and-directional-fov.md) |
| [elemental-reactions.html](elemental-reactions.html) | 원소 반응 4×4 매트릭스(융해/독기/파쇄/동상/플라즈마/전기독) + 효과 | `elemental::Reaction::from_pair` |
| [item-tiers-rarity-drops.html](item-tiers-rarity-drops.html) | 티어별 스탯 범위 · 레어도 백분위 임계 · 레벨별 드롭 가중치 곡선 | [specs/item-random-stats.md](../../specs/item-random-stats.md), `tier_weight`/`Rarity::from_roll` |
| [coordinate-system.html](coordinate-system.html) | 타일 그리드 ↔ Bevy 월드 픽셀 변환 | `tile_to_world_coords`/`world_to_tile_coords`, [docs/map.md](../map.md) |
