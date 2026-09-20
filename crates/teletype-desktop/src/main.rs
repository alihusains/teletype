fn main() {
    teletype_desktop_lib::run();
    // Mark teardown before AppState drops: the Parakeet provider must not
    // free its C context (and the global Metal device) after the run loop
    // has ended — ggml_metal_rsets_free aborts there.
    teletype_speech::parakeet::mark_teardown();
}
