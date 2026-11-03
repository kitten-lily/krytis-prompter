//! The GTK4 prompt dialog.
//!
//! On a compositor with wlr-layer-shell (niri, umbriel) it is an overlay layer
//! surface with exclusive keyboard focus, so it can't open behind a fullscreen
//! window and every key goes to it. Elsewhere it falls back to a normal
//! toplevel; its app-id is `krytis-prompter` for a window rule.
//! docs/skills/ui.md § Layer shell.

use std::{cell::RefCell, rc::Rc};

use gtk::{gdk, glib, prelude::*};
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};
use zeroize::Zeroizing;

use crate::theme::Look;
use crate::ui::{Answer, PromptKind, PromptRequest, PromptUi, Reply, password_strength};

#[derive(Default)]
pub struct GtkUi {
    dialog: RefCell<Option<Rc<Dialog>>>,
    /// Added to the display once; its CSS is reloaded for each new session, so
    /// a changed noctalia font or radius applies to the next prompt.
    style: RefCell<Option<gtk::CssProvider>>,
}

impl GtkUi {
    pub fn new() -> Self {
        Self::default()
    }

    fn restyle(&self) {
        let mut style = self.style.borrow_mut();
        let provider = style.get_or_insert_with(|| {
            let provider = gtk::CssProvider::new();
            if let Some(display) = gdk::Display::default() {
                gtk::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
            }
            provider
        });
        provider.load_from_data(&Look::load().css());
    }
}

impl PromptUi for GtkUi {
    fn prompt(&self, request: &PromptRequest, reply: Reply) {
        if self.dialog.borrow().is_none() {
            self.restyle();
        }
        let dialog = self.dialog.borrow_mut().get_or_insert_with(Dialog::build).clone();
        dialog.show(request, reply);
    }

    fn close(&self) {
        if let Some(dialog) = self.dialog.borrow_mut().take() {
            dialog.reply.borrow_mut().take();
            dialog.window.destroy();
        }
    }
}

struct Dialog {
    window: gtk::Window,
    content: gtk::Box,
    heading: gtk::Label,
    description: gtk::Label,
    warning: gtk::Label,
    password: gtk::PasswordEntry,
    confirm: gtk::PasswordEntry,
    strength: gtk::LevelBar,
    choice: gtk::CheckButton,
    cancel: gtk::Button,
    ok: gtk::Button,
    kind: RefCell<PromptKind>,
    password_new: RefCell<bool>,
    has_choice: RefCell<bool>,
    reply: RefCell<Option<Reply>>,
}

impl Dialog {
    fn build() -> Rc<Self> {
        let window =
            gtk::Window::builder().resizable(false).default_width(420).css_classes(["krytis-prompter"]).build();
        if gtk4_layer_shell::is_supported() {
            window.init_layer_shell();
            window.set_layer(Layer::Overlay);
            window.set_keyboard_mode(KeyboardMode::Exclusive);
            window.set_namespace(Some("krytis-prompter"));
        } else {
            window.add_css_class("toplevel");
            window.set_modal(true);
        }

        let content =
            gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(12).css_classes(["card"]).build();
        let header = gtk::Box::builder().spacing(12).build();
        let icon = gtk::Image::builder()
            .icon_name("dialog-password-symbolic")
            .pixel_size(32)
            .valign(gtk::Align::Start)
            .build();
        let texts = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(4).hexpand(true).build();
        let label =
            |class: &str| gtk::Label::builder().wrap(true).xalign(0.0).max_width_chars(48).css_classes([class]).build();
        let heading = label("heading");
        let description = label("body");
        let warning = label("warning");
        texts.append(&heading);
        texts.append(&description);
        header.append(&icon);
        header.append(&texts);

        let password = gtk::PasswordEntry::builder().show_peek_icon(true).build();
        let confirm = gtk::PasswordEntry::builder().show_peek_icon(true).placeholder_text("Confirm password").build();
        let strength = gtk::LevelBar::builder().min_value(0.0).max_value(5.0).mode(gtk::LevelBarMode::Discrete).build();
        let choice = gtk::CheckButton::new();
        let buttons = gtk::Box::builder().spacing(8).halign(gtk::Align::End).build();
        let cancel = gtk::Button::new();
        let ok = gtk::Button::builder().css_classes(["suggested-action"]).build();
        buttons.append(&cancel);
        buttons.append(&ok);

        for w in [header.upcast_ref::<gtk::Widget>(), warning.upcast_ref(), password.upcast_ref(), confirm.upcast_ref()]
        {
            content.append(w);
        }
        for w in [strength.upcast_ref::<gtk::Widget>(), choice.upcast_ref(), buttons.upcast_ref()] {
            content.append(w);
        }
        window.set_child(Some(&content));

        let dialog = Rc::new(Self {
            window,
            content,
            heading,
            description,
            warning,
            password,
            confirm,
            strength,
            choice,
            cancel,
            ok,
            kind: RefCell::new(PromptKind::Password),
            password_new: RefCell::new(false),
            has_choice: RefCell::new(false),
            reply: RefCell::new(None),
        });
        dialog.connect();
        dialog
    }

