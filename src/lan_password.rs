use hbb_common::{config::Config, ResultType};

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
#[derive(serde::Serialize, serde::Deserialize, Zeroize, ZeroizeOnDrop)]
struct SavedPassword {
    username: String,
    password: String,
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
fn entry() -> ResultType<keyring::Entry> {
    use hbb_common::sha2::{Digest, Sha256};
    let account = format!("{:x}", Sha256::digest(Config::file().to_string_lossy().as_bytes()));
    Ok(keyring::Entry::new("SubnetDesk-EasyTier-Embedded.LANServerPassword", &account)?)
}

pub(crate) fn set_credentials(username: &str, password: &[u8]) -> ResultType<()> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        hbb_common::lan::validate_password(password)?;
        let saved = SavedPassword {
            username: username.to_owned(),
            password: String::from_utf8(password.to_vec())?,
        };
        let entry = entry()?;
        let previous = match entry.get_password() {
            Ok(value) => Some(Zeroizing::new(value)),
            Err(keyring::Error::NoEntry) => None,
            Err(error) => return Err(error.into()),
        };
        let encoded = Zeroizing::new(serde_json::to_string(&saved)?);
        entry.set_password(&encoded)?;
        if let Err(error) = Config::set_lan_credentials(username, password) {
            let rollback = match previous {
                Some(value) => entry.set_password(&value),
                None => entry.delete_password(),
            };
            if let Err(rollback_error) = rollback {
                hbb_common::log::error!("无法回滚 LAN 系统凭据存储: {rollback_error}");
            }
            return Err(error);
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Config::set_lan_credentials(username, password)?;
        Ok(())
    }
}

pub(crate) fn load() -> ResultType<String> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        if !Config::lan_credentials_configured() { return Ok(String::new()); }
        let encoded = match entry()?.get_password() {
            Ok(value) => Zeroizing::new(value),
            Err(keyring::Error::NoEntry) => return Ok(String::new()),
            Err(error) => return Err(error.into()),
        };
        let saved: SavedPassword = serde_json::from_str(&encoded)
            .map_err(|_| hbb_common::anyhow::anyhow!("系统凭据存储中的 LAN 密码数据无效"))?;
        // 配置恢复或从其他入口修改密码后，不回显过期的凭据。
        if saved.username == Config::get_lan_access_username()
            && Config::verify_lan_credentials(&saved.username, saved.password.as_bytes())?
        {
            return Ok(saved.password.clone());
        }
    }
    Ok(String::new())
}
