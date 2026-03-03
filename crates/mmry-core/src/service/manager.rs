use crate::Result;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use sysinfo::Pid;
use sysinfo::System;

pub struct ServiceManager {
    state_dir: PathBuf,
}

impl ServiceManager {
    pub fn new() -> Result<Self> {
        let state_dir = get_state_dir()?;
        std::fs::create_dir_all(&state_dir)?;
        Ok(Self { state_dir })
    }

    pub fn pid_file(&self) -> PathBuf {
        self.state_dir.join("service.pid")
    }

    pub fn port_file(&self) -> PathBuf {
        self.state_dir.join("service.port")
    }

    pub fn is_running(&self) -> bool {
        if let Ok(pid) = self.read_pid() {
            process_exists(pid)
        } else {
            false
        }
    }

    pub fn read_pid(&self) -> Result<u32> {
        let content = std::fs::read_to_string(self.pid_file())?;
        content
            .trim()
            .parse()
            .map_err(|e| crate::Error::Service(format!("Invalid PID: {e}")))
    }

    pub fn read_port(&self) -> Result<u16> {
        let content = std::fs::read_to_string(self.port_file())?;
        content
            .trim()
            .parse()
            .map_err(|e| crate::Error::Service(format!("Invalid port: {e}")))
    }

    pub fn start(&self, foreground: bool) -> Result<()> {
        if self.is_running() {
            return Err(crate::Error::Service("Service already running".into()));
        }

        let exe = std::env::current_exe()?;
        let service_exe = exe
            .parent()
            .ok_or_else(|| crate::Error::Service("Cannot find service binary".into()))?
            .join("mmry-service");

        if !service_exe.exists() {
            return Err(crate::Error::Service(
                "mmry-service binary not found. Please install it first.".into(),
            ));
        }

        if foreground {
            let status = Command::new(&service_exe).arg("--foreground").status()?;

            if !status.success() {
                return Err(crate::Error::Service("Service failed to start".into()));
            }
        } else {
            // Start in background
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                Command::new(&service_exe)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .process_group(0) // Create new process group
                    .spawn()?;
            }

            #[cfg(not(unix))]
            {
                Command::new(&service_exe)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?;
            }

            // Wait for service to start (check for PID file and running process)
            let mut attempts = 0;
            while attempts < 20 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if self.is_running() {
                    break;
                }
                attempts += 1;
            }

