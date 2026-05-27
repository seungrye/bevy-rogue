//! 세이브 백엔드 추상화 — native(파일 시스템) / wasm(localStorage) 단일 경로화.
//!
//! 게임 코드는 트레이트 `SaveBackend` 만 사용하며, 구체 구현은 cfg 로 갈린다.
//! - native: `FileBackend` — 기존 atomic rename(tmp → path) 흐름 보존.
//! - wasm:   `WebStorageBackend` — `localStorage.setItem/getItem/removeItem`.
//!
//! 직렬화는 호출자(`save/mod.rs`)가 RON 으로 처리하고, 백엔드는 단순히 String
//! 페이로드를 다룬다 (인코딩 중립).

use super::SaveConfig;

/// 세이브 데이터의 read/write/delete 만 다루는 최소 인터페이스.
///
/// 실패 시 패닉 금지 — wasm 의 시크릿 모드/quota 등 정상 실패 경로가 있으므로
/// write/delete 는 에러를 삼키고(또는 로그만 남기고), read 는 None 으로 표현.
pub trait SaveBackend {
    /// 저장된 페이로드를 읽는다. 없거나 실패 시 None.
    fn read(&self) -> Option<String>;
    /// 페이로드를 저장한다. 실패 시 noop (로그만).
    fn write(&self, content: &str);
    /// 저장된 페이로드를 삭제한다. 실패 시 noop.
    fn delete(&self);
}

// ── native: FileBackend ──────────────────────────────────────────────────────

