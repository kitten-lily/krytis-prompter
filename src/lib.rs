//! krytis-prompter: a standalone `org.gnome.keyring.SystemPrompter`.
//!
//! The library exists so the interop tests can run the protocol core
//! ([`daemon`]) on a private bus with a scripted UI; the binary is its only
//! other user.

pub mod daemon;
pub mod exchange;
pub mod gtk_ui;
pub mod theme;
pub mod ui;
