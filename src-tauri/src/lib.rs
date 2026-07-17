mod commands;
mod state;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use state::AppState;
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(Arc::new(AppState {
            emulator: Mutex::new(m6809_core::Emulator::new()),
            running: Arc::new(AtomicBool::new(false)),
            trace: Mutex::new(Vec::new()),
            run_speed: Mutex::new(state::RunSpeed::default()),
            trace_limit: Mutex::new(200),
        }))
        .invoke_handler(tauri::generate_handler![
            commands::reset_emulator,
            commands::step,
            commands::run_emulator,
            commands::is_emulator_running,
            commands::pause_emulator,
            commands::get_cpu_state,
            commands::get_memory,
            commands::write_memory,
            commands::load_binary_file,
            commands::load_binary_bytes,
            commands::export_binary_file,
            commands::assemble_source,
            commands::disassemble_range,
            commands::set_breakpoint,
            commands::clear_breakpoint,
            commands::set_load_config,
            commands::get_trace,
            commands::clear_trace,
            commands::set_cpu_register,
            commands::toggle_cpu_flag,
            commands::get_breakpoints,
            commands::clear_all_breakpoints,
            commands::trigger_irq,
            commands::trigger_firq,
            commands::trigger_nmi,
            commands::set_run_speed,
            commands::set_trace_limit,
            commands::set_watchpoint,
            commands::clear_watchpoint,
            commands::get_watchpoints,
            commands::clear_all_watchpoints,
            commands::save_session_file,
            commands::load_session_file,
            commands::list_machine_profiles,
            commands::get_machine_state,
            commands::get_video_frame,
            commands::get_acia_config_cmd,
            commands::set_acia_config_cmd,
            commands::get_acia_terminal_cmd,
            commands::acia_send_input_cmd,
            commands::acia_run_steps_cmd,
            commands::acia_send_and_run_cmd,
            commands::clear_acia_terminal_cmd,
            commands::set_machine_profile,
            commands::machine_key_event,
            commands::machine_keys_clear,
            commands::get_cpu_variant,
            commands::set_cpu_variant,
            commands::get_pia_config_cmd,
            commands::set_pia_config_cmd,
            commands::get_pia_state_cmd,
            commands::set_pia_input_cmd,
            commands::get_ay_config_cmd,
            commands::set_ay_config_cmd,
            commands::get_ay_state_cmd,
            commands::set_ay_port_input_cmd,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}