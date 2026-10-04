use std::ffi::c_void;
use std::process::Command;

use cleaner_core::{
  ApplicationSource, InstalledApplication, PortError, UninstallRequest, UninstallResponse,
  UninstallStatus,
};
use std::os::windows::ffi::OsStringExt;

use super::{package_name_is_safe, DiscoverySnapshot, PendingApplication, PlatformAction};

const UNINSTALL_REGISTRY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
const KEY_READ: u32 = 0x0002_0019;
const KEY_WOW64_64KEY: u32 = 0x0100;
const KEY_WOW64_32KEY: u32 = 0x0200;
const ERROR_SUCCESS: i32 = 0;
const ERROR_NO_MORE_ITEMS: i32 = 259;
const REG_SZ: u32 = 1;
const REG_EXPAND_SZ: u32 = 2;
const REG_DWORD: u32 = 4;
const SW_SHOWNORMAL: i32 = 1;

#[cfg(target_pointer_width = "64")]
const REGISTRY_VIEWS: &[u32] = &[KEY_WOW64_64KEY, KEY_WOW64_32KEY];
#[cfg(target_pointer_width = "32")]
const REGISTRY_VIEWS: &[u32] = &[0];

#[derive(Clone)]
pub(super) struct RegistryApplication {
  hive: RegistryHive,
  view: u32,
  key_name: String,
  display_name: String,
  version: Option<String>,
  package_family_name: Option<String>,
}

#[derive(Clone, Copy)]
enum RegistryHive {
  CurrentUser,
  LocalMachine,
}

struct RegistryKey(HKey);

type HKey = *mut c_void;

#[link(name = "advapi32")]
extern "system" {
  fn RegOpenKeyExW(
    key: HKey,
    subkey: *const u16,
    options: u32,
    access: u32,
    result: *mut HKey,
  ) -> i32;
  fn RegEnumKeyExW(
    key: HKey,
    index: u32,
    name: *mut u16,
    name_length: *mut u32,
    reserved: *mut u32,
    class: *mut u16,
    class_length: *mut u32,
    last_write_time: *mut c_void,
  ) -> i32;
  fn RegQueryValueExW(
    key: HKey,
    value_name: *const u16,
    reserved: *mut u32,
    value_type: *mut u32,
    data: *mut u8,
    data_length: *mut u32,
  ) -> i32;
  fn RegCloseKey(key: HKey) -> i32;
}

#[link(name = "shell32")]
extern "system" {
  fn ShellExecuteW(
    window: HKey,
    operation: *const u16,
    file: *const u16,
    parameters: *const u16,
    directory: *const u16,
    show_command: i32,
  ) -> isize;
}

impl Drop for RegistryKey {
  fn drop(&mut self) {
    unsafe {
      RegCloseKey(self.0);
    }
  }
}

pub(super) fn discover() -> DiscoverySnapshot {
  let mut snapshot = DiscoverySnapshot::default();
  snapshot.warnings.extend([
    "Application usage is not measured; no application is classified as unused.".to_owned(),
    "Win32 applications open Windows Apps & Features because the registry does not prove an unambiguous WinGet package ID. UninstallString values are never executed.".to_owned(),
    "Entries with a validated package family name open that exact app page in Windows Settings; all Settings actions are handoffs, not completed removals.".to_owned(),
  ]);

  for hive in [RegistryHive::CurrentUser, RegistryHive::LocalMachine] {
    for view in REGISTRY_VIEWS {
      match enumerate_hive(hive, *view) {
        Ok(applications) => {
          snapshot
            .applications
            .extend(applications.into_iter().map(|record| {
              let source = if record.package_family_name.is_some() {
                ApplicationSource::WindowsPackageManager
              } else {
                ApplicationSource::WindowsRegistry
              };
              PendingApplication {
                name: record.display_name.clone(),
                version: record.version.clone(),
                source,
                action: PlatformAction::WindowsRegistry(record),
              }
            }));
        }
        Err(code) => snapshot.warnings.push(format!(
          "Could not enumerate the {} application registry view {} (Windows error {}).",
          hive.label(),
          view_label(*view),
          code
        )),
      }
    }
  }
  snapshot
}