            if !self.is_running() {
                return Err(crate::Error::Service("Service failed to start".into()));
            }
        }

        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        let pid = self.read_pid()?;

        if !process_exists(pid) {
            // Cleanup stale PID file
            std::fs::remove_file(self.pid_file()).ok();
            return Err(crate::Error::Service("Service not running".into()));
        }

        // Send termination signal
        #[cfg(unix)]
        {
            Command::new("kill").arg(pid.to_string()).status()?;
        }

        #[cfg(windows)]
        {
            Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .status()?;
        }

        // Wait for process to exit
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if !process_exists(pid) {
                break;
            }
        }

        // Cleanup files
        std::fs::remove_file(self.pid_file()).ok();
        std::fs::remove_file(self.port_file()).ok();

        Ok(())
    }

    pub fn status(&self) -> ServiceStatus {
        if let Ok(pid) = self.read_pid() {
            if process_exists(pid) {
                ServiceStatus::Running { pid }
            } else {
                ServiceStatus::Dead
            }
        } else {
            ServiceStatus::Stopped
        }
    }

    /// Install and enable the service for auto-start on system boot.
    ///
    /// On Linux: writes a systemd user unit and runs `systemctl --user enable mmry`.
    /// On macOS: writes a launchd plist and loads it.
    pub fn enable(&self) -> Result<EnableResult> {
        let mmry_bin = find_mmry_binary()?;

        #[cfg(target_os = "linux")]
        {
            self.enable_systemd(&mmry_bin)
        }

        #[cfg(target_os = "macos")]
        {
            self.enable_launchd(&mmry_bin)
        }

        #[cfg(target_os = "windows")]
        {
            Err(crate::Error::Service(
                "Automatic enable is not supported on Windows. \
                 Use Task Scheduler to create a startup task for mmry-service."
                    .into(),
            ))
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            Err(crate::Error::Service(
                "Automatic enable is not supported on this platform.".into(),
            ))
        }
    }

    /// Disable auto-start and remove the service unit/plist.
    ///
    /// On Linux: runs `systemctl --user disable mmry` and removes the unit file.
    /// On macOS: unloads and removes the launchd plist.
    pub fn disable(&self) -> Result<DisableResult> {
        #[cfg(target_os = "linux")]
        {
            self.disable_systemd()
        }

        #[cfg(target_os = "macos")]
        {
            self.disable_launchd()
        }

        #[cfg(target_os = "windows")]
        {
            Err(crate::Error::Service(
                "Automatic disable is not supported on Windows. \
                 Remove the task from Task Scheduler manually."
                    .into(),
            ))
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            Err(crate::Error::Service(
                "Automatic disable is not supported on this platform.".into(),
            ))
        }
    }

    /// Check whether the service is enabled for auto-start.
    pub fn is_enabled(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            Command::new("systemctl")
                .args(["--user", "is-enabled", "mmry"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        }

        #[cfg(target_os = "macos")]
        {
            launchd_plist_path().is_ok_and(|p| p.exists())
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            false
        }
    }

    #[cfg(target_os = "linux")]
    fn enable_systemd(&self, mmry_bin: &str) -> Result<EnableResult> {
        let service_dir = systemd_user_dir()?;
        std::fs::create_dir_all(&service_dir)?;

        let unit_path = service_dir.join("mmry.service");

        // Only write the unit file if it does not already exist.
        // If octo or another tool already created one we must not overwrite it.
        let wrote_unit = if unit_path.exists() {
            false
        } else {
            let unit = format!(
                "[Unit]\n\
                 Description=mmry Memory Service\n\
                 \n\
                 [Service]\n\
                 Type=simple\n\
                 ExecStart={mmry_bin} service run\n\
                 Restart=always\n\
                 RestartSec=5\n\
                 Environment=PATH=%h/.cargo/bin:%h/.local/bin:/usr/local/bin:/usr/bin:/bin\n\
                 Environment=HOME=%h\n\
                 \n\
                 [Install]\n\
                 WantedBy=default.target\n"
            );
            std::fs::write(&unit_path, unit)?;
            true
        };

        // daemon-reload so systemd picks up new/changed file
        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status();

        let status = Command::new("systemctl")
            .args(["--user", "enable", "mmry"])
            .status()?;

        if !status.success() {
            return Err(crate::Error::Service(
                "systemctl --user enable mmry failed".into(),
            ));
        }

        Ok(EnableResult {
            unit_path: unit_path.to_string_lossy().into_owned(),
            wrote_unit,
        })
    }

    #[cfg(target_os = "macos")]
    fn enable_launchd(&self, mmry_bin: &str) -> Result<EnableResult> {
        let plist_path = launchd_plist_path()?;

        if let Some(dir) = plist_path.parent() {
            std::fs::create_dir_all(dir)?;
        }

        let log_dir = dirs::home_dir()
            .ok_or_else(|| crate::Error::Service("Cannot find home directory".into()))?
            .join("Library")
            .join("Logs");
        std::fs::create_dir_all(&log_dir)?;

        let wrote_unit = if plist_path.exists() {
            false
        } else {
            let plist = format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.mmry.service</string>
  <key>ProgramArguments</key>
  <array>
    <string>{mmry_bin}</string>
    <string>service</string>
    <string>run</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
    <key>Crashed</key>
    <true/>
  </dict>
  <key>ThrottleInterval</key>
  <integer>5</integer>
  <key>StandardOutPath</key>
  <string>{stdout}</string>
  <key>StandardErrorPath</key>
  <string>{stderr}</string>
</dict>
</plist>"#,
                stdout = log_dir.join("mmry.stdout.log").display(),
                stderr = log_dir.join("mmry.stderr.log").display(),
            );
            std::fs::write(&plist_path, plist)?;
            true
        };

        // Unload first in case already loaded (ignore errors)
        let _ = Command::new("launchctl")
            .args(["unload", &plist_path.to_string_lossy()])
            .status();

        let status = Command::new("launchctl")
            .args(["load", &plist_path.to_string_lossy()])
            .status()?;

        if !status.success() {
            return Err(crate::Error::Service("launchctl load failed".into()));
        }

        Ok(EnableResult {
            unit_path: plist_path.to_string_lossy().into_owned(),
            wrote_unit,
        })
    }

    #[cfg(target_os = "linux")]
    fn disable_systemd(&self) -> Result<DisableResult> {
        let unit_path = systemd_user_dir()?.join("mmry.service");

        if !unit_path.exists() {
            return Err(crate::Error::Service(
                "mmry service unit not found; nothing to disable".into(),
            ));
        }

        // Stop if running via systemd
        let _ = Command::new("systemctl")
            .args(["--user", "stop", "mmry"])
            .status();

        let status = Command::new("systemctl")
            .args(["--user", "disable", "mmry"])
            .status()?;

        if !status.success() {
            return Err(crate::Error::Service(
                "systemctl --user disable mmry failed".into(),
            ));
        }

        // Remove the unit file
        std::fs::remove_file(&unit_path)?;

        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status();

        Ok(DisableResult {
            unit_path: unit_path.to_string_lossy().into_owned(),
        })
    }

    #[cfg(target_os = "macos")]
    fn disable_launchd(&self) -> Result<DisableResult> {
        let plist_path = launchd_plist_path()?;

        if !plist_path.exists() {
            return Err(crate::Error::Service(
                "mmry launchd plist not found; nothing to disable".into(),
            ));
        }

        let _ = Command::new("launchctl")
            .args(["unload", &plist_path.to_string_lossy()])
            .status();

        std::fs::remove_file(&plist_path)?;

        Ok(DisableResult {
            unit_path: plist_path.to_string_lossy().into_owned(),
        })
    }
}

