use anyhow::Context;
use tracing_subscriber::{
    filter::Targets, fmt::MakeWriter, layer::SubscriberExt, util::SubscriberInitExt, Layer,
    Registry,
};

use crate::tracing::hprof;

#[derive(Debug)]
pub struct Config<T> {
    pub writer: T,
    pub filter: String,
    pub profile_filter: Option<String>,
}

impl<T> Config<T>
where
    T: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    pub fn init(self) -> anyhow::Result<()> {
        let targets_filter: Targets = self
            .filter
            .parse()
            .with_context(|| format!("invalid log filter: `{}`", self.filter))?;

        let writer = self.writer;

        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_target(false)
            .with_ansi(false)
            .with_writer(writer)
            .with_filter(targets_filter);

        match self.profile_filter {
            Some(spec) => {
                Registry::default().with(fmt_layer).with(hprof::SpanTree::new(&spec)).init();
            }
            None => {
                Registry::default().with(fmt_layer).init();
            }
        }

        Ok(())
    }
}
