//! `LnkJ_<Msg>` envelope writers: `{"type":"<Msg>","seq":<Seq>,"sig":<sig>,"data":<Val | null>}`.

use crate::error::LinkError;
use crate::registry::MessageSpec;
use crate::wire_json::push_name;

use super::names::Names;
use super::scl::{Code, Var, buf_var, call_raw, call_val, check_lit, fc_source, pos_var};
use super::text::GENERATED_MARKER;

/// Envelope writer source. Messages without payload have no `Val` parameter and write `"data":null`.
pub(crate) fn envelope_fc(names: &Names, m: &MessageSpec, name: &str) -> Result<String, LinkError> {
    let mut head = String::from("{\"type\":\"");
    push_name(&mut head, &m.name);
    head.push_str("\",\"seq\":");
    let mid = format!(",\"sig\":{},\"data\":", m.sig);
    let tail = if m.udt.is_some() { "}".to_string() } else { format!("{mid}null}}") };
    let path = format!("message {}", m.name);
    for lit in [&head, &mid, &tail] {
        check_lit(lit, &path)?;
    }

    let mut code = Code::new();
    match &m.udt {
        Some(u) => code.line(&format!("{GENERATED_MARKER} from message \"{}\" (id {}, \"{u}\" sig 16#{:08X}) - do not edit", m.name, m.id, m.sig)),
        None => code.line(&format!("{GENERATED_MARKER} from message \"{}\" (id {}, no payload) - do not edit", m.name, m.id)),
    }
    let data = if m.udt.is_some() { "<Val>" } else { "null" };
    code.line(&format!("// {head}<Seq>{mid}{data}}}"));
    code.line(&call_raw(&head));
    code.line("\"Json_WUInt\"(Buf := #Buf, Pos := #Pos, Val := #Seq);");
    let mut io = vec![buf_var(true), pos_var(true)];
    match &m.udt {
        Some(u) => {
            code.line(&call_raw(&mid));
            code.line(&call_val(&names.writer(u)?, "#Val"));
            code.line(&call_raw(&tail));
            io.push(Var::new("Val", format!("\"{u}\""), "data"));
        }
        None => code.line(&call_raw(&tail)),
    }
    let inputs = [Var::new("Seq", "UInt", "엔벨로프 seq (1..65535, 0 = 없음)")];
    Ok(fc_source(name, &inputs, &io, &[], &code.finish()))
}
