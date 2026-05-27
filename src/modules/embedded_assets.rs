//! 빌드 시점에 디렉터리 스캔으로 임베드된 RON 에셋 슬라이스.
//!
//! `build.rs` 가 `assets/{quests,items,monsters,villagers}` 를 스캔해 생성한
//! `$OUT_DIR/embedded_assets.rs` 를 그대로 `include!` 한다. 새 .ron 파일을
//! 디렉터리에 추가하기만 하면 wasm 분기에서도 자동으로 인식된다.
//!
//! 노출 상수:
//! - `EMBEDDED_QUESTS`     — `assets/quests/*.ron`
//! - `EMBEDDED_ITEMS`      — quest_items / weapons / armors / consumables / start_loadout
//! - `EMBEDDED_MONSTERS`   — monsters.ron
//! - `EMBEDDED_VILLAGERS`  — villagers.ron

include!(concat!(env!("OUT_DIR"), "/embedded_assets.rs"));

/// 이름으로 임베드된 RON 문자열을 찾는다 (없으면 None).
pub fn find_embedded<'a>(table: &'a [(&'a str, &'a str)], name: &str) -> Option<&'a str> {
    table.iter().find_map(|(n, content)| if *n == name { Some(*content) } else { None })
}

/// REMOTE 우선 파싱 → 실패 시 임베드 폴백.
///
/// 흐름:
/// 1. `remote` 가 `Some` 이면 `ron::from_str::<T>` 시도.
///    - 성공 → 그대로 반환.
///    - 실패 → `warn!("REMOTE {label} 파싱 실패 → 임베드 폴백: {e}")` 로그 후 임베드 시도.
/// 2. `embedded_fn()` 으로 임베드 슬라이스를 조회.
///    - `None` → panic (`EMBEDDED {label} 누락`): build.rs 가 채워 넣었어야 하는데 누락.
///    - `Some(text)` → `ron::from_str::<T>` 시도.
///       - 성공 → 반환.
///       - 실패 → panic (`[치명적] 임베드 {label} 파싱 실패: {e}`):
///                빌드 시점에 정상이었던 콘텐츠가 깨졌다는 뜻이라 진짜 치명적.
///
/// site DB 의 일부 카테고리가 게임 스키마와 어긋나도 그 카테고리만 임베드로 폴백되어
/// 게임 전체가 죽지 않게 보호하는 헬퍼.
pub fn parse_remote_or_embedded<T: serde::de::DeserializeOwned>(
    label: &str,
    remote: Option<&str>,
    embedded_fn: impl FnOnce() -> Option<&'static str>,
) -> T {
    if let Some(r) = remote {
        match ron::from_str::<T>(r) {
            Ok(v) => return v,
            Err(e) => bevy::log::warn!("REMOTE {} 파싱 실패 → 임베드 폴백: {}", label, e),
        }
    }
    let embedded = embedded_fn().unwrap_or_else(|| panic!("EMBEDDED {} 누락", label));
    ron::from_str(embedded)
        .unwrap_or_else(|e| panic!("[치명적] 임베드 {} 파싱 실패: {}", label, e))
}

// ── REMOTE 우선 → 빌드 임베드 폴백 헬퍼 ────────────────────────────────────────
//
// 게임의 로드 시스템들은 아래 헬퍼만 호출하면 된다.
//   - 시작 시 site 가 보낸 콘텐츠가 `remote_content::install_from_json` 으로
//     설치돼 있으면 그 RON 을 우선 반환.
//   - 그렇지 않으면(설치 실패 / native / 누락) 빌드 임베드 슬라이스에서 찾아 반환.
//
// native 빌드에서는 REMOTE 가 절대 set 되지 않으므로 항상 임베드 폴백 → 회귀 0.

/// `quest_items.ron` 등 단일 item 파일을 가져온다 (REMOTE 우선, 임베드 폴백).
pub fn item_ron(filename: &str) -> Option<&'static str> {
    if let Some(s) = crate::modules::remote_content::remote_item(filename) {
        return Some(s);
    }
    find_embedded(EMBEDDED_ITEMS, filename)
}