#[derive(Debug, Clone)]
pub enum ServiceStatus {
    Running { pid: u32 },
    Stopped,
    Dead, // PID file exists but process not running
}

#[derive(Debug)]
pub struct EnableResult {
    /// Path to the unit file / plist that was created or already existed.
    pub unit_path: String,
    /// Whether a new unit file was written (false if one already existed).
    pub wrote_unit: bool,
}

#[derive(Debug)]
pub struct DisableResult {
    /// Path to the unit file / plist that was removed.
    pub unit_path: String,
}

fn process_exists(pid: u32) -> bool {
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, false);
    sys.process(Pid::from_u32(pid)).is_some()
}

fn get_state_dir() -> Result<PathBuf> {
    let base = std::env::var("XDG_STATE_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".local").join("state")))
        .ok_or_else(|| crate::Error::Service("Could not determine state directory".into()))?;

    Ok(base.join("mmry"))
}

/// Locate the mmry binary, preferring the same directory as the current executable.
fn find_mmry_binary() -> Result<String> {
    let exe = std::env::current_exe()?;
    if let Some(dir) = exe.parent() {
        let candidate = dir.join("mmry");
        if candidate.exists() {
            return Ok(candidate.to_string_lossy().into_owned());
        }
        // Also check mmry-service (the service binary itself)
        let service_candidate = dir.join("mmry-service");
        if service_candidate.exists() {
            // The binary running is mmry-service, but enable should use mmry cli
            // since that is what the systemd unit calls via `mmry service run`.
        }
    }
    // Fall back to PATH lookup
    if let Ok(output) = Command::new("which").arg("mmry").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(path);
            }
        }
    }
    Err(crate::Error::Service(
        "Could not find mmry binary. Ensure it is installed and on PATH.".into(),
    ))
}

#[cfg(target_os = "linux")]
fn systemd_user_dir() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| crate::Error::Service("Cannot find home directory".into()))?;
    Ok(home.join(".config").join("systemd").join("user"))
}

#[cfg(target_os = "macos")]
fn launchd_plist_path() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| crate::Error::Service("Cannot find home directory".into()))?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join("com.mmry.service.plist"))
}
