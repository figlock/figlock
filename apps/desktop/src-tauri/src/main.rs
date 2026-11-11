// Release builds on Windows are a GUI app, not a console one.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    figlock_desktop::run();
}
