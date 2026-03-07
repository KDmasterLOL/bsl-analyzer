//! HTTP Service metadata structures.
//!
//! Represents 1C:Enterprise HTTP service configuration.

use serde::{Deserialize, Serialize};

/// HTTP Service metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HTTPService {
    name: String,
    root_url: String,
    url_templates: Vec<HTTPServiceURLTemplate>,
    uri: Option<String>,
}

impl HTTPService {
    /// Create a new HTTP service.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), root_url: String::new(), url_templates: Vec::new(), uri: None }
    }

    /// Get the service name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the root URL.
    pub fn root_url(&self) -> &str {
        &self.root_url
    }

    /// Get URL templates.
    pub fn url_templates(&self) -> &[HTTPServiceURLTemplate] {
        &self.url_templates
    }

    /// Get module URI (path to .bsl file).
    pub fn uri(&self) -> Option<&str> {
        self.uri.as_deref()
    }

    /// Iterate over all methods from all URL templates.
    pub fn all_methods(
        &self,
    ) -> impl Iterator<Item = (&HTTPServiceURLTemplate, &HTTPServiceMethod)> {
        self.url_templates.iter().flat_map(|t| t.methods.iter().map(move |m| (t, m)))
    }
}

/// URL template in HTTP service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HTTPServiceURLTemplate {
    name: String,
    template: String,
    methods: Vec<HTTPServiceMethod>,
}

impl HTTPServiceURLTemplate {
    /// Create a new URL template.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), template: String::new(), methods: Vec::new() }
    }

    /// Get the template name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the URL template pattern.
    pub fn template(&self) -> &str {
        &self.template
    }

    /// Get methods.
    pub fn methods(&self) -> &[HTTPServiceMethod] {
        &self.methods
    }
}

/// HTTP method in URL template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HTTPServiceMethod {
    name: String,
    http_method: String,
    handler: String,
}

impl HTTPServiceMethod {
    /// Create a new HTTP method.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), http_method: String::new(), handler: String::new() }
    }

    /// Get the method name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the HTTP method (GET, POST, etc.).
    pub fn http_method(&self) -> &str {
        &self.http_method
    }

    /// Get the handler function name.
    pub fn handler(&self) -> &str {
        &self.handler
    }

    /// Check if handler is empty.
    pub fn is_handler_empty(&self) -> bool {
        self.handler.is_empty()
    }
}

/// Builder for HTTPService.
#[derive(Debug, Default)]
pub struct HTTPServiceBuilder {
    name: String,
    root_url: String,
    url_templates: Vec<HTTPServiceURLTemplate>,
    uri: Option<String>,
}

impl HTTPServiceBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set the root URL.
    pub fn root_url(mut self, root_url: impl Into<String>) -> Self {
        self.root_url = root_url.into();
        self
    }

    /// Add a URL template.
    pub fn add_url_template(mut self, template: HTTPServiceURLTemplate) -> Self {
        self.url_templates.push(template);
        self
    }

    /// Set the module URI.
    pub fn uri(mut self, uri: impl Into<String>) -> Self {
        self.uri = Some(uri.into());
        self
    }

    /// Build the HTTPService.
    pub fn build(self) -> HTTPService {
        HTTPService {
            name: self.name,
            root_url: self.root_url,
            url_templates: self.url_templates,
            uri: self.uri,
        }
    }
}

/// Builder for HTTPServiceURLTemplate.
#[derive(Debug, Default)]
pub struct HTTPServiceURLTemplateBuilder {
    name: String,
    template: String,
    methods: Vec<HTTPServiceMethod>,
}

impl HTTPServiceURLTemplateBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set the URL template pattern.
    pub fn template(mut self, template: impl Into<String>) -> Self {
        self.template = template.into();
        self
    }

    /// Add a method.
    pub fn add_method(mut self, method: HTTPServiceMethod) -> Self {
        self.methods.push(method);
        self
    }

    /// Build the HTTPServiceURLTemplate.
    pub fn build(self) -> HTTPServiceURLTemplate {
        HTTPServiceURLTemplate { name: self.name, template: self.template, methods: self.methods }
    }
}

/// Builder for HTTPServiceMethod.
#[derive(Debug, Default)]
pub struct HTTPServiceMethodBuilder {
    name: String,
    http_method: String,
    handler: String,
}

impl HTTPServiceMethodBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set the HTTP method.
    pub fn http_method(mut self, http_method: impl Into<String>) -> Self {
        self.http_method = http_method.into();
        self
    }

    /// Set the handler.
    pub fn handler(mut self, handler: impl Into<String>) -> Self {
        self.handler = handler.into();
        self
    }

    /// Build the HTTPServiceMethod.
    pub fn build(self) -> HTTPServiceMethod {
        HTTPServiceMethod { name: self.name, http_method: self.http_method, handler: self.handler }
    }
}
