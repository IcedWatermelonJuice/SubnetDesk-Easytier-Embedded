//! 密码加密的同机配置备份。恢复只在下一次主进程持有单实例锁后执行。
use hbb_common::{anyhow::{anyhow, bail}, config::Config, ResultType};
use hbb_common::sodiumoxide::crypto::{hash::sha256, pwhash::argon2id13 as pwhash, secretbox};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs::{self, File}, io::{Read, Write}, path::Path, sync::Mutex};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const MAGIC: &[u8; 8] = b"SDCFG001";
const MAX_BLOB: usize = 64 * 1024 * 1024;
const MAX_DATA: usize = 16 * 1024 * 1024;
const MAX_FILES: usize = 4096;
const RESTORE_SERVICE: &str = "SubnetDesk-EasyTier-Embedded.ConfigurationRestore";
const PASSWORD_SERVICE: &str = "SubnetDesk-EasyTier-Embedded.LANServerPassword";
const IDENTITY_SERVICE: &str = "com.zibochen.SubnetDesk.LANIdentities";
const PEER_PASSWORD_SERVICE: &str = "com.zibochen.SubnetDesk.LANCredentials";
const PENDING: &str = ".configuration-restore.json";

#[derive(Default, Serialize)]
struct Status { busy: bool, request_id: String, success: bool, error: String, restore_pending: bool }
static STATUS: Mutex<Option<Status>> = Mutex::new(None);

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct Request { action: String, #[serde(default)] path: String, #[serde(default)] password: String, request_id: String }

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct Entry { path: String, data: Vec<u8> }

#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct Credential { service: String, account: String, value: Option<String> }

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct Snapshot { version: u32, machine: Vec<u8>, files: Vec<Entry>, easytier: Option<Vec<u8>>, credentials: Vec<Credential> }

#[derive(Serialize)]
struct RestoreResult { success: bool, message: String, error: String, original_backup_path: String }
static RESTORE_RESULT: Mutex<Option<RestoreResult>> = Mutex::new(None);

fn paths() -> ResultType<&'static hbb_common::app_paths::AppPaths> {
    hbb_common::app_paths::get().ok_or_else(|| anyhow!("应用数据目录尚未初始化"))
}

fn machine() -> ResultType<Vec<u8>> {
    // 不调用 get_uuid 的 Config 后备路径：启动恢复不能提前初始化旧 Config 缓存。
    let id = hbb_common::machine_uid::get().map_err(|_| anyhow!("无法确认当前电脑的配置加密标识"))?;
    Ok(sha256::hash(id.as_bytes()).0.to_vec())
}

fn password_account() -> String {
    use hbb_common::sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(Config::file().to_string_lossy().as_bytes()))
}

