//! The effect fixture with one input channel and two output channels: the host has to fold the
//! mixer's stereo pair down to the single channel the plugin asked for.

mod filter {
    #![allow(dead_code)]

    include!("filter_plugin.rs");
}

clack_plugin::clack_export_entry!(filter::MonoInFilterEntry);
