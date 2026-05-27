//! 시작 시 원격(site `/api/game/content/v1`)에서 받아온 RON 콘텐츠를 보관하는 모듈.
//!
//! 흐름:
//!   1. wasm 진입점 `start(content_json)` 가 site 에서 받아온 JSON 문자열을 넘긴다.
//!   2. `install_from_json` 이 한 번만 `REMOTE` 에 set 한다(`OnceLock`).
//!   3. 게임의 로드 시스템(quest/item/villager/monster) 은 `embedded_assets` 의
//!      헬퍼를 통해 **REMOTE 가 있으면 REMOTE 를, 없으면 빌드 시 임베드된 슬라이스**
//!      를 사용한다.
//!
//! native 빌드는 `install_from_json` 을 호출하지 않으므로 모든 헬퍼가 항상
//! `None` 을 반환 → 회귀 0. wasm 분기에서도 fetch 실패시 `start` 가 `None` 을
//! 넘기면 install 자체가 일어나지 않아 임베드 폴백으로 진행한다.
//!
//! 협의된 JSON 스키마(site `/api/game/content/v1`):
//! ```json
//! {
//!   "version": 1,
//!   "generated_at": "<ISO8601>",
//!   "quests": [{ "id": "...", "ron": "QuestDef(...)" }, ...],
//!   "items": {
//!     "quest_items.ron":   "...",
//!     "weapons.ron":       "...",
//!     "armors.ron":        "...",
//!     "consumables.ron":   "...",
//!     "start_loadout.ron": "..."
//!   },
//!   "villagers": "Villagers(villagers: [...])",
//!   "monsters":  "Monsters(monsters: [...])"
//! }
//! ```

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

/// 원격에서 받아온 RON 콘텐츠. JSON 1회 파싱 후 정적으로 보관.
#[derive(Debug, Default)]
pub struct RemoteContent {
    /// `(파일명_또는_id, RON 문자열)`. 임베드 슬라이스(`EMBEDDED_QUESTS`)와 같은 모양.
    /// site 가 보낸 id 에 `.ron` 확장자가 없으면 자동으로 붙여 일관성 유지.
    pub quests: Vec<(String, String)>,
    /// `quest_items.ron`, `weapons.ron`, ... → RON 문자열.
    pub items: HashMap<String, String>,
    /// `villagers.ron` 의 RON 문자열(있으면).
    pub villagers: Option<String>,
    /// `monsters.ron` 의 RON 문자열(있으면).
    pub monsters: Option<String>,
}

/// 한 번만 set 가능한 전역 슬롯. native 에서는 절대 set 되지 않는다.
static REMOTE: OnceLock<RemoteContent> = OnceLock::new();

// ── JSON wire 포맷 (협의 스키마와 1:1) ─────────────────────────────────────────

#[derive(Deserialize)]
struct WireQuest {
    id: String,
    ron: String,
}

#[derive(Deserialize)]
struct WireContent {
    #[allow(dead_code)]
    #[serde(default)]
    version: Option<u32>,
    #[allow(dead_code)]
    #[serde(default)]
    generated_at: Option<String>,
    #[serde(default)]
    quests: Vec<WireQuest>,
    #[serde(default)]
    items: HashMap<String, String>,
    #[serde(default)]
    villagers: Option<String>,
    #[serde(default)]
    monsters: Option<String>,
}

/// site 가 보낸 JSON 문자열을 파싱해 전역 `REMOTE` 에 한 번만 set 한다.
///
/// 동작:
///   - JSON 파싱 실패 → `Err(메시지)`.
///   - 이미 set 되어 있으면 새 값을 적용하지 않고 `Ok(())` (idempotent).
///   - `quests[*].id` 에 `.ron` 확장자가 없으면 자동으로 추가(키 일관성).
///   - 누락 필드는 부분 install — 있는 카테고리만 REMOTE, 나머지는 빌드 임베드 폴백.
pub fn install_from_json(json: &str) -> Result<(), String> {
    let wire: WireContent = serde_json::from_str(json)
        .map_err(|e| format!("원격 콘텐츠 JSON 파싱 실패: {}", e))?;

    let mut quests: Vec<(String, String)> = Vec::with_capacity(wire.quests.len());
    for q in wire.quests {
        let name = if q.id.ends_with(".ron") { q.id } else { format!("{}.ron", q.id) };
        quests.push((name, q.ron));
    }

    let content = RemoteContent {
        quests,
        items: wire.items,
        villagers: wire.villagers,
        monsters: wire.monsters,
    };

    // 두 번째 호출은 무시(첫 install 의 결과를 유지) — set 의 Err 는 그냥 무시.
    let _ = REMOTE.set(content);
    Ok(())
}

// ── 조회 헬퍼(읽기 전용, 'static) ───────────────────────────────────────────────

/// REMOTE 에 quest 가 들어 있으면 `[(파일명, RON)]` 슬라이스를 반환.
pub fn remote_quests() -> Option<&'static [(String, String)]> {
    REMOTE.get().and_then(|c| if c.quests.is_empty() { None } else { Some(c.quests.as_slice()) })
}

