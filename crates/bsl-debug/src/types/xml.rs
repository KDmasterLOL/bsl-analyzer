use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
use std::io::Cursor;

const NS_REQUEST: &str = "http://v8.1c.ru/8.3/debugger/debugRDBGRequestResponse";
const NS_BASE: &str = "http://v8.1c.ru/8.3/debugger/debugBaseData";
const NS_BP: &str = "http://v8.1c.ru/8.3/debugger/debugBreakpoints";
const NS_CALC: &str = "http://v8.1c.ru/8.3/debugger/debugCalculations";
const NS_AUTO: &str = "http://v8.1c.ru/8.3/debugger/debugAutoAttach";
const NS_RTE: &str = "http://v8.1c.ru/8.3/debugger/debugRTEFilter";

pub struct XmlRequestBuilder {
    pub(crate) writer: Writer<Cursor<Vec<u8>>>,
}

impl XmlRequestBuilder {
    pub fn new(request_type: &str, debugger_id: &str, infobase: &str) -> Self {
        let mut writer = Writer::new(Cursor::new(Vec::new()));

        let mut root = BytesStart::new("request");
        root.push_attribute(("xmlns", NS_REQUEST));
        root.push_attribute(("xmlns:xs", "http://www.w3.org/2001/XMLSchema"));
        root.push_attribute(("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"));
        root.push_attribute(("xsi:type", request_type));

        writer.write_event(Event::Start(root)).unwrap();

        write_text_element(&mut writer, "idOfDebuggerUI", debugger_id);
        write_text_element(&mut writer, "infoBaseAlias", infobase);

        Self { writer }
    }

    pub fn text_element(&mut self, name: &str, value: &str) -> &mut Self {
        write_text_element(&mut self.writer, name, value);
        self
    }

    pub fn bool_element(&mut self, name: &str, value: bool) -> &mut Self {
        write_text_element(&mut self.writer, name, if value { "true" } else { "false" });
        self
    }

    pub fn int_element(&mut self, name: &str, value: i64) -> &mut Self {
        write_text_element(&mut self.writer, name, &value.to_string());
        self
    }

    pub fn start(&mut self, name: &str) -> &mut Self {
        self.writer.write_event(Event::Start(BytesStart::new(name))).unwrap();
        self
    }

    pub fn start_ns(&mut self, name: &str, ns: &str) -> &mut Self {
        let mut el = BytesStart::new(name);
        el.push_attribute(("xmlns", ns));
        self.writer.write_event(Event::Start(el)).unwrap();
        self
    }

    pub fn start_with_child_ns(&mut self, name: &str, prefix: &str, ns: &str) -> &mut Self {
        let mut el = BytesStart::new(name);
        el.push_attribute((format!("xmlns:{prefix}").as_str(), ns));
        self.writer.write_event(Event::Start(el)).unwrap();
        self
    }

    pub fn prefixed_text(&mut self, prefix: &str, name: &str, value: &str) -> &mut Self {
        let qname = format!("{prefix}:{name}");
        write_text_element(&mut self.writer, &qname, value);
        self
    }

    pub fn prefixed_bool(&mut self, prefix: &str, name: &str, value: bool) -> &mut Self {
        self.prefixed_text(prefix, name, if value { "true" } else { "false" })
    }

    pub fn start_prefixed(&mut self, prefix: &str, name: &str) -> &mut Self {
        let qname = format!("{prefix}:{name}");
        self.writer.write_event(Event::Start(BytesStart::new(&qname))).unwrap();
        self
    }

    pub fn end_prefixed(&mut self, prefix: &str, name: &str) -> &mut Self {
        let qname = format!("{prefix}:{name}");
        self.writer.write_event(Event::End(BytesEnd::new(&qname))).unwrap();
        self
    }

    pub fn end(&mut self, name: &str) -> &mut Self {
        self.writer.write_event(Event::End(BytesEnd::new(name))).unwrap();
        self
    }

    pub fn write_module_id(
        &mut self,
        wrapper: &str,
        extension: &str,
        object_id: &str,
        property_id: &str,
    ) -> &mut Self {
        self.start(wrapper);

        let mod_type = if extension.is_empty() { "ConfigModule" } else { "ExtensionModule" };
        self.text_element("type", mod_type);
        self.text_element("URL", "");
        self.text_element("extensionName", extension);
        self.text_element("objectID", object_id);
        self.text_element("propertyID", property_id);
        self.int_element("extId", 0);

        self.end(wrapper)
    }

