use std::{io, path::Path};

use crate::mcp_install::error::InstallError;

#[derive(Debug, Clone)]
pub(super) struct CommandOutput {
    pub(super) status: i32,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

pub(super) trait CommandRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        cwd: &Path,
    ) -> Result<CommandOutput, InstallError>;
}

pub(super) trait FileStore {
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
    fn write_string(&self, path: &Path, contents: &str) -> io::Result<()>;
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    fn exists(&self, path: &Path) -> bool;
}
