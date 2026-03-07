//! Web Service (SOAP) metadata structures.
//!
//! Represents 1C:Enterprise SOAP web service configuration.

use serde::{Deserialize, Serialize};

/// Web Service metadata (SOAP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebService {
    name: String,
    namespace: String,
    operations: Vec<WebServiceOperation>,
    uri: Option<String>,
}

impl WebService {
    /// Create a new web service.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), namespace: String::new(), operations: Vec::new(), uri: None }
    }

    /// Get the service name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the namespace.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Get operations.
    pub fn operations(&self) -> &[WebServiceOperation] {
        &self.operations
    }

    /// Get module URI (path to .bsl file).
    pub fn uri(&self) -> Option<&str> {
        self.uri.as_deref()
    }
}

/// Web service operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebServiceOperation {
    name: String,
    procedure_name: String,
    parameters: Vec<WebServiceParameter>,
}

impl WebServiceOperation {
    /// Create a new operation.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), procedure_name: String::new(), parameters: Vec::new() }
    }

    /// Get the operation name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the handler procedure name.
    pub fn procedure_name(&self) -> &str {
        &self.procedure_name
    }

    /// Get operation parameters.
    pub fn parameters(&self) -> &[WebServiceParameter] {
        &self.parameters
    }

    /// Check if handler is empty.
    pub fn is_handler_empty(&self) -> bool {
        self.procedure_name.is_empty()
    }
}

/// Web service operation parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebServiceParameter {
    name: String,
}

impl WebServiceParameter {
    /// Create a new parameter.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    /// Get the parameter name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Builder for WebService.
#[derive(Debug, Default)]
pub struct WebServiceBuilder {
    name: String,
    namespace: String,
    operations: Vec<WebServiceOperation>,
    uri: Option<String>,
}

impl WebServiceBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set the namespace.
    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = namespace.into();
        self
    }

    /// Add an operation.
    pub fn add_operation(mut self, operation: WebServiceOperation) -> Self {
        self.operations.push(operation);
        self
    }

    /// Set the module URI.
    pub fn uri(mut self, uri: impl Into<String>) -> Self {
        self.uri = Some(uri.into());
        self
    }

    /// Build the WebService.
    pub fn build(self) -> WebService {
        WebService {
            name: self.name,
            namespace: self.namespace,
            operations: self.operations,
            uri: self.uri,
        }
    }
}

/// Builder for WebServiceOperation.
#[derive(Debug, Default)]
pub struct WebServiceOperationBuilder {
    name: String,
    procedure_name: String,
    parameters: Vec<WebServiceParameter>,
}

impl WebServiceOperationBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set the procedure name.
    pub fn procedure_name(mut self, procedure_name: impl Into<String>) -> Self {
        self.procedure_name = procedure_name.into();
        self
    }

    /// Add a parameter.
    pub fn add_parameter(mut self, parameter: WebServiceParameter) -> Self {
        self.parameters.push(parameter);
        self
    }

    /// Build the WebServiceOperation.
    pub fn build(self) -> WebServiceOperation {
        WebServiceOperation {
            name: self.name,
            procedure_name: self.procedure_name,
            parameters: self.parameters,
        }
    }
}
