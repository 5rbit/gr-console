//! `plc-link` binary: see `plc_link_server::cli`.

fn main() {
    std::process::exit(plc_link_server::cli::main());
}
