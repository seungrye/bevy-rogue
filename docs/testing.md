# 테스트 & 커버리지

스펙→테스트→구현 흐름은 [개발 프로세스](development-process.md) 참고. 이 문서는 **커버리지 측정**과 **Bevy 시스템 테스트 작성법**을 다룬다.

## 목표 지표

**nightly 기준 Functions 100% + Branches 100%.** 모든 함수가 최소 1회 실행되고, 모든 분기(`if/else`, `match` arm, `?`, `&&`/`||`)가 양방향으로 실행된다.

> branch 와 line 은 크게 갈린다. 분기 없는 시스템 함수가 통째로 미실행이면 branch% 는 높아도 실제론 미테스트다. 그래서 **Functions% 와 Branches% 를 함께** 본다.

## 커버리지 측정 (`scripts/`)

```bash
# 진짜 branch+function 커버리지 (nightly, 느림). 이게 목표 지표.
RUST_COV_BRANCH=1 ./scripts/coverage.sh report

# 빠른 반복용 (stable, region). 단 assert!/format! 의 실패메시지 클로저까지
# 세므로 region 100% 는 원천 불가 — 미커버 '라인' 찾기 용도로만 쓴다.
./scripts/coverage.sh report

# 직전 측정 profdata 로 특정 파일의 미커버(실행 0회) 라인만 출력 — 재컴파일 없이 즉시.
./scripts/uncovered.sh src/modules/<모듈>/mod.rs

# 단일 모듈 빠른 통과 확인 (바이너리 크레이트라 --lib 없음)
RUSTFLAGS="-C instrument-coverage" cargo test --bin bevy-rogue "modules::<모듈>"
```

cargo 는 `target` 락 때문에 **한 번에 하나만** 실행한다.

## 테스트 이름 규칙

**한글, 의도가 드러나는 서술형 문장.** 직역·명사나열 금지. `조건하면_대상은_결과가_된다/한다/않는다` 형태로, 길어도 된다.

```rust
fn 검을_장착하면_유효공격력은_7이다() { ... }          // 좋음
fn 동상반응은_플레이어를_기절시키지_않는다() { ... }    // 좋음
fn weapon_attack_sword_is_7() { ... }                  // 나쁨 (영어)
fn 무기공격력_검_7() { ... }                            // 나쁨 (직역·명사나열)
```

## Bevy 0.13 시스템 테스트 (App 하네스)

`app.world` 는 **필드**다 (0.14 의 `world_mut()` 아님).

```rust
let mut app = App::new();
app.add_event::<MyEvent>()
   .add_systems(Update, my_system);
let e = app.world.spawn((CompA, CompB)).id();
app.world.send_event(MyEvent { .. });
app.update();                                  // Commands 는 update() 끝에 flush됨
assert!(app.world.entity(e).contains::<Added>());
let v = app.world.get::<CompA>(e).unwrap();
let n = app.world.query::<&CompA>().iter(&app.world).count();
assert!(app.world.get_entity(e).is_none());    // despawn 확인
```

상황별 셋업:

| 시스템이 필요로 하는 것 | 테스트 셋업 |
|---|---|
| `Res<AssetServer>` (폰트/이미지 로드) | `app.add_plugins(MinimalPlugins).add_plugins(bevy::asset::AssetPlugin::default()); app.init_asset::<Font>(); app.init_asset::<Image>();` |
| `Res<Time>` | `app.init_resource::<Time>();` 후 `app.world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(x))` (TimePlugin 없이 수동 제어) |
| `Res<ButtonInput<KeyCode>>` | `app.insert_resource(ButtonInput::<KeyCode>::default());` 후 `.press(KeyCode::X)` → 단일 update 에서 `just_pressed` |
| `Plugin::build()` 커버 | `App::new(); app.add_plugins(XPlugin);` — update 안 해도 시스템 등록만으로 build() 실행 |
| 아이템 레지스트리 | `crate::modules::item::build_test_registry()` |

**함정:** `tile_to_world_coords(x,y)` 는 `Vec2` 반환 → `Transform`/`MovingTo`(Vec3)엔 `.extend(0.0)`. `world_to_tile_coords` 는 `[0,MAP_WIDTH)` 로 **클램프**하므로, 클램프 뒤 범위검사 같은 방어분기는 `map.width < MAP_WIDTH` 인 좁은 맵으로만 도달한다.

## 테스트용 seam 리팩터링

파일 IO 고정경로·`rand`·시간처럼 그대로는 테스트하기 어려운 분기는 **동작을 바꾸지 않는 선에서 seam 을 주입**한다. 기본 동작은 불변, 테스트에서만 다른 값 주입:

- 고정경로 → 경로를 인자로 받는 순수 함수 + 상수를 넘기는 얇은 시스템 wrapper. 예: `read_start_loadout(path)`, `apply_loadout_unless_save(.., save_path)`.
- 세이브 경로 등은 `Resource`(예: `SaveConfig{path}`)로 주입해 임시파일로 양쪽 분기 검증. **테스트가 실제 `save/progress.ron` 을 만들거나 지우면 안 된다.**
- `rand` 의존 분기는 이벤트를 다수 발생시켜 통계적으로 양쪽을 커버.
- 정상 입력으로 도달 불가한 방어 분기는 억지로 만들지 말고 `// 도달 불가 방어코드` 주석으로 명시한다.
