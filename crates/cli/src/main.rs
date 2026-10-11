//! `ubl`: the UNBEATEN launcher without a window. Handy for scripting, for
//! headless setups (a Steam Deck in Game Mode can run `ubl watch`), and for
//! testing the launcher against a local server.

use std::path::PathBuf;
use std::process::ExitCode;
use std::thread::sleep;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use unbeaten_core::api::LinkPoll;
use unbeaten_core::auth_store::TokenStore;
use unbeaten_core::charts::chart_hash;
use unbeaten_core::process::GameWatch;
use unbeaten_core::{AppDirs, Launcher};

#[derive(Parser)]
#[command(name = "ubl", version, about = "UNBEATEN launcher, command line")]
struct Cli {
    /// UNBEATEN server (default: the saved setting, else https://unbeaten.cc)
    #[arg(long, global = true)]
    server: Option<String>,
    /// Folder for settings, sync state and (with --token-file unset) the token file
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    /// Keep the token in this file instead of the OS credential store
    #[arg(long, global = true)]
    token_file: Option<PathBuf>,
    /// UNBEATABLE's arcade-highscores.json (default: detected)
    #[arg(long, global = true)]
    save: Option<PathBuf>,
    /// UNBEATABLE's CustomSongs folder (default: detected)
    #[arg(long, global = true)]
    custom_songs: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show where Steam, the game, its save and CustomSongs were found
    Detect,
    /// Print the ubh1 hash of chart files
    Hash { files: Vec<PathBuf> },
    /// Sign in: prints a code to approve on the website
    Login {
        /// Don't open the browser
        #[arg(long)]
        no_browser: bool,
    },
    /// Sign out and forget the token
    Logout,
    /// Show the signed-in player
    Me,
    /// List ranked maps and their state on this computer
    Maps {
        /// Only maps that aren't installed and up to date
        #[arg(long)]
        todo: bool,
    },
    /// Install or update a ranked map by id
    Install { id: i64 },
    /// Update every outdated map
    UpdateAll,
    /// Sync new scores once
    Sync,
    /// Sync whenever the save file changes, until stopped
    Watch {
        /// Seconds between checks
        #[arg(long, default_value_t = 2)]
        interval: u64,
    },
}

fn print_json(value: &impl serde::Serialize) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_default()
    );
}

fn open_launcher(cli: &Cli) -> Result<Launcher, String> {
    let dirs = match &cli.state_dir {
        Some(d) => AppDirs::single(d),
        None => AppDirs::standard(),
    };
    let tokens = match &cli.token_file {
        Some(f) => TokenStore::file_only(f.clone()),
        None => TokenStore::new(&dirs.config),
    };
    let mut l = Launcher::open(dirs, tokens);
    let mut settings = l.settings.clone();
    if let Some(s) = &cli.server {
        settings.server_url = s.trim_end_matches('/').to_string();
    }
    if cli.save.is_some() {
        settings.save_file = cli.save.clone();
    }
    if cli.custom_songs.is_some() {
        settings.custom_songs = cli.custom_songs.clone();
    }
    // Command-line paths apply to this run only; the server is remembered.
    let persist = unbeaten_core::settings::Settings {
        save_file: l.settings.save_file.clone(),
        custom_songs: l.settings.custom_songs.clone(),
        ..settings.clone()
    };
    l.update_settings(persist).map_err(|e| e.to_string())?;
    l.settings = settings;
    Ok(l)
}

fn sync_and_print(l: &mut Launcher) -> Result<(), String> {
    let report = l.sync().map_err(|e| e.to_string())?;
    print_json(&report);
    Ok(())
}