fn valid_peer_account(account: &str) -> bool {
    account.strip_prefix("lan-credential-").is_some_and(|fingerprint| {
        fingerprint.len() == 64 && fingerprint.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn entry(credential: &Credential) -> ResultType<keyring::Entry> {
    let account = if credential.service == PASSWORD_SERVICE { password_account() } else { credential.account.clone() };
    Ok(keyring::Entry::new(&credential.service, &account)?)
}

fn read_credential(service: &str, account: &str) -> ResultType<Credential> {
    let mut value = Credential { service: service.into(), account: account.into(), value: None };
    value.value = match entry(&value)?.get_password() {
        Ok(text) => Some(text), Err(keyring::Error::NoEntry) => None,
        Err(_) => bail!("无法读取系统凭据存储，请解锁系统钥匙串后重试"),
    };
    Ok(value)
}

fn write_credentials(values: &[Credential]) -> ResultType<()> {
    for value in values {
        let item = entry(value)?;
        match &value.value {
            Some(text) => item.set_password(text).map_err(|_| anyhow!("无法恢复系统凭据"))?,
            None => match item.delete_password() {
                Ok(()) | Err(keyring::Error::NoEntry) => {},
                Err(_) => bail!("无法恢复系统凭据"),
            },
        }
    }
    Ok(())
}

fn safe_relative(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && value.split('/').all(|part| {
        !part.is_empty() && part != "." && part != ".."
            && !part.ends_with(['.', ' '])
            && !part.chars().any(|c| c.is_control() || matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
            && !matches!(part.split('.').next().unwrap_or_default().to_ascii_uppercase().as_str(),
                "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "COM2" | "COM3" | "COM4" | "COM5" | "COM6" | "COM7" | "COM8" | "COM9" | "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5" | "LPT6" | "LPT7" | "LPT8" | "LPT9")
    })
}

fn excluded(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    path == "autostart.xml" || path.ends_with("_lan_peers.toml") || path.ends_with("_status.toml")
}

fn check_path(path: &Path) -> ResultType<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) => {
                if meta.file_type().is_symlink() { bail!("拒绝符号链接路径"); }
                #[cfg(windows)] {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 { bail!("拒绝重解析点路径"); }
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(_) => bail!("无法检查文件路径"),
        }
    }
    Ok(())
}

fn read_limited(path: &Path) -> ResultType<Vec<u8>> {
    check_path(path)?;
    let file = File::open(path).map_err(|_| anyhow!("无法打开配置或备份文件"))?;
    if !file.metadata()?.is_file() { bail!("路径不是普通文件"); }
    let mut data = Vec::new();
    file.take((MAX_BLOB + 1) as u64).read_to_end(&mut data)?;
    if data.len() > MAX_BLOB { data.zeroize(); bail!("备份文件超过 64 MiB 限制"); }
    Ok(data)
}

fn collect(directory: &Path, relative: &str, files: &mut Vec<Entry>, total: &mut usize) -> ResultType<()> {
    check_path(directory)?;
    for item in fs::read_dir(directory)? {
        let item = item?;
        let name = item.file_name().into_string().map_err(|_| anyhow!("配置文件名编码无效"))?;
        let relative = if relative.is_empty() { name } else { format!("{relative}/{name}") };
        if !safe_relative(&relative) { bail!("配置文件路径无效"); }
        if excluded(&relative) { continue; }
        check_path(&item.path())?;
        if item.file_type()?.is_dir() { collect(&item.path(), &relative, files, total)?; }
        else if item.file_type()?.is_file() {
            let data = read_limited(&item.path())?;
            *total += data.len();
            if *total > MAX_DATA || files.len() >= MAX_FILES { bail!("配置数据超过备份容量限制"); }
            files.push(Entry { path: relative, data });
        } else { bail!("配置目录包含不支持的文件类型"); }
    }
    Ok(())
}

fn capture() -> ResultType<Snapshot> {
    let mut result = Snapshot { version: 1, machine: machine()?, files: Vec::new(), easytier: None, credentials: Vec::new() };
    collect(&paths()?.config(), "", &mut result.files, &mut 0)?;
    let store = crate::easytier::backup_store_path().map_err(|error| anyhow!(error))?;
    if store.exists() { result.easytier = Some(read_limited(&store)?); }
    result.credentials.push(read_credential(PASSWORD_SERVICE, "lan-server")?);
    // 直接读取已捕获的 LocalConfig 文本，不触碰进程全局配置缓存。
    let mut identities = HashSet::new();
    for file in &result.files {
        if !file.path.contains('/') && file.path.ends_with("_local.toml") {
            let text = std::str::from_utf8(&file.data).map_err(|_| anyhow!("本机配置编码无效"))?;
            let config: hbb_common::toml::Value = hbb_common::toml::from_str(text).map_err(|_| anyhow!("本机配置格式无效"))?;
            if let Some(table) = config.get("lan_identities").and_then(|value| value.as_table()) {
                identities.extend(table.keys().cloned());
            }
        }
    }
    for id in identities {
        result.credentials.push(read_credential(IDENTITY_SERVICE, &id)?);
    }
    for file in &result.files {
        if let Some(account) = file.path.strip_prefix("peers/").and_then(|name| name.strip_suffix(".toml")) {
            if valid_peer_account(account) {
                result.credentials.push(read_credential(PEER_PASSWORD_SERVICE, account)?);
            }
        }
    }
    validate(&result)?;
    Ok(result)
}

fn validate(snapshot: &Snapshot) -> ResultType<()> {
    if snapshot.version != 1 || snapshot.machine != machine()? { bail!("此备份仅支持在原电脑恢复"); }
    if snapshot.files.len() > MAX_FILES || snapshot.credentials.len() > MAX_FILES + 1025 { bail!("备份条目数量过多"); }
    let mut names = HashSet::new();
    let mut size = 0usize;
    for item in &snapshot.files {
        if !safe_relative(&item.path) || excluded(&item.path) || !names.insert(item.path.to_lowercase()) { bail!("备份包含无效或重复的配置路径"); }
        size = size.saturating_add(item.data.len());
    }
    for name in &names {
        let mut ancestor = name.as_str();
        while let Some((parent, _)) = ancestor.rsplit_once('/') {
            if names.contains(parent) { bail!("备份路径同时被用作文件和目录"); }
            ancestor = parent;
        }
    }
    if let Some(data) = &snapshot.easytier {
        size = size.saturating_add(data.len());
        crate::easytier::validate_backup_store(data).map_err(|error| anyhow!(error))?;
    }
    if size > MAX_DATA { bail!("配置数据超过备份容量限制"); }
    let mut credentials = HashSet::new();
    for value in &snapshot.credentials {
        let valid = match value.service.as_str() {
            PASSWORD_SERVICE => value.account == "lan-server",
            IDENTITY_SERVICE => Uuid::parse_str(&value.account).is_ok_and(|id| id.to_string() == value.account),
            PEER_PASSWORD_SERVICE => valid_peer_account(&value.account),
            _ => false,
        };
        if !valid || !credentials.insert((&value.service, &value.account)) || value.value.as_ref().is_some_and(|value| value.len() > 64 * 1024) {
            bail!("备份中的系统凭据条目无效");
        }
    }
    Ok(())
}

fn serialize(snapshot: &Snapshot) -> ResultType<Zeroizing<Vec<u8>>> {
    let data = Zeroizing::new(serde_json::to_vec(snapshot)?);
    if data.len() > MAX_BLOB - 128 { bail!("序列化后的配置超过备份容量限制"); }
    Ok(data)
}

fn encrypt(data: &[u8], password: &[u8]) -> ResultType<Vec<u8>> {
    let salt = pwhash::gen_salt();
    let mut key = secretbox::Key([0; secretbox::KEYBYTES]);
    pwhash::derive_key(&mut key.0, password, &salt, pwhash::OPSLIMIT_INTERACTIVE, pwhash::MEMLIMIT_INTERACTIVE)
        .map_err(|_| anyhow!("无法派生备份密钥"))?;
    let nonce = secretbox::gen_nonce();
    let mut result = MAGIC.to_vec();
    result.extend_from_slice(&salt.0);
    result.extend_from_slice(&nonce.0);
    result.extend_from_slice(&secretbox::seal(data, &nonce, &key));
    Ok(result)
}

fn decrypt(data: &[u8], password: &[u8]) -> ResultType<Zeroizing<Vec<u8>>> {
    let head = MAGIC.len() + pwhash::SALTBYTES + secretbox::NONCEBYTES;
    if data.len() < head + secretbox::MACBYTES || data.len() > MAX_BLOB || &data[..MAGIC.len()] != MAGIC { bail!("备份格式无效或版本不受支持"); }
    let salt = pwhash::Salt::from_slice(&data[MAGIC.len()..MAGIC.len()+pwhash::SALTBYTES]).ok_or_else(|| anyhow!("备份格式无效"))?;
    let nonce = secretbox::Nonce::from_slice(&data[MAGIC.len()+pwhash::SALTBYTES..head]).ok_or_else(|| anyhow!("备份格式无效"))?;
    let mut key = secretbox::Key([0; secretbox::KEYBYTES]);
    pwhash::derive_key(&mut key.0, password, &salt, pwhash::OPSLIMIT_INTERACTIVE, pwhash::MEMLIMIT_INTERACTIVE)
        .map_err(|_| anyhow!("无法派生备份密钥"))?;
    secretbox::open(&data[head..], &nonce, &key).map(Zeroizing::new).map_err(|_| anyhow!("备份密码错误或文件已损坏"))
}

fn write_atomic(path: &Path, data: &[u8], overwrite: bool) -> ResultType<()> {
    check_path(path)?;
    let parent = path.parent().ok_or_else(|| anyhow!("文件路径无效"))?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(data)?;
    file.as_file().sync_all()?;
    if overwrite { file.persist(path).map_err(|_| anyhow!("无法原子保存配置文件"))?; }
    else { file.persist_noclobber(path).map_err(|_| anyhow!("无法保存备份，请选择尚不存在的文件路径"))?; }
    Ok(())
}

fn private_directory(path: &Path) -> ResultType<()> {
    fs::create_dir(path)?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)] {
        use std::os::windows::ffi::OsStrExt;
        use windows::{core::PCWSTR, Win32::{Foundation::{HLOCAL, LocalFree}, Security::{Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, SetFileSecurityW, PSECURITY_DESCRIPTOR, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION}}};
        let sid = crate::platform::windows::current_process_user_sid_string()?;
        let sddl: Vec<u16> = format!("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;{sid})").encode_utf16().chain(Some(0)).collect();
        let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(sddl.as_ptr()), 1, &mut descriptor, None)?;
            let ok = SetFileSecurityW(PCWSTR(name.as_ptr()), DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION, descriptor);
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
            if !ok.as_bool() { return Err(std::io::Error::last_os_error().into()); }
        }
    }
    Ok(())
}

