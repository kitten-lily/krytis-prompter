//! D-Bus activated: the bus starts this on the first call to
//! `org.gnome.keyring.SystemPrompter`, through `krytis-prompter.service`.

use std::{process::ExitCode, rc::Rc, time::Duration};

use gtk::glib;
use krytis_prompter::{daemon, gtk_ui::GtkUi};

const USAGE: &str = "usage: krytis-prompter [--idle-timeout SECONDS]

Serves org.gnome.keyring.SystemPrompter on the session bus and exits after
SECONDS (default 60, 0 = never) with no prompt open.";

fn main() -> ExitCode {
    let idle_timeout = match parse_args(std::env::args().skip(1)) {
        Ok(Action::Run(timeout)) => timeout,
        Ok(Action::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Ok(Action::Version) => {
            println!("krytis-prompter {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("krytis-prompter: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    glib::set_prgname(Some("krytis-prompter"));
    if let Err(e) = gtk::init() {
        eprintln!("krytis-prompter: cannot start GTK: {e}");
        return ExitCode::FAILURE;
    }

    let main_loop = glib::MainLoop::new(None, false);
    let code = Rc::new(std::cell::Cell::new(ExitCode::SUCCESS));
    {
        let (main_loop, code) = (main_loop.clone(), code.clone());
        glib::MainContext::default().spawn_local(async move {
            if let Err(e) = serve(idle_timeout).await {
                eprintln!("krytis-prompter: {e}");
                code.set(ExitCode::FAILURE);
            }
            main_loop.quit();
        });
    }
    main_loop.run();
    code.get()
}

async fn serve(idle_timeout: Option<Duration>) -> zbus::Result<()> {
    let conn = zbus::connection::Builder::session().build().await?;
    // Subscribe before claiming: the call that activated us arrives the moment
    // the name is ours (daemon::Prompter).
    let prompter = daemon::Prompter::new(conn).await?;
    if !prompter.claim_name().await? {
        eprintln!("krytis-prompter: {} already has an owner; leaving it alone", daemon::BUS_NAME);
        return Ok(());
    }
    prompter.run(Rc::new(GtkUi::new()), daemon::Config { idle_timeout }).await
}

enum Action {
    Run(Option<Duration>),
    Help,
    Version,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Action, String> {
    let mut idle = Some(Duration::from_secs(60));
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Action::Help),
            "-V" | "--version" => return Ok(Action::Version),
            "--idle-timeout" => {
                let value = args.next().ok_or("--idle-timeout needs a value")?;
                let secs: u64 =
                    value.parse().map_err(|_| format!("--idle-timeout: not a number of seconds: {value}"))?;
                idle = (secs > 0).then(|| Duration::from_secs(secs));
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Action::Run(idle))
}
