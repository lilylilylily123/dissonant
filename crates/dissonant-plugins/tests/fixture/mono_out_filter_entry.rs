//! The effect fixture with two input channels and one output channel: the host has to spread the
//! single channel the plugin produced across both sides of the mixer.

mod filter {
    #![allow(dead_code)]

    include!("filter_plugin.rs");
}

clack_plugin::clack_export_entry!(filter::MonoOutFilterEntry);
