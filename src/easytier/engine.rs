use super::Result;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    env,
    ffi::OsString,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpListener},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

const ENGINE_VERSION: &str = "2.6.4-8428a89d";
const QUERY_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_OUTPUT: usize = 2 * 1024 * 1024;
const MAX_ROWS: usize = 1024;
const ENGINE_FAILURE: &str = if cfg!(windows) {
    "EasyTier 引擎已退出。Windows 建立虚拟网卡通常需要管理员权限。请检查引擎组件和网络配置。"
} else {
    "EasyTier 引擎已退出。请检查虚拟网卡权限、引擎组件和网络配置。"
};

pub(super) struct Components {
    core: PathBuf,
    cli: PathBuf,
}

#[derive(Default)]
pub(super) struct Snapshot {
    pub ipv4: Option<String>,
    pub nodes: Vec<Node>,
    pub routes: Vec<Route>,
    pub queried_at: Option<i64>,
}

#[derive(Serialize)]
pub(super) struct Node {
    name: String,
    address: String,
    latency: Option<f64>,
    protocol: Option<String>,
}

#[derive(Serialize)]
pub(super) struct Route {
    destination: String,
    next_hop: String,
    hops: Option<i32>,
    latency: Option<i32>,
}

#[derive(Deserialize)]
struct PeerRow {
    hostname: String,
    ipv4: String,
    cidr: String,
    cost: String,
    lat_ms: String,
    tunnel_proto: String,
}

#[derive(Deserialize)]
struct RouteRow {
    ipv4: String,
    proxy_cidrs: String,
    next_hop_ipv4: String,
    next_hop_hostname: String,
    path_len: i32,
    path_latency: i32,
    next_hop_ipv4_lat_first: String,
    next_hop_hostname_lat_first: String,
    path_len_lat_first: i32,
    path_latency_lat_first: i32,
}

struct ManagedChild(Child);

impl Drop for ManagedChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

pub(super) struct Running {
    child: ManagedChild,
    pub profile_id: Uuid,
    pub profile_toml: String,
    started: Instant,
    addr: SocketAddrV4,
    instance_id: Uuid,
    cli: PathBuf,
    latency_first: bool,
}

fn command(path: &Path) -> Command {
    let mut cmd = Command::new(path);
    if let Some(dir) = path.parent() {
        cmd.current_dir(dir);
    }
    // EasyTier CLI 允许用环境变量覆盖选项，包括 RPC 地址和日志级别。
    for (name, _) in env::vars_os() {
        let upper = name.to_string_lossy().to_ascii_uppercase();
        if upper.starts_with("ET_") || matches!(upper.as_str(), "RUST_LOG" | "RUST_BACKTRACE") {
            cmd.env_remove(name);
        }
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(winapi::um::winbase::CREATE_NO_WINDOW);
    }
    cmd
}

fn spawn(cmd: &mut Command) -> Result<ManagedChild> {
    cmd.spawn().map(ManagedChild).map_err(|error| {
        if error.kind() == std::io::ErrorKind::PermissionDenied
            || (cfg!(windows) && error.raw_os_error() == Some(740))
        {
            "当前权限不足，无法启动 EasyTier 组件。Windows 建立虚拟网卡通常需要管理员权限。"
        } else {
            "无法启动 EasyTier 组件。请检查随应用分发的程序和依赖文件。"
        }
    })
}

fn write_config(child: &mut ManagedChild, toml: String) -> Result<thread::JoinHandle<bool>> {
    let mut stdin = child.0.stdin.take().ok_or("无法连接 EasyTier 的配置输入。")?;
    thread::Builder::new()
        .name("easytier-input".to_owned())
        .spawn(move || stdin.write_all(toml.as_bytes()).is_ok())
        .map_err(|_| "无法写入 EasyTier 配置。")
}

