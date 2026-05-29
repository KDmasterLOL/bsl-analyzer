use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebService {
    name: String,
    namespace: String,
    operations: Vec<WebServiceOperation>,
    uri: Option<String>,
}

impl WebService {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), namespace: String::new(), operations: Vec::new(), uri: None }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn operations(&self) -> &[WebServiceOperation] {
        &self.operations
    }

    pub fn uri(&self) -> Option<&str> {
        self.uri.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebServiceOperation {
    name: String,
    procedure_name: String,
    parameters: Vec<WebServiceParameter>,
}

impl WebServiceOperation {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), procedure_name: String::new(), parameters: Vec::new() }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn procedure_name(&self) -> &str {
        &self.procedure_name
    }

    pub fn parameters(&self) -> &[WebServiceParameter] {
        &self.parameters
    }

    pub fn is_handler_empty(&self) -> bool {
        self.procedure_name.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebServiceParameter {
    name: String,
}

impl WebServiceParameter {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Debug, Default)]
pub struct WebServiceBuilder {
    name: String,
    namespace: String,
    operations: Vec<WebServiceOperation>,
    uri: Option<String>,
}

impl WebServiceBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = namespace.into();
        self
    }

    pub fn add_operation(mut self, operation: WebServiceOperation) -> Self {
        self.operations.push(operation);
        self
    }

    pub fn uri(mut self, uri: impl Into<String>) -> Self {
        self.uri = Some(uri.into());
        self
    }

    pub fn build(self) -> WebService {
        WebService {
            name: self.name,
            namespace: self.namespace,
            operations: self.operations,
            uri: self.uri,
        }
    }
}

#[derive(Debug, Default)]
pub struct WebServiceOperationBuilder {
    name: String,
    procedure_name: String,
    parameters: Vec<WebServiceParameter>,
}

impl WebServiceOperationBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn procedure_name(mut self, procedure_name: impl Into<String>) -> Self {
        self.procedure_name = procedure_name.into();
        self
    }

    pub fn add_parameter(mut self, parameter: WebServiceParameter) -> Self {
        self.parameters.push(parameter);
        self
    }

    pub fn build(self) -> WebServiceOperation {
        WebServiceOperation {
            name: self.name,
            procedure_name: self.procedure_name,
            parameters: self.parameters,
        }
    }
}
