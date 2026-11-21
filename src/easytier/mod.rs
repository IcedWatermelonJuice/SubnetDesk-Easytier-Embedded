mod engine;
mod profile;
mod source_policy;

use engine::{Components, Running, Snapshot};
use profile::{Patch, Profile, Store};
use serde::Deserialize;
use serde_json::json;
use std::{
    net::IpAddr,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
        Arc, Mutex, OnceLock, RwLock,
    },
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

type Result<T> = std::result::Result<T, &'static str>;

static CONTROLLER: OnceLock<Option<Controller>> = OnceLock::new();
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

const POLL_INTERVAL: Duration = Duration::from_millis(300);
const START_QUERY_INTERVAL: Duration = Duration::from_millis(750);
const SOURCE_QUERY_INTERVAL: Duration = Duration::from_secs(5);
const MAX_COMMAND: usize = 512 * 1024;

#[derive(Deserialize)]
struct Request {
    request_id: Option<String>,
    #[serde(flatten)]
    action: Action,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum Action {
    Save {
        id: Option<Uuid>,
        name: String,
        toml: Option<String>,
        fields: Option<Patch>,
    },
    Select { id: Uuid },
    Start { id: Option<Uuid> },
    SetRemoteOnly { enabled: bool },
    Stop,
    Refresh,
    Delete { id: Uuid },
    Duplicate { id: Uuid, name: Option<String> },
    Import { path: PathBuf, name: Option<String> },
    Export { path: PathBuf, id: Option<Uuid> },
}

#[derive(Clone, Copy, Default, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum State {
    #[default]
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

#[derive(Default)]
struct View {
    available: bool,
    store: Arc<Store>,
    running_id: Option<Uuid>,
    running_config_changed: bool,
    state: State,
    snapshot: Arc<Snapshot>,
    error: Option<&'static str>,
    completed_request_id: Option<String>,
    command_error: Option<&'static str>,
}

struct Controller {
    view: Arc<RwLock<View>>,
    sender: SyncSender<String>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}

impl Controller {
    fn new() -> Option<Self> {
        let (sender, receiver) = mpsc::sync_channel(16);
        let view = Arc::new(RwLock::new(View::default()));
        let target = Arc::clone(&view);
        let worker = thread::Builder::new()
            .name("easytier-controller".to_owned())
            .spawn(move || Worker::new(target).run(receiver))
            .ok()?;
        Some(Self {
            view,
            sender,
            worker: Mutex::new(Some(worker)),
        })
    }
}

fn controller() -> Option<&'static Controller> {
    if !crate::desktop_runtime::is_host() || SHUTTING_DOWN.load(Ordering::Acquire) {
        return None;
    }
    CONTROLLER.get_or_init(Controller::new).as_ref()
}

pub(crate) fn source_allowed(ip: IpAddr) -> bool {
    let Some(controller) = CONTROLLER.get().and_then(Option::as_ref) else {
        return false;
    };
    let view = controller.view.read().unwrap();
    matches!(view.state, State::Running)
        && source_policy::accepts(&view.snapshot.allowed_sources, view.snapshot.observed_at, ip)
}

pub(super) fn shutdown() {
    SHUTTING_DOWN.store(true, Ordering::Release);
    if let Some(controller) = CONTROLLER.get().and_then(Option::as_ref) {
        let worker = controller.worker.lock().unwrap().take();
        if let Some(worker) = worker {
            if worker.join().is_err() {
                hbb_common::log::warn!("EasyTier 后台线程异常退出");
            }
        }
    }
}