fn run(cli: Cli) -> Result<(), String> {
    match &cli.cmd {
        Cmd::Hash { files } => {
            for f in files {
                let data = std::fs::read(f).map_err(|e| format!("{}: {e}", f.display()))?;
                println!(
                    "{}  {}",
                    chart_hash(&data).unwrap_or_else(|| "-".repeat(64)),
                    f.display()
                );
            }
            return Ok(());
        }
        Cmd::Detect => {
            let l = open_launcher(&cli)?;
            print_json(&l.paths());
            return Ok(());
        }
        _ => {}
    }

    let mut l = open_launcher(&cli)?;
    match cli.cmd {
        Cmd::Login { no_browser } => {
            let link = l.start_link().map_err(|e| e.to_string())?;
            eprintln!(
                "Approve this code on {}:\n\n    {}\n",
                link.verification_uri, link.user_code
            );
            eprintln!("Direct link: {}", link.verification_uri_complete);
            if !no_browser {
                open_browser(&link.verification_uri_complete);
            }
            let deadline = Instant::now() + Duration::from_secs(link.expires_in);
            let mut interval = Duration::from_secs(link.interval.max(1));
            while Instant::now() < deadline {
                sleep(interval);
                match l.poll_link(&link.device_code).map_err(|e| e.to_string())? {
                    LinkPoll::Pending => {}
                    LinkPoll::SlowDown => interval += Duration::from_secs(1),
                    LinkPoll::Denied => return Err("the sign-in was denied".into()),
                    LinkPoll::Expired => return Err("the code expired; run login again".into()),
                    LinkPoll::Approved { user, .. } => {
                        eprintln!("Signed in as {}.", user.username);
                        return Ok(());
                    }
                }
            }
            Err("the code expired; run login again".into())
        }
        Cmd::Logout => {
            l.sign_out();
            eprintln!("Signed out.");
            Ok(())
        }
        Cmd::Me => {
            let me = l.refresh_me().map_err(|e| e.to_string())?;
            print_json(&me);
            Ok(())
        }
        Cmd::Maps { todo } => {
            let maps = l.maps().map_err(|e| e.to_string())?;
            for m in maps {
                let state = format!("{:?}", m.status.state);
                if todo && state == "Installed" {
                    continue;
                }
                println!(
                    "{:>5}  v{:<3} {:<13} {}{}",
                    m.map.id,
                    m.map.version,
                    state,
                    m.map.install_name(),
                    m.status
                        .local_folder
                        .filter(|f| m.status.renamed && f != m.map.install_name())
                        .map(|f| format!("  (as \"{f}\")"))
                        .unwrap_or_default()
                );
            }
            Ok(())
        }
        Cmd::Install { id } => {
            let out = l.install(id).map_err(|e| e.to_string())?;
            print_json(&out);
            Ok(())
        }
        Cmd::UpdateAll => {
            for id in l.outdated_maps().map_err(|e| e.to_string())? {
                let out = l.install(id).map_err(|e| e.to_string())?;
                eprintln!("updated {}", out.folder.display());
            }
            Ok(())
        }
        Cmd::Sync => {
            l.refresh_me().map_err(|e| e.to_string())?;
            sync_and_print(&mut l)
        }
        Cmd::Watch { interval } => {
            l.refresh_me().map_err(|e| e.to_string())?;
            let mut game = GameWatch::new();
            let mut was_running = false;
            let _ = l.save_changed();
            sync_and_print(&mut l)?;
            loop {
                sleep(Duration::from_secs(interval.max(1)));
                let running = game.is_running();
                if running != was_running {
                    eprintln!("UNBEATABLE {}", if running { "started" } else { "closed" });
                }
                let exited = was_running && !running;
                was_running = running;
                if l.save_changed() || exited {
                    // Let the game finish writing.
                    sleep(Duration::from_millis(1500));
                    let _ = l.save_changed();
                    if let Err(e) = sync_and_print(&mut l) {
                        eprintln!("sync failed: {e}");
                    }
                }
            }
        }
        Cmd::Hash { .. } | Cmd::Detect => unreachable!(),
    }
}

fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    if r.is_err() {
        eprintln!("(couldn't open a browser; open the link above yourself)");
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
