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
            eprintln!("To install into 1C infobase:");
            eprintln!(
                "  rtools config extension import -d <database> -e BSL_Analyzer -i {}",
                output.display()
            );
            eprintln!("  rtools config extension apply -d <database> -e BSL_Analyzer");
        }
    }

    Ok(())
}
