# GTK user interface

## Layer shell

On a compositor supporting wlr-layer-shell, the window is an overlay layer
surface with exclusive keyboard mode and namespace `krytis-prompter`. With no
anchors, the compositor centers its natural-size surface. This keeps the prompt
above fullscreen windows and routes keyboard input to it.

When layer shell is unavailable, use the same `gtk::Window` as a modal normal
toplevel. `glib::set_prgname("krytis-prompter")` supplies the Wayland application
identity used by compositor rules. Never fall back merely because theming is
missing.

`mise run live-check` is the visual proof: it launches the actual binary on a
private bus and asks the operator to type a fixed phrase. Check placement,
exclusive keyboard focus, password masking, Cancel/Escape, colors and idle exit.
Headless tests cover the protocol, not the surface.

## Prompt behavior

The dialog persists through one caller session. After an answer it clears both
password entries and becomes insensitive while the caller validates the answer.
A wrong-password `PerformPrompt` reuses it, displays the new warning and enables
it again. `StopPrompting` or caller disappearance destroys it.

`password-new` shows a confirmation entry and strength meter. A mismatch stays in
the dialog and never invokes the protocol callback. Password, confirm, choice
checkbox, Cancel, Escape and window close each answer at most once.

## Theming

GTK automatically loads the user's GTK4 CSS. noctalia's GTK template defines the
libadwaita-style color names used by the application CSS, at user priority above
the application provider. The application also defines a dark fallback palette,
so it remains legible when noctalia's GTK template is disabled or absent.

For geometry, read `[shell].font_family` and
`[shell].corner_radius_scale` from sorted `$XDG_CONFIG_HOME/noctalia/*.toml`, then
`$XDG_STATE_HOME/noctalia/settings.toml`; later values win. Clamp radius scale to
0–2 and restrict font names to characters that cannot escape a CSS string.
Missing, unreadable or invalid files use the fallback.
