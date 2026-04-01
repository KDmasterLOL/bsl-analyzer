use std::{io, path::Path};

use crate::mcp_install::error::InstallError;

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

pub trait CommandRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        cwd: &Path,
    ) -> Result<CommandOutput, InstallError>;
}

pub trait FileStore {
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
    fn write_string(&self, path: &Path, contents: &str) -> io::Result<()>;
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    fn exists(&self, path: &Path) -> bool;
}