/// `villagers.ron` RON 문자열 (REMOTE 우선, 임베드 폴백).
pub fn villagers_ron() -> Option<&'static str> {
    if let Some(s) = crate::modules::remote_content::remote_villagers() {
        return Some(s);
    }
    find_embedded(EMBEDDED_VILLAGERS, "villagers.ron")
}

/// `monsters.ron` RON 문자열 (REMOTE 우선, 임베드 폴백).
pub fn monsters_ron() -> Option<&'static str> {
    if let Some(s) = crate::modules::remote_content::remote_monsters() {
        return Some(s);
    }
    find_embedded(EMBEDDED_MONSTERS, "monsters.ron")
}

/// `assets/quests/*.ron` 전체 목록을 `(파일명, RON)` iterator 로 반환한다.
///
/// **머지 시맨틱 (id 별 override)**: REMOTE 에 있는 퀘스트는 같은 파일명의 임베드
/// 퀘스트를 덮어쓰고, REMOTE 에 없는 임베드 퀘스트는 그대로 유지된다. site DB 가
/// 부분만 import 된 상태에서도 게임이 임베드된 퀘스트 전체를 잃지 않게 보호한다.
///
/// REMOTE 가 비어 있으면(set 안 됨 또는 빈 vec) 임베드 슬라이스를 그대로 반환.
pub fn all_quests() -> Box<dyn Iterator<Item = (&'static str, &'static str)>> {
    let remote = crate::modules::remote_content::remote_quests();
    Box::new(merge_quests(EMBEDDED_QUESTS, remote).into_iter())
}

