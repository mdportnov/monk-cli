mod block_page;
mod pidfile;
pub mod scheduler;
mod server;
mod service;
mod supervisor;

pub use pidfile::PidFile;
pub use server::run;
pub use service::{run as service_run, ServiceAction};
pub use supervisor::Supervisor;

/// Spawn `monk daemon run` detached from the calling terminal so the daemon
/// survives the terminal (and shell session) closing.
///
/// - unix: a new process group takes it out of the shell's job table, so a
///   tty hangup never propagates (the daemon additionally treats SIGHUP as
///   config-reload, never exit).
/// - windows: a plain child shares the parent's console and is killed when
///   that console window closes; DETACHED_PROCESS breaks that tie.
pub fn spawn_detached() -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.args(["daemon", "run"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn().map(|_| ())
}
