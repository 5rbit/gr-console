//! 구조체(UDT) 통째 읽기·쓰기 실기 탐색 — S7-1500 OPC UA 서버에서 `CMD.TaskData` 를
//! (1) 구조체 노드 하나로 읽기 · (2) 리프 노드 여럿을 한 Read 로 읽기를 비교하고,
//! `--write` 면 **방금 읽은 값을 그대로** (3) 구조체 한 번 · (4) 리프 그룹 쓰기로 되써 시간을 잰다.
//!
//! 안전: 쓰기 전과 매 쓰기 사이에 `CMD.Header` 여섯 필드가 모두 0 인지 본다(GRM 은 헤더가 채워져야 로봇으로
//! 보낸다 — `FB_CL_Comm_Robot` TaskExist). 0 이 아니면 즉시 멈춘다. Command 비트는 건드리지 않는다.
//!
//! ```text
//! cargo run -p opcua-cmd --example struct_probe -- <endpoint> <root nodeid> <node cache json> [reps] [--write] [--dump file]
//! ```

use std::time::{Duration, Instant};

use opcua::types::{AttributeId, DataValue, NodeId, NumericRange, ReadValueId, TimestampsToReturn, Variant, WriteValue};
use opcua_cmd::OpcUaConfig;

fn stats(v: &[f64]) -> String {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let avg = s.iter().sum::<f64>() / s.len().max(1) as f64;
    let p = |q: f64| s[((s.len() as f64 - 1.0) * q).round() as usize];
    format!("n={} min={:.1} avg={:.1} p50={:.1} p95={:.1} max={:.1} ms", s.len(), s[0], avg, p(0.5), p(0.95), s[s.len() - 1])
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let write = args.iter().any(|a| a == "--write");
    let pos: Vec<&String> = args.iter().skip(1).filter(|a| !a.starts_with("--")).collect();
    let (endpoint, root, cache) = (pos[0].clone(), pos[1].clone(), pos[2].clone());
    let reps: usize = pos.get(3).and_then(|s| s.parse().ok()).unwrap_or(20);

    let cache_json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&cache).expect("cache")).expect("cache json");
    let members = cache_json["members"].as_object().expect("members");
    let leaves: Vec<(String, NodeId)> =
        members.iter().filter(|(k, _)| k.starts_with("TaskData.")).map(|(k, v)| (k.clone(), v.as_str().unwrap().parse::<NodeId>().expect("node id"))).collect();
    let header: Vec<NodeId> = members.iter().filter(|(k, _)| k.starts_with("Header.")).map(|(_, v)| v.as_str().unwrap().parse::<NodeId>().unwrap()).collect();
    let task_node: NodeId = format!("{root}.\"TaskData\"").parse().expect("task node");

    let cfg = OpcUaConfig { endpoint, pki_dir: Some(std::env::temp_dir().join("gr-struct-probe-pki")), register_nodes: false, ..OpcUaConfig::default() };
    let conn = opcua_cmd::connect(&cfg).await.expect("connect");
    let s = conn.session.clone();
    println!("connected: {} ; TaskData leaves {} ; header fields {}", conn.endpoint_url, leaves.len(), header.len());

    let read = |ids: Vec<NodeId>| {
        let s = s.clone();
        async move {
            let rv: Vec<ReadValueId> = ids.into_iter().map(ReadValueId::new_value).collect();
            s.read(&rv, TimestampsToReturn::Neither, 0.0).await.expect("read")
        }
    };
    let header_zero = || {
        let header = header.clone();
        async move {
            let v = read(header).await;
            v.iter().all(|d| match &d.value {
                Some(Variant::Byte(0)) | Some(Variant::UInt16(0)) | Some(Variant::UInt32(0)) | Some(Variant::Int16(0)) | Some(Variant::SByte(0)) => true,
                other => {
                    println!("header not zero: {other:?}");
                    false
                }
            })
        }
    };

    // ---- 구조체 노드: 값의 형태
    let first = read(vec![task_node.clone()]).await.remove(0);
    let dtype = s.read(&[ReadValueId { node_id: task_node.clone(), attribute_id: AttributeId::DataType as u32, ..Default::default() }], TimestampsToReturn::Neither, 0.0).await.expect("dtype");
    match &first.value {
        Some(Variant::ExtensionObject(eo)) => {
            let dbg = format!("{eo:?}");
            println!("TaskData struct: status={:?} type_id={:?} datatype={:?}\n  body(debug, 300 chars)={}", first.status, eo.binary_type_id(), dtype[0].value, &dbg[..dbg.len().min(300)]);
        }
        other => println!("TaskData struct value is not an ExtensionObject: status={:?} value={other:?}", first.status),
    }

    // ---- 읽기 지연: 구조체 1 노드 vs 리프 51 노드(한 Read)
    let leaf_ids: Vec<NodeId> = leaves.iter().map(|(_, n)| n.clone()).collect();
    let (mut t_struct, mut t_leaf) = (Vec::new(), Vec::new());
    for _ in 0..reps {
        let t = Instant::now();
        let _ = read(vec![task_node.clone()]).await;
        t_struct.push(ms(t));
        let t = Instant::now();
        let _ = read(leaf_ids.clone()).await;
        t_leaf.push(ms(t));
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    println!("READ struct (1 node)      : {}", stats(&t_struct));
    println!("READ leaves ({} nodes)    : {}", leaf_ids.len(), stats(&t_leaf));

    // ---- 등록 노드로 같은 비교
    let reg_struct = s.register_nodes(std::slice::from_ref(&task_node)).await.ok().and_then(|v| v.into_iter().next());
    let reg_leaves = s.register_nodes(&leaf_ids).await.ok();
    if let (Some(rs), Some(rl)) = (reg_struct.clone(), reg_leaves.clone()) {
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for _ in 0..reps {
            let t = Instant::now();
            let _ = read(vec![rs.clone()]).await;
            a.push(ms(t));
            let t = Instant::now();
            let _ = read(rl.clone()).await;
            b.push(ms(t));
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        println!("READ struct registered    : {}", stats(&a));
        println!("READ leaves registered    : {}", stats(&b));
    }

    // 읽기 전용 덤프 — 구조체 원시 바이트와 리프 값(인코더 오프라인 검증용). 쓰기 없음.
    if let Some(p) = args.iter().position(|a| a == "--dump").and_then(|i| args.get(i + 1)) {
        let v = read(vec![task_node.clone()]).await.remove(0);
        let raw: Vec<u8> = match &v.value {
            // ByteStringBody.raw 는 비공개 — 디버그 표현의 바이트 배열을 읽는다(탐색 도구 전용).
            Some(Variant::ExtensionObject(eo)) => {
                let d = format!("{eo:?}");
                d.split_once("value: Some([").and_then(|(_, r)| r.split_once("])")).map(|(nums, _)| nums.split(",").filter_map(|n| n.trim().parse::<u8>().ok()).collect()).unwrap_or_default()
            }
            _ => Vec::new(),
        };
        let lv = read(leaf_ids.clone()).await;
        let leaves_json: serde_json::Map<String, serde_json::Value> = leaves.iter().zip(lv.iter()).map(|((k, _), d)| (k.clone(), serde_json::Value::String(format!("{:?}", d.value)))).collect();
        let out = serde_json::json!({ "raw": raw, "leaves": leaves_json });
        std::fs::write(p, serde_json::to_string_pretty(&out).unwrap()).expect("dump");
        println!("dumped {} raw bytes + {} leaves to {p}", raw.len(), leaves_json.len());
    }

    if !write {
        println!("(read-only run — add --write to test write-back of the same value)");
        let _ = s.disconnect().await;
        conn.event_loop.abort();
        return;
    }

    // ---- 쓰기: 같은 값을 되쓴다
    if !header_zero().await {
        println!("ABORT: CMD.Header is not all zero — a task could be relayed. Nothing written.");
        let _ = s.disconnect().await;
        conn.event_loop.abort();
        return;
    }
    let before_struct = read(vec![task_node.clone()]).await.remove(0);
    let before_leaves = read(leaf_ids.clone()).await;
    let snap = format!("{:?}", before_struct.value);
    // 운용과 같은 경로: 본문을 opcua_cmd::structs::to_variant 로 다시 싸서 쓴다(검증이 통과하면 인코더 출력 = 이 본문).
    let (enc_id, body) = before_struct.value.as_ref().and_then(opcua_cmd::structs::raw_body).expect("raw struct body");
    println!("struct body {} B, encoding {enc_id}", body.len());

    let target_struct = reg_struct.clone().unwrap_or(task_node.clone());
    let target_leaves = reg_leaves.clone().unwrap_or(leaf_ids.clone());
    let (mut w_struct, mut w_leaf) = (Vec::new(), Vec::new());
    for i in 0..reps {
        if i % 5 == 0 && !header_zero().await {
            println!("ABORT during test: header changed");
            break;
        }
        let wv = WriteValue { node_id: target_struct.clone(), attribute_id: AttributeId::Value as u32, index_range: NumericRange::None, value: DataValue::value_only(opcua_cmd::structs::to_variant(&enc_id, body.clone())) };
        let t = Instant::now();
        let r = s.write(&[wv]).await.expect("write struct");
        w_struct.push(ms(t));
        if i == 0 {
            println!("WRITE struct status: {:?}", r);
        }
        let wl: Vec<WriteValue> = target_leaves
            .iter()
            .zip(before_leaves.iter())
            .map(|(n, d)| WriteValue { node_id: n.clone(), attribute_id: AttributeId::Value as u32, index_range: NumericRange::None, value: DataValue::value_only(d.value.clone().unwrap()) })
            .collect();
        let t = Instant::now();
        let r = s.write(&wl).await.expect("write leaves");
        w_leaf.push(ms(t));
        if i == 0 {
            let bad: Vec<_> = r.iter().filter(|c| !c.is_good()).collect();
            println!("WRITE leaves: {} results, bad {:?}", r.len(), bad);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    println!("WRITE struct (1 node)     : {}", stats(&w_struct));
    println!("WRITE leaves ({} nodes)   : {}", target_leaves.len(), stats(&w_leaf));
    let after = read(vec![task_node.clone()]).await.remove(0);
    println!("value unchanged after writes: {}", format!("{:?}", after.value) == snap);
    let _ = s.disconnect().await;
    conn.event_loop.abort();
}