    fn connect(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let on = move |f: fn(&Dialog)| {
            let weak = weak.clone();
            move || {
                if let Some(d) = weak.upgrade() {
                    f(&d);
                }
            }
        };
        let submit = on(Dialog::submit);
        self.ok.connect_clicked(move |_| submit());
        let submit = on(Dialog::submit);
        self.password.connect_activate(move |_| submit());
        let submit = on(Dialog::submit);
        self.confirm.connect_activate(move |_| submit());
        let cancel = on(Dialog::cancel);
        self.cancel.connect_clicked(move |_| cancel());
        let changed = on(Dialog::password_changed);
        self.password.connect_changed(move |_| changed());

        let cancel = on(Dialog::cancel);
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                cancel();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        self.window.add_controller(keys);
        let cancel = on(Dialog::cancel);
        self.window.connect_close_request(move |_| {
            cancel();
            glib::Propagation::Stop
        });
    }

    fn show(&self, request: &PromptRequest, reply: Reply) {
        let props = &request.props;
        let password = request.kind == PromptKind::Password;
        *self.kind.borrow_mut() = request.kind;
        *self.password_new.borrow_mut() = password && props.password_new;
        *self.has_choice.borrow_mut() = props.choice_label.is_some();
        *self.reply.borrow_mut() = Some(reply);

        self.window.set_title(Some(props.title.as_deref().unwrap_or("Authentication required")));
        // gcr's own dialog shows `message` as the heading and `description`
        // under it; the title is the window title.
        self.heading.set_text(props.message.as_deref().or(props.title.as_deref()).unwrap_or("Authentication required"));
        set_optional(&self.description, props.description.as_deref());
        set_optional(&self.warning, props.warning.as_deref());
        self.password.set_visible(password);
        self.password.set_text("");
        self.password.set_placeholder_text(Some(if props.password_new { "New password" } else { "Password" }));
        self.confirm.set_visible(password && props.password_new);
        self.confirm.set_text("");
        self.strength.set_visible(password && props.password_new);
        self.strength.set_value(0.0);
        self.choice.set_label(props.choice_label.as_deref());
        self.choice.set_visible(props.choice_label.is_some());
        self.choice.set_active(props.choice_chosen);
        self.cancel.set_label(props.cancel_label.as_deref().unwrap_or("Cancel"));
        self.ok.set_label(props.continue_label.as_deref().unwrap_or("Continue"));
        self.content.set_sensitive(true);
        self.window.present();
        if password {
            self.password.grab_focus();
        } else {
            self.ok.grab_focus();
        }
    }

    fn password_changed(&self) {
        self.strength.set_value(f64::from(password_strength(&self.password.text())));
    }

    fn submit(&self) {
        if self.reply.borrow().is_none() {
            return;
        }
        let choice_chosen = *self.has_choice.borrow() && self.choice.is_active();
        let answer = match *self.kind.borrow() {
            PromptKind::Confirm => Answer::Confirm { choice_chosen },
            PromptKind::Password => {
                let secret = Zeroizing::new(self.password.text().to_string());
                if *self.password_new.borrow() && *secret != self.confirm.text().as_str() {
                    set_optional(&self.warning, Some("The passwords do not match."));
                    self.confirm.grab_focus();
                    return;
                }
                Answer::Password { secret, choice_chosen }
            }
        };
        self.finish(answer);
    }

    fn cancel(&self) {
        self.finish(Answer::Cancel);
    }

    /// Answers once, clears the entries, and greys the dialog out until the
    /// caller prompts again (a wrong password) or stops.
    fn finish(&self, answer: Answer) {
        let Some(reply) = self.reply.borrow_mut().take() else { return };
        self.password.set_text("");
        self.confirm.set_text("");
        self.content.set_sensitive(false);
        reply(answer);
    }
}

fn set_optional(label: &gtk::Label, text: Option<&str>) {
    label.set_text(text.unwrap_or_default());
    label.set_visible(text.is_some());
}
