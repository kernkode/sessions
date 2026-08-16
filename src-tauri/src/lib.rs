//! Sessions: a desktop console to manage several CLI agent sessions.

pub mod app;
pub mod commands;
pub mod config;
pub mod git;
pub mod launcher;
pub mod metrics;
pub mod model;
pub mod paths;
pub mod pty;
pub mod store;

use std::sync::Arc;

use tauri::Manager;

use crate::app::AppState;
use crate::config::ConfigStore;
use crate::paths::Paths;
use crate::store::Store;

pub fn run() {
    let paths = match Paths::resolve() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Sessions: no se pudo resolver ~/.sessions: {e}");
            return;
        }
    };
    if let Err(e) = paths.bootstrap() {
        eprintln!("Sessions: no se pudo preparar {}: {e}", paths.root.display());
    }

    let config = match ConfigStore::load(paths.clone()) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("Sessions: error cargando configuración: {e}");
            return;
        }
    };
    let store = Arc::new(Store::load(paths));
    // Processes do not survive a restart: no saved session is still running.
    store.reset_runtime_state();
    store.prune_scrollback();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(commands::handler())
        .setup(move |app| {
            let state = AppState::new(app.handle().clone(), config.clone(), store.clone());
            app.manage(state);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    state.shutdown();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error al arrancar Sessions");
}
