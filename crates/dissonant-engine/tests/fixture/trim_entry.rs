//! The saturator fixture held well below its knee, built as a real `.clap` shared object.
//!
//! See `saturator_plugin.rs` for the plugin itself and `boost_entry.rs` for its counterpart.

mod saturator {
    // Each build uses one of the two drive aliases; the other travels along unused.
    #![allow(dead_code)]

    include!("saturator_plugin.rs");
}

clack_plugin::clack_export_entry!(saturator::TrimEntry);