    pub fn write_target_id_light(&mut self, wrapper: &str, target_id_str: &str) -> &mut Self {
        let mut el = BytesStart::new(wrapper);
        el.push_attribute(("xmlns:dbg", NS_BASE));
        el.push_attribute(("xsi:type", "dbg:DebugTargetIdLight"));
        self.writer.write_event(Event::Start(el)).unwrap();
        write_text_element_ns(&mut self.writer, "id", target_id_str, NS_BASE);
        self.end(wrapper)
    }

    pub fn build(mut self) -> Vec<u8> {
        self.writer.write_event(Event::End(BytesEnd::new("request"))).unwrap();
        self.writer.into_inner().into_inner()
    }
}

fn write_text_element(writer: &mut Writer<Cursor<Vec<u8>>>, name: &str, value: &str) {
    writer.write_event(Event::Start(BytesStart::new(name))).unwrap();
    writer.write_event(Event::Text(BytesText::new(value))).unwrap();
    writer.write_event(Event::End(BytesEnd::new(name))).unwrap();
}

fn write_text_element_ns(writer: &mut Writer<Cursor<Vec<u8>>>, name: &str, value: &str, ns: &str) {
    let mut el = BytesStart::new(name);
    el.push_attribute(("xmlns", ns));
    writer.write_event(Event::Start(el)).unwrap();
    writer.write_event(Event::Text(BytesText::new(value))).unwrap();
    writer.write_event(Event::End(BytesEnd::new(name))).unwrap();
}

pub struct XmlResponseReader<'a> {
    reader: Reader<&'a [u8]>,
}

impl<'a> XmlResponseReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        let mut reader = Reader::from_reader(data);
        reader.config_mut().trim_text(true);
        Self { reader }
    }

    pub fn read_text(&mut self, target: &str) -> Option<String> {
        let target_bytes = target.as_bytes();
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = e.name();
                    let local = local_name(name.as_ref());
                    if local == target_bytes {
                        return self.read_inner_text();
                    }
                }
                Ok(Event::Eof) => return None,
                Err(_) => return None,
                _ => {}
            }
        }
    }

    fn read_inner_text(&mut self) -> Option<String> {
        match self.reader.read_event() {
            Ok(Event::Text(e)) => Some(String::from_utf8_lossy(e.as_ref()).to_string()),
            _ => Some(String::new()),
        }
    }
}

pub fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().position(|&b| b == b':') {
        Some(pos) => &name[pos + 1..],
        None => name,
    }
}

pub fn local_name_owned(name: &[u8]) -> Vec<u8> {
    local_name(name).to_vec()
}

pub fn build_attach_request(debugger_id: &str, infobase: &str) -> Vec<u8> {
    let mut b = XmlRequestBuilder::new("RDBGAttachDebugUIRequest", debugger_id, infobase);
    b.start("options").bool_element("foregroundAbility", true).end("options");
    b.build()
}

pub fn build_detach_request(debugger_id: &str, infobase: &str) -> Vec<u8> {
    XmlRequestBuilder::new("RDBGDetachDebugUIRequest", debugger_id, infobase).build()
}

pub fn build_init_settings_request(debugger_id: &str, infobase: &str) -> Vec<u8> {
    let mut b = XmlRequestBuilder::new("RDBGSetInitialDebugSettingsRequest", debugger_id, infobase);
    b.start("data").end("data");
    b.build()
}

pub fn build_set_auto_attach_request(
    debugger_id: &str,
    infobase: &str,
    target_types: &[&str],
) -> Vec<u8> {
    let mut b = XmlRequestBuilder::new("RDBGSetAutoAttachSettingsRequest", debugger_id, infobase);
    b.start("autoAttachSettings");
    for tt in target_types {
        write_text_element_ns(&mut b.writer, "targetType", tt, NS_AUTO);
    }
    b.end("autoAttachSettings");
    b.build()
}

pub fn build_get_targets_request(debugger_id: &str, infobase: &str) -> Vec<u8> {
    XmlRequestBuilder::new("RDBGSGetDbgTargetsRequest", debugger_id, infobase).build()
}

