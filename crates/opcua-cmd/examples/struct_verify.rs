//! 운용 경로의 구조체 검증만 실기에서 돌린다(읽기 전용) — `CmdWriter` 를 `struct_write = false` 로 띄워 세션 튜닝
//! (`RegisterNodes` + 구조체·리프 한 Read 비교)이 끝나면 `IoStats` 의 검증 결과를 찍고 닫는다. 아무것도 쓰지 않는다.
//!
//! ```text
//! cargo run -p opcua-cmd --example struct_verify -- <endpoint> <server root, e.g. GR[1].CMD> <node cache json> [spec json]
//! ```
//! 노드 캐시는 임시 사본으로 쓴다(원본을 고치지 않는다). spec 기본값은 옆의 `taskdata.spec.json`(계약에서 생성·검사됨).

use std::time::Duration;

use opcua_cmd::structs::StructSpec;
use opcua_cmd::{CmdWriter, OpcState, OpcUaConfig};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (endpoint, root, cache) = (args[0].clone(), args[1].clone(), args[2].clone());
    let spec_path = args.get(3).cloned().unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/examples/taskdata.spec.json").to_string());
    let spec: StructSpec = serde_json::from_str(&std::fs::read_to_string(&spec_path).expect("spec")).expect("spec json");

    let tmp = std::env::temp_dir().join("gr-struct-verify");
    let _ = std::fs::create_dir_all(&tmp);
    let cache_copy = tmp.join("nodes.json");
    std::fs::copy(&cache, &cache_copy).expect("copy node cache");

    let bases = opcua_cmd::array_bases_from_paths(spec.leaves().map(|(p, _)| p));
    let cfg = OpcUaConfig {
        endpoint,
        root_path: root,
        node_cache: Some(cache_copy),
        pki_dir: Some(tmp.join("pki")),
        array_bases: bases,
        register_nodes: true,
        struct_specs: vec![spec],
        struct_write: false,
        ..OpcUaConfig::default()
    };
    let (writer, mut rx) = CmdWriter::spawn(cfg);
    let ready = tokio::time::timeout(Duration::from_secs(20), async { rx.wait_for(|s| matches!(s, OpcState::Ready { .. })).await.map(|_| ()) }).await;
    println!("state: {:?}", writer.state());
    if ready.is_err() {
        println!("not ready within 20 s");
    }
    // 튜닝은 Ready 직후 끝나 있다(같은 루프에서 set_tuned 후 상태 전환) — 여유를 조금 둔다.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let st = writer.stats();
    println!("registered={} max_read={} max_write={}", st.registered, st.max_nodes_per_read, st.max_nodes_per_write);
    println!("struct_verified={:?} struct_active={:?} note={:?}", st.struct_verified, st.struct_active, st.struct_note);
    writer.shutdown();
    let _ = tokio::time::timeout(Duration::from_secs(3), rx.wait_for(|s| matches!(s, OpcState::Disconnected))).await;
}
