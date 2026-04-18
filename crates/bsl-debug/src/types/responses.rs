use quick_xml::events::Event;
use quick_xml::Reader;

use super::xml::local_name_owned as local_name;

/// Result of attachDebugUI.
#[derive(Debug)]
pub enum AttachResult {
    Ok,
    IBNotRegistered,
    IBInDebug,
    CredentialsRequired,
    Unknown(String),
}

impl AttachResult {
    pub fn parse(data: &[u8]) -> Self {
        match read_element_text(data, "result") {
            Some(s) => match s.as_str() {
                "registered" => Self::Ok,
                "notRegistered" | "ibNotRegistered" => Self::IBNotRegistered,
                "ibInDebug" => Self::IBInDebug,
                "credentialsRequired" | "fullCredentialsRequired" => Self::CredentialsRequired,
                other => Self::Unknown(other.to_string()),
            },
            None => Self::Unknown("no result element".to_string()),
        }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok)
    }

    pub fn error_message(&self) -> Option<&str> {
        match self {
            Self::Ok => None,
            Self::IBNotRegistered => Some("infobase not registered for debugging on the server"),
            Self::IBInDebug => Some("infobase is already being debugged by another debugger"),
            Self::CredentialsRequired => Some("debug server requires authentication"),
            Self::Unknown(s) => Some(s),
        }
    }
}

/// A debug target returned by getDbgTargets.
#[derive(Debug, Clone)]
pub struct DebugTarget {
    pub id: String,
    pub seq_no: String,
    pub target_type: String,
    pub user_name: String,
}