pub fn build_attach_targets_request(
    debugger_id: &str,
    infobase: &str,
    attach: bool,
    target_ids: &[&str],
) -> Vec<u8> {
    let mut b =
        XmlRequestBuilder::new("RDBGAttachDetachDebugTargetsRequest", debugger_id, infobase);
    b.bool_element("attach", attach);
    for tid in target_ids {
        b.start("id");
        write_text_element_ns(&mut b.writer, "id", tid, NS_BASE);
        b.end("id");
    }
    b.build()
}

pub fn build_set_breakpoints_request(
    debugger_id: &str,
    infobase: &str,
    breakpoints: &[BreakpointDef],
) -> Vec<u8> {
    let mut b = XmlRequestBuilder::new("RDBGSetBreakpointsRequest", debugger_id, infobase);

    let mut modules: std::collections::HashMap<(&str, &str, &str), Vec<&BreakpointDef>> =
        std::collections::HashMap::new();
    for bp in breakpoints {
        modules.entry((&bp.extension, &bp.object_id, &bp.property_id)).or_default().push(bp);
    }

    b.start("bpWorkspace");
    for ((ext, obj_id, prop_id), bps) in &modules {
        let mut el = BytesStart::new("moduleBPInfo");
        el.push_attribute(("xmlns", NS_BP));
        el.push_attribute(("xmlns:base", NS_BASE));
        b.writer.write_event(Event::Start(el)).unwrap();

        b.start("id");
        let mod_type = if ext.is_empty() { "ConfigModule" } else { "ExtensionModule" };
        b.prefixed_text("base", "type", mod_type);
        b.prefixed_text("base", "URL", "");
        b.prefixed_text("base", "extensionName", ext);
        b.prefixed_text("base", "objectID", obj_id);
        b.prefixed_text("base", "propertyID", prop_id);
        b.prefixed_text("base", "extId", "0");
        b.end("id");

        for bp in bps {
            b.start("bpInfo");
            b.int_element("line", bp.line as i64);
            b.bool_element("isActive", true);
            if let Some(ref cond) = bp.condition {
                b.text_element("condition", cond);
                b.bool_element("breakOnCondition", true);
            }
            b.end("bpInfo");
        }
        b.end("moduleBPInfo");
    }
    b.end("bpWorkspace");

    b.build()
}

pub fn build_set_break_on_rte_request(
    debugger_id: &str,
    infobase: &str,
    stop: bool,
    filter: Option<&str>,
) -> Vec<u8> {
    let mut b =
        XmlRequestBuilder::new("RDBGSetRunTimeErrorProcessingRequest", debugger_id, infobase);
    b.start_with_child_ns("state", "rte", NS_RTE);
    b.prefixed_bool("rte", "stopOnErrors", stop);
    if let Some(text) = filter {
        b.prefixed_bool("rte", "analyzeErrorStr", true);
        b.start_prefixed("rte", "strTemplate");
        b.prefixed_bool("rte", "include", true);
        b.prefixed_text("rte", "str", text);
        b.end_prefixed("rte", "strTemplate");
    }
    b.end("state");
    b.build()
}

pub fn build_step_request(
    debugger_id: &str,
    infobase: &str,
    target_id: &str,
    action: &str,
) -> Vec<u8> {
    let mut b = XmlRequestBuilder::new("RDBGStepRequest", debugger_id, infobase);
    b.write_target_id_light("targetID", target_id);
    b.text_element("action", action);
    b.build()
}

pub fn build_get_callstack_request(debugger_id: &str, infobase: &str, target_id: &str) -> Vec<u8> {
    let mut b = XmlRequestBuilder::new("RDBGGetCallStackRequest", debugger_id, infobase);
    b.write_target_id_light("targetID", target_id);
    b.build()
}

pub fn build_eval_expr_request(
    debugger_id: &str,
    infobase: &str,
    target_id: &str,
    expression: &str,
    stack_level: i64,
    result_id: &str,
) -> Vec<u8> {
    let mut b = XmlRequestBuilder::new("RDBGEvalExprRequest", debugger_id, infobase);
    b.write_target_id_light("targetID", target_id);
    b.int_element("calcWaitingTime", 100);
    b.start_with_child_ns("expr", "calc", NS_CALC);

    b.prefixed_text("calc", "stackLevel", &stack_level.to_string());
    b.start_prefixed("calc", "srcCalcInfo");
    b.prefixed_text("calc", "expressionResultID", result_id);
    b.start_prefixed("calc", "calcItem");
    b.prefixed_text("calc", "itemType", "expression");
    b.prefixed_text("calc", "expression", expression);
    b.end_prefixed("calc", "calcItem");
    b.end_prefixed("calc", "srcCalcInfo");

    b.start_prefixed("calc", "presOptions");
    b.prefixed_text("calc", "maxTextSize", "307200");
    b.end_prefixed("calc", "presOptions");

    b.end("expr");
    b.build()
}

