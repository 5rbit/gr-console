use s7::testing::FakeS7Server;
use s7::{DbRead, ProbeResult, S7Client, S7Config};

async fn server(pdu: u16) -> FakeS7Server {
    let store = FakeS7Server::store();
    let srv = FakeS7Server::spawn(store, pdu).await.unwrap();
    srv.set_db(900, (0..2000u32).map(|i| (i % 251) as u8).collect());
    srv.set_db(41, vec![0xAB; 3000]);
    srv
}

fn cfg(srv: &FakeS7Server) -> S7Config {
    S7Config { host: srv.addr.ip().to_string(), port: srv.addr.port(), ..Default::default() }
}

#[tokio::test]
async fn read_chunks_at_pdu_240_and_960() {
    for pdu in [240u16, 960] {
        let srv = server(pdu).await;
        let mut c = S7Client::connect(&cfg(&srv)).await.unwrap();
        assert_eq!(c.pdu_size(), pdu);
        let all = c.read_db(900, 0, 2000).await.unwrap();
        assert_eq!(all, srv.get_db(900).unwrap());
        let part = c.read_db(900, 1990, 10).await.unwrap();
        assert_eq!(part, &srv.get_db(900).unwrap()[1990..2000]);
    }
}

#[tokio::test]
async fn read_many_packs_small_items_and_reports_item_errors() {
    let srv = server(960).await;
    let mut c = S7Client::connect(&cfg(&srv)).await.unwrap();
    let reads = vec![
        DbRead { db: 900, start: 0, len: 4 },
        DbRead { db: 900, start: 7, len: 3 }, // odd length -> pad handling
        DbRead { db: 41, start: 2999, len: 1 },
        DbRead { db: 41, start: 2999, len: 2 }, // out of range
        DbRead { db: 5, start: 0, len: 1 },     // missing DB
        DbRead { db: 900, start: 0, len: 1500 }, // larger than a PDU -> chunked read
    ];
    let res = c.read_many(&reads).await.unwrap();
    assert_eq!(res[0].as_ref().unwrap(), &srv.get_db(900).unwrap()[0..4]);
    assert_eq!(res[1].as_ref().unwrap(), &srv.get_db(900).unwrap()[7..10]);
    assert_eq!(res[2].as_ref().unwrap(), &[0xAB]);
    assert!(res[3].as_ref().unwrap_err().is_item(0x05));
    assert!(res[4].as_ref().unwrap_err().is_item(0x0A));
    assert_eq!(res[5].as_ref().unwrap().len(), 1500);
}

#[tokio::test]
async fn write_roundtrip_and_chunking() {
    let srv = server(240).await;
    let mut c = S7Client::connect(&cfg(&srv)).await.unwrap();
    let payload: Vec<u8> = (0..700u32).map(|i| (i * 7 % 256) as u8).collect();
    c.write_db(900, 100, &payload).await.unwrap();
    let back = c.read_db(900, 100, 700).await.unwrap();
    assert_eq!(back, payload);
    assert!(c.write_db(900, 1999, &[1, 2]).await.unwrap_err().is_item(0x05));
    assert!(c.write_db(77, 0, &[1]).await.unwrap_err().is_item(0x0A));
}

#[tokio::test]
async fn probe_size_exact_larger_smaller_missing() {
    let srv = server(960).await;
    let mut c = S7Client::connect(&cfg(&srv)).await.unwrap();
    assert_eq!(c.probe_db_size(900, 2000).await.unwrap(), ProbeResult::Exact);
    assert_eq!(c.probe_db_size(900, 1500).await.unwrap(), ProbeResult::Larger { actual: Some(2000) });
    assert_eq!(c.probe_db_size(900, 2500).await.unwrap(), ProbeResult::Smaller { actual: 2000 });
    assert_eq!(c.probe_db_size(123, 10).await.unwrap(), ProbeResult::Missing);
}
