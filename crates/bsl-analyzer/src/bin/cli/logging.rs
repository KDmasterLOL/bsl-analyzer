use std::{env, fs, io, path::PathBuf, sync::Arc};

pub fn setup_logging(
    log_file: Option<PathBuf>,
    profile_filter: Option<String>,
    json_profile_filter: Option<String>,
) -> anyhow::Result<()> {
    use tracing_subscriber::fmt::writer::BoxMakeWriter;

    let writer: BoxMakeWriter = match log_file {
        Some(file) => BoxMakeWriter::new(Arc::new(fs::File::create(&file)?)),
        None => BoxMakeWriter::new(io::stderr),
    };

    // Suppress noisy Salsa internal logs that fire on every input change.
    let user_filter = env::var("BSL_LOG").ok().unwrap_or_else(|| "warn".to_owned());
    let filter = format!("{},salsa=warn", user_filter);

    bsl_analyzer::tracing::Config { writer, filter, profile_filter, json_profile_filter }.init()?;

    Ok(())
}
