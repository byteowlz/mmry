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
}

#[derive(Debug, Clone)]
pub enum ServiceStatus {
    Running { pid: u32 },
    Stopped,
    Dead, // PID file exists but process not running
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
