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
}
