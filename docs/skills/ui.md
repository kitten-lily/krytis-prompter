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

## The live check in a container

krytis has no package manager, so a krytis host has no GTK 4, gtk4-layer-shell or
gcr-4 development files, and `pkg-config` is missing; `mise run live-check` says
so and stops. `mise run live-check --container` builds
`containers/live-check.Containerfile` (CI's Ubuntu and packages, the Rust from
`mise.toml`) and runs the check there, on the host's compositor. What it needs,
each found the hard way on 2026-11-03:

- **The Wayland socket**, bind-mounted, with `XDG_RUNTIME_DIR` mode 0700 inside.
  Rootless podman's root is the invoking user, so it may connect.
- **The theme inputs.** Without `~/.config/gtk-4.0` (whose `gtk.css` imports
  noctalia's palette) and the host fonts, the dialog renders in the fallback
  palette and the container's default font. It still works, so this is easy to
  mistake for a theming bug. The first run on 2026-11-03 had only the noctalia
  settings mounted, and the operator rightly refused to tick the box. The fonts
  are all four directories the container's fontconfig searches
  (`/usr/share/fonts`, `/usr/local/share/fonts`, `~/.local/share/fonts`,
  `~/.fonts`), each mounted only if it exists on the host; a font in an unmounted
  one is silently substituted.
- **`label=disable`** rather than `:z`/`:Z`, which would relabel the host's socket
  and config files.
- **Its own `CARGO_TARGET_DIR`** under `target/`, so it never invalidates a host
  build, and a named volume for the crate registry.

An unticked box fails the check after the dialog has answered. The dialog then
stays visible and insensitive (§ Prompt behavior) until the test exits: that is
the failed test not calling `StopPrompting`, not a hung daemon.

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
