//! The saturator fixture driven hard, built as a real `.clap` shared object.
//!
//! See `saturator_plugin.rs` for the plugin itself and `trim_entry.rs` for its counterpart: the
//! two together are what makes the order of an insert chain observable.

mod saturator {
    // Each build uses one of the two drive aliases; the other travels along unused.
    #![allow(dead_code)]

    include!("saturator_plugin.rs");
}

clack_plugin::clack_export_entry!(saturator::BoostEntry);