fn run_command(cmd: &mut Command, input: Option<String>, capture: bool) -> Result<Vec<u8>> {
    if input.is_some() {
        cmd.stdin(Stdio::piped());
    }
    if capture {
        cmd.stdout(Stdio::piped());
    }
    let mut child = spawn(cmd)?;
    let writer = input.map(|config| write_config(&mut child, config)).transpose()?;
    let reader = child
        .0
        .stdout
        .take()
        .map(|stdout| {
            thread::Builder::new()
                .name("easytier-query".to_owned())
                .spawn(move || {
                    let mut bytes = Vec::new();
                    stdout.take((MAX_OUTPUT + 1) as u64).read_to_end(&mut bytes)?;
                    Ok::<_, std::io::Error>(bytes)
                })
                .map_err(|_| "无法读取 EasyTier 查询结果。")
        })
        .transpose()?;
    let deadline = Instant::now() + QUERY_TIMEOUT;
    loop {
        if let Some(status) = child.0.try_wait().map_err(|_| "无法读取 EasyTier 进程状态。")? {
            if !status.success() {
                return Err("EasyTier 命令失败。");
            }
            break;
        }
        if Instant::now() >= deadline || super::SHUTTING_DOWN.load(Ordering::Acquire) {
            return Err("EasyTier 命令已超时或取消。");
        }
        thread::sleep(Duration::from_millis(40));
    }
    if writer.is_some_and(|worker| !matches!(worker.join(), Ok(true))) {
        return Err("无法完整写入 EasyTier 配置。");
    }
    if let Some(worker) = reader {
        let bytes = worker
            .join()
            .ok()
            .and_then(std::result::Result::ok)
            .filter(|bytes| bytes.len() <= MAX_OUTPUT)
            .ok_or("EasyTier 查询结果无效。")?;
        return Ok(bytes);
    }
    Ok(Vec::new())
}

impl Components {
    pub fn locate() -> Result<Self> {
        let exe = env::current_exe().map_err(|_| "找不到 SubnetDesk 应用程序目录。")?;
        let dir = exe.parent().ok_or("找不到 SubnetDesk 应用程序目录。")?.join("easytier");
        let binary = |name| dir.join(format!("{name}{}", env::consts::EXE_SUFFIX));
        let core = binary("easytier-core");
        let cli = binary("easytier-cli");
        if core.is_file() && cli.is_file() {
            Ok(Self { core, cli })
        } else {
            Err("当前应用包缺少 EasyTier core 或 CLI 组件。")
        }
    }

    pub fn validate(&self, config: &str) -> Result<()> {
        let mut cmd = command(&self.core);
        cmd.args(["--disable-env-parsing", "--check-config", "--config-file", "-"]);
        if run_command(&mut cmd, Some(config.to_owned()), false).is_err() {
            return Err("EasyTier v2.6.4 无法校验该 TOML。请检查配置、引擎组件和运行权限。");
        }
        Ok(())
    }

    pub fn verify_versions(&self) -> Result<()> {
        for (path, name) in [(&self.core, "easytier-core"), (&self.cli, "easytier-cli")] {
            let output = run_command(command(path).arg("--version"), None, true)?;
            let expected = format!("{name} {ENGINE_VERSION}");
            let matches = String::from_utf8(output).ok().is_some_and(|output| {
                let version = output.trim();
                version == expected || (name == "easytier-core" && version.strip_suffix('~') == Some(expected.as_str()))
            });
            if !matches {
                return Err("EasyTier core 和 CLI 必须同时使用随应用提供的 v2.6.4 版本。");
            }
        }
        Ok(())
    }
}

impl Running {
    pub fn start(
        components: Components,
        profile_id: Uuid,
        profile_toml: String,
        instance_id: Uuid,
        config: String,
    ) -> Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .map_err(|_| "无法选择 EasyTier 本机管理端口。")?;
        let port = listener.local_addr().map_err(|_| "无法读取 EasyTier 本机管理端口。")?.port();
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
        let mut cmd = command(&components.core);
        cmd.args([
            "--config-file",
            "-",
            "--disable-env-parsing",
            "--rpc-portal",
            &addr.to_string(),
            "--rpc-portal-whitelist",
            "127.0.0.1/32",
            "--console-log-level",
            "off",
            "--file-log-level",
            "off",
        ])
        .stdin(Stdio::piped());
        let latency_first = config
            .parse::<toml_edit::Document>()
            .ok()
            .and_then(|doc| doc.as_table().get("flags")?.as_table_like()?.get("latency_first")?.as_bool())
            .unwrap_or_default();

