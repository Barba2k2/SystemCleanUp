use std::error::Error;
use std::io::{self, Write};
use std::process;

use clap::{Parser, Subcommand};
use cleaner_apps::ApplicationManager;
use cleaner_core::{
  ApplicationDiscoveryPort, ApplicationDiscoveryRequest, ApplicationSource, CandidateCategory,
  CandidatePreview, CleanupRequest, DomainEvent, EventPublisher, PreviewRequest, RemovalMode,
  ScanRequest, UninstallRequest, UninstallStatus,
};
use cleaner_files::FileCleaner;

#[derive(Debug, Parser)]
#[command(
  name = "cleaner",
  about = "Review cleanup candidates and installed applications"
)]
struct Cli {
  #[command(subcommand)]
  command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
  Files,
  Apps,
}

#[derive(Default)]
struct ConsoleEvents;

impl EventPublisher for ConsoleEvents {
  fn publish(&self, event: DomainEvent) {
    if let DomainEvent::Progress(progress) = event {
      println!("{:?}: {}", progress.operation, progress.message);
    }
  }
}

fn main() {
  if let Err(error) = run(Cli::parse()) {
    eprintln!("Error: {error}");
    process::exit(1);
  }
}

fn run(cli: Cli) -> Result<(), Box<dyn Error>> {
  let events = ConsoleEvents;
  match cli.command {
    Commands::Files => run_files(&events),
    Commands::Apps => run_apps(&events),
  }
}

fn run_files(events: &dyn EventPublisher) -> Result<(), Box<dyn Error>> {
  let cleaner = FileCleaner::new();
  let scan = cleaner.scan(
    ScanRequest {
      categories: vec![
        CandidateCategory::UserCache,
        CandidateCategory::UserTemporary,
        CandidateCategory::DiagnosticLog,
      ],
    },
    events,
  )?;
  print_warnings(&scan.warnings);
  if scan.candidates.is_empty() {
    println!("No cleanup candidates were found in the approved roots.");
    return Ok(());
  }

  println!("\nCleanup candidates:");
  for (index, candidate) in scan.candidates.iter().enumerate() {
    println!(
      "{}. [{:?} / {:?}] {} ({} bytes) — {}",
      index + 1,
      candidate.category,
      candidate.risk,
      candidate.path.display(),
      candidate.size_bytes,
      candidate.reason
    );
  }

  let Some(selected_indices) = choose_indices(scan.candidates.len())? else {
    println!("Cleanup cancelled; no files were changed.");
    return Ok(());
  };
  let selected_candidate_ids = selected_indices
    .iter()
    .map(|index| scan.candidates[*index].id.clone())
    .collect();
  let preview = cleaner.prepare_preview(
    PreviewRequest {
      scan_id: scan.scan_id,
      selected_candidate_ids,
    },
    events,
  )?;

  let mut eligible_count = 0;
  println!("\nPreview:");
  for entry in &preview.entries {
    match entry {
      CandidatePreview::Eligible { candidate } => {
        eligible_count += 1;
        println!("Eligible: {}", candidate.path.display());
      }
      CandidatePreview::Blocked { candidate, reason } => {
        println!("Blocked: {} — {reason}", candidate.path.display());
      }
    }
  }
  if eligible_count == 0 {
    println!("No selected candidate is eligible; no files were changed.");
    return Ok(());
  }

  let Some(removal_mode) = choose_removal_mode()? else {
    println!("Cleanup cancelled; no files were changed.");
    return Ok(());
  };
  let selected_count = preview.entries.len();
  if removal_mode == RemovalMode::Permanent {
    println!("Permanent deletion cannot be restored.");
  }
  let confirmation = match removal_mode {
    RemovalMode::Trash => format!("TRASH {selected_count}"),
    RemovalMode::Permanent => format!("PERMANENT {selected_count}"),
  };
  println!(
    "Type `{confirmation}` to confirm the {selected_count} selected candidate(s) ({eligible_count} eligible); anything else cancels."
  );
  if prompt("> ")? != confirmation {
    println!("Cleanup cancelled; no files were changed.");
    return Ok(());
  }

  let response = cleaner.execute_cleanup(
    CleanupRequest {
      preview_id: preview.preview_id,
      removal_mode,
      confirmed: true,
    },
    events,
  )?;
  println!(
    "Cleanup processed {} selected candidate(s): {} removed, {} failed.",
    response.removed_candidate_ids.len() + response.failed_candidate_ids.len(),
    response.removed_candidate_ids.len(),
    response.failed_candidate_ids.len()
  );
  for failed_id in response.failed_candidate_ids {
    if let Some(candidate) = scan
      .candidates
      .iter()
      .find(|candidate| candidate.id == failed_id)
    {
      println!("Failed: {}", candidate.path.display());
    }
  }
  Ok(())
}