fn staging_key(id: &str) -> ResultType<keyring::Entry> {
    let account = format!("{}:{id}", paths()?.ipc_namespace());
    Ok(keyring::Entry::new(RESTORE_SERVICE, &account)?)
}

fn seal_stage(path: &Path, data: &[u8], key: &secretbox::Key) -> ResultType<()> {
    let nonce = secretbox::gen_nonce();
    let mut encoded = nonce.0.to_vec();
    encoded.extend_from_slice(&secretbox::seal(data, &nonce, key));
    write_atomic(path, &encoded, false)
}

fn open_stage(path: &Path, key: &secretbox::Key) -> ResultType<Zeroizing<Vec<u8>>> {
    let data = read_limited(path)?;
    if data.len() < secretbox::NONCEBYTES + secretbox::MACBYTES { bail!("恢复暂存文件损坏"); }
    let nonce = secretbox::Nonce::from_slice(&data[..secretbox::NONCEBYTES]).ok_or_else(|| anyhow!("恢复暂存文件损坏"))?;
    secretbox::open(&data[secretbox::NONCEBYTES..], &nonce, key).map(Zeroizing::new).map_err(|_| anyhow!("恢复暂存文件损坏"))
}

fn export(path: &Path, password: &[u8]) -> ResultType<()> {
    if !path.is_absolute() { bail!("请选择绝对文件路径"); }
    let parent = fs::canonicalize(path.parent().ok_or_else(|| anyhow!("备份路径无效"))?)?;
    let root = fs::canonicalize(&paths()?.root)?;
    let store = crate::easytier::backup_store_path().map_err(|error| anyhow!(error))?;
    if parent.starts_with(&root) || parent.starts_with(store.parent().ok_or_else(|| anyhow!("方案目录无效"))?) { bail!("备份文件必须保存在应用数据和 EasyTier 配置目录之外"); }
    let snapshot = capture()?;
    let data = serialize(&snapshot)?;
    write_atomic(path, &encrypt(&data, password)?, false)
}

