//! Create/test/deploy/test workflow for `guide-history-service` instances,
//! driven from Settings — the one-click "Install on This PC" in
//! Programming History, and the "Deploy & Manage Instances" section.
//! Local installs work on Linux (systemd user unit) and Windows (a boot-time
//! SYSTEM scheduled task); remote installs are Linux over SSH only, since
//! SSH servers on Windows are too rarely set up to build on.
//!
//! Design choices, stated once here rather than re-justified at each call
//! site:
//! - **The bundled binary, not a build.** Release packages ship
//!   `guide-history-service` beside the app (see `bundled_service_binary`),
//!   so installing needs no source checkout or Rust toolchain. A remote host
//!   gets the same binary, so one with a different CPU architecture is
//!   detected and refused with a clear message, never silently attempted.
//! - **Windows runs it as SYSTEM at boot**, so it records while nobody is
//!   signed in. That needs one UAC prompt: the elevated step copies the exe
//!   under Program Files (admin-only, since SYSTEM runs it), registers the
//!   task, and opens the port to private networks in Windows Firewall —
//!   other PCs can't reach it otherwise, and a SYSTEM process never gets
//!   the "allow access" prompt.
//! - **SSH via the system's own `ssh`/`scp`** (`tokio::process::Command`),
//!   not an embedded SSH client — reuses whatever the user already has
//!   configured (`~/.ssh/config`, known_hosts, agent) and this app never
//!   implements host-key verification itself. `-o BatchMode=yes` on every
//!   invocation is load-bearing, not decorative: it makes `ssh` fail fast
//!   with a clear error instead of hanging on an interactive password
//!   prompt this app will never answer (password auth isn't supported —
//!   key-based only, via `ssh-agent` or an explicit key path). Host-key
//!   checking is never overridden (`StrictHostKeyChecking` stays at
//!   whatever the user's own `ssh` config already says) — an untrusted
//!   host fails closed with instructions to `ssh` in by hand once first,
//!   not a silent MITM-downgrading bypass.
//! - **`systemctl --user` + `loginctl enable-linger`**, not a system-level
//!   unit — no `sudo` needed on the target host, just that user's own SSH
//!   access. Linger is what keeps the service running after the deploying
//!   SSH session disconnects (a systemd user manager instance normally
//!   stops once that user's last session ends); if enabling it fails (no
//!   polkit rule, a locked-down box), the service is still enabled and
//!   started, but the result carries a `warning` rather than silently
//!   leaving a landmine that stops working the next time everyone logs out.
//!
//! Mirrors `downloads.rs`'s proven shape for a long-running background job
//! with live progress: an `Arc<Inner>` holding a `Mutex<HashMap<id,
//! DeployStatus>>`, updated in place by a spawned task and polled per-frame
//! by the UI — no `Msg` channel involved, since nothing here needs to
//! survive past this session (a deploy in flight when the app closes is
//! simply abandoned, same as a download would be).
//!
//! No secret ever enters `settings.json`: `DeployTarget` stores only a
//! host/port/username/key *path*, never key contents or a password.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::state::settings::{DeployKind, DeployTarget};

#[derive(Clone, Debug)]
pub enum DeployStatus {
    TestingConnection,
    ConnectionOk {
        remote_arch: String,
    },
    ConnectionFailed(String),
    Transferring,
    Installing,
    HealthChecking,
    Running {
        programs_cached: u64,
        /// Set when everything succeeded except `loginctl enable-linger` —
        /// the service is running right now but may stop the next time the
        /// target's user fully logs out. Carries the exact remediation
        /// command to run there.
        warning: Option<String>,
    },
    Failed(String),
    Removing,
    Removed,
}

struct Inner {
    states: Mutex<HashMap<String, DeployStatus>>,
    runtime: tokio::runtime::Handle,
    ctx: egui::Context,
}

impl Inner {
    fn set(&self, id: &str, status: DeployStatus) {
        self.states.lock().unwrap().insert(id.to_string(), status);
        self.ctx.request_repaint();
    }
}

#[derive(Clone)]
pub struct Deploys {
    inner: Arc<Inner>,
}

