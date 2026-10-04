use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use cleaner_core::{
  ApplicationDiscoveryPort, ApplicationDiscoveryRequest, ApplicationDiscoveryResponse,
  ApplicationId, ApplicationRepositoryPort, DomainEvent, EventPublisher, InstalledApplication,
  NativeUninstallPort, PortError, UninstallRequest, UninstallResponse, UninstallStatus,
};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

pub struct ApplicationManager {
  catalog: Mutex<ApplicationCatalog>,
  namespace: u64,
}

static NEXT_MANAGER_NAMESPACE: AtomicU64 = AtomicU64::new(1);

impl Default for ApplicationManager {
  fn default() -> Self {
    let sequence = NEXT_MANAGER_NAMESPACE.fetch_add(1, Ordering::Relaxed);
    let time = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map(|duration| duration.as_nanos() as u64)
      .unwrap_or_default();
    Self {
      catalog: Mutex::default(),
      namespace: time.rotate_left(13) ^ u64::from(std::process::id()) ^ sequence,
    }
  }
}

#[derive(Default)]
struct ApplicationCatalog {
  next_id: u64,
  entries: HashMap<ApplicationId, CatalogEntry>,
}

struct CatalogEntry {
  application: InstalledApplication,
  action: PlatformAction,
}

struct PendingApplication {
  name: String,
  version: Option<String>,
  source: cleaner_core::ApplicationSource,
  action: PlatformAction,
}

#[cfg(target_os = "macos")]
#[derive(Clone)]
enum PlatformAction {
  MacBundle {
    path: std::path::PathBuf,
    bundle_id: Option<String>,
  },
}

#[cfg(target_os = "windows")]
#[derive(Clone)]
enum PlatformAction {
  WindowsRegistry(windows::RegistryApplication),
}

#[cfg(target_os = "linux")]
#[derive(Clone)]
enum PlatformAction {
  Flatpak { application_id: String },
  Snap { package_name: String },
  DesktopEntry,
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
#[derive(Clone)]
enum PlatformAction {
  Unsupported,
}

impl ApplicationManager {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn discover_applications(
    &self,
    events: &dyn EventPublisher,
  ) -> Result<ApplicationDiscoveryResponse, PortError> {
    let snapshot = discover_platform_applications();
    let mut catalog = self
      .catalog
      .lock()
      .map_err(|_| PortError::OperationFailed {
        message: "The application catalog is unavailable.".to_owned(),
      })?;

    let mut next_id = catalog.next_id;
    let mut entries = HashMap::with_capacity(snapshot.applications.len());
    let mut applications = Vec::with_capacity(snapshot.applications.len());

    for pending in snapshot.applications {
      let id_number = next_id;
      next_id = next_id
        .checked_add(1)
        .ok_or_else(|| PortError::OperationFailed {
          message: "The application catalog identifier space is exhausted.".to_owned(),
        })?;
      let id = ApplicationId(format!(
        "app-{:08x}-{:016x}-{id_number:016x}",
        std::process::id(),
        self.namespace
      ));
      let application = InstalledApplication {
        id: id.clone(),
        name: pending.name,
        version: pending.version,
        source: pending.source,
      };

      applications.push(application.clone());
      entries.insert(
        id,
        CatalogEntry {
          application,
          action: pending.action,
        },
      );
    }

    catalog.next_id = next_id;
    catalog.entries = entries;
    let application_ids = applications
      .iter()
      .map(|application| application.id.clone())
      .collect();
    events.publish(DomainEvent::ApplicationsDiscovered { application_ids });

    Ok(ApplicationDiscoveryResponse {
      applications,
      warnings: snapshot.warnings,
    })
  }