        // core 自行 bind。端口释放与 bind 间有竞争窗口，查询严格限定本次 UUID。
        drop(listener);
        let mut child = spawn(&mut cmd)?;
        write_config(&mut child, config)?;
        Ok(Self {
            child,
            profile_id,
            profile_toml,
            started: Instant::now(),
            addr,
            instance_id,
            cli: components.cli,
            latency_first,
        })
    }

    pub fn ensure_running(&mut self) -> Result<()> {
        match self.child.0.try_wait() {
            Ok(None) => Ok(()),
            Ok(Some(_)) => Err(ENGINE_FAILURE),
            Err(_) => Err("无法读取 EasyTier 引擎状态。"),
        }
    }

    pub fn starting_timed_out(&self) -> bool {
        self.started.elapsed() >= Duration::from_secs(30)
    }

    fn query<T: DeserializeOwned>(&mut self, subcommand: &str) -> Result<T> {
        self.ensure_running()?;
        let output = run_command(
            command(&self.cli).args([
                OsString::from("--rpc-portal"),
                self.addr.to_string().into(),
                OsString::from("--instance-id"),
                self.instance_id.to_string().into(),
                OsString::from("--output"),
                OsString::from("json"),
                OsString::from(subcommand),
                OsString::from("list"),
            ]),
            None,
            true,
        )
        .map_err(|_| "当前受管 EasyTier 实例尚未响应查询。")?;
        self.ensure_running()?;
        serde_json::from_slice(&output).map_err(|_| "EasyTier 返回了不兼容的查询结果。")
    }

    pub fn refresh(&mut self) -> Result<Snapshot> {
        let peers: Vec<PeerRow> = self.query("peer")?;
        let local = peers
            .iter()
            .find(|peer| peer.cost == "Local")
            .ok_or("EasyTier 尚未返回本机节点。")?;
        let ipv4 = ipv4_address(&local.ipv4).map(|_| {
            if local.cidr.is_empty() {
                local.ipv4.clone()
            } else {
                local.cidr.clone()
            }
        });
        let routes: Vec<RouteRow> = self.query("route")?;
        Ok(Snapshot {
            ipv4,
            queried_at: Some(chrono::Utc::now().timestamp_millis()),
            nodes: peers
                .into_iter()
                .filter(|peer| peer.cost != "Local")
                .take(MAX_ROWS)
                .map(|peer| Node {
                    name: safe_text(&peer.hostname),
                    address: ipv4_address(&peer.ipv4).unwrap_or_default(),
                    latency: peer.lat_ms.parse::<f64>().ok().filter(|latency| latency.is_finite()),
                    protocol: if peer.tunnel_proto.is_empty() || peer.tunnel_proto == "-" {
                        None
                    } else {
                        Some(safe_text(&peer.tunnel_proto))
                    },
                })
                .collect(),
            routes: routes
                .into_iter()
                .flat_map(|route| route.rows(self.latency_first))
                .take(MAX_ROWS)
                .collect(),
        })
    }

    pub fn stop(&mut self) -> Result<()> {
        if self.child.0.try_wait().map_err(|_| "无法读取 EasyTier 引擎状态。")?.is_some() {
            return Ok(());
        }
        #[cfg(unix)]
        {
            let pid = self.child.0.id() as hbb_common::libc::pid_t;
            // 前台 core 在 Unix 注册 SIGTERM 并在退出前释放实例。
            if unsafe { hbb_common::libc::kill(pid, hbb_common::libc::SIGTERM) } == 0 {
                let deadline = Instant::now() + Duration::from_secs(2);
                while Instant::now() < deadline {
                    if matches!(self.child.0.try_wait(), Ok(Some(_))) {
                        return Ok(());
                    }
                    thread::sleep(Duration::from_millis(40));
                }
            }
        }
        // Windows 官方 CLI 无前台实例停止子命令。只回收持有句柄的子进程。
        if self.child.0.kill().is_err() && !matches!(self.child.0.try_wait(), Ok(Some(_))) {
            return Err("无法停止本应用启动的 EasyTier 引擎。");
        }
        self.child.0.wait().map_err(|_| "无法确认 EasyTier 引擎已停止。")?;
        Ok(())
    }
}

fn ipv4_address(value: &str) -> Option<String> {
    value.parse::<Ipv4Addr>().ok().filter(|addr| !addr.is_unspecified()).map(|addr| addr.to_string())
}

fn safe_text(value: &str) -> String {
    value.chars().filter(|character| !character.is_control()).take(128).collect()
}

impl RouteRow {
    fn rows(&self, latency_first: bool) -> impl Iterator<Item = Route> + '_ {
        let (ip, hostname, hops, latency) = if latency_first && self.path_len_lat_first > 0 {
            (&self.next_hop_ipv4_lat_first, &self.next_hop_hostname_lat_first, self.path_len_lat_first, self.path_latency_lat_first)
        } else {
            (&self.next_hop_ipv4, &self.next_hop_hostname, self.path_len, self.path_latency)
        };
        let next_hop = safe_text(if ip.is_empty() || ip == "-" { hostname } else { ip });
        std::iter::once(self.ipv4.as_str())
            .chain(self.proxy_cidrs.split(',').map(str::trim))
            .filter(|destination| !destination.is_empty())
            .map(move |destination| Route {
                destination: safe_text(destination),
                next_hop: next_hop.clone(),
                hops: Some(hops),
                latency: Some(latency),
            })
    }
}
