//! Fixed allowlisted worker protocol; no shell, arbitrary executable, or inherited credentials.
use crate::{Tool, ToolError};
use futures_util::future::BoxFuture;
use serde_json::Value;
use std::{
    io::{Read, Write},
    process::Stdio,
    sync::Arc,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
const LIMIT: usize = 64 * 1024;

pub struct ProcessTool(Arc<dyn Tool>);
impl ProcessTool {
    pub fn builtin(tool: Arc<dyn Tool>) -> Self {
        Self(tool)
    }
}
impl Tool for ProcessTool {
    fn name(&self) -> &'static str {
        self.0.name()
    }
    fn description(&self) -> String {
        self.0.description()
    }
    fn parameters(&self) -> Value {
        self.0.parameters()
    }
    fn run<'a>(&'a self, args: Value) -> BoxFuture<'a, Result<Value, ToolError>> {
        Box::pin(async move {
            if !matches!(self.name(), "echo" | "add") {
                return Err(ToolError::Execution(
                    "tool is not approved for the isolated worker".into(),
                ));
            }
            let input = serde_json::to_vec(&args).map_err(failure)?;
            if input.len() > LIMIT {
                return Err(ToolError::InvalidArguments(
                    "tool input exceeded 64 KiB".into(),
                ));
            }
            let mut command =
                tokio::process::Command::new(std::env::current_exe().map_err(failure)?);
            command
                .arg("--llmn-tool-worker")
                .arg(self.name())
                .env_clear()
                .current_dir(std::env::temp_dir())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            #[cfg(windows)]
            {
                if let Some(root) = std::env::var_os("SystemRoot") {
                    command.env("SystemRoot", root);
                }
                command.creation_flags(0x08000000); // CREATE_NO_WINDOW
            }
            #[cfg(test)]
            {
                command = tokio::process::Command::new(std::env::current_exe().map_err(failure)?);
                command
                    .args(["--exact", "process::tests::worker_fixture", "--nocapture"])
                    .env_clear()
                    .env("LLMN_ISOLATION_CHILD", "1")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .kill_on_drop(true);
                #[cfg(windows)]
                command.creation_flags(0x08000000);
            }
            let mut child = command.spawn().map_err(failure)?;
            #[cfg(test)]
            CHILD_PID.store(child.id().unwrap(), std::sync::atomic::Ordering::SeqCst);
            // Worker cannot execute until stdin is sent, after resource containment.
            #[cfg(windows)]
            let _job = Job::attach(child.id().ok_or_else(|| failure("worker has no PID"))?)?;
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| failure("missing worker stdin"))?;
            let mut stdout = child
                .stdout
                .take()
                .ok_or_else(|| failure("missing worker stdout"))?
                .take((LIMIT + 1) as u64);
            let mut bytes = Vec::new();
            let input_task = async move {
                stdin.write_all(&input).await?;
                stdin.shutdown().await?;
                drop(stdin);
                Ok::<_, std::io::Error>(())
            };
            let output_task = async { stdout.read_to_end(&mut bytes).await.map(|_| ()) };
            tokio::try_join!(input_task, output_task).map_err(failure)?;
            if bytes.len() > LIMIT {
                return Err(failure("tool output exceeded 64 KiB"));
            }
            let status = child.wait().await.map_err(failure)?;
            if !status.success() {
                return Err(failure(format!("worker failed: {status}")));
            }
            let response: Value = serde_json::from_slice(&bytes).map_err(failure)?;
            if let Some(error) = response.get("error").and_then(Value::as_str) {
                return Err(failure(error));
            }
            response
                .get("result")
                .cloned()
                .ok_or_else(|| failure("invalid worker response"))
        })
    }
}
fn failure(error: impl std::fmt::Display) -> ToolError {
    ToolError::Execution(error.to_string())
}