/// 파일 시스템 기반 백엔드. tmp 에 쓴 뒤 rename 으로 원자적 교체.
#[cfg(not(target_arch = "wasm32"))]
pub struct FileBackend {
    pub path: String,
    pub tmp: String,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileBackend {
    pub fn from_config(config: &SaveConfig) -> Self {
        Self { path: config.path.clone(), tmp: config.tmp.clone() }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl SaveBackend for FileBackend {
    fn read(&self) -> Option<String> {
        std::fs::read_to_string(&self.path).ok()
    }

    fn write(&self, content: &str) {
        // 상위 디렉터리 보장 (parent 가 비어있으면 건너뜀).
        let parent = std::path::Path::new(&self.tmp).parent()
            .filter(|p| !p.as_os_str().is_empty());
        if let Some(parent) = parent {
            if let Err(e) = std::fs::create_dir_all(parent) {
                bevy::log::error!("세이브 디렉터리 생성 실패: {e}"); return;
            }
        }
        if let Err(e) = std::fs::write(&self.tmp, content) {
            bevy::log::error!("세이브 파일 쓰기 실패: {e}"); return;
        }
        if let Err(e) = std::fs::rename(&self.tmp, &self.path) {
            bevy::log::error!("세이브 파일 교체 실패: {e}"); return;
        }
    }

    fn delete(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

// ── wasm: WebStorageBackend ──────────────────────────────────────────────────

/// 브라우저 `window.localStorage` 기반 백엔드. key 는 SaveConfig path 의 basename.
/// 시크릿 모드/quota 등으로 storage 접근이 실패해도 패닉하지 않고 noop 한다.
#[cfg(target_arch = "wasm32")]
pub struct WebStorageBackend {
    pub key: String,
}

#[cfg(target_arch = "wasm32")]
impl WebStorageBackend {
    pub fn from_config(config: &SaveConfig) -> Self {
        Self { key: config.storage_key().to_string() }
    }

    /// `window.localStorage` 핸들을 얻는다 — 없으면 None (시크릿 모드 등).
    fn storage() -> Option<web_sys::Storage> {
        let win = web_sys::window()?;
        // local_storage() → Result<Option<Storage>, JsValue>: 둘 다 None 으로 평탄화.
        win.local_storage().ok().flatten()
    }
}

#[cfg(target_arch = "wasm32")]
impl SaveBackend for WebStorageBackend {
    fn read(&self) -> Option<String> {
        let storage = Self::storage()?;
        storage.get_item(&self.key).ok().flatten()
    }

    fn write(&self, content: &str) {
        let Some(storage) = Self::storage() else {
            bevy::log::warn!("localStorage 미가용 — 세이브 쓰기 무시");
            return;
        };
        if let Err(e) = storage.set_item(&self.key, content) {
            // QuotaExceededError 등.
            bevy::log::warn!("localStorage setItem 실패: {:?}", e);
        }
    }

    fn delete(&self) {
        let Some(storage) = Self::storage() else { return };
        let _ = storage.remove_item(&self.key);
    }
}

// ── 백엔드 생성 헬퍼 ─────────────────────────────────────────────────────────

/// 현재 타깃에 맞는 SaveBackend 박싱 구현을 만든다.
pub fn make_backend(config: &SaveConfig) -> Box<dyn SaveBackend> {
    #[cfg(not(target_arch = "wasm32"))]
    { Box::new(FileBackend::from_config(config)) }
    #[cfg(target_arch = "wasm32")]
    { Box::new(WebStorageBackend::from_config(config)) }
}

// ── 단위 테스트 ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;

    // ── 트레이트 시그니처 안정성 (회귀 방지) ──────────────────────────────────

    #[test]
    fn save_backend_트레이트는_read_write_delete_세_메서드를_노출한다() {
        // 컴파일 가능 자체로 트레이트 시그니처의 회귀 방지. 한 곳에서 모든
        // dyn 메서드를 호출할 수 있음을 확인한다.
        fn _check<B: SaveBackend + ?Sized>(b: &B) {
            let _ = b.read();
            b.write("dummy");
            b.delete();
        }
        let cfg = SaveConfig::default();
        let backend = make_backend(&cfg);
        // 호출 자체는 native FileBackend(SAVE_PATH) 로 디스패치 — 부작용은 신경 X
        // (실제 SAVE_PATH 는 절대 건드리지 않도록 임시 백업 패턴은 다른 테스트에 위임).
        let _ = backend.read();
    }

    // ── key 결정(wasm 백엔드용) ──────────────────────────────────────────────

    #[test]
    fn storage_key는_path의_basename을_반환한다() {
        let cfg = SaveConfig { path: "save/progress.ron".into(), tmp: "save/progress.ron.tmp".into() };
        assert_eq!(cfg.storage_key(), "progress.ron");
    }

    #[test]
    fn storage_key는_슬래시가_없으면_path_자체를_쓴다() {
        let cfg = SaveConfig { path: "bare.ron".into(), tmp: "bare.ron.tmp".into() };
        assert_eq!(cfg.storage_key(), "bare.ron");
    }

    #[test]
    fn storage_key는_여러_단계의_경로에서도_마지막_컴포넌트만_쓴다() {
        let cfg = SaveConfig { path: "a/b/c/progress.ron".into(), tmp: "a/b/c/progress.ron.tmp".into() };
        assert_eq!(cfg.storage_key(), "progress.ron");
    }

    // ── native FileBackend round-trip (RON 직렬화는 호출자가 처리) ────────────

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn 파일_백엔드는_write_후_read하면_같은_페이로드를_돌려준다() {
        // 임시 파일에 RON 페이로드(예시) 를 직접 왕복시켜본다.
        let n: u32 = std::process::id();
        let path = std::env::temp_dir().join(format!("bevy_rogue_be_{}_{}.ron", n, line!()));
        let tmp  = std::env::temp_dir().join(format!("bevy_rogue_be_{}_{}.ron.tmp", n, line!()));
        let cfg = SaveConfig {
            path: path.to_string_lossy().into_owned(),
            tmp:  tmp.to_string_lossy().into_owned(),
        };
        let be = FileBackend::from_config(&cfg);
        assert!(be.read().is_none(), "사전: 파일 없으면 None");

        let payload = r#"(version: 1, data: "ok")"#;
        be.write(payload);
        let got = be.read();
        assert_eq!(got.as_deref(), Some(payload));

        be.delete();
        assert!(be.read().is_none(), "delete 후 None");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&tmp);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn 파일_백엔드의_delete는_파일이_없어도_패닉하지_않는다() {
        let cfg = SaveConfig {
            path: std::env::temp_dir().join("bevy_rogue_no_such_file.ron").to_string_lossy().into_owned(),
            tmp:  std::env::temp_dir().join("bevy_rogue_no_such_file.ron.tmp").to_string_lossy().into_owned(),
        };
        let be = FileBackend::from_config(&cfg);
        be.delete(); // no-op
    }

    // ── make_backend factory ──────────────────────────────────────────────────

    #[test]
    fn make_backend는_현재_타깃에_맞는_백엔드를_만든다() {
        let cfg = SaveConfig::default();
        let _be = make_backend(&cfg);
        // 타입 디스패치 자체가 확인되면 OK — read 호출은 부작용 없는 read.
    }

    // ── RON round-trip (호출자 책임) — 트레이트 결합 확인 ─────────────────────

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn 백엔드는_RON_페이로드를_그대로_보존한다_왕복후_동일() {
        // 호출자가 RON 으로 인코딩한 페이로드를 백엔드가 변형 없이 보존하는지 확인.
        // WebStorageBackend 도 같은 String 페이로드 인터페이스라 의미가 동일하다.
        let n: u32 = std::process::id();
        let path = std::env::temp_dir().join(format!("bevy_rogue_ron_{}.ron", n));
        let tmp = std::env::temp_dir().join(format!("bevy_rogue_ron_{}.ron.tmp", n));
        let cfg = SaveConfig {
            path: path.to_string_lossy().into_owned(),
            tmp:  tmp.to_string_lossy().into_owned(),
        };
        let be = FileBackend::from_config(&cfg);
        let original = r#"(version: 5, global_seed: 12345, global_turn: 7)"#;
        be.write(original);
        let restored = be.read().expect("read 후 Some");
        assert_eq!(restored, original);
        be.delete();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&tmp);
    }
}
