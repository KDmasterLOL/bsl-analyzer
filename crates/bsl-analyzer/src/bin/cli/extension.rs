use std::{error::Error, fs, path::PathBuf};

use clap::Subcommand;

#[derive(Subcommand)]
pub enum ExtensionCommands {
    Export {
        #[arg(short, long)]
        output: PathBuf,
    },
}

pub fn run(command: ExtensionCommands) -> Result<(), Box<dyn Error + Send + Sync>> {
    match command {
        ExtensionCommands::Export { output } => {
            static EXTENSION_ZIP: &[u8] =
                include_bytes!(concat!(env!("OUT_DIR"), "/extension.zip"));

            let cursor = std::io::Cursor::new(EXTENSION_ZIP);
            let mut archive = zip::ZipArchive::new(cursor)?;

            let mut count = 0;
            for i in 0..archive.len() {
                let mut entry = archive.by_index(i)?;
                if entry.is_dir() {
                    continue;
                }
                let dest = output.join(entry.name());
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut out_file = fs::File::create(&dest)?;
                std::io::copy(&mut entry, &mut out_file)?;
                count += 1;
            }

            eprintln!("Extension exported to: {}", output.display());
            eprintln!("Files: {count}");
            eprintln!();
            eprintln!("To install into a 1C infobase (Designer):");
            eprintln!("  1. Конфигурация -> Расширения конфигурации -> add the exported folder");
            eprintln!(
                "  2. Администрирование -> Публикация на веб-сервере -> enable HTTP services"
            );
            eprintln!(
                "  3. Grant the role BSL_ОсновнаяРоль to the user the MCP server connects as"
            );
            eprintln!();
            eprintln!("Verify: curl http://<host>/<base>/hs/bsl-analyzer/version");
            eprintln!("Full guide: docs/mcp/TOOLS_AND_EXTENSION.md");
        }
    }

    Ok(())
}