impl Deploys {
    pub fn new(runtime: tokio::runtime::Handle, ctx: egui::Context) -> Self {
        Self {
            inner: Arc::new(Inner {
                states: Mutex::new(HashMap::new()),
                runtime,
                ctx,
            }),
        }
    }

    /// Polled every frame by the Settings UI — no async round trip.
    pub fn status(&self, id: &str) -> Option<DeployStatus> {
        self.inner.states.lock().unwrap().get(id).cloned()
    }

    pub fn test_connection(&self, id: String, target: DeployTarget) {
        let inner = self.inner.clone();
        inner.set(&id, DeployStatus::TestingConnection);
        self.inner.runtime.spawn(async move {
            let result = match target.kind {
                DeployKind::Local => check_local().await,
                DeployKind::Remote => check_remote(&target).await,
            };
            let status = match result {
                Ok(remote_arch) => DeployStatus::ConnectionOk { remote_arch },
                Err(e) => DeployStatus::ConnectionFailed(e),
            };
            inner.set(&id, status);
        });
    }

    pub fn deploy(&self, id: String, target: DeployTarget) {
        let inner = self.inner.clone();
        inner.set(&id, DeployStatus::TestingConnection);
        self.inner.runtime.spawn(async move {
            let result = run_deploy(&inner, &id, &target).await;
            let status = match result {
                Ok((programs_cached, warning)) => DeployStatus::Running {
                    programs_cached,
                    warning,
                },
                Err(e) => DeployStatus::Failed(e),
            };
            inner.set(&id, status);
        });
    }

    /// The second "test" in create/test/deploy/test — re-checks a
    /// previously-deployed instance without redeploying anything.
    pub fn check_status(&self, id: String, target: DeployTarget) {
        let inner = self.inner.clone();
        inner.set(&id, DeployStatus::HealthChecking);
        self.inner.runtime.spawn(async move {
            let host = http_host(&target);
            let status = match health_check(&host, target.listen_port).await {
                Ok(programs_cached) => DeployStatus::Running {
                    programs_cached,
                    warning: None,
                },
                Err(e) => DeployStatus::Failed(e),
            };
            inner.set(&id, status);
        });
    }

    pub fn remove(&self, id: String, target: DeployTarget) {
        let inner = self.inner.clone();
        inner.set(&id, DeployStatus::Removing);
        self.inner.runtime.spawn(async move {
            let status = match run_remove(&target).await {
                Ok(()) => DeployStatus::Removed,
                Err(e) => DeployStatus::Failed(e),
            };
            inner.set(&id, status);
        });
    }
}

fn http_host(target: &DeployTarget) -> String {
    match target.kind {
        DeployKind::Local => "127.0.0.1".to_string(),
        DeployKind::Remote => target.ssh_host.clone(),
    }
}

async fn run_deploy(
    inner: &Inner,
    id: &str,
    target: &DeployTarget,
) -> Result<(u64, Option<String>), String> {
    match target.kind {
        DeployKind::Local => {
            check_local().await?;
        }
        DeployKind::Remote => {
            check_remote(target).await?;
        }
    }

    #[cfg(windows)]
    if target.kind == DeployKind::Local {
        inner.set(id, DeployStatus::Installing);
        windows_local::install(target).await?;
        inner.set(id, DeployStatus::HealthChecking);
        let programs_cached = health_check(&http_host(target), target.listen_port).await?;
        return Ok((programs_cached, None));
    }

    inner.set(id, DeployStatus::Transferring);
    let (exec_path, data_path) = match target.kind {
        DeployKind::Local => install_local_binary().await?,
        DeployKind::Remote => install_remote_binary(target).await?,
    };

    inner.set(id, DeployStatus::Installing);
    let unit = render_unit(target, &exec_path, &data_path);
    let warning = match target.kind {
        DeployKind::Local => {
            install_local_unit(&unit)?;
            start_local_service().await?;
            enable_local_linger().await.err()
        }
        DeployKind::Remote => {
            install_remote_unit(target, &unit).await?;
            start_remote_service(target).await?;
            enable_remote_linger(target).await.err()
        }
    };

    inner.set(id, DeployStatus::HealthChecking);
    let programs_cached = health_check(&http_host(target), target.listen_port).await?;

    Ok((programs_cached, warning))
}

