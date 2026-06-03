use std::{io::Write as _, marker::PhantomData, time::Instant};

use rustc_hash::FxHashSet;
use tracing::{
    span::{Attributes, Id},
    Event, Level, Subscriber,
};
use tracing_subscriber::{filter, fmt::MakeWriter, layer::Context, registry::LookupSpan, Layer};

struct JsonData {
    name: &'static str,
    start: std::time::Instant,
}

impl JsonData {
    fn new(name: &'static str) -> Self {
        Self { name, start: Instant::now() }
    }
}

#[derive(Debug)]
pub struct TimingLayer<S, W> {
    writer: W,
    _inner: PhantomData<fn(S)>,
}

impl<S, W> TimingLayer<S, W>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    #[allow(clippy::new_ret_no_self)]
    pub fn new(spec: &str, writer: W) -> impl Layer<S> {
        let filter = JsonFilter::from_spec(spec);

        let profile_filter = filter::filter_fn(move |metadata| {
            let allowed = match &filter.allowed_names {
                Some(names) => names.contains(metadata.name()),
                None => true,
            };

            allowed && metadata.is_span() && metadata.level() >= &Level::INFO
        });

        Self { writer, _inner: PhantomData }.with_filter(profile_filter)
    }
}

impl<S, W> Layer<S> for TimingLayer<S, W>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let span = ctx.span(id).unwrap();

        let data = JsonData::new(attrs.metadata().name());
        span.extensions_mut().insert(data);
    }

    fn on_event(&self, _event: &Event<'_>, _ctx: Context<'_, S>) {}

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        #[derive(serde::Serialize)]
        struct JsonDataInner {
            name: &'static str,
            elapsed_ms: u128,
        }

        let span = ctx.span(&id).unwrap();
        let Some(data) = span.extensions_mut().remove::<JsonData>() else {
            return;
        };

        let data = JsonDataInner { name: data.name, elapsed_ms: data.start.elapsed().as_millis() };
        let mut out = serde_json::to_string(&data).expect("Unable to serialize data");
        out.push('\n');
        self.writer.make_writer().write_all(out.as_bytes()).expect("Unable to write data");
    }
}

#[derive(Default, Clone, Debug)]
struct JsonFilter {
    allowed_names: Option<FxHashSet<String>>,
}

impl JsonFilter {
    fn from_spec(spec: &str) -> Self {
        let allowed_names = if spec == "*" {
            None
        } else {
            Some(FxHashSet::from_iter(spec.split('|').map(String::from)))
        };

        Self { allowed_names }
    }
}
