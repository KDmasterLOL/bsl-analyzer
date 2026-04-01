mod adapters;
mod error;
mod model;
mod ports;
mod preset;
mod usecase;

pub use error::InstallError;
pub use model::{
    default_server_name, ApplyDecision, InstallPreset, InstallRequest, InstallResult, InstallScope,
    InstallStatus, InstallTarget, InstallTargetSelector,
};

use crate::mcp_install::adapters::{RealCommandRunner, RealFileStore};

pub fn install(request: InstallRequest) -> Result<InstallResult, InstallError> {
    let plan = usecase::build_install_plan(request)?;
    let runner = RealCommandRunner;
    let files = RealFileStore;
    adapters::apply_install_plan(&plan, &runner, &files)
}