fn stage(path: &Path, password: &[u8]) -> ResultType<()> {
    let marker = paths()?.root.join(PENDING);
    if marker.exists() { bail!("已有待恢复配置，请先完整退出并重新启动应用"); }
    let encoded = read_limited(path)?;
    let data = decrypt(&encoded, password)?;
    let snapshot: Snapshot = serde_json::from_slice(&data).map_err(|_| anyhow!("备份内容无效"))?;
    validate(&snapshot)?;
    let id = Uuid::new_v4().to_string();
    let directory = paths()?.root.join(format!(".restore-{id}"));
    private_directory(&directory)?;
    let key = secretbox::gen_key();
    let secret = Zeroizing::new(serde_json::to_string(&key.0.to_vec())?);
    staging_key(&id)?.set_password(&secret).map_err(|_| anyhow!("无法安全保存恢复密钥"))?;
    seal_stage(&directory.join("snapshot.enc"), &data, &key)?;
    write_atomic(&marker, id.as_bytes(), false)?;
    Ok(())
}

fn restore_easytier(data: Option<&[u8]>) -> ResultType<()> {
    let path = crate::easytier::backup_store_path().map_err(|error| anyhow!(error))?;
    match data {
        Some(data) => write_atomic(&path, data, true),
        None => match fs::remove_file(&path) { Ok(()) => Ok(()), Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()), Err(error) => Err(error.into()) },
    }
}

