mod adapters;
mod error;
mod model;
mod ports;
mod preset;
mod program;
mod usecase;

pub use error::InstallError;
pub use model::{
    default_server_name, InstallPreset, InstallRequest, InstallResult, InstallScope, InstallStatus,
    InstallTarget, InstallTargetSelector,
};

use crate::mcp_install::adapters::{RealCommandRunner, RealFileStore};

pub fn install(request: InstallRequest) -> Result<InstallResult, InstallError> {
    install_many(vec![request])
}

pub fn install_many(requests: Vec<InstallRequest>) -> Result<InstallResult, InstallError> {
    let mut actions = Vec::new();
    for request in requests {
        let mut plan = usecase::build_install_plan(request)?;
        actions.append(&mut plan.actions);
    }

    let plan = model::InstallPlan { actions };
    let runner = RealCommandRunner;
    let files = RealFileStore;
    adapters::apply_install_plan(&plan, &runner, &files)
}