async fn run_remove(target: &DeployTarget) -> Result<(), String> {
    match target.kind {
        #[cfg(windows)]
        DeployKind::Local => windows_local::uninstall().await,
        #[cfg(not(windows))]
        DeployKind::Local => {
            // Best-effort — an already-stopped/never-started unit
            // shouldn't block cleaning up the rest.
            let _ = systemctl_user(&["disable", "--now", "guide-history-service.service"]).await;
            if let Ok(path) = local_unit_path() {
                let _ = std::fs::remove_file(&path);
            }
            if let Some(install_dir) =
                crate::paths::data_dir().map(|d| d.join("guide-history-service"))
            {
                let _ = std::fs::remove_dir_all(&install_dir);
            }
            systemctl_user(&["daemon-reload"]).await
        }
        DeployKind::Remote => {
            let install_dir = remote_install_dir(target);
            let cmd = format!(
                "systemctl --user disable --now guide-history-service.service; \
                 rm -f ~/.config/systemd/user/guide-history-service.service; \
                 rm -rf {install_dir}; \
                 systemctl --user daemon-reload"
            );
            ssh_output(target, &cmd).await.map(|_| ())
        }
    }
}

fn remote_install_dir(target: &DeployTarget) -> String {
    let configured = target.remote_install_dir.trim();
    if configured.is_empty() {
        "~/.dvrdesk-guide-history-service".to_string()
    } else {
        configured.to_string()
    }
}

// --- Connection checks ------------------------------------------------

async fn check_local() -> Result<String, String> {
    bundled_service_binary()?;
    #[cfg(unix)]
    for bin in ["systemctl"] {
        let ok = tokio::process::Command::new("which")
            .arg(bin)
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            return Err(format!(
                "`{bin}` not found on PATH — required for local deploy."
            ));
        }
    }
    Ok(std::env::consts::ARCH.to_string())
}

async fn check_remote(target: &DeployTarget) -> Result<String, String> {
    let out = ssh_output(target, "uname -m").await?;
    let remote_arch = out.trim().to_string();
    let local_arch = std::env::consts::ARCH;
    if remote_arch != local_arch {
        return Err(format!(
            "Remote architecture ({remote_arch}) doesn't match this machine's build \
             ({local_arch}) — same-architecture deploy isn't supported yet."
        ));
    }
    Ok(remote_arch)
}

// --- Bundled binary -----------------------------------------------------

#[cfg(windows)]
const SERVICE_EXE: &str = "guide-history-service.exe";
#[cfg(not(windows))]
const SERVICE_EXE: &str = "guide-history-service";

/// The `guide-history-service` binary shipped with this app: beside the
/// executable (the Windows zip, or an unpacked build), under
/// `/usr/lib/dvrdesk-native/` (the .deb), or — in a debug build only — the
/// sibling source tree's own build output.
fn bundled_service_binary() -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        candidates.push(dir.join(SERVICE_EXE));
    }
    #[cfg(target_os = "linux")]
    candidates.push(PathBuf::from("/usr/lib/dvrdesk-native").join(SERVICE_EXE));
    #[cfg(debug_assertions)]
    {
        let src = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../guide-history-service/target"));
        candidates.push(src.join("release").join(SERVICE_EXE));
        candidates.push(src.join("debug").join(SERVICE_EXE));
    }
    candidates.iter().find(|p| p.is_file()).cloned().ok_or_else(|| {
        format!(
            "The guide history service isn't included with this copy of DVRDesk (looked in {}). \
             Reinstall DVRDesk Native from its release download.",
            candidates.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
        )
    })
}

// --- Transfer -------------------------------------------------------------