pub(super) fn get_option(key: &str) -> Option<String> {
    let wants_status = key == "easytier-status";
    let profile = key.strip_prefix("easytier-profile:");
    if !wants_status && profile.is_none() {
        return None;
    }
    let Some(controller) = controller() else {
        return Some(if wants_status {
            json!({
                "available": false, "profiles": [], "state": "failed",
                "error": "EasyTier 后台不可用。", "nodes": [], "routes": [],
            }).to_string()
        } else {
            "null".to_owned()
        });
    };
    let view = controller.view.read().unwrap();
    if let Some(id) = profile {
        let profile = Uuid::parse_str(id).ok().and_then(|id| view.store.get(id).ok());
        return Some(match profile {
            Some(profile) => json!({
                "id": profile.id, "name": profile.name, "toml": profile.toml,
                "fields": profile::fields(&profile.toml),
            }).to_string(),
            None => "null".to_owned(),
        });
    }
    let profiles: Vec<_> = view.store.profiles.iter()
        .map(|profile| json!({ "id": profile.id, "name": profile.name }))
        .collect();
    Some(json!({
        "available": view.available,
        "profiles": profiles,
        "selected_id": view.store.selected_id,
        "running_id": view.running_id,
        "running_config_changed": view.running_config_changed,
        "remote_only": crate::lan_server::easytier_only(),
        "state": view.state,
        "ipv4": view.snapshot.ipv4,
        "error": view.error,
        "nodes": view.snapshot.nodes,
        "routes": view.snapshot.routes,
        "queried_at": view.snapshot.queried_at,
        "completed_request_id": view.completed_request_id,
        "command_error": view.command_error,
    }).to_string())
}

pub(super) fn submit(value: String) {
    let Some(controller) = controller() else {
        return;
    };
    if value.len() > MAX_COMMAND {
        let mut view = controller.view.write().unwrap();
        let error = "EasyTier 操作内容过大。";
        view.error = Some(error);
        if let Some(id) = request_id(&value) {
            view.completed_request_id = Some(id);
            view.command_error = Some(error);
        }
        return;
    }
    if let Err(error) = controller.sender.try_send(value) {
        let message = match error {
            mpsc::TrySendError::Full(message) | mpsc::TrySendError::Disconnected(message) => message,
        };
        let mut view = controller.view.write().unwrap();
        let error = "EasyTier 正在处理其他操作，或后台不可用。";
        view.error = Some(error);
        if let Some(id) = request_id(&message) {
            view.completed_request_id = Some(id);
            view.command_error = Some(error);
        }
    }
}

fn request_id(message: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(message).ok()?;
    let id = value.get("request_id")?.as_str()?;
    valid_request_id(id).then(|| id.to_owned())
}

fn valid_request_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)
}

struct Worker {
    view: Arc<RwLock<View>>,
    path: Option<PathBuf>,
    store: Arc<Store>,
    running: Option<Running>,
    state: State,
    snapshot: Arc<Snapshot>,
    error: Option<&'static str>,
    next_query: Instant,
}

impl Worker {
    fn new(view: Arc<RwLock<View>>) -> Self {
        let (path, store, error) = match Store::open() {
            Ok((path, store)) => (Some(path), store, None),
            Err(error) => (None, Store::default(), Some(error)),
        };
        Self {
            view,
            path,
            store: Arc::new(store),
            running: None,
            state: State::Stopped,
            snapshot: Arc::new(Snapshot::default()),
            error,
            next_query: Instant::now(),
        }
    }

