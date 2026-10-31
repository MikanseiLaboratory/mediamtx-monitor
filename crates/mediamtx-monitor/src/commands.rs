//! Tauri の `invoke` ハンドラ（薄いアダプタ）。
//!
//! 実装は `commands_core` に置き、ここでは引数の受け渡しと
//! `State<'_, AppState>` からの取り出しだけを行う。

use serde_json::Value;
use tauri::State;

use crate::commands_core::{dispatch, CmdResult};
use crate::AppState;

macro_rules! proxy {
    ($noargs:ident) => {
        #[tauri::command]
        pub fn $noargs(state: State<'_, AppState>) -> CmdResult<Value> {
            dispatch(&state.engine, stringify!($noargs), &Value::Null)
        }
    };
    ($name:ident, $arg:ident) => {
        #[tauri::command]
        pub fn $name(state: State<'_, AppState>, $arg: Value) -> CmdResult<Value> {
            let args = serde_json::json!({ stringify!($arg): $arg });
            dispatch(&state.engine, stringify!($name), &args)
        }
    };
    ($name:ident, $arg:ident, $arg2:ident) => {
        #[tauri::command]
        pub fn $name(state: State<'_, AppState>, $arg: Value, $arg2: Value) -> CmdResult<Value> {
            let args = serde_json::json!({ stringify!($arg): $arg, stringify!($arg2): $arg2 });
            dispatch(&state.engine, stringify!($name), &args)
        }
    };
}

proxy!(get_settings);
proxy!(get_monitor_status);
proxy!(get_snapshot);
proxy!(get_update, window_ms);
proxy!(get_global_config);
proxy!(start_monitor);
proxy!(stop_monitor);
proxy!(save_settings, config);
proxy!(apply_settings, config);
proxy!(test_connection, config);
proxy!(get_events, limit);
proxy!(get_series, request);
proxy!(list_paths, source);
proxy!(get_path_conf, name);
proxy!(add_path, name, conf);
proxy!(patch_path, name, diff);
proxy!(delete_path, name);
proxy!(patch_global_config, patch);
proxy!(list_connections, kind);
proxy!(kick_connection, kind, id);
proxy!(console_get, path);
proxy!(export_range, request);