pub fn build_eval_local_vars_request(
    debugger_id: &str,
    infobase: &str,
    target_id: &str,
    stack_level: i64,
    result_id: &str,
) -> Vec<u8> {
    let mut b = XmlRequestBuilder::new("RDBGEvalLocalVariablesRequest", debugger_id, infobase);
    b.write_target_id_light("targetID", target_id);
    b.int_element("calcWaitingTime", 100);
    b.start_with_child_ns("expr", "calc", NS_CALC);

    b.prefixed_text("calc", "stackLevel", &stack_level.to_string());
    b.start_prefixed("calc", "srcCalcInfo");
    b.prefixed_text("calc", "expressionResultID", result_id);
    b.end_prefixed("calc", "srcCalcInfo");

    b.start_prefixed("calc", "presOptions");
    b.prefixed_text("calc", "maxTextSize", "307200");
    b.end_prefixed("calc", "presOptions");

    b.end("expr");
    b.build()
}

pub fn build_eval_expand_request(
    debugger_id: &str,
    infobase: &str,
    target_id: &str,
    path: &[crate::types::base::CalcPathItem],
    view: crate::types::base::ViewInterface,
    stack_level: i64,
    result_id: &str,
) -> Vec<u8> {
    use crate::types::base::{CalcPathItem, ViewInterface};

    let mut b = XmlRequestBuilder::new("RDBGEvalExprRequest", debugger_id, infobase);
    b.write_target_id_light("targetID", target_id);
    b.int_element("calcWaitingTime", 100);
    b.start_with_child_ns("expr", "calc", NS_CALC);

    b.prefixed_text("calc", "stackLevel", &stack_level.to_string());
    b.start_prefixed("calc", "srcCalcInfo");
    b.prefixed_text("calc", "expressionResultID", result_id);

    for item in path {
        b.start_prefixed("calc", "calcItem");
        match item {
            CalcPathItem::Expression(name) => {
                b.prefixed_text("calc", "itemType", "expression");
                b.prefixed_text("calc", "expression", name);
            }
            CalcPathItem::Property(name) => {
                b.prefixed_text("calc", "itemType", "property");
                b.prefixed_text("calc", "property", name);
            }
            CalcPathItem::Index(idx) => {
                b.prefixed_text("calc", "itemType", "index");
                b.prefixed_text("calc", "index", &idx.to_string());
            }
        }
        b.end_prefixed("calc", "calcItem");
    }

    let iface = match view {
        ViewInterface::Context => "context",
        ViewInterface::Collection => "collection",
        ViewInterface::None => "none",
    };
    b.prefixed_text("calc", "interfaces", iface);
    b.end_prefixed("calc", "srcCalcInfo");

    b.start_prefixed("calc", "presOptions");
    b.prefixed_text("calc", "maxTextSize", "307200");
    b.end_prefixed("calc", "presOptions");

    b.end("expr");
    b.build()
}

pub struct BreakpointDef {
    pub extension: String,
    pub object_id: String,
    pub property_id: String,
    pub line: u32,
    pub condition: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_attach_xml() {
        let xml = build_set_auto_attach_request(
            "test-id",
            "test-db",
            &["Client", "Server", "HTTPService"],
        );
        let s = String::from_utf8(xml).unwrap();
        assert!(
            s.contains("<autoAttachSettings>"),
            "autoAttachSettings must NOT have xmlns (1C XDTO). Got: {s}"
        );
        assert!(s.contains(r#"<targetType xmlns="http://v8.1c.ru/8.3/debugger/debugAutoAttach">HTTPService</targetType>"#),
            "targetType must have xmlns. Got: {s}");
        assert!(s.contains(r#"<targetType xmlns="http://v8.1c.ru/8.3/debugger/debugAutoAttach">Client</targetType>"#),
            "all targetType elements must have xmlns. Got: {s}");
    }
}