pub(super) fn uninstall(
  application: &InstalledApplication,
  action: &PlatformAction,
  request: &UninstallRequest,
) -> Result<UninstallResponse, PortError> {
  if !request.confirmed {
    return Err(PortError::ConfirmationRequired);
  }
  if application.id != request.application_id {
    return Err(PortError::OperationFailed {
      message: "The uninstall request does not match the selected application.".to_owned(),
    });
  }
  let PlatformAction::WindowsRegistry(record) = action;
  if !registration_is_current(record) {
    return Err(PortError::CandidateChanged);
  }
  let target =
    settings_uri(record.package_family_name.as_deref()).ok_or(PortError::OperationFailed {
      message: "The package family name contains unsupported characters.".to_owned(),
    })?;
  open_settings_uri(&target)?;

  Ok(UninstallResponse {
    application_id: application.id.clone(),
    status: UninstallStatus::DelegatedToSystem,
  })
}

fn enumerate_hive(hive: RegistryHive, view: u32) -> Result<Vec<RegistryApplication>, i32> {
  let uninstall_root = open_registry_key(hive.handle(), UNINSTALL_REGISTRY_PATH, view)?;
  let mut applications = Vec::new();
  let mut index = 0u32;

  loop {
    let mut name = vec![0u16; 1024];
    let mut name_length = name.len() as u32;
    let status = unsafe {
      RegEnumKeyExW(
        uninstall_root.0,
        index,
        name.as_mut_ptr(),
        &mut name_length,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
      )
    };
    if status == ERROR_NO_MORE_ITEMS {
      break;
    }
    if index == u32::MAX {
      break;
    }
    index += 1;
    if status != ERROR_SUCCESS {
      continue;
    }
    let key_name = std::ffi::OsString::from_wide(&name[..name_length as usize])
      .to_string_lossy()
      .into_owned();
    if key_name.is_empty() {
      continue;
    }
    let Some(key) = open_optional_registry_key(uninstall_root.0, &key_name, view) else {
      continue;
    };
    let display_name = read_registry_string(key.0, "DisplayName");
    let system_component = read_registry_dword(key.0, "SystemComponent") == Some(1);
    if !super::windows_entry_is_visible(display_name.as_deref(), system_component) {
      continue;
    }

    let package_family_name =
      read_registry_string(key.0, "PackageFamilyName").filter(|value| package_name_is_safe(value));
    applications.push(RegistryApplication {
      hive,
      view,
      key_name,
      display_name: display_name.unwrap_or_default(),
      version: read_registry_string(key.0, "DisplayVersion").and_then(|version| nonempty(&version)),
      package_family_name,
    });
  }
  Ok(applications)
}

fn registration_is_current(application: &RegistryApplication) -> bool {
  let Ok(root) = open_registry_key(
    application.hive.handle(),
    UNINSTALL_REGISTRY_PATH,
    application.view,
  ) else {
    return false;
  };
  let Some(key) = open_optional_registry_key(root.0, &application.key_name, application.view)
  else {
    return false;
  };
  let display_name = read_registry_string(key.0, "DisplayName");
  let system_component = read_registry_dword(key.0, "SystemComponent") == Some(1);
  super::windows_entry_is_visible(display_name.as_deref(), system_component)
    && display_name.as_deref() == Some(application.display_name.as_str())
    && read_registry_string(key.0, "PackageFamilyName").filter(|value| package_name_is_safe(value))
      == application.package_family_name
}

fn open_registry_key(hive: HKey, subkey: &str, view: u32) -> Result<RegistryKey, i32> {
  let wide_subkey = wide_null(subkey);
  let mut key = std::ptr::null_mut();
  let status = unsafe { RegOpenKeyExW(hive, wide_subkey.as_ptr(), 0, KEY_READ | view, &mut key) };
  if status == ERROR_SUCCESS {
    Ok(RegistryKey(key))
  } else {
    Err(status)
  }
}

fn open_optional_registry_key(parent: HKey, subkey: &str, view: u32) -> Option<RegistryKey> {
  open_registry_key(parent, subkey, view).ok()
}

