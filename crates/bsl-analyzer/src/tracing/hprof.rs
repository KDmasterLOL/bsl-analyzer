use std::{
    fmt::Write,
    marker::PhantomData,
    mem,
    time::{Duration, Instant},
};

use rustc_hash::FxHashSet;
use tracing::{
    field::{Field, Visit},
    span::Attributes,
    Event, Id, Level, Subscriber,
};
use tracing_subscriber::{filter, layer::Context, registry::LookupSpan, Layer};

#[derive(Debug)]
pub struct SpanTree<S> {
    aggregate: bool,
    write_filter: WriteFilter,
    _inner: PhantomData<fn(S)>,
}

impl<S> SpanTree<S>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    // Returns `impl Layer<S>` instead of Self to enable tracing layer composition.
    // This is a common pattern in tracing ecosystem for building reusable layers.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(spec: &str) -> impl Layer<S> {
        let (write_filter, allowed_names) = WriteFilter::from_spec(spec);

        let profile_filter = filter::filter_fn(move |metadata| {
            let allowed = match &allowed_names {
                Some(names) => names.contains(metadata.name()),
                None => true,
            };

            allowed && metadata.is_span() && metadata.level() >= &Level::INFO
        });

        Self { aggregate: true, write_filter, _inner: PhantomData }.with_filter(profile_filter)
    }

    pub fn disabled() -> impl Layer<S> {
        Self { aggregate: false, write_filter: WriteFilter::default(), _inner: PhantomData }
            .with_filter(filter::filter_fn(|_| false))
    }
}

struct Data {
    start: Instant,
    children: Vec<Node>,
    fields: String,
}

impl Data {
    fn new(attrs: &Attributes<'_>) -> Self {
        let mut data = Self { start: Instant::now(), children: Vec::new(), fields: String::new() };

        let mut visitor = DataVisitor { string: &mut data.fields };
        attrs.record(&mut visitor);
        data
    }

    fn into_node(self, name: &'static str) -> Node {
        Node {
            name,
            fields: self.fields,
            count: 1,
            duration: self.start.elapsed(),
            children: self.children,
        }
    }
}

struct DataVisitor<'a> {
    string: &'a mut String,
}

impl Visit for DataVisitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        write!(self.string, "{} = {:?} ", field.name(), value).unwrap();
    }
}

impl<S> Layer<S> for SpanTree<S>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let span = ctx.span(id).unwrap();

        let data = Data::new(attrs);
        span.extensions_mut().insert(data);
    }

    fn on_event(&self, _event: &Event<'_>, _ctx: Context<'_, S>) {}

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let span = ctx.span(&id).unwrap();
        let data = span.extensions_mut().remove::<Data>().unwrap();
        let mut node = data.into_node(span.name());

        match span.parent() {
            Some(parent_span) => {
                parent_span.extensions_mut().get_mut::<Data>().unwrap().children.push(node);
            }
            None => {
                if self.aggregate {
                    node.aggregate()
                }
                node.print(&self.write_filter)
            }
        }
    }
}

#[derive(Default)]
struct Node {
    name: &'static str,
    fields: String,
    count: u32,
    duration: Duration,
    children: Vec<Node>,
}

impl Node {
    fn print(&self, filter: &WriteFilter) {
        self.go(0, filter)
    }

    fn go(&self, level: usize, filter: &WriteFilter) {
        if self.duration > filter.longer_than && level < filter.depth {
            let duration_ms = self.duration.as_secs_f64() * 1000.0;
            let current_indent = level * 2;

            let mut out = String::new();
            let _ = write!(out, "{:current_indent$}{:>8.2}ms   {:<30}", "", duration_ms, self.name);

            if !self.fields.is_empty() {
                let _ = write!(out, " @ {}", self.fields);
            }

            if self.count > 1 {
                let _ = write!(out, " ({} calls)", self.count);
            }

            // Use tracing so output goes to BSL_LOG_FILE when set (important for LSP mode)
            tracing::warn!(target: "bsl_profile", "{out}");

            for child in &self.children {
                child.go(level + 1, filter)
            }
        }
    }

    fn aggregate(&mut self) {
        if self.children.is_empty() {
            return;
        }

        self.children.sort_by_key(|it| it.name);
        let mut idx = 0;
        for i in 1..self.children.len() {
            if self.children[idx].name == self.children[i].name {
                let child = mem::take(&mut self.children[i]);
                self.children[idx].duration += child.duration;
                self.children[idx].count += child.count;
                self.children[idx].children.extend(child.children);
            } else {
                idx += 1;
                assert!(idx <= i);
                self.children.swap(idx, i);
            }
        }
        self.children.truncate(idx + 1);
        for child in &mut self.children {
            child.aggregate()
        }
    }
}

#[derive(Default, Clone, Debug)]
struct WriteFilter {
    depth: usize,
    longer_than: Duration,
}

impl WriteFilter {
    fn from_spec(mut spec: &str) -> (WriteFilter, Option<FxHashSet<String>>) {
        let longer_than = if let Some(idx) = spec.rfind('>') {
            let longer_than = spec[idx + 1..].parse().expect("invalid profile longer_than");
            spec = &spec[..idx];
            Duration::from_millis(longer_than)
        } else {
            Duration::new(0, 0)
        };

        let depth = if let Some(idx) = spec.rfind('@') {
            let depth: usize = spec[idx + 1..].parse().expect("invalid profile depth");
            spec = &spec[..idx];
            depth
        } else {
            999
        };

        let allowed =
            if spec == "*" { None } else { Some(spec.split('|').map(String::from).collect()) };

        (WriteFilter { depth, longer_than }, allowed)
    }
}