/// Call before configuration, secrets, logging, UI or storage initialization in every host binary.
/// The worker only accepts compiled-in tools. Unknown names fail closed.
pub fn worker_entry() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("--llmn-tool-worker") {
        return;
    }
    let result = (|| -> Result<Value, ToolError> {
        let tool: Box<dyn Tool> = match args.next().as_deref() {
            Some("echo") => Box::new(crate::builtin::Echo),
            Some("add") => Box::new(crate::builtin::Add),
            _ => return Err(failure("tool not allowed")),
        };
        if args.next().is_some() {
            return Err(failure("unexpected worker argument"));
        }
        let mut input = Vec::new();
        std::io::stdin()
            .take((LIMIT + 1) as u64)
            .read_to_end(&mut input)
            .map_err(failure)?;
        if input.len() > LIMIT {
            return Err(failure("tool input exceeded 64 KiB"));
        }
        let args: Value = serde_json::from_slice(&input).map_err(failure)?;
        tool.run_sync(args)
    })();
    let response = match result {
        Ok(result) => serde_json::json!({"result":result}),
        Err(error) => serde_json::json!({"error":error.to_string()}),
    };
    let bytes = serde_json::to_vec(&response).unwrap();
    if bytes.len() > LIMIT {
        std::process::exit(2);
    }
    if std::io::stdout().write_all(&bytes).is_err() {
        std::process::exit(3);
    }
    std::process::exit(0);
}

#[cfg(windows)]
struct Job(usize);
#[cfg(windows)]
impl Job {
    fn attach(pid: u32) -> Result<Self, ToolError> {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::{JobObjects::*, Threading::*},
        };
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(failure(std::io::Error::last_os_error()));
            }
            let job = Self(handle as usize);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
                | JOB_OBJECT_LIMIT_PROCESS_MEMORY
                | JOB_OBJECT_LIMIT_PROCESS_TIME;
            limits.BasicLimitInformation.ActiveProcessLimit = 1;
            limits.BasicLimitInformation.PerProcessUserTimeLimit = 10 * 10_000_000;
            limits.ProcessMemoryLimit = 256 * 1024 * 1024;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            ) == 0
            {
                return Err(failure(std::io::Error::last_os_error()));
            }
            let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if process.is_null() {
                return Err(failure(std::io::Error::last_os_error()));
            }
            let assigned = AssignProcessToJobObject(handle, process);
            let error = std::io::Error::last_os_error();
            CloseHandle(process);
            if assigned == 0 {
                return Err(failure(error));
            }
            Ok(job)
        }
    }
}
#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0 as _);
        }
    }
}

#[cfg(test)]
static CHILD_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_fixture() {
        if std::env::var_os("LLMN_ISOLATION_CHILD").is_none() {
            return;
        }
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn dropping_run_terminates_blocking_worker() {
        let tool = ProcessTool::builtin(Arc::new(crate::builtin::Echo));
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            tool.run(serde_json::json!({"text":"test"})),
        )
        .await;
        assert!(result.is_err());
        let pid = CHILD_PID.load(std::sync::atomic::Ordering::SeqCst);
        assert_ne!(pid, 0);
        use windows_sys::Win32::{Foundation::CloseHandle, System::Threading::*};
        unsafe {
            let process = OpenProcess(0x00100000, 0, pid);
            if !process.is_null() {
                assert_eq!(WaitForSingleObject(process, 2000), 0);
                CloseHandle(process);
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn dropping_run_reaps_blocking_worker() {
        let tool = ProcessTool::builtin(Arc::new(crate::builtin::Echo));
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            tool.run(serde_json::json!({"text":"test"})),
        )
        .await;
        assert!(result.is_err());
        let pid = CHILD_PID.load(std::sync::atomic::Ordering::SeqCst);
        assert_ne!(pid, 0);
        // A zombie still has /proc/PID: require both termination and reaping.
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while std::path::Path::new(&format!("/proc/{pid}")).exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("cancelled worker must terminate and be reaped");
    }
}