fn read_registry_string(key: HKey, name: &str) -> Option<String> {
  let wide_name = wide_null(name);
  let mut value_type = 0u32;
  let mut byte_length = 0u32;
  let status = unsafe {
    RegQueryValueExW(
      key,
      wide_name.as_ptr(),
      std::ptr::null_mut(),
      &mut value_type,
      std::ptr::null_mut(),
      &mut byte_length,
    )
  };
  if status != ERROR_SUCCESS
    || !matches!(value_type, REG_SZ | REG_EXPAND_SZ)
    || byte_length > 1_048_576
  {
    return None;
  }
  let mut bytes = vec![0u8; byte_length as usize];
  let status = unsafe {
    RegQueryValueExW(
      key,
      wide_name.as_ptr(),
      std::ptr::null_mut(),
      &mut value_type,
      bytes.as_mut_ptr(),
      &mut byte_length,
    )
  };
  if status != ERROR_SUCCESS || !matches!(value_type, REG_SZ | REG_EXPAND_SZ) {
    return None;
  }
  bytes.truncate(byte_length as usize);
  let units: Vec<u16> = bytes
    .chunks_exact(2)
    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
    .take_while(|unit| *unit != 0)
    .collect();
  String::from_utf16(&units).ok()
}

fn read_registry_dword(key: HKey, name: &str) -> Option<u32> {
  let wide_name = wide_null(name);
  let mut value_type = 0u32;
  let mut byte_length = std::mem::size_of::<u32>() as u32;
  let mut bytes = [0u8; 4];
  let status = unsafe {
    RegQueryValueExW(
      key,
      wide_name.as_ptr(),
      std::ptr::null_mut(),
      &mut value_type,
      bytes.as_mut_ptr(),
      &mut byte_length,
    )
  };
  (status == ERROR_SUCCESS && value_type == REG_DWORD && byte_length == 4)
    .then(|| u32::from_le_bytes(bytes))
}

fn open_settings_uri(uri: &str) -> Result<(), PortError> {
  let operation = wide_null("open");
  let target = wide_null(uri);
  let result = unsafe {
    ShellExecuteW(
      std::ptr::null_mut(),
      operation.as_ptr(),
      target.as_ptr(),
      std::ptr::null(),
      std::ptr::null(),
      SW_SHOWNORMAL,
    )
  };
  if result <= 32 {
    return Err(PortError::OperationFailed {
      message: "Windows Settings could not open the selected application's page.".to_owned(),
    });
  }
  Ok(())
}

fn settings_uri(package_family_name: Option<&str>) -> Option<String> {
  match package_family_name {
    Some(package_family_name) if package_name_is_safe(package_family_name) => Some(format!(
      "ms-settings:appsfeatures-app?{package_family_name}"
    )),
    Some(_) => None,
    None => Some("ms-settings:appsfeatures".to_owned()),
  }
}

fn wide_null(value: &str) -> Vec<u16> {
  value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn nonempty(value: &str) -> Option<String> {
  let value = value.trim();
  (!value.is_empty()).then(|| value.to_owned())
}

fn view_label(view: u32) -> &'static str {
  match view {
    KEY_WOW64_64KEY => "64-bit",
    KEY_WOW64_32KEY => "32-bit",
    _ => "native",
  }
}

impl RegistryHive {
  fn handle(self) -> HKey {
    let raw = match self {
      RegistryHive::CurrentUser => 0x8000_0001u32 as i32 as isize,
      RegistryHive::LocalMachine => 0x8000_0002u32 as i32 as isize,
    };
    raw as HKey
  }

  fn label(self) -> &'static str {
    match self {
      RegistryHive::CurrentUser => "current-user",
      RegistryHive::LocalMachine => "local-machine",
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn settings_uri_uses_only_a_validated_package_family_name() {
    assert_eq!(
      settings_uri(Some("Contoso.Editor_123abc")).as_deref(),
      Some("ms-settings:appsfeatures-app?Contoso.Editor_123abc")
    );
    assert_eq!(
      settings_uri(None).as_deref(),
      Some("ms-settings:appsfeatures")
    );
    assert_eq!(settings_uri(Some("bad/name")), None);
    assert_eq!(settings_uri(Some("?other-app")), None);
  }
}
