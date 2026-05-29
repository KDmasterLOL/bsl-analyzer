use quick_xml::events::Event;
use quick_xml::Reader;

use super::xml::local_name_owned as local_name;

#[derive(Debug, Clone)]
pub struct EventStackFrame {
    pub object_id: String,
    pub property_id: String,
    pub extension: String,
    pub line_no: u32,
    pub presentation: String,
}

#[derive(Debug, Clone)]
pub enum DebugEvent {
    TargetStarted {
        target_id: String,
        target_type: String,
    },
    TargetQuit {
        target_id: String,
    },
    CallStackFormed {
        target_id: String,
        stop_by_bp: bool,
        line_no: u32,
        module_extension: String,
        module_object_id: String,
        module_property_id: String,
        message: Option<String>,
        send_message_only: bool,
        call_stack: Vec<EventStackFrame>,
    },
    ExprEvaluated {
        result_id: String,
        raw_xml: Vec<u8>,
    },
    RuntimeException {
        target_id: String,
        description: String,
        line_no: u32,
        module_extension: String,
        module_object_id: String,
        module_property_id: String,
    },
}

pub fn parse_ping_events(data: &[u8]) -> Vec<DebugEvent> {
    let mut reader = Reader::from_reader(data);
    reader.config_mut().trim_text(true);

    let mut events = Vec::new();
    let mut cmd_id = String::new();
    let mut target_id = String::new();
    let mut target_type = String::new();
    let mut in_result = false;
    let mut in_module_id = false;
    let mut in_exception = false;
    let mut current_element = Vec::new();

    let mut stop_by_bp = false;
    let mut line_no = 0u32;
    let mut mod_ext = String::new();
    let mut mod_obj = String::new();
    let mut mod_prop = String::new();
    let mut message = None;
    let mut send_message_only = false;
    let mut call_stack_frames: Vec<EventStackFrame> = Vec::new();
    let mut in_call_stack = false;
    let mut in_cs_module_id = false;
    let mut cs_obj = String::new();
    let mut cs_prop = String::new();
    let mut cs_ext = String::new();
    let mut cs_line = 0u32;
    let mut cs_pres = String::new();

    let mut exception_descr = String::new();

    let mut result_id = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_slice() {
                    b"result" => {
                        in_result = true;
                        cmd_id.clear();
                        target_id.clear();
                        target_type.clear();
                        stop_by_bp = false;
                        line_no = 0;
                        mod_ext.clear();
                        mod_obj.clear();
                        mod_prop.clear();
                        message = None;
                        send_message_only = false;
                        exception_descr.clear();
                        in_exception = false;
                        result_id.clear();
                    }
                    b"callStack" if in_result => {
                        in_call_stack = true;
                        cs_obj.clear();
                        cs_prop.clear();
                        cs_ext.clear();
                        cs_line = 0;
                        cs_pres.clear();
                    }
                    b"moduleID" if in_call_stack => in_cs_module_id = true,
                    b"moduleID" if in_result => in_module_id = true,
                    b"exception" if in_result => in_exception = true,
                    _ if in_result => current_element = local.to_vec(),
                    _ => {}
                }

                if local.as_slice() == b"result" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref().ends_with(b"type") {
                            let val = String::from_utf8_lossy(attr.value.as_ref()).to_string();
                            if val.contains("CallStackFormed") {
                                cmd_id = "callStackFormed".to_string();
                            } else if val.contains("Started") {
                                cmd_id = "targetStarted".to_string();
                            } else if val.contains("Quit") {
                                cmd_id = "targetQuit".to_string();
                            } else if val.contains("ExprEvaluated") {
                                cmd_id = "exprEvaluated".to_string();
                            } else if val.contains("Rte") {
                                cmd_id = "rteProcessing".to_string();
                            }
                        }
                    }
                }
            }
            Ok(Event::Text(ref e)) if in_result => {
                let text = String::from_utf8_lossy(e.as_ref()).to_string();
                if in_cs_module_id {
                    match current_element.as_slice() {
                        b"objectID" => cs_obj = text,
                        b"propertyID" => cs_prop = text,
                        b"extensionName" => cs_ext = text,
                        _ => {}
                    }
                } else if in_call_stack {
                    match current_element.as_slice() {
                        b"lineNo" => cs_line = text.parse().unwrap_or(0),
                        b"presentation" => cs_pres = text,
                        _ => {}
                    }
                } else if in_module_id {
                    match current_element.as_slice() {
                        b"extensionName" => mod_ext = text,
                        b"objectID" => mod_obj = text,
                        b"propertyID" => mod_prop = text,
                        _ => {}
                    }
                } else if in_exception {
                    if current_element.as_slice() == b"descr" {
                        exception_descr = text;
                    }
                } else {
                    match current_element.as_slice() {
                        b"cmdID" => cmd_id = text,
                        b"id" if target_id.is_empty() => {
                            target_id = text;
                        }
                        b"targetType" => target_type = text,
                        b"stopByBP" => stop_by_bp = text == "true",
                        b"lineNo" => line_no = text.parse().unwrap_or(0),
                        b"sendMessageOnly" => send_message_only = text == "true",
                        b"expressionResultID" => result_id = text,
                        _ => {}
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_slice() {
                    b"result" => {
                        in_result = false;
                        let event = match cmd_id.as_str() {
                            "targetStarted" => Some(DebugEvent::TargetStarted {
                                target_id: target_id.clone(),
                                target_type: target_type.clone(),
                            }),
                            "targetQuit" => {
                                Some(DebugEvent::TargetQuit { target_id: target_id.clone() })
                            }
                            "callStackFormed" => Some(DebugEvent::CallStackFormed {
                                target_id: target_id.clone(),
                                stop_by_bp,
                                line_no,
                                module_extension: mod_ext.clone(),
                                module_object_id: mod_obj.clone(),
                                module_property_id: mod_prop.clone(),
                                message: message.clone(),
                                send_message_only,
                                call_stack: std::mem::take(&mut call_stack_frames),
                            }),
                            "exprEvaluated" => Some(DebugEvent::ExprEvaluated {
                                result_id: result_id.clone(),
                                raw_xml: data.to_vec(),
                            }),
                            "rteProcessing" => Some(DebugEvent::RuntimeException {
                                target_id: target_id.clone(),
                                description: exception_descr.clone(),
                                line_no,
                                module_extension: mod_ext.clone(),
                                module_object_id: mod_obj.clone(),
                                module_property_id: mod_prop.clone(),
                            }),
                            _ => None,
                        };
                        if let Some(ev) = event {
                            events.push(ev);
                        }
                    }
                    b"callStack" if in_call_stack => {
                        let pres = base64_decode_utf8(&cs_pres);
                        call_stack_frames.push(EventStackFrame {
                            object_id: cs_obj.clone(),
                            property_id: cs_prop.clone(),
                            extension: cs_ext.clone(),
                            line_no: cs_line,
                            presentation: pres,
                        });
                        in_call_stack = false;
                        in_cs_module_id = false;
                    }
                    b"moduleID" => {
                        if in_cs_module_id {
                            in_cs_module_id = false;
                        } else {
                            in_module_id = false;
                        }
                    }
                    b"exception" => in_exception = false,
                    _ => {}
                }
                current_element.clear();
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    events
}

fn base64_decode_utf8(encoded: &str) -> String {
    let clean: String = encoded.chars().filter(|c| !c.is_whitespace()).collect();
    let mut buf = vec![0u8; clean.len()];
    let len = base64_decode_slice(clean.as_bytes(), &mut buf);
    String::from_utf8(buf[..len].to_vec()).unwrap_or_default()
}

fn base64_decode_slice(input: &[u8], output: &mut [u8]) -> usize {
    const TABLE: [u8; 256] = {
        let mut t = [0xFFu8; 256];
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut i = 0;
        while i < 64 {
            t[alphabet[i] as usize] = i as u8;
            i += 1;
        }
        t
    };
    let mut si = 0;
    let mut di = 0;
    while si + 3 < input.len() {
        let (a, b, c, d) = (
            TABLE[input[si] as usize],
            TABLE[input[si + 1] as usize],
            TABLE[input[si + 2] as usize],
            TABLE[input[si + 3] as usize],
        );
        if a == 0xFF {
            break;
        }
        output[di] = (a << 2) | (b >> 4);
        di += 1;
        if input[si + 2] != b'=' {
            output[di] = (b << 4) | (c >> 2);
            di += 1;
        }
        if input[si + 3] != b'=' {
            output[di] = (c << 6) | d;
            di += 1;
        }
        si += 4;
    }
    di
}
