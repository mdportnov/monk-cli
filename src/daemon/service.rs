use crate::{platform, Result};

#[derive(Debug, Clone)]
pub enum ServiceAction {
    Install,
    Uninstall { purge: bool },
}

pub fn run(action: ServiceAction) -> Result<String> {
    let bin = std::env::current_exe()?.to_string_lossy().into_owned();
    match action {
        ServiceAction::Install => platform::install_service(&bin),
        ServiceAction::Uninstall { purge } => platform::uninstall_service(purge),
    }
}
