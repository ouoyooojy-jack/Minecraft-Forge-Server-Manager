// Release builds detach from the console; debug builds keep it so `println!`
// and panics are visible while developing.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    mc_server_manager_lib::run()
}
