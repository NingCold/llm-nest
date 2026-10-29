//! Current-user protected desktop credentials. Never exposed as a generic file API.
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

pub(crate) const THIRTY_DAYS_MS: u64 = 30 * 24 * 60 * 60 * 1000;
pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
pub(crate) fn supported() -> bool {
    cfg!(windows)
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Session {
    pub account: Uuid,
    pub cookie: String,
    pub expires_at: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Unlock {
    pub secret: String,
    pub expires_at: u64,
}
pub(crate) struct Vault {
    root: PathBuf,
    #[cfg(test)]
    _temp: Option<tempfile::TempDir>,
}
fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
impl Vault {
    pub(crate) fn new() -> Self {
        #[cfg(not(test))]
        let root = crate::config_path::default_data_dir().join("secure-credentials");
        #[cfg(test)]
        let temp = tempfile::tempdir().unwrap();
        #[cfg(test)]
        let root = temp.path().to_owned();
        Self {
            root,
            #[cfg(test)]
            _temp: Some(temp),
        }
    }
    fn server_dir(&self, server: &str) -> PathBuf {
        self.root.join(hash(server))
    }
    fn session_path(&self, server: &str) -> PathBuf {
        self.server_dir(server).join("session.bin")
    }
    #[cfg(test)]
    pub(crate) fn test_session_path(&self, server: &str) -> PathBuf {
        self.session_path(server)
    }
    fn unlock_path(&self, server: &str, account: Uuid, device: Uuid) -> PathBuf {
        self.server_dir(server)
            .join(hash(&account.to_string()))
            .join(format!("{}.bin", hash(&device.to_string())))
    }
    fn read<T: DeserializeOwned>(&self, path: &Path) -> Result<Option<T>, String> {
        if !supported() {
            return Ok(None);
        }
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("无法读取受保护的本机凭据".into()),
        };
        let bytes = protect(&bytes, path, false)?;
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| "本机凭据格式无效".into())
    }
    fn write(&self, path: &Path, value: &impl Serialize) -> Result<(), String> {
        let bytes = serde_json::to_vec(value).map_err(|_| "无法编码本机凭据")?;
        let bytes = protect(&bytes, path, true)?;
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent).map_err(|_| "无法创建本机凭据目录")?;
        let mut file =
            tempfile::NamedTempFile::new_in(parent).map_err(|_| "无法创建本机凭据文件")?;
        file.write_all(&bytes).map_err(|_| "无法保存本机凭据")?;
        file.as_file().sync_all().map_err(|_| "无法同步本机凭据")?;
        file.persist(path).map_err(|_| "无法替换本机凭据")?;
        Ok(())
    }
    pub(crate) fn load_session(&self, server: &str) -> Result<Option<Session>, String> {
        let value: Option<Session> = self.read(&self.session_path(server))?;
        if let Some(value) = &value
            && value.expires_at <= now_ms()
        {
            self.clear(server, Some(value.account))?;
            return Ok(None);
        }
        Ok(value)
    }
    pub(crate) fn save_session(&self, server: &str, value: &Session) -> Result<(), String> {
        self.write(&self.session_path(server), value)
    }
    pub(crate) fn remove_session(&self, server: &str) -> Result<(), String> {
        remove(&self.session_path(server))
    }
    pub(crate) fn clear(&self, server: &str, account: Option<Uuid>) -> Result<(), String> {
        let session = self.remove_session(server);
        let unlock = if let Some(account) = account {
            // Only this fixed, hashed account directory contains unlock records.
            let dir = self.server_dir(server).join(hash(&account.to_string()));
            match std::fs::remove_dir_all(dir) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err("无法清除本机自动解锁材料".into()),
            }
        } else {
            Ok(())
        };
        session.and(unlock)
    }
    pub(crate) fn save_unlock(
        &self,
        server: &str,
        account: Uuid,
        device: Uuid,
        secret: String,
    ) -> Result<u64, String> {
        if secret.is_empty() || secret.len() > 4096 {
            return Err("自动解锁材料长度无效".into());
        }
        let expires_at = now_ms() + THIRTY_DAYS_MS;
        self.write(
            &self.unlock_path(server, account, device),
            &Unlock { secret, expires_at },
        )?;
        Ok(expires_at)
    }
    pub(crate) fn load_unlock(
        &self,
        server: &str,
        account: Uuid,
        device: Uuid,
    ) -> Result<Option<Unlock>, String> {
        let path = self.unlock_path(server, account, device);
        let value: Option<Unlock> = self.read(&path)?;
        if let Some(value) = &value
            && value.expires_at <= now_ms()
        {
            remove(&path)?;
            return Ok(None);
        }
        Ok(value)
    }
    pub(crate) fn remove_unlock(
        &self,
        server: &str,
        account: Uuid,
        device: Uuid,
    ) -> Result<(), String> {
        remove(&self.unlock_path(server, account, device))
    }
}
fn remove(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("无法清除本机凭据".into()),
    }
}
#[cfg(not(windows))]
fn protect(_: &[u8], _: &Path, _: bool) -> Result<Vec<u8>, String> {
    Err("此平台尚不支持安全保存登录与自动解锁材料".into())
}
#[cfg(windows)]
fn protect(bytes: &[u8], path: &Path, encrypt: bool) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    };
    let entropy = Sha256::digest(path.to_string_lossy().as_bytes());
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: entropy.len() as u32,
        pbData: entropy.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // Omitting LOCAL_MACHINE binds the blob to the current Windows user. UI is forbidden.
    let ok = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if ok == 0 {
        return Err("Windows 无法保护或解锁本机凭据".into());
    }
    let result =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(result)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn locked_credential_failures_are_visible_and_cleanup_still_removes_unlock() {
        use std::os::windows::fs::OpenOptionsExt;
        let vault = Vault::new();
        let server = "https://example.test";
        let account = Uuid::new_v4();
        let device = Uuid::new_v4();
        let session = Session {
            account,
            cookie: "nest_session=test-cookie".into(),
            expires_at: now_ms() + THIRTY_DAYS_MS,
        };
        vault.save_session(server, &session).unwrap();
        vault
            .save_unlock(server, account, device, "test-secret".into())
            .unwrap();
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(vault.session_path(server))
            .unwrap();
        let error = vault.save_session(server, &session).unwrap_err();
        assert!(!error.contains("test-cookie"));
        assert!(vault.clear(server, Some(account)).is_err());
        assert!(
            vault
                .load_unlock(server, account, device)
                .unwrap()
                .is_none()
        );
        drop(lock);
        vault.clear(server, Some(account)).unwrap();
        assert!(vault.load_session(server).unwrap().is_none());
    }
    #[test]
    fn dpapi_restart_scope_expiry_and_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault {
            root: dir.path().into(),
            _temp: None,
        };
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let device = Uuid::new_v4();
        let server = "https://example.test";
        vault
            .save_unlock(server, a, device, "test-local-secret".into())
            .unwrap();
        vault
            .save_session(
                server,
                &Session {
                    account: a,
                    cookie: "nest_session=test-cookie".into(),
                    expires_at: now_ms() + THIRTY_DAYS_MS,
                },
            )
            .unwrap();
        let reopened = Vault {
            root: dir.path().into(),
            _temp: None,
        };
        assert_eq!(
            reopened
                .load_unlock(server, a, device)
                .unwrap()
                .unwrap()
                .secret,
            "test-local-secret"
        );
        assert_eq!(reopened.load_session(server).unwrap().unwrap().account, a);
        assert!(reopened.load_unlock(server, b, device).unwrap().is_none());
        assert!(
            reopened
                .load_unlock("https://other.test", a, device)
                .unwrap()
                .is_none()
        );
        let path = vault.unlock_path(server, a, device);
        let bytes = std::fs::read(&path).unwrap();
        assert!(
            !bytes
                .windows(b"test-local-secret".len())
                .any(|v| v == b"test-local-secret")
        );
        let other = vault.unlock_path(server, b, device);
        std::fs::create_dir_all(other.parent().unwrap()).unwrap();
        std::fs::write(&other, &bytes).unwrap();
        assert!(reopened.load_unlock(server, b, device).is_err());
        vault
            .write(
                &path,
                &Unlock {
                    secret: "expired".into(),
                    expires_at: now_ms() - 1,
                },
            )
            .unwrap();
        assert!(reopened.load_unlock(server, a, device).unwrap().is_none());
        assert!(!path.exists());
        vault
            .save_unlock(server, a, device, "expire-with-session".into())
            .unwrap();
        vault
            .save_session(
                server,
                &Session {
                    account: a,
                    cookie: "nest_session=expired".into(),
                    expires_at: now_ms() - 1,
                },
            )
            .unwrap();
        assert!(reopened.load_session(server).unwrap().is_none());
        assert!(reopened.load_unlock(server, a, device).unwrap().is_none());
        std::fs::write(&path, b"broken").unwrap_err();
        vault.save_unlock(server, a, device, "test".into()).unwrap();
        std::fs::write(&path, b"broken").unwrap();
        assert!(reopened.load_unlock(server, a, device).is_err());
    }
}
