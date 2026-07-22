use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HTTPService {
    name: String,
    root_url: String,
    url_templates: Vec<HTTPServiceURLTemplate>,
    uri: Option<String>,
}

impl HTTPService {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), root_url: String::new(), url_templates: Vec::new(), uri: None }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn root_url(&self) -> &str {
        &self.root_url
    }

    pub fn url_templates(&self) -> &[HTTPServiceURLTemplate] {
        &self.url_templates
    }

    pub fn uri(&self) -> Option<&str> {
        self.uri.as_deref()
    }

    pub fn all_methods(
        &self,
    ) -> impl Iterator<Item = (&HTTPServiceURLTemplate, &HTTPServiceMethod)> {
        self.url_templates.iter().flat_map(|t| t.methods.iter().map(move |m| (t, m)))
    }

    /// Heap bytes owned by this service, memoised by `ide-db`'s
    /// `parse_http_service_query` for Salsa's `heap_size` hook: its name/URL
    /// strings plus the URL-template vec and each template's own owned payload.
    /// New heap-owning fields must be added here too.
    pub fn estimated_heap_size(&self) -> usize {
        self.name.capacity()
            + self.root_url.capacity()
            + self.uri.as_ref().map_or(0, String::capacity)
            + stdx::heap::vec_bytes::<HTTPServiceURLTemplate>(self.url_templates.len())
            + self
                .url_templates
                .iter()
                .map(HTTPServiceURLTemplate::estimated_heap_size)
                .sum::<usize>()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HTTPServiceURLTemplate {
    name: String,
    template: String,
    methods: Vec<HTTPServiceMethod>,
}

impl HTTPServiceURLTemplate {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), template: String::new(), methods: Vec::new() }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn template(&self) -> &str {
        &self.template
    }

    pub fn methods(&self) -> &[HTTPServiceMethod] {
        &self.methods
    }

    /// Heap bytes owned by this template: its name/template strings plus the
    /// method vec and each method's own owned payload.
    pub fn estimated_heap_size(&self) -> usize {
        self.name.capacity()
            + self.template.capacity()
            + stdx::heap::vec_bytes::<HTTPServiceMethod>(self.methods.len())
            + self.methods.iter().map(HTTPServiceMethod::estimated_heap_size).sum::<usize>()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HTTPServiceMethod {
    name: String,
    http_method: String,
    handler: String,
}

impl HTTPServiceMethod {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), http_method: String::new(), handler: String::new() }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn http_method(&self) -> &str {
        &self.http_method
    }

    pub fn handler(&self) -> &str {
        &self.handler
    }

    pub fn is_handler_empty(&self) -> bool {
        self.handler.is_empty()
    }

    /// Heap bytes owned by this method: its name/HTTP-verb/handler strings.
    pub fn estimated_heap_size(&self) -> usize {
        self.name.capacity() + self.http_method.capacity() + self.handler.capacity()
    }
}

#[derive(Debug, Default)]
pub struct HTTPServiceBuilder {
    name: String,
    root_url: String,
    url_templates: Vec<HTTPServiceURLTemplate>,
    uri: Option<String>,
}

impl HTTPServiceBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn root_url(mut self, root_url: impl Into<String>) -> Self {
        self.root_url = root_url.into();
        self
    }

    pub fn add_url_template(mut self, template: HTTPServiceURLTemplate) -> Self {
        self.url_templates.push(template);
        self
    }

    pub fn uri(mut self, uri: impl Into<String>) -> Self {
        self.uri = Some(uri.into());
        self
    }

    pub fn build(self) -> HTTPService {
        HTTPService {
            name: self.name,
            root_url: self.root_url,
            url_templates: self.url_templates,
            uri: self.uri,
        }
    }
}

#[derive(Debug, Default)]
pub struct HTTPServiceURLTemplateBuilder {
    name: String,
    template: String,
    methods: Vec<HTTPServiceMethod>,
}

impl HTTPServiceURLTemplateBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn template(mut self, template: impl Into<String>) -> Self {
        self.template = template.into();
        self
    }

    pub fn add_method(mut self, method: HTTPServiceMethod) -> Self {
        self.methods.push(method);
        self
    }

    pub fn build(self) -> HTTPServiceURLTemplate {
        HTTPServiceURLTemplate { name: self.name, template: self.template, methods: self.methods }
    }
}

#[derive(Debug, Default)]
pub struct HTTPServiceMethodBuilder {
    name: String,
    http_method: String,
    handler: String,
}

impl HTTPServiceMethodBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn http_method(mut self, http_method: impl Into<String>) -> Self {
        self.http_method = http_method.into();
        self
    }

    pub fn handler(mut self, handler: impl Into<String>) -> Self {
        self.handler = handler.into();
        self
    }

    pub fn build(self) -> HTTPServiceMethod {
        HTTPServiceMethod { name: self.name, http_method: self.http_method, handler: self.handler }
    }
}
