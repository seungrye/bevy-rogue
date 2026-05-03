# 개발 프로세스: Spec-Driven TDD

## 기본 원칙

모든 기능은 **스펙 → 테스트 → 구현** 순서로 진행한다.

## 워크플로우

### 1. 스펙 작성 (`specs/`)

기능 구현 전에 `specs/` 디렉터리에 마크다운 스펙 파일을 먼저 작성한다.

```
specs/
  map-generation.md
  player-movement.md
  fov-system.md
  ...
```

스펙 파일 구조:
```markdown
# 기능명

## 목적
이 기능이 해결하는 문제

## 동작 명세
- [ ] 조건 A일 때 결과 X가 발생한다
- [ ] 조건 B일 때 결과 Y가 발생한다

## 엣지 케이스
- 경계 조건, 예외 상황 등
```

### 2. 테스트 작성 (Red)

스펙의 각 항목을 실패하는 테스트로 작성한다. Rust 테스트는 해당 모듈 하단의 `#[cfg(test)]` 블록에 작성한다:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 동작_명세를_그대로_테스트명으로() {
        // 스펙의 "조건 A일 때 결과 X" 항목에 대응
    }
}
```

### 3. 구현 (Green)

테스트를 통과시키는 최소한의 코드만 작성한다.

```bash
cargo test                          # 전체 테스트
cargo test <모듈명>                  # 특정 모듈만
cargo test -- --nocapture           # println! 출력 포함
```

### 4. 리팩터링 (Refactor)

테스트가 통과된 상태에서 코드를 정리한다. 테스트가 계속 통과하는지 확인하며 진행한다.

## Bevy 시스템 테스트

Bevy ECS 시스템은 `App`을 직접 구성해서 테스트한다:

```rust
#[test]
fn 시스템_동작_테스트() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
       .insert_resource(/* 필요한 리소스 */)
       .add_systems(Update, 테스트할_시스템);

    app.update();

    // 결과 검증
}
```

## 스펙과 코드의 연결

구현 코드에서 스펙 항목을 참조할 때는 파일명으로만 연결한다:

```rust
// specs/player-movement.md 참고
fn player_movement(...) { ... }
```

## 커밋 규칙

| 단계 | 커밋 메시지 접두사 |
|------|-------------------|
| 스펙 작성 | `spec: ` |
| 테스트 작성 | `test: ` |
| 구현 | `feat:` / `fix:` |
| 리팩터링 | `refactor:` |