fn run_apps(events: &dyn EventPublisher) -> Result<(), Box<dyn Error>> {
  let manager = ApplicationManager::new();
  let discovery = manager.discover(&ApplicationDiscoveryRequest {}, events)?;
  print_warnings(&discovery.warnings);
  if discovery.applications.is_empty() {
    println!("No applications were found by the available inventory sources.");
    return Ok(());
  }

  println!("\nInstalled applications (no usage classification is performed):");
  for (index, application) in discovery.applications.iter().enumerate() {
    println!(
      "{}. {} | {} | {}",
      index + 1,
      application.name,
      application
        .version
        .as_deref()
        .unwrap_or("version unavailable"),
      application_source_name(application.source)
    );
  }

  let Some(index) = choose_one_index(discovery.applications.len())? else {
    println!("Application action cancelled.");
    return Ok(());
  };
  let application = &discovery.applications[index];
  println!(
    "Type `UNINSTALL` to act on `{}`; anything else cancels.",
    application.name
  );
  if prompt("> ")? != "UNINSTALL" {
    println!("Application action cancelled.");
    return Ok(());
  }

  let response = manager.uninstall_application(
    UninstallRequest {
      application_id: application.id.clone(),
      confirmed: true,
    },
    events,
  )?;
  match response.status {
    UninstallStatus::Completed
      if application.source == ApplicationSource::MacApplicationsDirectory =>
    {
      println!("Moved the selected application bundle to Trash. Its user data was left in place.");
    }
    UninstallStatus::Completed => {
      println!("The package manager completed the selected application removal.");
    }
    UninstallStatus::DelegatedToSystem => {
      println!("The native settings page was opened. The application removal is not complete; finish it there.");
    }
  }
  Ok(())
}

fn choose_indices(candidate_count: usize) -> Result<Option<Vec<usize>>, io::Error> {
  loop {
    let input =
      prompt("Select candidate numbers separated by commas, `all`, or Enter to cancel: ")?;
    let input = input.trim();
    if input.is_empty() {
      return Ok(None);
    }
    if input.eq_ignore_ascii_case("all") {
      return Ok(Some((0..candidate_count).collect()));
    }

    let mut indices = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut valid = true;
    for value in input.split(',').map(str::trim) {
      match value.parse::<usize>() {
        Ok(number) if number > 0 && number <= candidate_count && seen.insert(number) => {
          indices.push(number - 1);
        }
        _ => {
          valid = false;
          break;
        }
      }
    }
    if valid && !indices.is_empty() {
      return Ok(Some(indices));
    }
    eprintln!("Enter unique numbers from 1 to {candidate_count}, or `all`.");
  }
}

fn choose_one_index(application_count: usize) -> Result<Option<usize>, io::Error> {
  loop {
    let input = prompt("Select one application number, or Enter to cancel: ")?;
    let input = input.trim();
    if input.is_empty() {
      return Ok(None);
    }
    if let Ok(number) = input.parse::<usize>() {
      if number > 0 && number <= application_count {
        return Ok(Some(number - 1));
      }
    }
    eprintln!("Enter one number from 1 to {application_count}.");
  }
}

fn choose_removal_mode() -> Result<Option<RemovalMode>, io::Error> {
  loop {
    match prompt("Removal mode: `trash`, `permanent`, or Enter to cancel: ")?
      .trim()
      .to_ascii_lowercase()
      .as_str()
    {
      "trash" => return Ok(Some(RemovalMode::Trash)),
      "permanent" => return Ok(Some(RemovalMode::Permanent)),
      "" => return Ok(None),
      _ => eprintln!("Choose `trash` or `permanent`."),
    }
  }
}

fn application_source_name(source: ApplicationSource) -> &'static str {
  match source {
    ApplicationSource::MacApplicationsDirectory => "macOS Applications",
    ApplicationSource::WindowsRegistry => "Windows registry",
    ApplicationSource::WindowsPackageManager => "Windows package",
    ApplicationSource::LinuxPackageManager => "Linux package manager",
    ApplicationSource::LinuxDesktopEntry => "Linux desktop entry (uninstall unavailable)",
    ApplicationSource::Other => "other source",
  }
}

fn print_warnings(warnings: &[String]) {
  for warning in warnings {
    eprintln!("Warning: {warning}");
  }
}

fn prompt(message: &str) -> Result<String, io::Error> {
  print!("{message}");
  io::stdout().flush()?;
  let mut answer = String::new();
  io::stdin().read_line(&mut answer)?;
  Ok(answer.trim_end_matches(&['\r', '\n'][..]).to_owned())
}