/// REMOTE 에 해당 item 파일이 있으면 RON 문자열 반환. 예: `"quest_items.ron"`.
pub fn remote_item(filename: &str) -> Option<&'static str> {
    REMOTE.get().and_then(|c| c.items.get(filename).map(|s| s.as_str()))
}

/// REMOTE 에 villagers.ron RON 문자열이 있으면 반환.
pub fn remote_villagers() -> Option<&'static str> {
    REMOTE.get().and_then(|c| c.villagers.as_deref())
}

/// REMOTE 에 monsters.ron RON 문자열이 있으면 반환.
pub fn remote_monsters() -> Option<&'static str> {
    REMOTE.get().and_then(|c| c.monsters.as_deref())
}

// ── 테스트 ────────────────────────────────────────────────────────────────────
//
// 주의: `REMOTE` 는 프로세스 전역 `OnceLock` 이므로 테스트 1회만 install 가능.
// 따라서 install 동작 검증은 별도 프로세스(integration test)에서 한 번만 수행한다.
// 단위 테스트(여기)는 install 없이 가능한 것만:
//   - 파싱 자체의 성공/실패/누락 필드 처리(내부 `parse_only` 헬퍼로 분리).
//   - REMOTE 가 set 되기 전(즉, install 전) 헬퍼들이 None 을 반환.

#[cfg(test)]
fn parse_only(json: &str) -> Result<RemoteContent, String> {
    let wire: WireContent = serde_json::from_str(json)
        .map_err(|e| format!("원격 콘텐츠 JSON 파싱 실패: {}", e))?;
    let mut quests: Vec<(String, String)> = Vec::with_capacity(wire.quests.len());
    for q in wire.quests {
        let name = if q.id.ends_with(".ron") { q.id } else { format!("{}.ron", q.id) };
        quests.push((name, q.ron));
    }
    Ok(RemoteContent {
        quests,
        items: wire.items,
        villagers: wire.villagers,
        monsters: wire.monsters,
    })
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;

    #[test]
    fn 정상_JSON은_모든_필드를_담은_RemoteContent로_파싱된다() {
        let json = r#"{
            "version": 1,
            "generated_at": "2026-05-24T00:00:00Z",
            "quests": [
                {"id": "infiltration_quest", "ron": "QuestDef(id:\"infiltration_quest\")"},
                {"id": "another.ron",        "ron": "QuestDef(id:\"another\")"}
            ],
            "items": {
                "quest_items.ron":   "QuestItems(items: [])",
                "weapons.ron":       "Weapons(weapons: [])",
                "armors.ron":        "Armors(armors: [])",
                "consumables.ron":   "Consumables(consumables: [])",
                "start_loadout.ron": "StartLoadout(weapon: None)"
            },
            "villagers": "Villagers(villagers: [])",
            "monsters":  "Monsters(monsters: [])"
        }"#;
        let c = parse_only(json).expect("정상 JSON 은 Ok 여야 한다");
        assert_eq!(c.quests.len(), 2);
        // id 에 .ron 이 없으면 자동으로 붙는다.
        assert_eq!(c.quests[0].0, "infiltration_quest.ron");
        // id 에 이미 .ron 이 있으면 그대로.
        assert_eq!(c.quests[1].0, "another.ron");
        assert_eq!(c.items.len(), 5);
        assert!(c.villagers.is_some());
        assert!(c.monsters.is_some());
    }

    #[test]
    fn 잘못된_JSON은_Err를_반환한다() {
        let res = parse_only("{ not json");
        assert!(res.is_err(), "잘못된 JSON 은 Err 여야 한다");
        let msg = res.unwrap_err();
        assert!(msg.contains("원격 콘텐츠 JSON 파싱 실패"), "에러 메시지에 한국어 컨텍스트가 포함되어야 한다: {}", msg);
    }

    #[test]
    fn 누락된_필드들은_기본값으로_채워진다_부분_install() {
        // quests 만 있고 나머지 모두 없음.
        let json = r#"{
            "version": 1,
            "quests": [{"id": "only_one", "ron": "QuestDef(id:\"only_one\")"}]
        }"#;
        let c = parse_only(json).expect("부분 JSON 도 파싱돼야 한다");
        assert_eq!(c.quests.len(), 1);
        assert!(c.items.is_empty(), "items 누락 → 빈 HashMap");
        assert!(c.villagers.is_none(), "villagers 누락 → None");
        assert!(c.monsters.is_none(), "monsters 누락 → None");
    }

    #[test]
    fn 빈_quests_배열은_빈_벡터로_파싱된다() {
        let json = r#"{ "quests": [] }"#;
        let c = parse_only(json).expect("빈 quests 도 정상");
        assert!(c.quests.is_empty());
    }

    #[test]
    fn REMOTE_가_set_되지_않은_상태에서_헬퍼들은_None을_반환한다() {
        // 이 테스트는 OnceLock 가 아직 set 되지 않았을 때만 의미가 있다.
        // 같은 프로세스에서 install_from_json 이 호출되면 None 보장이 깨지므로,
        // 단위 테스트 모듈 내에서는 절대 install 하지 않는다(아래 install 테스트들도
        // parse_only 로만 검증).
        assert!(remote_quests().is_none());
        assert!(remote_item("weapons.ron").is_none());
        assert!(remote_villagers().is_none());
        assert!(remote_monsters().is_none());
    }
}