  pub fn uninstall_application(
    &self,
    request: UninstallRequest,
    events: &dyn EventPublisher,
  ) -> Result<UninstallResponse, PortError> {
    if !request.confirmed {
      return Err(PortError::ConfirmationRequired);
    }

    let entry = {
      let catalog = self
        .catalog
        .lock()
        .map_err(|_| PortError::OperationFailed {
          message: "The application catalog is unavailable.".to_owned(),
        })?;
      catalog
        .entries
        .get(&request.application_id)
        .map(|entry| (entry.application.clone(), entry.action.clone()))
        .ok_or_else(|| PortError::OperationFailed {
          message: "The selected application is not in the current inventory.".to_owned(),
        })?
    };

    let response = uninstall_platform_application(&entry.0, &entry.1, &request)?;
    if response.status == UninstallStatus::Completed {
      events.publish(DomainEvent::NativeUninstallFinished {
        application_id: response.application_id.clone(),
      });
    }
    Ok(response)
  }
}

impl ApplicationDiscoveryPort for ApplicationManager {
  fn discover(
    &self,
    _request: &ApplicationDiscoveryRequest,
    events: &dyn EventPublisher,
  ) -> Result<ApplicationDiscoveryResponse, PortError> {
    self.discover_applications(events)
  }
}

impl ApplicationRepositoryPort for ApplicationManager {
  fn get_application(
    &self,
    application_id: &ApplicationId,
  ) -> Result<InstalledApplication, PortError> {
    let catalog = self
      .catalog
      .lock()
      .map_err(|_| PortError::OperationFailed {
        message: "The application catalog is unavailable.".to_owned(),
      })?;
    catalog
      .entries
      .get(application_id)
      .map(|entry| entry.application.clone())
      .ok_or_else(|| PortError::OperationFailed {
        message: "The selected application is not in the current inventory.".to_owned(),
      })
  }
}

impl NativeUninstallPort for ApplicationManager {
  fn uninstall(
    &self,
    application: &InstalledApplication,
    request: &UninstallRequest,
    events: &dyn EventPublisher,
  ) -> Result<UninstallResponse, PortError> {
    if application.id != request.application_id {
      return Err(PortError::OperationFailed {
        message: "The uninstall request does not match the selected application.".to_owned(),
      });
    }
    if self.get_application(&request.application_id)? != *application {
      return Err(PortError::CandidateChanged);
    }
    self.uninstall_application(request.clone(), events)
  }
}

#[derive(Default)]
struct DiscoverySnapshot {
  applications: Vec<PendingApplication>,
  warnings: Vec<String>,
}

#[cfg(target_os = "macos")]
fn discover_platform_applications() -> DiscoverySnapshot {
  macos::discover()
}

#[cfg(target_os = "windows")]
fn discover_platform_applications() -> DiscoverySnapshot {
  windows::discover()
}

#[cfg(target_os = "linux")]
fn discover_platform_applications() -> DiscoverySnapshot {
  linux::discover()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn discover_platform_applications() -> DiscoverySnapshot {
  DiscoverySnapshot {
    warnings: vec!["Application inventory is not supported on this operating system.".to_owned()],
    ..DiscoverySnapshot::default()
  }
}

#[cfg(target_os = "macos")]
fn uninstall_platform_application(
  application: &InstalledApplication,
  action: &PlatformAction,
  request: &UninstallRequest,
) -> Result<UninstallResponse, PortError> {
  macos::uninstall(application, action, request)
}

#[cfg(target_os = "windows")]
fn uninstall_platform_application(
  application: &InstalledApplication,
  action: &PlatformAction,
  request: &UninstallRequest,
) -> Result<UninstallResponse, PortError> {
  windows::uninstall(application, action, request)
}

#[cfg(target_os = "linux")]
fn uninstall_platform_application(
  application: &InstalledApplication,
  action: &PlatformAction,
  request: &UninstallRequest,
) -> Result<UninstallResponse, PortError> {
  linux::uninstall(application, action, request)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn uninstall_platform_application(
  _application: &InstalledApplication,
  _action: &PlatformAction,
  _request: &UninstallRequest,
) -> Result<UninstallResponse, PortError> {
  Err(PortError::AdapterUnavailable {
    operation: "uninstalling applications on this operating system",
  })
}

fn package_name_is_safe(value: &str) -> bool {
  !value.is_empty()
    && value.as_bytes()[0].is_ascii_alphanumeric()
    && value
      .bytes()
      .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn decode_xml_entities(value: &str) -> String {
  let mut decoded = String::with_capacity(value.len());
  let mut remaining = value;
  while let Some(entity_start) = remaining.find('&') {
    decoded.push_str(&remaining[..entity_start]);
    remaining = &remaining[entity_start..];
    let Some(entity_end) = remaining.find(';') else {
      decoded.push_str(remaining);
      return decoded;
    };
    let entity = &remaining[1..entity_end];
    let replacement = match entity {
      "amp" => Some('&'),
      "lt" => Some('<'),
      "gt" => Some('>'),
      "quot" => Some('"'),
      "apos" => Some('\''),
      _ if entity.starts_with("#x") || entity.starts_with("#X") => {
        u32::from_str_radix(&entity[2..], 16)
          .ok()
          .and_then(char::from_u32)
      }
      _ if entity.starts_with('#') => entity[1..].parse::<u32>().ok().and_then(char::from_u32),
      _ => None,
    };
    if let Some(character) = replacement {
      decoded.push(character);
    } else {
      decoded.push_str(&remaining[..=entity_end]);
    }
    remaining = &remaining[entity_end + 1..];
  }
  decoded.push_str(remaining);
  decoded
}

fn plist_xml_string(xml: &str, key: &str) -> Option<String> {
  let key_tag = format!("<key>{key}</key>");
  let key_position = xml.find(&key_tag)? + key_tag.len();
  let value = xml[key_position..].trim_start();
  let value = value.strip_prefix("<string>")?;
  let end = value.find("</string>")?;
  let decoded = decode_xml_entities(&value[..end]);
  let decoded = decoded.trim();
  (!decoded.is_empty()).then(|| decoded.to_owned())
}

fn windows_entry_is_visible(display_name: Option<&str>, system_component: bool) -> bool {
  !system_component && display_name.is_some_and(|name| !name.trim().is_empty())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[derive(Default)]
  struct RecordingEvents(Mutex<Vec<DomainEvent>>);

  impl EventPublisher for RecordingEvents {
    fn publish(&self, event: DomainEvent) {
      if let Ok(mut events) = self.0.lock() {
        events.push(event);
      }
    }
  }

  #[test]
  fn uninstallation_always_requires_explicit_confirmation() {
    let manager = ApplicationManager::new();
    let error = manager
      .uninstall_application(
        UninstallRequest {
          application_id: ApplicationId("not-in-catalog".to_owned()),
          confirmed: false,
        },
        &RecordingEvents::default(),
      )
      .expect_err("unconfirmed requests must be rejected before lookup");

    assert!(matches!(error, PortError::ConfirmationRequired));
  }

  #[test]
  fn package_identifiers_reject_option_and_path_injection() {
    assert!(package_name_is_safe("org.example.Editor_2"));
    assert!(!package_name_is_safe("--help"));
    assert!(!package_name_is_safe("org.example/Editor"));
    assert!(!package_name_is_safe(""));
  }

  #[test]
  fn parses_plist_strings_and_xml_entities() {
    let xml = "<plist><dict><key>CFBundleIdentifier</key><string>org.example.&amp;app</string><key>CFBundleShortVersionString</key><string>1.2.3</string></dict></plist>";

    assert_eq!(
      plist_xml_string(xml, "CFBundleIdentifier").as_deref(),
      Some("org.example.&app")
    );
    assert_eq!(
      plist_xml_string(xml, "CFBundleShortVersionString").as_deref(),
      Some("1.2.3")
    );
    assert_eq!(plist_xml_string(xml, "Missing"), None);
  }

  #[test]
  fn filters_windows_system_components_and_missing_names() {
    assert!(windows_entry_is_visible(Some("Editor"), false));
    assert!(!windows_entry_is_visible(Some("Editor"), true));
    assert!(!windows_entry_is_visible(None, false));
    assert!(!windows_entry_is_visible(Some("  "), false));
  }
}