fn restore_original_directory(config: &Path, directory: &Path) -> ResultType<()> {
    let old = directory.join("previous-config");
    if old.exists() {
        if config.exists() { fs::rename(&config, directory.join(format!("failed-config-{}", Uuid::new_v4())))?; }
        fs::rename(&old, &config)?;
    }
    Ok(())
}

fn rollback(directory: &Path, previous: &Snapshot) -> ResultType<()> {
    restore_original_directory(&paths()?.config(), directory)?;
    restore_easytier(previous.easytier.as_deref())?;
    write_credentials(&previous.credentials)
}

fn apply(directory: &Path, snapshot: &Snapshot, key: &secretbox::Key) -> ResultType<()> {
    let next = directory.join("next-config");
    private_directory(&next)?;
    for item in &snapshot.files {
        let file = next.join(&item.path);
        fs::create_dir_all(file.parent().ok_or_else(|| anyhow!("配置路径无效"))?)?;
        write_atomic(&file, &item.data, false)?;
    }
    // 系统任务及运行缓存不属于备份，保留当前电脑上的原文件。
    for item in fs::read_dir(paths()?.config())? {
        let item = item?;
        if item.file_name().to_str().is_some_and(excluded) {
            write_atomic(&next.join(item.file_name()), &read_limited(&item.path())?, false)?;
        }
    }
    let mut previous = capture()?;
    // 新备份中的身份可能是当前配置没有的条目，也要记录其原值以便回滚。
    for value in &snapshot.credentials {
        if !previous.credentials.iter().any(|old| old.service == value.service && old.account == value.account) {
            previous.credentials.push(read_credential(&value.service, &value.account)?);
        }
    }
    seal_stage(&directory.join("rollback.enc"), &serialize(&previous)?, key)?;
    let result = (|| -> ResultType<()> {
        fs::rename(paths()?.config(), directory.join("previous-config"))?;
        fs::rename(&next, paths()?.config())?;
        restore_easytier(snapshot.easytier.as_deref())?;
        write_credentials(&snapshot.credentials)?;
        write_atomic(&directory.join("committed"), b"1", false)?;
        Ok(())
    })();
    if result.is_err() {
        rollback(directory, &previous)?;
        write_atomic(&directory.join("rolled-back"), b"1", false)?;
    }
    result
}

fn main_launch() -> bool {
    let mut skip = false;
    for arg in std::env::args().skip(1) {
        if skip { skip = false; continue; }
        if arg == "--data-dir" { skip = true; continue; }
        if arg.starts_with("--data-dir=") || matches!(arg.as_str(), "--portable" | "--background" | "--tray" | "--server") { continue; }
        return false;
    }
    !skip
}

