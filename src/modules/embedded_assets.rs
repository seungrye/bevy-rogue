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
/// REMOTE 가 set 돼 있으면 그 목록 우선, 아니면 빌드 임베드 슬라이스.
///
/// 반환 타입을 `Box<dyn Iterator<...>>` 로 둔 이유: REMOTE 경로는 `Vec` 슬라이스
/// (`&[(String, String)]`) 이고 임베드 경로는 `&[(&str, &str)]` 이라 두 소스의
/// 아이템 타입이 달라 정적 합집합 iterator 로 표현하기 어렵다. 호출자는
/// `(&str, &str)` 쌍만 보면 되므로 dyn 박싱 비용은 무시 가능(시작 1회만).
pub fn all_quests() -> Box<dyn Iterator<Item = (&'static str, &'static str)>> {
    if let Some(remote) = crate::modules::remote_content::remote_quests() {
        return Box::new(remote.iter().map(|(n, r)| (n.as_str(), r.as_str())));
    }
    Box::new(EMBEDDED_QUESTS.iter().map(|(n, r)| (*n, *r)))
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
