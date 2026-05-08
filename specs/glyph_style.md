# 글리프 스타일 시스템

## 목적

아이템 글리프를 세 가지 스타일 중 하나로 표시한다.  
CLI 옵션으로 초기 스타일을 지정하고, 인게임에서 `G` 키로 순환 전환한다.

## 스타일 정의

| 스타일 ID | 이름 | 폰트 |
|-----------|------|------|
| `ascii` | ASCII | FiraMono-Medium.ttf |
| `unicode` | 유니코드 심볼 | NotoSansSymbols2-Regular.ttf |
| `icon` | RPG 아이콘 | rpg-awesome.ttf |

## 아이템별 글리프 매핑

| 아이템 | ASCII | Unicode (U+) | GameIcon (U+) |
|--------|-------|--------------|--------------|
| 검 | `/` | 🗡 1F5E1 | E946 (ra-broadsword) |
| 창 | `\|` | ⬆ 2B06 | EAAC (ra-spear-head) |
| 활 | `)` | ➤ 27A4 | E978 (ra-crossbow) |
| 가죽 갑옷 | `]` | 🛡 1F6E1 | EA96 (ra-shield) |
| 체력 물약 | `!` | ❤ 2764 | EA72 (ra-potion) |

## 동작 명세

- [x] CLI `--glyph-style <ascii|unicode|icon>` 으로 초기 스타일 지정 (기본값: `ascii`)
- [x] `G` 키로 ascii → unicode → icon → ascii 순서로 순환
- [x] 스타일 전환 시 월드에 존재하는 모든 아이템 엔티티의 글리프·폰트가 즉시 갱신된다
- [x] 스타일 전환 시 로그에 현재 스타일 이름이 출력된다
- [x] 신규 드롭 아이템은 현재 스타일로 스폰된다