/// 必须在 global_init、日志与 Config 首次读取前调用。
pub(crate) fn apply_pending_on_startup() -> ResultType<()> {
    let marker = paths()?.root.join(PENDING);
    if !marker.exists() || !main_launch() { return Ok(()); }
    if !crate::desktop_runtime::claim()? { return Ok(()); }
    hbb_common::sodiumoxide::init().map_err(|_| anyhow!("无法初始化恢复加密"))?;
    let id = String::from_utf8(read_limited(&marker)?)?;
    if Uuid::parse_str(&id).is_err() || Uuid::parse_str(&id)?.to_string() != id { bail!("恢复标记无效"); }
    let directory = paths()?.root.join(format!(".restore-{id}"));
    check_path(&directory)?;
    let secret = Zeroizing::new(staging_key(&id)?.get_password().map_err(|_| anyhow!("无法获取本机恢复密钥"))?);
    let bytes = Zeroizing::new(serde_json::from_str::<Vec<u8>>(&secret)?);
    let key = secretbox::Key::from_slice(&bytes).ok_or_else(|| anyhow!("恢复密钥无效"))?;
    let snapshot: Snapshot = serde_json::from_slice(&open_stage(&directory.join("snapshot.enc"), &key)?)?;
    validate(&snapshot)?;
    if !directory.join("committed").exists() {
        if directory.join("rollback.enc").exists() {
            let previous: Snapshot = serde_json::from_slice(&open_stage(&directory.join("rollback.enc"), &key)?)?;
            validate(&previous)?;
            rollback(&directory, &previous)?;
            *RESTORE_RESULT.lock().unwrap() = Some(RestoreResult { success: false, message: String::new(), error: "上次恢复中断，已回滚原配置，请重新导入备份".into(), original_backup_path: directory.to_string_lossy().into_owned() });
        } else if let Err(error) = apply(&directory, &snapshot, &key) {
            // apply 的回滚失败会保留事务；禁止带着部分配置继续启动。
            if directory.join("rollback.enc").exists() && !directory.join("rolled-back").exists() {
                return Err(error);
            }
            *RESTORE_RESULT.lock().unwrap() = Some(RestoreResult { success: false, message: String::new(), error: "恢复失败，已回滚原配置，请重新导入备份".into(), original_backup_path: directory.to_string_lossy().into_owned() });
        }
    }
    if directory.join("committed").exists() {
        *RESTORE_RESULT.lock().unwrap() = Some(RestoreResult { success: true, message: "配置已恢复，原配置副本已保留".into(), error: String::new(), original_backup_path: directory.join("previous-config").to_string_lossy().into_owned() });
    }
    fs::remove_file(&marker)?;
    // 保留私有事务目录及原配置供人工回滚；其中凭据始终加密。
    Ok(())
}

pub(crate) fn status() -> String {
    let mut status = STATUS.lock().unwrap();
    let status = status.get_or_insert_with(Status::default);
    status.restore_pending = paths().is_ok_and(|paths| paths.root.join(PENDING).exists());
    let mut value = serde_json::to_value(status).unwrap_or_default();
    if let Some(result) = RESTORE_RESULT.lock().unwrap().as_ref() {
        value["restore_result"] = serde_json::to_value(result).unwrap_or_default();
    }
    value.to_string()
}

fn cancel_restore() -> ResultType<()> {
    let marker = paths()?.root.join(PENDING);
    if !marker.exists() { return Ok(()); }
    let id = String::from_utf8(read_limited(&marker)?)?;
    let parsed = Uuid::parse_str(&id).map_err(|_| anyhow!("恢复标记无效"))?;
    if parsed.to_string() != id { bail!("恢复标记无效"); }
    let directory = paths()?.root.join(format!(".restore-{id}"));
    check_path(&directory)?;
    if directory.join("rollback.enc").exists() { bail!("恢复事务已经开始，不能取消"); }
    fs::remove_file(&marker)?;
    match staging_key(&id)?.delete_password() {
        Ok(()) | Err(keyring::Error::NoEntry) => {},
        Err(_) => bail!("待恢复已取消，但无法清除系统存储中的临时恢复密钥"),
    }
    if directory.exists() {
        let resolved = fs::canonicalize(&directory)?;
        if resolved.parent() != Some(paths()?.root.as_path()) { bail!("恢复暂存目录越界"); }
        fs::remove_dir_all(&resolved)?;
    }
    Ok(())
}

