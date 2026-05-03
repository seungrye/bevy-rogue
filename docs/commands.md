# 빌드 및 실행 명령어

```bash
cargo run               # 게임 실행 (디버그)
cargo build --release   # 최적화 빌드
cargo check             # 링킹 없이 빠른 타입/오류 검사
cargo test              # 테스트 실행
```

`Cargo.toml`에 주석 처리된 `[profile.dev.package."*"]` (의존성에만 opt-level=3 적용)을 활성화하면 게임 로직 반복 작업 시 증분 빌드 속도가 빨라진다.
