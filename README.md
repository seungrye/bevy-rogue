# Bevy Rogue

Rust + [Bevy 0.13](https://bevyengine.org) 으로 만든 타일 기반 로그라이크. 절차적으로 생성되는 던전·마을·숲을 탐험하고, 턴제 전투와 원소 반응, 퀘스트를 진행한다.

## 실행

```bash
cargo run                       # 디버그 실행
cargo run -- --algorithm bsp       # 시작 맵 생성기 지정 (-a)
cargo run -- --glyph-style unicode # 글리프 스타일 (ascii|unicode|icon, -g)
```

자세한 빌드/커버리지 명령은 [docs/commands.md](docs/commands.md).

## 주요 특징

- **절차적 맵** — 23종 생성기(던전·동굴·마을·숲·미로·도시·바다/섬·WFC), Water/Sand 타일, 시드 기반 결정론적 재현
- **존 시스템** — 포탈로 마을/던전/숲/Named 존을 오가며, 존별 맵·탐험기록 유지
- **시야** — 방향(facing) 기반 두-반원 FOV(앞 넓고 뒤 좁음), 몬스터도 같은 모델로 탐지
- **전투** — 턴제 근접 전투 + 활 원거리 조준, 속도(에너지) 기반 행동 순서
- **원소 반응** — 화염·얼음·독·번개 조합으로 융해·파쇄·플라즈마 등 반응 발동
- **아이템** — 무기/방어구/소비/퀘스트 아이템, 무기·방어구 랜덤 스탯·레어도 파밍, 인벤토리·장비창, 3종 글리프 스타일
- **퀘스트** — RON 정의 기반 상태머신, 마을 NPC 대화, 잠입(스텔스) 퀘스트
- **자동 저장** — 매 턴 RON 저장(시드 + 탐험기록 비트팩), 미니맵

조작키는 [docs/keybindings.md](docs/keybindings.md).

## 문서

| 문서 | 내용 |
|------|------|
| [architecture.md](docs/architecture.md) | 모듈 구성·플러그인 등록·좌표/시드·방향 FOV |
| [map.md](docs/map.md) | 맵 리소스·23종 생성 알고리즘·Water/Sand·에셋 |
| [keybindings.md](docs/keybindings.md) | 전체 조작키 레퍼런스 |
| [development-process.md](docs/development-process.md) | Spec-Driven TDD 프로세스 |
| [testing.md](docs/testing.md) | 커버리지 측정·Bevy 시스템 테스트 작성법 |
| [roguelike-feature-checklist.md](docs/roguelike-feature-checklist.md) | 현재 기능·보완 후보 |

기능 설계 명세는 `specs/`, AI 작업 가이드는 [CLAUDE.md](CLAUDE.md).
