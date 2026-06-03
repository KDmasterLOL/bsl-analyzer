#![deny(rust_2018_idioms)]

use std::sync::Arc;

use bsl_metadata::{Configuration, Name};

#[derive(Clone, Debug)]
pub struct VisibleConfig {
    pub name: Option<String>,
    pub configuration: Arc<Configuration>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum ConfigId {
    Root,
    Resolved(u32),
    Unknown(Name),
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::Configuration;

    #[test]
    fn config_id_round_trip() {
        let root = ConfigId::Root;
        let res = ConfigId::Resolved(7);
        let unk = ConfigId::Unknown("Контрагенты".to_string());

        assert_eq!(root, ConfigId::Root);
        assert_eq!(res, ConfigId::Resolved(7));
        assert_ne!(res, ConfigId::Resolved(8));
        assert_eq!(unk, ConfigId::Unknown("Контрагенты".to_string()));
        assert_ne!(unk, ConfigId::Unknown("Номенклатура".to_string()));
    }

    #[test]
    fn visible_config_round_trip() {
        let cfg = Arc::new(Configuration::new("test"));
        let main = VisibleConfig { name: None, configuration: cfg.clone() };
        let ext = VisibleConfig { name: Some("ExtA".into()), configuration: cfg };

        assert!(main.name.is_none());
        assert_eq!(ext.name.as_deref(), Some("ExtA"));
        assert!(Arc::ptr_eq(&main.configuration, &ext.configuration));
    }
}