pub(crate) fn submit(command: String) {
    let command = Zeroizing::new(command);
    let request = serde_json::from_str::<Request>(&command);
    let mut state = STATUS.lock().unwrap();
    if state.as_ref().is_some_and(|status| status.busy) { return; }
    let request = match request {
        Ok(request) => request,
        Err(_) => { *state = Some(Status { error: "配置备份命令无效".into(), ..Default::default() }); return; }
    };
    *state = Some(Status { busy: true, request_id: request.request_id.clone(), ..Default::default() });
    drop(state);
    let request_id = request.request_id.clone();
    let worker = std::thread::Builder::new().name("configuration-backup".into()).spawn(move || {
        let result = (|| -> ResultType<()> {
            if !crate::desktop_runtime::is_host() { bail!("请在主应用中执行配置备份或恢复"); }
            if request.action != "cancel_restore" && request.password.len() < 8 { bail!("备份密码过短，请使用更长的密码"); }
            if request.password.len() > 1024 { bail!("备份密码过长，请缩短后重试"); }
            hbb_common::sodiumoxide::init().map_err(|_| anyhow!("无法初始化备份加密"))?;
            match request.action.as_str() {
                "export" => export(Path::new(&request.path), request.password.as_bytes()),
                "restore" => stage(Path::new(&request.path), request.password.as_bytes()),
                "cancel_restore" => cancel_restore(),
                _ => bail!("不支持的配置备份操作"),
            }
        })();
        let success = result.is_ok();
        let error = result.err().map(|error| error.to_string()).unwrap_or_default();
        *STATUS.lock().unwrap() = Some(Status { request_id: request.request_id.clone(), success, error, ..Default::default() });
    });
    if worker.is_err() { *STATUS.lock().unwrap() = Some(Status { request_id, error: "无法启动配置备份任务".into(), ..Default::default() }); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_backup_rejects_wrong_password_and_corruption() {
        hbb_common::sodiumoxide::init().unwrap();
        let original = b"private settings";
        let mut encrypted = encrypt(original, b"correct password").unwrap();
        assert_eq!(&**decrypt(&encrypted, b"correct password").unwrap(), original);
        assert!(decrypt(&encrypted, b"wrong password").is_err());
        let last = encrypted.len() - 1;
        encrypted[last] ^= 1;
        assert!(decrypt(&encrypted, b"correct password").is_err());
        assert!(decrypt(b"broken", b"correct password").is_err());
    }
    #[test]
    fn unsafe_archive_paths_are_rejected() {
        for path in ["../secret", "/absolute", "C:/file", "a\\b", "a/../b", "a//b", "NUL.toml", "name./a", "a "] { assert!(!safe_relative(path), "{path}"); }
        assert!(safe_relative("peers/a012345.toml"));
        assert!(excluded("AUTOSTART.XML"));
    }
    #[test]
    fn duplicate_configuration_paths_are_rejected() {
        let snapshot = Snapshot {
            version: 1, machine: machine().unwrap(), easytier: None, credentials: vec![],
            files: vec![Entry { path: "settings.toml".into(), data: vec![] },
                Entry { path: "SETTINGS.toml".into(), data: vec![] }],
        };
        assert!(validate(&snapshot).is_err());
    }
    #[test]
    fn interrupted_directory_swap_restores_original_even_if_empty_config_was_recreated() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config");
        let transaction = temp.path().join("transaction");
        fs::create_dir(&config).unwrap();
        fs::create_dir(&transaction).unwrap();
        fs::write(config.join("old.toml"), "old settings").unwrap();
        fs::rename(&config, transaction.join("previous-config")).unwrap();
        // 模拟崩溃后 app_paths.initialize 自动创建的空配置目录。
        fs::create_dir(&config).unwrap();
        restore_original_directory(&config, &transaction).unwrap();
        assert_eq!(fs::read_to_string(config.join("old.toml")).unwrap(), "old settings");
        restore_original_directory(&config, &transaction).unwrap();
        assert!(config.join("old.toml").exists());
    }
}
