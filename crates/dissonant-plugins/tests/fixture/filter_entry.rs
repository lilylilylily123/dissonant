//! The effect fixture in its ordinary stereo layout, built as a real `.clap` shared object.
//!
//! See `filter_plugin.rs` for the plugin itself, and `mono_in_filter_entry.rs` /
//! `mono_out_filter_entry.rs` for the two lopsided layouts the host has to fold.

mod filter {
    // Each build uses one of the three layout aliases; the other two travel along unused.
    #![allow(dead_code)]

    include!("filter_plugin.rs");
}

clack_plugin::clack_export_entry!(filter::StereoFilterEntry);
