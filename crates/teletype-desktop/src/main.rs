// Release builds on Windows are GUI subsystem: no console window behind the
// app. Debug builds keep the console for log output.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() {
    // §9.2 installer smoke: `teletype --self-test` runs the pipeline
    // headlessly and exits with the smoke's code, without starting the app.
    let args: Vec<String> = std::env::args().collect();
    if teletype_desktop_lib::self_test::has_self_test_flag(&args) {
        // A GUI-subsystem build has no console of its own; attach to the
        // parent's so the smoke's stdout reaches CI.
        #[cfg(windows)]
        unsafe {
            let _ = windows::Win32::System::Console::AttachConsole(
                windows::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
        std::process::exit(teletype_desktop_lib::self_test::run());
    }

    // Teardown is marked from the RunEvent::Exit hook inside run(), before
    // AppState (and the Parakeet model) is dropped. Marking it here would run
    // after the drop, which is too late to prevent the Metal-device free at
    // exit.
    teletype_desktop_lib::run();
}
