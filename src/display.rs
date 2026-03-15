// src/display.rs
//
// Display-related types for the on-screen keyboard UI.

use fltk::{
    button::Button,
    enums::Color,
    frame::Frame,
};

use crate::config;
use crate::keyboards::Action;

// =============================================================================
// UI colour palette (resolved from config)
// =============================================================================

/// All UI colours resolved from [`config::ColorsConfig`] into FLTK [`Color`] values.
/// Implements `Copy` because [`Color`] is a newtype over `u32`.
#[derive(Clone, Copy)]
pub struct Colors {
    pub key_normal:              Color,
    pub key_mod:                 Color,
    pub mod_active:              Color,
    pub nav_sel:                 Color,
    pub status_bar_bg:           Color,
    pub status_ind_bg:           Color,
    pub status_ind_text:         Color,
    pub status_ind_active_text:  Color,
    pub conn_disconnected:       Color,
    pub conn_connecting:         Color,
    pub conn_connected:          Color,
    pub win_bg:                  Color,
    pub disp_bg:                 Color,
    pub disp_text:               Color,
    pub lang_btn_inactive:       Color,
    pub lang_btn_label:          Color,
    pub key_label_normal:        Color,
    pub key_label_mod:           Color,
}

impl Colors {
    pub fn from_config(cfg: &config::ColorsConfig) -> Self {
        let c = |rgb: &config::ColorRgb| Color::from_rgb(rgb.0, rgb.1, rgb.2);
        Colors {
            key_normal:              c(&cfg.key_normal),
            key_mod:                 c(&cfg.key_mod),
            mod_active:              c(&cfg.mod_active),
            nav_sel:                 c(&cfg.nav_sel),
            status_bar_bg:           c(&cfg.status_bar_bg),
            status_ind_bg:           c(&cfg.status_ind_bg),
            status_ind_text:         c(&cfg.status_ind_text),
            status_ind_active_text:  c(&cfg.status_ind_active_text),
            conn_disconnected:       c(&cfg.conn_disconnected),
            conn_connecting:         c(&cfg.conn_connecting),
            conn_connected:          c(&cfg.conn_connected),
            win_bg:                  c(&cfg.win_bg),
            disp_bg:                 c(&cfg.disp_bg),
            disp_text:               c(&cfg.disp_text),
            lang_btn_inactive:       c(&cfg.lang_btn_inactive),
            lang_btn_label:          c(&cfg.lang_btn_label),
            key_label_normal:        c(&cfg.key_label_normal),
            key_label_mod:           c(&cfg.key_label_mod),
        }
    }
}

// =============================================================================
// Modifier button descriptor
// =============================================================================

/// A modifier-key button together with its action and base (inactive) color.
/// Stored in a shared list so execute_action can update visual state.
pub struct ModBtn {
    pub btn:      Button,
    pub action:   Action,
    pub base_col: Color,
    /// Corresponding status-bar indicator frame (shared between LShift & RShift).
    pub status:   Option<Frame>,
}