async fn install_local_binary() -> Result<(String, String), String> {
    let src = bundled_service_binary()?;
    let install_dir = crate::paths::data_dir()
        .ok_or_else(|| "no data directory available".to_string())?
        .join("guide-history-service");
    let bin_dir = install_dir.join("bin");
    tokio::fs::create_dir_all(&bin_dir)
        .await
        .map_err(|e| e.to_string())?;
    let dest = bin_dir.join("guide-history-service");
    tokio::fs::copy(&src, &dest)
        .await
        .map_err(|e| format!("Failed to copy binary: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(&dest)
            .await
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(0o755);
        tokio::fs::set_permissions(&dest, perms)
            .await
            .map_err(|e| e.to_string())?;
    }
    let data_path = install_dir.join("guide-history.json");
    Ok((
        dest.to_string_lossy().to_string(),
        data_path.to_string_lossy().to_string(),
    ))
}

async fn install_remote_binary(target: &DeployTarget) -> Result<(String, String), String> {
    let src = bundled_service_binary()?;
    let install_dir = remote_install_dir(target);
    // Left unquoted (including any leading `~`) deliberately, so the
    // remote shell still expands `~` — this means paths with spaces or
    // quotes aren't supported in v1, the same practical limitation as many
    // CLI tools; the default value never hits it.
    ssh_output(
        target,
        &format!("mkdir -p {install_dir}/bin ~/.config/systemd/user"),
    )
    .await?;
    let remote_bin = format!("{install_dir}/bin/guide-history-service");
    scp_upload(target, &src, &remote_bin).await?;
    ssh_output(target, &format!("chmod +x {remote_bin}")).await?;
    let data_path = format!("{install_dir}/guide-history.json");

    // The commands above all run through the remote *shell* (or, for scp,
    // its own home-relative path handling), which is what expands a
    // leading `~` — but the two paths returned here get embedded straight
    // into the systemd unit's `ExecStart=`/`Environment=` lines by
    // `render_unit`, and systemd parses those itself, with no shell in the
    // loop at all. A literal `~` there isn't expanded — `ExecStart=` in
    // particular requires an absolute path — so it fails at
    // `systemctl --user enable`/`start` with a "bad unit file setting"
    // error despite every step up to this point having succeeded. Resolve
    // the remote `$HOME` once and expand it here so only the unit-file
    // paths are affected.
    let home = resolve_remote_home(target).await?;
    Ok((
        expand_tilde(&remote_bin, &home),
        expand_tilde(&data_path, &home),
    ))
}

async fn resolve_remote_home(target: &DeployTarget) -> Result<String, String> {
    let out = ssh_output(target, "echo $HOME").await?;
    let home = out.trim().to_string();
    if home.is_empty() || !home.starts_with('/') {
        return Err(format!(
            "Could not resolve a usable $HOME on {} (got {home:?})",
            target.ssh_host
        ));
    }
    Ok(home)
}

/// `~/foo` or bare `~` → an absolute path under `home`. Anything else
/// (including an already-absolute path) passes through unchanged.
fn expand_tilde(path: &str, home: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        format!("{home}/{rest}")
    } else if path == "~" {
        home.to_string()
    } else {
        path.to_string()
    }
}

// --- Install / start ------------------------------------------------------

fn render_unit(target: &DeployTarget, exec_path: &str, data_path: &str) -> String {
    format!(
        "[Unit]\n\
         Description=DVRDesk Guide History Service\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         ExecStart={exec_path}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         Environment=GHS_CHANNELS_DVR_URL={url}\n\
         Environment=GHS_POLL_SECS={poll}\n\
         Environment=GHS_RETENTION_SECS={retention}\n\
         Environment=GHS_FETCH_WINDOW_SECS={window}\n\
         Environment=GHS_LISTEN_ADDR=0.0.0.0:{port}\n\
         Environment=GHS_DATA_PATH={data_path}\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        url = target.dvr_server_url,
        poll = target.poll_secs,
        retention = target.retention_secs,
        window = target.fetch_window_secs,
        port = target.listen_port,
    )
}

fn local_unit_path() -> Result<PathBuf, String> {
    let base =
        directories::BaseDirs::new().ok_or_else(|| "no home directory available".to_string())?;
    Ok(base
        .config_dir()
        .join("systemd/user/guide-history-service.service"))
}

fn install_local_unit(unit: &str) -> Result<(), String> {
    let path = local_unit_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, unit).map_err(|e| format!("Failed to write unit file: {e}"))
}

async fn install_remote_unit(target: &DeployTarget, unit: &str) -> Result<(), String> {
    ssh_write_stdin(
        target,
        "cat > ~/.config/systemd/user/guide-history-service.service",
        unit.as_bytes(),
    )
    .await
}

