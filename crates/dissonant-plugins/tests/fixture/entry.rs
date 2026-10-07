//! The fixture plugin, built as a real `.clap` shared object for the scanner tests to find.
//!
//! `cargo test` builds example targets, so `target/<profile>/examples/` always holds this library
//! by the time the integration tests run. The plugin itself lives next door and is shared with the
//! in-process host tests, so the two can never drift apart.

mod sine {
    include!("sine_plugin.rs");
}

clack_plugin::clack_export_entry!(sine::SineEntry);