/// `all_quests` 의 머지 로직을 테스트 가능하게 분리한 순수 함수.
///
/// `embedded` 를 기본으로 두고, `remote` 에 있는 항목은 같은 파일명의 임베드를
/// override 한다. remote 가 None 이면 embedded 를 그대로 복사. embedded 와 remote
/// 의 문자열 lifetime 은 모두 `'static` (REMOTE 는 OnceLock 로 영구 저장).
pub fn merge_quests(
    embedded: &'static [(&'static str, &'static str)],
    remote: Option<&'static [(String, String)]>,
) -> Vec<(&'static str, &'static str)> {
    use std::collections::HashMap;
    let mut map: HashMap<&'static str, &'static str> =
        embedded.iter().map(|(n, r)| (*n, *r)).collect();
    if let Some(remote) = remote {
        for (n, r) in remote.iter() {
            map.insert(n.as_str(), r.as_str());
        }
    }
    map.into_iter().collect()
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;

    #[test]
    fn 임베드된_퀘스트_목록은_assets_quests_디렉터리_전체를_담는다() {
        // build.rs 가 assets/quests/*.ron 을 스캔해 채워넣었어야 한다.
        // 디스크에 있는 .ron 개수와 임베드 슬라이스 크기가 정확히 같다면
        // 자동 enumerate 가 동작한다는 보장이 된다.
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let dir = std::path::PathBuf::from(manifest_dir).join("assets/quests");
        let on_disk: usize = std::fs::read_dir(&dir)
            .expect("assets/quests 읽기 실패")
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("ron"))
            .count();
        assert!(on_disk > 0, "테스트 사전조건: assets/quests 에 .ron 이 있어야 한다");
        assert_eq!(
            EMBEDDED_QUESTS.len(),
            on_disk,
            "EMBEDDED_QUESTS 가 디스크의 .ron 개수와 다르다 — build.rs 자동 enumerate 검증"
        );
    }

    #[test]
    fn 임베드된_몬스터_파일은_정확히_monsters_ron_하나다() {
        assert_eq!(EMBEDDED_MONSTERS.len(), 1);
        assert_eq!(EMBEDDED_MONSTERS[0].0, "monsters.ron");
        assert!(!EMBEDDED_MONSTERS[0].1.is_empty());
    }

    #[test]
    fn 임베드된_빌리저_파일은_정확히_villagers_ron_하나다() {
        assert_eq!(EMBEDDED_VILLAGERS.len(), 1);
        assert_eq!(EMBEDDED_VILLAGERS[0].0, "villagers.ron");
        assert!(!EMBEDDED_VILLAGERS[0].1.is_empty());
    }

    #[test]
    fn 임베드된_아이템_파일들은_핵심_다섯개를_담는다() {
        let names: std::collections::HashSet<&str> = EMBEDDED_ITEMS.iter().map(|(n, _)| *n).collect();
        for needed in &["quest_items.ron", "weapons.ron", "armors.ron", "consumables.ron", "start_loadout.ron"] {
            assert!(names.contains(needed), "items 슬라이스에 {needed} 가 없다");
        }
    }

    #[test]
    fn find_embedded는_존재하는_이름의_내용을_반환한다() {
        let got = find_embedded(EMBEDDED_MONSTERS, "monsters.ron");
        assert!(got.is_some(), "monsters.ron 은 임베드돼 있어야 한다");
        assert!(!got.unwrap().is_empty());
    }

    #[test]
    fn find_embedded는_존재하지_않는_이름이면_None을_반환한다() {
        let got = find_embedded(EMBEDDED_MONSTERS, "does_not_exist.ron");
        assert!(got.is_none());
    }

    // ── REMOTE 우선 헬퍼들 (REMOTE 미설치 상태에서 임베드 폴백 검증) ──────────
    //
    // 같은 프로세스 내 OnceLock 한계상 단위 테스트에서는 REMOTE 우선 경로를 직접
    // install 해서 검증할 수 없다(설치 시 다른 native 테스트의 회귀 보장이 깨짐).
    // 따라서 단위 테스트는 "REMOTE 가 비어 있으면 임베드 슬라이스와 같은 결과"
    // 만 검증한다 — 이게 native 빌드의 실제 동작이기도 하다.

    #[test]
    fn item_ron_은_REMOTE_없을때_임베드_파일을_반환한다() {
        let got = item_ron("weapons.ron");
        assert!(got.is_some(), "임베드 weapons.ron 폴백이 동작해야 한다");
        let direct = find_embedded(EMBEDDED_ITEMS, "weapons.ron").unwrap();
        assert_eq!(got.unwrap(), direct);
    }

    #[test]
    fn item_ron_은_존재하지_않는_파일이면_None을_반환한다() {
        assert!(item_ron("not_a_real_file.ron").is_none());
    }

    #[test]
    fn villagers_ron_은_REMOTE_없을때_임베드_파일을_반환한다() {
        let got = villagers_ron();
        let direct = find_embedded(EMBEDDED_VILLAGERS, "villagers.ron").unwrap();
        assert_eq!(got.unwrap(), direct);
    }

    #[test]
    fn monsters_ron_은_REMOTE_없을때_임베드_파일을_반환한다() {
        let got = monsters_ron();
        let direct = find_embedded(EMBEDDED_MONSTERS, "monsters.ron").unwrap();
        assert_eq!(got.unwrap(), direct);
    }

    #[test]
    fn merge_quests_는_remote_없으면_embedded를_그대로_반환한다() {
        let embedded: &'static [(&'static str, &'static str)] = &[
            ("a.ron", "QuestDef(id:\"a\")"),
            ("b.ron", "QuestDef(id:\"b\")"),
        ];
        let merged = merge_quests(embedded, None);
        let names: std::collections::HashSet<&str> = merged.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ["a.ron", "b.ron"].into_iter().collect());
    }

    #[test]
    fn merge_quests_는_remote가_같은_id를_override한다() {
        let embedded: &'static [(&'static str, &'static str)] = &[
            ("a.ron", "EMBEDDED_A"),
            ("b.ron", "EMBEDDED_B"),
        ];
        // REMOTE 가 a.ron 만 override. b.ron 은 임베드 그대로 살아 있어야 함.
        let remote_owned: Vec<(String, String)> = vec![
            ("a.ron".to_string(), "REMOTE_A".to_string()),
        ];
        let remote_static: &'static [(String, String)] = Box::leak(remote_owned.into_boxed_slice());
        let merged = merge_quests(embedded, Some(remote_static));
        let map: std::collections::HashMap<&str, &str> = merged.into_iter().collect();
        assert_eq!(map.get("a.ron"), Some(&"REMOTE_A"), "REMOTE 가 같은 id 를 override");
        assert_eq!(map.get("b.ron"), Some(&"EMBEDDED_B"), "임베드 only 는 그대로 유지");
    }

    #[test]
    fn merge_quests_는_remote에만_있는_새_퀘스트도_포함한다() {
        let embedded: &'static [(&'static str, &'static str)] = &[("a.ron", "EMBEDDED_A")];
        let remote_owned: Vec<(String, String)> = vec![
            ("new_quest.ron".to_string(), "REMOTE_NEW".to_string()),
        ];
        let remote_static: &'static [(String, String)] = Box::leak(remote_owned.into_boxed_slice());
        let merged = merge_quests(embedded, Some(remote_static));
        let map: std::collections::HashMap<&str, &str> = merged.into_iter().collect();
        assert_eq!(map.len(), 2);
        assert!(map.contains_key("a.ron"));
        assert!(map.contains_key("new_quest.ron"));
    }

    // ── parse_remote_or_embedded 단위 테스트 ─────────────────────────────────

    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct TestDef { name: String, value: i32 }

    #[test]
    fn parse_remote_or_embedded_는_REMOTE_정상이면_REMOTE를_반환한다() {
        let remote = "(name: \"remote\", value: 1)";
        let embedded_used = std::cell::Cell::new(false);
        let got: TestDef = parse_remote_or_embedded("test.ron", Some(remote), || {
            embedded_used.set(true);
            Some("(name: \"embedded\", value: 2)")
        });
        assert_eq!(got, TestDef { name: "remote".into(), value: 1 });
        assert!(!embedded_used.get(), "REMOTE 정상이면 임베드는 조회조차 하지 않는다");
    }

    #[test]
    fn parse_remote_or_embedded_는_REMOTE_파싱_실패시_임베드로_폴백한다() {
        let bad_remote = "이건 RON 이 아니다 @@@";
        let got: TestDef = parse_remote_or_embedded(
            "test.ron",
            Some(bad_remote),
            || Some("(name: \"fallback\", value: 99)"),
        );
        assert_eq!(got, TestDef { name: "fallback".into(), value: 99 });
    }

    #[test]
    fn parse_remote_or_embedded_는_REMOTE_None이면_임베드를_사용한다() {
        let got: TestDef = parse_remote_or_embedded(
            "test.ron",
            None,
            || Some("(name: \"embedded\", value: 7)"),
        );
        assert_eq!(got, TestDef { name: "embedded".into(), value: 7 });
    }

    #[test]
    #[should_panic(expected = "EMBEDDED test.ron 누락")]
    fn parse_remote_or_embedded_는_임베드도_없으면_panic한다() {
        let _: TestDef = parse_remote_or_embedded("test.ron", None, || None);
    }

    #[test]
    #[should_panic(expected = "[치명적] 임베드 test.ron 파싱 실패")]
    fn parse_remote_or_embedded_는_임베드_파싱_실패시_panic한다() {
        let _: TestDef = parse_remote_or_embedded(
            "test.ron",
            None,
            || Some("이것도 RON 이 아니다 @@@"),
        );
    }

    #[test]
    #[should_panic(expected = "[치명적] 임베드 test.ron 파싱 실패")]
    fn parse_remote_or_embedded_는_REMOTE_실패_후_임베드도_실패하면_panic한다() {
        let _: TestDef = parse_remote_or_embedded(
            "test.ron",
            Some("나쁜 REMOTE"),
            || Some("나쁜 EMBEDDED 도 같이"),
        );
    }

    #[test]
    fn all_quests_는_REMOTE_없을때_임베드_퀘스트_전체를_반환한다() {
        let collected: Vec<(&str, &str)> = all_quests().collect();
        assert_eq!(collected.len(), EMBEDDED_QUESTS.len(),
            "REMOTE 미설치 → 임베드 슬라이스 개수와 동일해야 한다");
        // 이름 set 비교(순서 무관 안전).
        let from_iter: std::collections::HashSet<&str> = collected.iter().map(|(n, _)| *n).collect();
        let from_slice: std::collections::HashSet<&str> = EMBEDDED_QUESTS.iter().map(|(n, _)| *n).collect();
        assert_eq!(from_iter, from_slice);
    }
}
