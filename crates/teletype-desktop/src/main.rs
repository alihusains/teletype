fn main() {
    // Teardown is marked from the RunEvent::Exit hook inside run(), before
    // AppState (and the Parakeet model) is dropped. Marking it here would run
    // after the drop, which is too late to prevent the Metal-device free at
    // exit.
    teletype_desktop_lib::run();
}
