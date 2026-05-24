# 빌드 및 실행 명령어

```bash
cargo run               # 게임 실행 (디버그)
cargo build --release   # 최적화 빌드
cargo check             # 링킹 없이 빠른 타입/오류 검사
cargo test              # 테스트 실행
```

커버리지 측정과 Bevy 시스템 테스트 작성법은 [테스트 & 커버리지](testing.md) 참고
(`./scripts/coverage.sh`, `RUST_COV_BRANCH=1 ./scripts/coverage.sh`, `./scripts/uncovered.sh <파일>`).

## 빌드 가속

- **mold 링커 (기본 적용)** — `.cargo/config.toml` 에서 `linker=clang-19 + -fuse-ld=mold`. 거대한 Bevy 바이너리의 링크 시간(빌드 병목)을 mold 가 대체한다. 별도 조작 불필요.
- **병렬 빌드** — 같은 파일의 `[build] jobs = 7` (논리 코어 8 중 7개 사용).
- **`fast-dev` 피처 (opt-in, dev 반복용)** — `cargo run --features fast-dev` 로 Bevy 를 동적 링크(.so)해 증분 빌드를 더 줄인다. **배포에는 쓰지 말 것**(런타임에 `libbevy_dylib.so` 필요). 기본 빌드·테스트·커버리지·`scripts/package.sh` 는 이 피처 없이 정적 링크한다.
- **nightly 프런트엔드 병렬 (opt-in)** — `RUSTFLAGS="-Z threads=7" cargo +nightly build`. stable 전역 설정에 넣으면 깨지므로 nightly 명령에서만 쓴다.
- **의존성 최적화** — `Cargo.toml` 에 주석 처리된 `[profile.dev.package."*"]`(의존성에만 opt-level=3)을 켜면 게임이 디버그에서도 부드럽게 돌지만 최초 빌드는 느려진다(런타임 ↔ 빌드속도 트레이드오프).
