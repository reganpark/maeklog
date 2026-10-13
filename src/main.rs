#![forbid(unsafe_code)]

mod action_timing;
mod actions;
mod batch;
mod branch_context;
mod branch_panel;
#[cfg(test)]
mod branch_tests;
mod client_routing;
mod codex_hooks;
mod config;
mod connection;
mod corrections;
mod daily;
mod day_picker;
mod decision_policy;
mod evidence_spans;
mod herdr;
mod hook;
mod i18n;
mod journal;
mod language_picker;
mod language_settings;
mod latest;
mod legacy;
mod link_picker;
mod link_refresh;
mod live_panel;
mod live_summary;
mod live_turn;
mod local_time;
mod maintenance;
mod memory;
mod model;
mod model_catalog;
mod model_picker;
mod model_settings;
mod panel;
mod panel_menu;
mod panel_selection;
mod panel_style;
mod participation;
mod process;
mod record_editor;
mod reducer_diagnostics;
mod repo_facts;
#[cfg(test)]
mod restart;
mod retry;
mod scheduler;
mod storage;
mod summarizer;
mod task_links;
mod task_report;
mod timing;
mod transcript;
mod work_links;
mod worker;

use std::error::Error;

pub type AppResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

fn main() {
    let mut arguments = std::env::args().skip(1);
    let command = arguments.next().unwrap_or_else(|| "help".to_string());

    // Refresh once at startup; tr! itself performs only cached reads. Keep errors
    // off protocol stdout, and let collection retain raw input if preferences are bad.
    if let Err(error) = i18n::refresh_cached() {
        eprintln!("maeklog: {error}");
    }

    if command == "hook" {
        hook::entry();
        return;
    }

    let result = match command.as_str() {
        "client" => client_routing::cli(&arguments.collect::<Vec<_>>()),
        "live" => live_turn::cli(&arguments.collect::<Vec<_>>()),
        "catch-up" => live_summary::cli(),
        "daily" => daily::cli(&arguments.collect::<Vec<_>>()),
        "memory" => memory::cli(&arguments.collect::<Vec<_>>()),
        "model" => model_settings::cli(&arguments.collect::<Vec<_>>()),
        "language" => language_settings::cli(&arguments.collect::<Vec<_>>()),
        "links" => work_links::cli(&arguments.collect::<Vec<_>>()),
        "task" => task_report::cli(&arguments.collect::<Vec<_>>()),
        "action" => actions::run(arguments.next().as_deref()),
        "panel" => panel::run(false),
        "render-once" => panel::run(true),
        "render-history" => panel::run_view(true, true),
        "worker" => scheduler::run(),
        "worker-space" => scheduler::run_space(arguments.next().as_deref()),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        "version" | "--version" | "-V" => {
            println!("maeklog {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        unknown => Err(crate::tr!(
            format!("알 수 없는 명령: {unknown}"),
            format!("unknown command: {unknown}")
        )
        .into()),
    };

    if let Err(error) = result {
        eprintln!("maeklog: {error}");
        std::process::exit(1);
    }
}

fn print_help() {
    println!(
        "maeklog {}\n\n\
         {}:\n  \
         maeklog panel\n  \
         maeklog render-once\n  \
         maeklog render-history\n  \
         maeklog catch-up\n  \
         maeklog model <show|list|set MODEL [--effort EFFORT]>\n  \
         maeklog language <show|set auto|en|ko>\n  \
         maeklog live event CLIENT_PID THREAD_ID\n  \
         maeklog daily <list|request YYYY-MM-DD|request unknown|resume>\n  \
         maeklog memory <list|show KEY|history KEY [PAGE]|legacy SPACE_KEY|clear KEY --confirm-shared KEY>\n  \
         maeklog links <show|refresh>\n  \
         maeklog links <primary|add|remove> <issue|pr> <N|OWNER/REPO#N|URL>\n  \
         maeklog links <none|auto> <issue|pr>\n  \
         maeklog task <context|report> SPOOL\n  \
         maeklog hook\n  \
         maeklog worker\n  \
         maeklog action <open|hide|enable|disable|clear-records|refresh|rebuild|startup|workspace-focused|workspace-closed|pane-closed|pane-exited|install-codex-hooks|remove-codex-hooks>",
        env!("CARGO_PKG_VERSION"),
        crate::tr!("사용법", "Usage")
    );
}