    fn publish(&self, completion: Option<(String, Option<&'static str>)>) {
        let available = Components::locate().is_ok();
        let mut view = self.view.write().unwrap();
        view.available = available;
        view.store = Arc::clone(&self.store);
        view.state = self.state;
        view.running_id = self.running.as_ref().map(|running| running.profile_id);
        view.running_config_changed = self.running.as_ref().is_some_and(|running| {
            running.hostname != crate::lan::device_display_name() || self.store.get(running.profile_id)
                .is_ok_and(|saved| saved.toml != running.profile_toml)
        });
        view.snapshot = Arc::clone(&self.snapshot);
        view.error = self.error;
        if let Some((id, error)) = completion {
            view.completed_request_id = Some(id);
            view.command_error = error;
        }
    }

    fn run(mut self, receiver: Receiver<String>) {
        self.publish(None);
        if crate::lan_server::easytier_only() {
            if let Err(error) = self.selected_or(None).and_then(|id| self.start(id)) {
                self.error = Some(error);
                self.state = State::Failed;
            }
            self.publish(None);
        }
        while !SHUTTING_DOWN.load(Ordering::Acquire) {
            self.poll();
            match receiver.recv_timeout(POLL_INTERVAL) {
                Ok(message) => {
                    if SHUTTING_DOWN.load(Ordering::Acquire) {
                        break;
                    }
                    self.handle_request(&message);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        if let Err(error) = self.stop() {
            self.error = Some(error);
        }
        self.publish(None);
    }

    fn handle_request(&mut self, message: &str) {
        match serde_json::from_str::<Request>(message) {
            Ok(request) => {
                let id = request.request_id.filter(|id| valid_request_id(id));
                let result = self.apply(request.action);
                self.error = result.err();
                self.publish(id.map(|id| (id, self.error)));
            }
            Err(_) => {
                self.error = Some("EasyTier 操作格式无效。");
                self.publish(request_id(message).map(|id| (id, self.error)));
            }
        }
    }

    fn update_store(&mut self, store: Store) -> Result<()> {
        let path = self.path.as_ref().ok_or("EasyTier 方案目录不可用。")?;
        store.save(path)?;
        self.store = Arc::new(store);
        Ok(())
    }

    fn apply(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Save { id, name, toml, fields } => {
                let current = id.map(|id| self.store.get(id)).transpose()?;
                let source = toml.or_else(|| current.map(|saved| saved.toml.clone()))
                    .unwrap_or_else(profile::default_toml);
                let toml = profile::patch(&source, fields)?;
                let components = Components::locate()?;
                components.verify_versions()?;
                components.validate(&toml)?;
                let mut store = (*self.store).clone();
                store.put(Profile {
                    id: id.unwrap_or_else(Uuid::new_v4),
                    name: profile::name(&name)?,
                    toml,
                })?;
                self.update_store(store)
            }
            Action::Select { id } => {
                self.store.get(id)?;
                let mut store = (*self.store).clone();
                store.selected_id = Some(id);
                self.update_store(store)
            }
            Action::Start { id } => {
                let id = self.selected_or(id)?;
                self.start(id)
            }
            Action::SetRemoteOnly { enabled } => {
                let target = if enabled && self.running.is_none() {
                    Some(self.selected_or(None)?)
                } else {
                    None
                };
                hbb_common::config::Config::set_option(
                    base::config::keys::OPTION_LAN_EASYTIER_ONLY.to_owned(),
                    if enabled { "Y" } else { "N" }.to_owned(),
                );
                // 启动失败时保留来源限制，不能无提示地恢复允许 LAN。
                if let Some(id) = target { self.start(id)?; }
                Ok(())
            }
            Action::Stop => {
                self.stop()?;
                hbb_common::config::Config::set_option(
                    base::config::keys::OPTION_LAN_EASYTIER_ONLY.to_owned(), "N".to_owned(),
                );
                Ok(())
            }
            Action::Refresh => self.refresh(),
            Action::Delete { id } => {
                self.store.get(id)?;
                if self.running.as_ref().is_some_and(|running| running.profile_id == id) {
                    return Err("请先停止使用此 EasyTier 方案的实例。");
                }
                let mut store = (*self.store).clone();
                store.profiles.retain(|profile| profile.id != id);
                store.normalize_selection();
                self.update_store(store)
            }
            Action::Duplicate { id, name } => {
                let source = self.store.get(id)?;
                let name = match name {
                    Some(name) => profile::name(&name)?,
                    None => format!("{} 副本", source.name.chars().take(76).collect::<String>()),
                };
                let mut store = (*self.store).clone();
                store.put(Profile {
                    id: Uuid::new_v4(),
                    name,
                    toml: source.toml.clone(),
                })?;
                self.update_store(store)
            }
            Action::Import { path, name } => {
                let toml = profile::import(&path)?;
                let toml = profile::patch(&toml, None)?;
                let components = Components::locate()?;
                components.verify_versions()?;
                components.validate(&toml)?;
                let name = name.or_else(|| path.file_stem().map(|name| name.to_string_lossy().into_owned()))
                    .unwrap_or_else(|| "导入的方案".to_owned());
                let mut store = (*self.store).clone();
                store.put(Profile {
                    id: Uuid::new_v4(),
                    name: profile::name(&name)?,
                    toml,
                })?;
                self.update_store(store)
            }
            Action::Export { path, id } => {
                let id = self.selected_or(id)?;
                profile::export(&path, &self.store.get(id)?.toml)
            }
        }
    }

    fn selected_or(&self, id: Option<Uuid>) -> Result<Uuid> {
        id.or(self.store.selected_id).ok_or("请先保存并选择 EasyTier 方案。")
    }

    fn start(&mut self, id: Uuid) -> Result<()> {
        let profile = self.store.get(id)?.clone();
        let hostname = crate::lan::device_display_name();
        if self.running.as_ref().is_some_and(|running| {
            running.profile_id == id && running.profile_toml == profile.toml && running.hostname == hostname
        }) {
            return Ok(());
        }
        let components = Components::locate()?;
        components.verify_versions()?;
        let instance_id = Uuid::new_v4();
        let config = profile::launch_toml(&profile.toml, instance_id, &hostname)?;
        // 目标方案校验成功后再停止当前实例。
        components.validate(&config)?;
        if SHUTTING_DOWN.load(Ordering::Acquire) {
            return Err("应用正在退出，已取消启动 EasyTier。");
        }

        self.stop()?;
        match Running::start(components, id, profile.toml, instance_id, config, hostname) {
            Ok(running) => {
                self.running = Some(running);
                self.state = State::Starting;
                self.next_query = Instant::now();
                Ok(())
            }
            Err(error) => {
                self.state = State::Failed;
                Err(error)
            }
        }
    }

    fn stop(&mut self) -> Result<()> {
        if self.running.is_some() {
            self.state = State::Stopping;
            self.snapshot = Arc::new(Snapshot::default());
            self.publish(None);
            let result = self.running.as_mut().map_or(Ok(()), Running::stop);
            if let Err(error) = result {
                self.state = State::Failed;
                return Err(error);
            }
        }
        self.running = None;
        self.snapshot = Arc::new(Snapshot::default());
        self.state = State::Stopped;
        Ok(())
    }

    fn refresh(&mut self) -> Result<()> {
        self.next_query = Instant::now() + SOURCE_QUERY_INTERVAL;
        let Some(running) = self.running.as_mut() else {
            return Ok(());
        };
        match running.refresh() {
            Ok(snapshot) => {
                self.state = State::Running;
                self.snapshot = Arc::new(snapshot);
                Ok(())
            }
            Err(error) => {
                self.snapshot = Arc::new(Snapshot::default());
                if matches!(self.state, State::Starting) && !running.starting_timed_out() {
                    self.next_query = Instant::now() + START_QUERY_INTERVAL;
                }
                Err(error)
            }
        }
    }

    fn fail_instance(&mut self, error: &'static str) {
        self.error = Some(self.stop().err().unwrap_or(error));
        self.state = State::Failed;
        self.publish(None);
    }

    fn poll(&mut self) {
        let Some(running) = self.running.as_mut() else {
            return;
        };
        if let Err(error) = running.ensure_running() {
            self.fail_instance(error);
            return;
        }
        let needs_sources = hbb_common::config::Config::get_option(
            base::config::keys::OPTION_LAN_EASYTIER_ONLY,
        ) == "Y";
        if (matches!(self.state, State::Starting) || needs_sources)
            && Instant::now() >= self.next_query
        {
            match self.refresh() {
                Ok(()) => {
                    self.error = None;
                    self.publish(None);
                }
                Err(_) if self.running.as_ref().is_some_and(Running::starting_timed_out) => {
                    self.fail_instance("EasyTier 启动超时。请检查组件、配置及虚拟网卡权限。");
                }
                Err(error) => {
                    self.error = Some(error);
                    self.publish(None);
                }
            }
        }
    }
}
