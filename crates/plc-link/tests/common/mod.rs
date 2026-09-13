#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;

use plc_layout::Contract;
use plc_link::{Codec, Registry, RegistryDoc};

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn fixture_dir() -> PathBuf {
    crate_dir().join("tests/fixtures/link")
}

pub fn fixture_contract() -> Contract {
    let c = Contract::load_dir(&fixture_dir()).expect("fixture contract");
    assert!(c.skipped.is_empty(), "fixture sources failed to parse: {:?}", c.skipped);
    c
}

pub fn fixture_doc() -> RegistryDoc {
    RegistryDoc::load(&fixture_dir().join("messages.toml")).expect("fixture registry")
}

pub fn fixture_registry() -> Registry {
    fixture_doc().resolve(&fixture_contract()).expect("resolve fixture registry")
}

pub fn fixture_codec() -> Codec {
    Codec::new(Arc::new(fixture_contract()), Arc::new(fixture_registry()))
}

pub fn gr2_contract() -> Contract {
    Contract::load_dir(&crate_dir().join("../../plc/contract/GR2_PLC")).expect("GR2_PLC contract")
}

pub fn repo_registry_doc() -> RegistryDoc {
    RegistryDoc::load(&crate_dir().join("../../plc/link/messages.toml")).expect("plc/link/messages.toml")
}