/// Parse getDbgTargets response.
pub fn parse_targets(data: &[u8]) -> Vec<DebugTarget> {
    let mut reader = Reader::from_reader(data);
    reader.config_mut().trim_text(true);

    let mut targets = Vec::new();
    let mut in_item = false;
    let mut current = DebugTarget {
        id: String::new(),
        seq_no: String::new(),
        target_type: String::new(),
        user_name: String::new(),
    };
    let mut current_element = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = local_name(e.name().as_ref());
                if local.as_slice() == b"item" {
                    in_item = true;
                    current = DebugTarget {
                        id: String::new(),
                        seq_no: String::new(),
                        target_type: String::new(),
                        user_name: String::new(),
                    };
                } else if in_item {
                    current_element = local.to_vec();
                }
            }
            Ok(Event::Text(ref e)) if in_item => {
                let text = String::from_utf8_lossy(e.as_ref()).to_string();
                match current_element.as_slice() {
                    b"id" => current.id = text,
                    b"seqNo" => current.seq_no = text,
                    b"targetType" => current.target_type = text,
                    b"userName" => {
                        current.user_name = decode_base64_utf8(&text).unwrap_or(text);
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let local = local_name(e.name().as_ref());
                if local.as_slice() == b"item" {
                    in_item = false;
                    if !current.id.is_empty() {
                        targets.push(current.clone());
                    }
                }
                current_element.clear();
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    targets
}

/// A stack frame from getCallStack response.
#[derive(Debug, Clone)]
pub struct StackFrame {
    pub line_no: u32,
    pub presentation: String,
    pub module_extension: String,
    pub module_object_id: String,
    pub module_property_id: String,
}

/// Parse getCallStack response.
pub fn parse_call_stack(data: &[u8]) -> Vec<StackFrame> {
    let mut reader = Reader::from_reader(data);
    reader.config_mut().trim_text(true);

    let mut frames = Vec::new();
    let mut in_call_stack = false;
    let mut in_module_id = false;
    let mut current = StackFrame {
        line_no: 0,
        presentation: String::new(),
        module_extension: String::new(),
        module_object_id: String::new(),
        module_property_id: String::new(),
    };
    let mut current_element = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_slice() {
                    b"callStack" => {
                        in_call_stack = true;
                        current = StackFrame {
                            line_no: 0,
                            presentation: String::new(),
                            module_extension: String::new(),
                            module_object_id: String::new(),
                            module_property_id: String::new(),
                        };
                    }
                    b"moduleID" if in_call_stack => in_module_id = true,
                    _ if in_call_stack => current_element = local.to_vec(),
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) if in_call_stack => {
                let text = String::from_utf8_lossy(e.as_ref()).to_string();
                if in_module_id {
                    match current_element.as_slice() {
                        b"extensionName" => current.module_extension = text,
                        b"objectID" => current.module_object_id = text,
                        b"propertyID" => current.module_property_id = text,
                        _ => {}
                    }
                } else {
                    match current_element.as_slice() {
                        b"lineNo" => current.line_no = text.parse().unwrap_or(0),
                        b"presentation" => {
                            current.presentation = decode_base64_utf8(&text).unwrap_or(text);
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_slice() {
                    b"callStack" => {
                        in_call_stack = false;
                        frames.push(current.clone());
                    }
                    b"moduleID" => in_module_id = false,
                    _ => {}
                }
                current_element.clear();
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    // 1C returns stack bottom-first, reverse to get top-first
    frames.reverse();
    frames
}

/// A variable value from eval responses.
#[derive(Debug, Clone)]
pub struct VarValue {
    pub name: String,
    pub type_name: String,
    pub presentation: String,
    pub is_expandable: bool,
    pub error: Option<String>,
}

/// Parse evalLocalVariables or evalExpr response.
pub fn parse_eval_result(data: &[u8]) -> Vec<VarValue> {
    let mut reader = Reader::from_reader(data);
    reader.config_mut().trim_text(true);

    let mut vars = Vec::new();
    let mut in_prop_info = false;
    let mut in_value_info = false;
    let mut current_name = String::new();
    let mut current_type = String::new();
    let mut current_pres = String::new();
    let mut current_expandable = false;
    let mut current_element = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_slice() {
                    b"valueOfContextPropInfo" => {
                        in_prop_info = true;
                        current_name.clear();
                        current_type.clear();
                        current_pres.clear();
                        current_expandable = false;
                    }
                    b"valueInfo" if in_prop_info => in_value_info = true,
                    _ => current_element = local.to_vec(),
                }
            }
            Ok(Event::Text(ref e)) if in_prop_info => {
                let text = String::from_utf8_lossy(e.as_ref()).to_string();
                if in_value_info {
                    match current_element.as_slice() {
                        b"typeName" => current_type = text,
                        b"pres" => {
                            current_pres = decode_base64_utf8(&text).unwrap_or(text);
                        }
                        b"isExpandable" => current_expandable = text == "true",
                        _ => {}
                    }
                } else if current_element.as_slice() == b"propName" {
                    current_name = text;
                }
            }
            Ok(Event::End(ref e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_slice() {
                    b"valueOfContextPropInfo" => {
                        in_prop_info = false;
                        vars.push(VarValue {
                            name: current_name.clone(),
                            type_name: current_type.clone(),
                            presentation: current_pres.clone(),
                            is_expandable: current_expandable,
                            error: None,
                        });
                    }
                    b"valueInfo" => in_value_info = false,
                    _ => {}
                }
                current_element.clear();
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    // Also try to parse single result (evalExpr)
    if vars.is_empty() {
        if let Some(pres) = read_element_text(data, "pres") {
            let type_name = read_element_text(data, "typeName").unwrap_or_default();
            let pres = decode_base64_utf8(&pres).unwrap_or(pres);
            vars.push(VarValue {
                name: String::new(),
                type_name,
                presentation: pres,
                is_expandable: false,
                error: None,
            });
        }
    }

    vars
}

fn read_element_text(data: &[u8], target: &str) -> Option<String> {
    let mut reader = Reader::from_reader(data);
    reader.config_mut().trim_text(true);
    let target_bytes = target.as_bytes();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) if local_name(e.name().as_ref()) == target_bytes => {
                if let Ok(Event::Text(t)) = reader.read_event() {
                    return Some(String::from_utf8_lossy(t.as_ref()).to_string());
                }
                return Some(String::new());
            }
            Ok(Event::Eof) => return None,
            Err(_) => return None,
            _ => {}
        }
    }
}

fn decode_base64_utf8(s: &str) -> Option<String> {
    use std::io::Read;
    let bytes = base64_decode(s.as_bytes())?;
    // 1C uses UTF-8 or UTF-16LE for base64-encoded strings
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        // UTF-16LE BOM
        let u16s: Vec<u16> =
            bytes[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Some(String::from_utf16_lossy(&u16s))
    } else {
        // Try as raw bytes first, then lossy UTF-8
        let mut s = String::new();
        (&bytes[..]).read_to_string(&mut s).ok()?;
        Some(s)
    }
}

fn base64_decode(input: &[u8]) -> Option<Vec<u8>> {
    // Simple base64 decoder (no external dep needed)
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    fn val(c: u8) -> Option<u8> {
        TABLE.iter().position(|&b| b == c).map(|p| p as u8)
    }

    let input: Vec<u8> =
        input.iter().copied().filter(|&b| b != b'\n' && b != b'\r' && b != b' ').collect();
    let mut out = Vec::with_capacity(input.len() * 3 / 4);

    for chunk in input.chunks(4) {
        let mut buf = [0u8; 4];
        let mut len = 0;
        for (i, &b) in chunk.iter().enumerate() {
            if b == b'=' {
                break;
            }
            buf[i] = val(b)?;
            len = i + 1;
        }
        if len >= 2 {
            out.push((buf[0] << 2) | (buf[1] >> 4));
        }
        if len >= 3 {
            out.push((buf[1] << 4) | (buf[2] >> 2));
        }
        if len >= 4 {
            out.push((buf[2] << 6) | buf[3]);
        }
    }

    Some(out)
}