async fn systemctl_user(args: &[&str]) -> Result<(), String> {
    let output = tokio::process::Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .await
        .map_err(|e| format!("Failed to run systemctl: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "systemctl {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

async fn start_local_service() -> Result<(), String> {
    systemctl_user(&["daemon-reload"]).await?;
    systemctl_user(&["enable", "--now", "guide-history-service.service"]).await
}

async fn start_remote_service(target: &DeployTarget) -> Result<(), String> {
    ssh_output(
        target,
        "systemctl --user daemon-reload && systemctl --user enable --now guide-history-service.service",
    )
    .await
    .map(|_| ())
}

async fn enable_local_linger() -> Result<(), String> {
    let user =
        std::env::var("USER").map_err(|_| "USER environment variable not set".to_string())?;
    let output = tokio::process::Command::new("loginctl")
        .args(["enable-linger", &user])
        .output()
        .await
        .map_err(|e| format!("Failed to run loginctl: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "loginctl enable-linger failed — the service will stop once you fully log out. \
             Run `sudo loginctl enable-linger {user}` to fix: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

async fn enable_remote_linger(target: &DeployTarget) -> Result<(), String> {
    match ssh_output(target, "loginctl enable-linger \"$(whoami)\"").await {
        Ok(_) => Ok(()),
        Err(e) => Err(format!(
            "loginctl enable-linger failed on {} — the service will stop once you fully log out \
             there. Run `sudo loginctl enable-linger {}` on that host to fix: {e}",
            target.ssh_host, target.ssh_user
        )),
    }
}

// --- Health check -----------------------------------------------------

async fn health_check(host: &str, port: u16) -> Result<u64, String> {
    let client = reqwest::Client::new();
    let url = format!("http://{host}:{port}/health");
    let mut last_err = String::new();
    for _ in 0..6 {
        match client
            .get(&url)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
                let count = body
                    .get("programs_cached")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                return Ok(count);
            }
            Ok(resp) => last_err = format!("HTTP {}", resp.status()),
            Err(e) => last_err = e.to_string(),
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    Err(format!(
        "Service didn't respond at {url} after starting: {last_err}"
    ))
}

// --- SSH/SCP helpers ------------------------------------------------------

fn ssh_command(target: &DeployTarget) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("ssh");
    cmd.arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ConnectTimeout=8")
        .arg("-p")
        .arg(target.ssh_port.to_string());
    let key = target.ssh_key_path.trim();
    if !key.is_empty() {
        cmd.arg("-i").arg(key);
    }
    cmd.arg(format!("{}@{}", target.ssh_user, target.ssh_host));
    cmd
}

async fn ssh_output(target: &DeployTarget, remote_cmd: &str) -> Result<String, String> {
    let mut cmd = ssh_command(target);
    cmd.arg(remote_cmd);
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Failed to run ssh: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "ssh exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn ssh_write_stdin(
    target: &DeployTarget,
    remote_cmd: &str,
    data: &[u8],
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;

    let mut cmd = ssh_command(target);
    cmd.arg(remote_cmd);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("Failed to run ssh: {e}"))?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "ssh: no stdin handle".to_string())?;
        stdin
            .write_all(data)
            .await
            .map_err(|e| format!("Failed to write to ssh stdin: {e}"))?;
    }
    let output = child
        .wait_with_output()
        .await
        .map_err(|e| format!("ssh failed: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "ssh exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

async fn scp_upload(target: &DeployTarget, local: &Path, remote_path: &str) -> Result<(), String> {
    let mut cmd = tokio::process::Command::new("scp");
    cmd.arg("-o")
        .arg("BatchMode=yes")
        .arg("-P")
        .arg(target.ssh_port.to_string());
    let key = target.ssh_key_path.trim();
    if !key.is_empty() {
        cmd.arg("-i").arg(key);
    }
    cmd.arg(local);
    cmd.arg(format!(
        "{}@{}:{}",
        target.ssh_user, target.ssh_host, remote_path
    ));
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Failed to run scp: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "scp exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

// --- Windows local install ------------------------------------------------

/// Everything that needs admin rights happens in one elevated PowerShell run
/// (one UAC prompt). The script is passed as `-EncodedCommand` (base64
/// UTF-16LE), which sidesteps both nested command-line quoting and the
/// script execution policy, and needs no temp file. An elevated process's
/// output can't be captured through `Start-Process -Verb RunAs`, so the
/// script reports through a small result file, tagged with a per-run token
/// so a stale file from an earlier run is never mistaken for this one.
#[cfg_attr(not(windows), allow(dead_code))]
mod windows_local {
    use crate::state::settings::DeployTarget;

    pub const TASK_NAME: &str = "DVRDesk Guide History";

    /// PowerShell single-quoted string literal.
    pub fn ps_quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', "''"))
    }

    pub fn install_script(target: &DeployTarget, source_exe: &str) -> String {
        // Built by concatenating single-quoted literals with `$data`, so no
        // PowerShell escaping is needed; the double quotes are for the
        // service's own command line (paths may contain spaces).
        let head = format!(
            "--channels-dvr-url \"{}\" --listen \"0.0.0.0:{}\" --data \"",
            target.dvr_server_url.replace('"', ""),
            target.listen_port,
        );
        let tail = format!(
            "\\guide-history.json\" --poll-secs {} --retention-secs {} --fetch-window-secs {}",
            target.poll_secs, target.retention_secs, target.fetch_window_secs,
        );
        let args = format!("({} + $data + {})", ps_quote(&head), ps_quote(&tail));
        format!(
            "$bin = Join-Path $env:ProgramFiles 'DVRDesk\\guide-history-service'\n\
             $data = Join-Path $env:ProgramData 'DVRDesk\\guide-history-service'\n\
             New-Item -ItemType Directory -Force -Path $bin, $data | Out-Null\n\
             {remove_task}\
             $exe = Join-Path $bin 'guide-history-service.exe'\n\
             Copy-Item -LiteralPath {src} -Destination $exe -Force\n\
             $action = New-ScheduledTaskAction -Execute $exe -Argument {args} -WorkingDirectory $data\n\
             $trigger = New-ScheduledTaskTrigger -AtStartup\n\
             $principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest\n\
             $settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 999 \
             -RestartInterval (New-TimeSpan -Minutes 1) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable\n\
             Register-ScheduledTask -TaskName $task -Action $action -Trigger $trigger -Principal $principal -Settings $settings | Out-Null\n\
             New-NetFirewallRule -DisplayName $task -Direction Inbound -Action Allow -Protocol TCP -LocalPort {port} -Profile Private,Domain | Out-Null\n\
             Start-ScheduledTask -TaskName $task\n",
            remove_task = REMOVE_TASK,
            src = ps_quote(source_exe),
            args = args,
            port = target.listen_port,
        )
    }

    pub fn uninstall_script() -> String {
        format!(
            "{REMOVE_TASK}\
             Remove-Item -LiteralPath (Join-Path $env:ProgramFiles 'DVRDesk\\guide-history-service') -Recurse -Force -ErrorAction SilentlyContinue\n\
             Remove-Item -LiteralPath (Join-Path $env:ProgramData 'DVRDesk\\guide-history-service') -Recurse -Force -ErrorAction SilentlyContinue\n"
        )
    }

    /// Stops and removes any existing task, running copy (which would hold
    /// the exe and the port) and firewall rule — shared by install (so a
    /// reinstall/update starts clean) and uninstall.
    const REMOVE_TASK: &str = "\
        if (Get-ScheduledTask -TaskName $task -ErrorAction SilentlyContinue) {\n\
          Stop-ScheduledTask -TaskName $task -ErrorAction SilentlyContinue\n\
          Unregister-ScheduledTask -TaskName $task -Confirm:$false\n\
        }\n\
        Get-Process -Name 'guide-history-service' -ErrorAction SilentlyContinue | Stop-Process -Force\n\
        Get-NetFirewallRule -DisplayName $task -ErrorAction SilentlyContinue | Remove-NetFirewallRule\n\
        Start-Sleep -Milliseconds 500\n";

    /// Wraps `body` with the task name, error handling and result reporting.
    pub fn elevated_script(body: &str, token: &str) -> String {
        format!(
            "$ErrorActionPreference = 'Stop'\n\
             $task = {task}\n\
             $result = Join-Path $env:ProgramData 'DVRDesk\\ghs-result.txt'\n\
             New-Item -ItemType Directory -Force -Path (Split-Path $result) | Out-Null\n\
             try {{\n{body}Set-Content -LiteralPath $result -Value {ok}\n}} catch {{\n\
             Set-Content -LiteralPath $result -Value ({token_q} + ' ' + ($_ | Out-String))\n\
             exit 1\n}}\n",
            task = ps_quote(TASK_NAME),
            ok = ps_quote(&format!("{token} ok")),
            token_q = ps_quote(token),
        )
    }

    /// Runs `script` via a non-elevated PowerShell that launches an elevated
    /// one and waits for it. Exit code 1223 = the UAC prompt was declined.
    pub fn launcher_script(elevated_b64: &str) -> String {
        format!(
            "try {{ $p = Start-Process -FilePath powershell.exe -Verb RunAs -Wait -PassThru -WindowStyle Hidden \
             -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-EncodedCommand','{elevated_b64}'); \
             exit $p.ExitCode }} catch {{ exit 1223 }}"
        )
    }

    pub fn encode(script: &str) -> String {
        use base64::Engine as _;
        let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        base64::engine::general_purpose::STANDARD.encode(utf16)
    }

    #[cfg(windows)]
    async fn run_elevated(body: &str) -> Result<(), String> {
        let token = format!(
            "{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let elevated = encode(&elevated_script(body, &token));
        let launcher = encode(&launcher_script(&elevated));
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let output = tokio::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &launcher])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .await
            .map_err(|e| format!("Failed to run PowerShell: {e}"))?;
        if output.status.code() == Some(1223) {
            return Err("Administrator permission is needed to install the service, and it was declined.".into());
        }
        let result_path = std::env::var("ProgramData")
            .map(|d| std::path::PathBuf::from(d).join("DVRDesk").join("ghs-result.txt"))
            .map_err(|_| "ProgramData isn't set".to_string())?;
        let result = std::fs::read_to_string(&result_path).unwrap_or_default();
        match result.trim().strip_prefix(&token) {
            Some(rest) if rest.trim() == "ok" => Ok(()),
            Some(rest) => Err(rest.trim().to_string()),
            None => Err(format!(
                "The elevated install step didn't report back (exit {:?}). {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            )),
        }
    }

    #[cfg(windows)]
    pub async fn install(target: &DeployTarget) -> Result<(), String> {
        let src = super::bundled_service_binary()?;
        run_elevated(&install_script(target, &src.to_string_lossy())).await
    }

    #[cfg(windows)]
    pub async fn uninstall() -> Result<(), String> {
        run_elevated(&uninstall_script()).await
    }
}

#[cfg(test)]
mod tests {
    use super::windows_local::*;
    use crate::state::settings::DeployTarget;

    #[test]
    fn ps_quote_doubles_single_quotes() {
        assert_eq!(ps_quote("C:\\Jo's dir"), "'C:\\Jo''s dir'");
    }

    #[test]
    fn encode_is_base64_utf16le() {
        // "a" → 61 00 → "YQA="
        assert_eq!(encode("a"), "YQA=");
    }

    /// Writes the generated scripts to `$GHS_PS_DUMP_DIR` (when set) so they
    /// can be syntax-checked with a real PowerShell parser.
    #[test]
    fn windows_scripts_render() {
        let target = DeployTarget::new_local("t".into(), "http://192.168.1.50:8089".into());
        let install = elevated_script(
            &install_script(&target, "C:\\Users\\Jo O'Neil\\DVRDesk\\guide-history-service.exe"),
            "abc123",
        );
        assert!(install.contains("'C:\\Users\\Jo O''Neil\\DVRDesk\\guide-history-service.exe'"));
        let uninstall = elevated_script(&uninstall_script(), "abc123");
        let launcher = launcher_script(&encode(&install));
        if let Ok(dir) = std::env::var("GHS_PS_DUMP_DIR") {
            let dir = std::path::Path::new(&dir);
            std::fs::write(dir.join("install.ps1"), &install).unwrap();
            std::fs::write(dir.join("uninstall.ps1"), &uninstall).unwrap();
            std::fs::write(dir.join("launcher.ps1"), &launcher).unwrap();
        }
        // The launcher is itself passed encoded on the command line, which
        // Windows caps at 32,767 characters.
        assert!(encode(&launcher).len() < 30_000, "launcher too long: {}", encode(&launcher).len());
    }
}
