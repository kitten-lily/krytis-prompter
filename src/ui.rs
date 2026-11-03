//! What the protocol core asks of a prompt UI, and what it gets back.
//!
//! The core ([`crate::daemon`]) owns the D-Bus protocol and the secret
//! exchange; a UI only draws and answers. The GTK dialog is one
//! implementation, the interop tests drive another.

use std::collections::HashMap;

use zbus::OwnedVariant;
use zbus::Variant;
use zeroize::Zeroizing;

/// The two prompt types of the GCR protocol (`PerformPrompt`'s `type`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    Password,
    Confirm,
}

/// The `GcrPrompt` properties a caller sets. They persist for the whole
/// prompting session: each `PerformPrompt` sends only what changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Props {
    pub title: Option<String>,
    pub message: Option<String>,
    pub description: Option<String>,
    pub warning: Option<String>,
    pub choice_label: Option<String>,
    pub choice_chosen: bool,
    pub password_new: bool,
    pub caller_window: Option<String>,
    pub continue_label: Option<String>,
    pub cancel_label: Option<String>,
}

impl Props {
    /// Applies a `PerformPrompt` property dictionary. Unknown keys and values of
    /// the wrong type are ignored, as `GcrSystemPrompter` would fail to set them.
    pub fn update(&mut self, changed: &HashMap<String, OwnedVariant>) {
        for (key, value) in changed {
            let value: &Variant<'_> = value;
            match (key.as_str(), value) {
                ("title", Variant::Str(s)) => self.title = non_empty(s),
                ("message", Variant::Str(s)) => self.message = non_empty(s),
                ("description", Variant::Str(s)) => self.description = non_empty(s),
                ("warning", Variant::Str(s)) => self.warning = non_empty(s),
                ("choice-label", Variant::Str(s)) => self.choice_label = non_empty(s),
                ("caller-window", Variant::Str(s)) => self.caller_window = non_empty(s),
                ("continue-label", Variant::Str(s)) => self.continue_label = non_empty(s),
                ("cancel-label", Variant::Str(s)) => self.cancel_label = non_empty(s),
                ("choice-chosen", Variant::Bool(b)) => self.choice_chosen = *b,
                ("password-new", Variant::Bool(b)) => self.password_new = *b,
                _ => {}
            }
        }
    }
}

fn non_empty(s: &zbus::Str<'_>) -> Option<String> {
    (!s.is_empty()).then(|| s.to_string())
}

/// One prompt for the UI to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptRequest {
    pub kind: PromptKind,
    pub props: Props,
}

/// The user's answer to a [`PromptRequest`].
pub enum Answer {
    /// Continue on a password prompt. For `password-new`, the UI has already
    /// checked the confirmation matches.
    Password { secret: Zeroizing<String>, choice_chosen: bool },
    /// Continue on a confirm prompt.
    Confirm { choice_chosen: bool },
    /// Cancel, Escape, or the dialog was closed.
    Cancel,
}

/// Called exactly once per [`PromptUi::prompt`], unless the prompt is closed first.
pub type Reply = Box<dyn FnOnce(Answer)>;

/// A prompt UI. Every call happens on the daemon's thread.
pub trait PromptUi {
    /// Shows `request`, or updates the prompt already showing for this session
    /// (a re-prompt after a wrong password arrives this way). Must not call
    /// `reply` before returning.
    fn prompt(&self, request: &PromptRequest, reply: Reply);

    /// The session ended (the caller stopped, cancelled or disappeared). Hide
    /// the prompt; a pending `reply` will be ignored.
    fn close(&self);
}

/// The `password-strength` reported back to the caller, and drawn by the UI.
///
/// gcr only defines it as 0 for an empty password and greater than 0 otherwise
/// (`GcrPrompt:password-strength`). Above that this is a coarse 1–5 estimate
/// from length and character classes, for the strength bar.
pub fn password_strength(password: &str) -> i32 {
    if password.is_empty() {
        return 0;
    }
    let classes = [
        password.chars().any(|c| c.is_lowercase()),
        password.chars().any(|c| c.is_uppercase()),
        password.chars().any(|c| c.is_numeric()),
        password.chars().any(|c| !c.is_alphanumeric()),
    ]
    .iter()
    .filter(|&&b| b)
    .count();
    let length = password.chars().count();
    let score = match length {
        0..=5 => 1,
        6..=9 => 2,
        10..=13 => 3,
        _ => 4,
    } + i32::from(classes >= 3);
    score.clamp(1, 5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strength_is_zero_only_for_empty() {
        assert_eq!(password_strength(""), 0);
        assert_eq!(password_strength("a"), 1);
        assert_eq!(password_strength("correct horse battery staple 9!"), 5);
    }

    #[test]
    fn props_keep_unsent_values_and_clear_on_empty() {
        let mut props = Props::default();
        let mut first = HashMap::new();
        first.insert("message".to_owned(), OwnedVariant::try_from(Variant::from("Unlock")).unwrap());
        first.insert("warning".to_owned(), OwnedVariant::try_from(Variant::from("Wrong")).unwrap());
        first.insert("password-new".to_owned(), OwnedVariant::from(true));
        props.update(&first);

        let mut second = HashMap::new();
        second.insert("warning".to_owned(), OwnedVariant::try_from(Variant::from("")).unwrap());
        props.update(&second);

        assert_eq!(props.message.as_deref(), Some("Unlock"));
        assert_eq!(props.warning, None);
        assert!(props.password_new);
    }
}
