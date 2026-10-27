// Controller blocking, built in: HidHide's signed driver (Nefarius, MIT) without the rest of its install.
// The driver files ship in resources/vendor/hidhide and are installed by this app itself (the same steps as HidHide's
// own setup: a root\HidHide device plus an upper filter on the HID, XnaComposite and XboxComposite classes), so there
// is no HidHide program, shortcut or Programs entry. Installing and removing need admin, so the app runs itself
// elevated for just that step (`--hidhide install <dir>` / `--hidhide remove`). Everything else talks to the driver's
// control device directly, which any user may do. Driver commands are from HidHide's Shared/HidHideIoctlContract.h.

use std::ffi::c_void;
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::core::GUID;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetPreparsedData, HidD_GetProductString, HidP_GetCaps, GUID_DEVINTERFACE_HID,
    HIDD_ATTRIBUTES, HIDP_CAPS,
};
use windows_sys::Win32::Devices::Properties::{DEVPKEY_Device_InstanceId, DEVPKEY_Device_Stack};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, ReadFile, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegGetValueW, RegOpenKeyExW, RegSetKeyValueW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_MULTI_SZ, RRF_RT_REG_MULTI_SZ,
};
use windows_sys::Win32::System::Services::{CloseServiceHandle, DeleteService, OpenSCManagerW, OpenServiceW, SC_MANAGER_CONNECT};
use windows_sys::Win32::System::IO::DeviceIoControl;

const HARDWARE_ID: &str = r"root\HidHide";
// the classes HidHide filters: HID devices, and the Xbox 360 / Xbox One (XInput) controllers
const FILTER_CLASSES: [&str; 3] = [
    "{745a17a0-74d3-11d0-b6fe-00a0c90f57da}",
    "{d61ca365-5af4-4486-998b-9db4734c6ca3}",
    "{05f5cfe2-4733-4950-a6bb-07aad01a3a84}",
];
const GUID_DEVINTERFACE_XUSB: GUID = GUID::from_u128(0xec87f1e3_c13b_4100_b5f7_8b84d54260cb);
// HidHide's standalone install: when it's there, its own uninstaller owns the driver, not ours
const NEFARIUS_DIR: &str = r"C:\Program Files\Nefarius Software Solutions\HidHide";
const MARKER: &str = r"aLogs\hidhide-driver"; // under %ProgramData%: this app installed the driver

const fn ioctl(function: u32) -> u32 {
    (32769 << 16) | (1 << 14) | (function << 2) // CTL_CODE(32769, function, METHOD_BUFFERED, FILE_READ_DATA)
}
const IOCTL_SET_BLACKLIST: u32 = ioctl(2051);
const IOCTL_GET_BLACKLIST: u32 = ioctl(2050);
const IOCTL_SET_ACTIVE: u32 = ioctl(2053);
const IOCTL_GET_ACTIVE: u32 = ioctl(2052);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn multi_sz(items: &[String]) -> Vec<u16> {
    let mut out: Vec<u16> = items.iter().flat_map(|s| s.encode_utf16().chain(Some(0))).collect();
    out.push(0);
    if items.is_empty() {
        out.push(0);
    }
    out
}
fn split_multi_sz(buf: &[u16]) -> Vec<String> {
    buf.split(|&c| c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect()
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}
fn open(path: &str) -> Option<Handle> {
    let h = unsafe {
        CreateFileW(wide(path).as_ptr(), 0x8000_0000, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, 0, null_mut())
    };
    (h != INVALID_HANDLE_VALUE).then_some(Handle(h))
}

// ---------------------------------------------------------------- talking to the driver

/// Is the driver running (installed by this app, or by HidHide's own setup)?
pub fn installed() -> bool {
    open(r"\\.\HidHide").is_some()
}

/// Is HidHide registered as a filter on the controller classes? Without it the driver runs but hides nothing.
pub fn filters_registered() -> bool {
    filter_on(FILTER_CLASSES[0])
}

/// Is HidHide in this device's driver stack right now (else it needs a restart of the device: replug, or setup)?
fn in_stack(id: &str) -> bool {
    let mut node = 0u32;
    if unsafe { CM_Locate_DevNodeW(&mut node, wide(id).as_ptr(), CM_LOCATE_DEVNODE_NORMAL) } != CR_SUCCESS {
        return false;
    }
    let mut buf = vec![0u16; 1024];
    let (mut size, mut kind) = ((buf.len() * 2) as u32, 0u32);
    let ok = unsafe { CM_Get_DevNode_PropertyW(node, &DEVPKEY_Device_Stack, &mut kind, buf.as_mut_ptr() as *mut u8, &mut size, 0) };
    ok == CR_SUCCESS && split_multi_sz(&buf).iter().any(|d| d.to_ascii_lowercase().ends_with("\\hidhide"))
}

fn control(code: u32, input: &[u8]) -> Result<Vec<u8>, String> {
    if input.len() > 0 || code == IOCTL_SET_BLACKLIST {
        let device = open(r"\\.\HidHide").ok_or("Controller blocking isn't installed.")?;
        let mut got = 0u32;
        let ok = unsafe { DeviceIoControl(device.0, code, input.as_ptr() as *const c_void, input.len() as u32, null_mut(), 0, &mut got, null_mut()) };
        if ok == 0 {
            return Err(format!("The HidHide driver refused the request (error {}).", unsafe { GetLastError() }));
        }
        return Ok(Vec::new());
    }
    let device = open(r"\\.\HidHide").ok_or("Controller blocking isn't installed.")?;
    let call = |out: &mut [u8], got: &mut u32| unsafe {
        let out_ptr = if out.is_empty() { null_mut() } else { out.as_mut_ptr() as *mut c_void };
        DeviceIoControl(device.0, code, input.as_ptr() as *const c_void, input.len() as u32, out_ptr, out.len() as u32, got, null_mut())
    };
    // a read asks for the size first: the driver wants the exact buffer (like HidHide's CLI does it)
    let mut got = 0u32;
    if call(&mut [], &mut got) == 0 {
        return Err(format!("The HidHide driver refused the request (error {}).", unsafe { GetLastError() }));
    }
    let mut out = vec![0u8; got as usize];
    if !out.is_empty() && call(&mut out, &mut got) == 0 {
        return Err(format!("The HidHide driver refused the request (error {}).", unsafe { GetLastError() }));
    }
    out.truncate(got as usize);
    Ok(out)
}

/// Hide the blocked controllers from every app (on), or show them again (off).
pub fn set_active(on: bool) -> Result<(), String> {
    control(IOCTL_SET_ACTIVE, &[on as u8]).map(|_| ())
}

/// Is hiding switched on right now (the meter turns it on only for the moment it waits on a result screen)?
pub fn active() -> Result<bool, String> {
    let device = open(r"\\.\HidHide").ok_or("Controller blocking isn't installed.")?;
    let (mut out, mut got) = ([0u8; 1], 0u32);
    let ok = unsafe { DeviceIoControl(device.0, IOCTL_GET_ACTIVE, null(), 0, out.as_mut_ptr() as *mut c_void, 1, &mut got, null_mut()) };
    if ok == 0 {
        return Err(format!("The HidHide driver refused the request (error {}).", unsafe { GetLastError() }));
    }
    Ok(out[0] != 0)
}

fn block_list() -> Result<Vec<String>, String> {
    let raw = control(IOCTL_GET_BLACKLIST, &[])?;
    Ok(split_multi_sz(&raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()))
}
fn listed(list: &[String], id: &str) -> bool {
    list.iter().any(|x| x.eq_ignore_ascii_case(id))
}

/// Put every connected game controller on the block list (keeping what's on it already); returns how many.
pub fn block_connected_controllers() -> Result<usize, String> {
    set_active(false)?; // a hidden controller can't be opened to check what it is
    let mut list = block_list()?;
    let found = pads();
    for id in found.iter().flat_map(|p| p.ids()) {
        if !listed(&list, &id) {
            list.push(id);
        }
    }
    let bytes: Vec<u8> = multi_sz(&list).iter().flat_map(|c| c.to_le_bytes()).collect();
    control(IOCTL_SET_BLACKLIST, &bytes)?;
    Ok(found.len())
}

/// Everything the Controller section shows: is the driver there, is hiding on right now, and each connected game
/// controller by name with whether it is on the block list.
pub fn status() -> serde_json::Value {
    if !installed() {
        return serde_json::json!({ "installed": false, "controllers": pads().iter().map(|p| serde_json::json!({ "name": p.name, "blocked": false })).collect::<Vec<_>>() });
    }
    let list = block_list().unwrap_or_default();
    let found = pads();
    let blocked_here: usize = found.iter().map(|p| p.ids().iter().filter(|id| listed(&list, id)).count()).sum();
    serde_json::json!({
        "installed": true,
        "hiding": active().unwrap_or(false),
        "filters": filters_registered(),
        "controllers": found.iter().map(|p| { watch(p); serde_json::json!({
            "id": p.hid,
            "name": p.name,
            "xinput": p.xusb.is_some(),
            "blocked": p.ids().iter().all(|id| listed(&list, id)),
            "attached": p.ids().iter().all(|id| in_stack(id)),
            "hidAttached": in_stack(&p.hid),
            "pressedMsAgo": pressed_ms_ago(&p.hid),
        }) }).collect::<Vec<_>>(),
        "listedElsewhere": list.len().saturating_sub(blocked_here),   // on the list but not connected now
    })
}

/// Put one connected controller on the block list, or take it off (its HID and XInput sides together).
pub fn set_blocked(hid_id: &str, on: bool) -> Result<(), String> {
    let pad = pads().into_iter().find(|p| p.hid.eq_ignore_ascii_case(hid_id)).ok_or("That controller isn't connected any more.")?;
    let mut list = block_list()?;
    for id in pad.ids() {
        if on && !listed(&list, &id) {
            list.push(id);
        } else if !on {
            list.retain(|x| !x.eq_ignore_ascii_case(&id));
        }
    }
    let bytes: Vec<u8> = multi_sz(&list).iter().flat_map(|c| c.to_le_bytes()).collect();
    control(IOCTL_SET_BLACKLIST, &bytes).map(|_| ())
}

// Which controller is which: a reader per controller notes when its input changes (a button, a stick), so its row
// can light up while you press it. Readers stop by themselves when the controller goes away or is hidden.
static PRESSED: std::sync::Mutex<Vec<(String, std::time::Instant)>> = std::sync::Mutex::new(Vec::new());
static WATCHED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn pressed_ms_ago(hid: &str) -> Option<u64> {
    let pressed = PRESSED.lock().unwrap_or_else(|e| e.into_inner());
    pressed.iter().find(|(id, _)| id == hid).map(|(_, at)| at.elapsed().as_millis() as u64)
}

fn watch(pad: &Pad) {
    {
        let mut watched = WATCHED.lock().unwrap_or_else(|e| e.into_inner());
        if watched.contains(&pad.hid) {
            return;
        }
        watched.push(pad.hid.clone());
    }
    let (hid, link, len) = (pad.hid.clone(), pad.link.clone(), pad.report_len.max(1) as usize);
    std::thread::spawn(move || {
        let h = unsafe { CreateFileW(wide(&link).as_ptr(), 0x8000_0000, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, 0, null_mut()) };
        if h != INVALID_HANDLE_VALUE {
            let h = Handle(h);
            let (mut last, mut buf) = (Vec::new(), vec![0u8; len]);
            loop {
                let mut got = 0u32;
                if unsafe { ReadFile(h.0, buf.as_mut_ptr(), len as u32, &mut got, null_mut()) } == 0 {
                    break;
                }
                let report = buf[..got as usize].to_vec();
                if !last.is_empty() && report != last {
                    let mut pressed = PRESSED.lock().unwrap_or_else(|e| e.into_inner());
                    pressed.retain(|(id, _)| id != &hid);
                    pressed.push((hid.clone(), std::time::Instant::now()));
                }
                last = report;
            }
        }
        WATCHED.lock().unwrap_or_else(|e| e.into_inner()).retain(|id| id != &hid);
    });
}

/// Proves blocking works: hides the blocked controllers for a moment, checks this app can no longer open them (as
/// the game couldn't), then shows them again. Returns one line per controller.
pub fn test_blocking() -> Result<Vec<String>, String> {
    let list = block_list()?;
    let targets: Vec<Pad> = pads().into_iter().filter(|p| p.ids().iter().all(|id| listed(&list, id))).collect();
    if targets.is_empty() {
        return Err("No connected controller is on the block list yet: press Set up controller blocking first.".into());
    }
    set_active(true)?;
    std::thread::sleep(std::time::Duration::from_millis(800));
    let hidden: Vec<bool> = targets.iter().map(|p| open_hid(&p.link).is_none()).collect();
    set_active(false)?;                                         // always back on, whatever the result
    Ok(targets.iter().zip(hidden).map(|(p, h)| format!("{}: {}", p.name, if h { "hidden from apps, then shown again" } else { "NOT hidden (try unplugging and replugging it once)" })).collect())
}

/// A connected game controller: its HID device (HidHide's own test: usage page 1 usage 4 or 5, usage page 5, or a
/// Steam controller) and, for XInput pads, the XUSB device above it, which is what most games read.
struct Pad {
    name: String,
    link: String,          // the HID interface path, to open it
    hid: String,           // device instance paths, for the block list
    xusb: Option<String>,
    report_len: u16,       // its input report size, for the "which one is it" light
}
impl Pad {
    fn ids(&self) -> Vec<String> {
        self.xusb.iter().cloned().chain(Some(self.hid.clone())).collect()
    }
}

fn pads() -> Vec<Pad> {
    let mut out: Vec<Pad> = Vec::new();
    for link in interface_list(&GUID_DEVINTERFACE_HID, None) {
        let Some((attributes, caps, product)) = open_hid(&link) else { continue };
        let steam = attributes.VendorID == 0x28DE && matches!(attributes.ProductID, 0x1142 | 0x1205);
        let gaming = steam || caps.UsagePage == 5 || (caps.UsagePage == 1 && matches!(caps.Usage, 4 | 5));
        if !gaming {
            continue;
        }
        let Some(hid) = instance_id(&link) else { continue };
        if out.iter().any(|p| p.hid.eq_ignore_ascii_case(&hid)) {
            continue;
        }
        let xusb = xusb_ancestor(&hid);
        let name = if product.trim().is_empty() { format!("Controller {:04X}:{:04X}", attributes.VendorID, attributes.ProductID) } else { product };
        out.push(Pad { name, link, hid, xusb, report_len: caps.InputReportByteLength });
    }
    out
}

fn open_hid(link: &str) -> Option<(HIDD_ATTRIBUTES, HIDP_CAPS, String)> {
    let h = unsafe { CreateFileW(wide(link).as_ptr(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, 0, null_mut()) };
    if h == INVALID_HANDLE_VALUE {
        return None;
    }
    let h = Handle(h);
    unsafe {
        let mut attributes: HIDD_ATTRIBUTES = std::mem::zeroed();
        attributes.Size = std::mem::size_of::<HIDD_ATTRIBUTES>() as u32;
        if !HidD_GetAttributes(h.0, &mut attributes) {
            return None;
        }
        let mut data = 0;
        if !HidD_GetPreparsedData(h.0, &mut data) {
            return None;
        }
        let mut caps: HIDP_CAPS = std::mem::zeroed();
        let status = HidP_GetCaps(data, &mut caps);
        HidD_FreePreparsedData(data);
        let mut name = [0u16; 128];
        let product = if HidD_GetProductString(h.0, name.as_mut_ptr() as *mut c_void, (name.len() * 2) as u32) {
            split_multi_sz(&name).into_iter().next().unwrap_or_default()
        } else {
            String::new()
        };
        (status >= 0).then_some((attributes, caps, product))
    }
}

fn interface_list(class: &GUID, device: Option<&str>) -> Vec<String> {
    let device = device.map(wide);
    let device_ptr = device.as_ref().map_or(null(), |d| d.as_ptr());
    let mut len = 0u32;
    unsafe {
        if CM_Get_Device_Interface_List_SizeW(&mut len, class, device_ptr, CM_GET_DEVICE_INTERFACE_LIST_PRESENT) != CR_SUCCESS {
            return Vec::new();
        }
        let mut buf = vec![0u16; len as usize];
        if CM_Get_Device_Interface_ListW(class, device_ptr, buf.as_mut_ptr(), len, CM_GET_DEVICE_INTERFACE_LIST_PRESENT) != CR_SUCCESS {
            return Vec::new();
        }
        split_multi_sz(&buf)
    }
}

fn instance_id(link: &str) -> Option<String> {
    let mut buf = vec![0u16; 512];
    let mut size = (buf.len() * 2) as u32;
    let mut kind = 0u32;
    let ok = unsafe {
        CM_Get_Device_Interface_PropertyW(wide(link).as_ptr(), &DEVPKEY_Device_InstanceId, &mut kind,
                                          buf.as_mut_ptr() as *mut u8, &mut size, 0)
    };
    (ok == CR_SUCCESS).then(|| split_multi_sz(&buf).into_iter().next()).flatten()
}

fn xusb_ancestor(id: &str) -> Option<String> {
    let mut node = 0u32;
    if unsafe { CM_Locate_DevNodeW(&mut node, wide(id).as_ptr(), CM_LOCATE_DEVNODE_NORMAL) } != CR_SUCCESS {
        return None;
    }
    for _ in 0..8 {
        let mut parent = 0u32;
        if unsafe { CM_Get_Parent(&mut parent, node, 0) } != CR_SUCCESS {
            return None;
        }
        node = parent;
        let mut buf = vec![0u16; 512];
        if unsafe { CM_Get_Device_IDW(node, buf.as_mut_ptr(), buf.len() as u32, 0) } != CR_SUCCESS {
            return None;
        }
        let parent_id = split_multi_sz(&buf).into_iter().next()?;
        if !interface_list(&GUID_DEVINTERFACE_XUSB, Some(&parent_id)).is_empty() {
            return Some(parent_id);
        }
    }
    None
}

// ---------------------------------------------------------------- install / remove (admin)

/// Run this app elevated for one admin step; Windows shows its approval prompt.
pub fn run_elevated(args: &str) -> Result<(), String> {
    // Windows' own "Run as administrator" (the same as right-click > Run as administrator): ShellExecuteEx called from
    // a background thread could hang without ever showing the prompt.
    use std::os::windows::process::CommandExt;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let quote = |s: &str| s.replace('\'', "''");
    let script = format!(
        "try {{ $p = Start-Process -FilePath '{}' -ArgumentList '{}' -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode }} catch {{ exit 1223 }}",
        quote(&exe.to_string_lossy()), quote(args));
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .creation_flags(0x0800_0000)
        .status()
        .map_err(|e| e.to_string())?;
    match status.code() {
        Some(0) => Ok(()),
        Some(1223) => Err("Admin approval is needed to set up controller blocking (the prompt was declined).".into()),
        code => Err(format!("Setting up the HidHide driver failed (code {}).", code.unwrap_or(-1))),
    }
}

/// `alogs-app.exe --hidhide install <driver dir>` / `--hidhide remove`, run elevated; returns the exit code.
pub fn admin_task(args: &[String]) -> i32 {
    let result = match args.first().map(String::as_str) {
        Some("install") => args.get(1).ok_or(0u32).and_then(|dir| install(Path::new(dir))),
        Some("remove") => remove(),
        _ => Err(0),
    };
    match result {
        Ok(()) => 0,
        Err(code) => code.max(1) as i32,
    }
}

fn install(dir: &Path) -> Result<(), u32> {
    if !installed() {
        install_driver(dir)?;
    }
    for class in FILTER_CLASSES {
        set_class_filter(class, true);
    }
    restart_controllers();
    Ok(())
}

/// Restart the connected game controllers so their driver stacks pick up the HidHide filter (what a replug does).
fn restart_controllers() {
    for id in pads().iter().flat_map(|p| p.ids()) {
        unsafe {
            let set = SetupDiCreateDeviceInfoList(null(), null_mut());
            if set == INVALID_HANDLE_VALUE as isize {
                continue;
            }
            let mut dev: SP_DEVINFO_DATA = std::mem::zeroed();
            dev.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
            if SetupDiOpenDeviceInfoW(set, wide(&id).as_ptr(), null_mut(), 0, &mut dev) != 0 {
                let mut params: SP_PROPCHANGE_PARAMS = std::mem::zeroed();
                params.ClassInstallHeader.cbSize = std::mem::size_of::<SP_CLASSINSTALL_HEADER>() as u32;
                params.ClassInstallHeader.InstallFunction = DIF_PROPERTYCHANGE;
                params.StateChange = DICS_PROPCHANGE;
                params.Scope = DICS_FLAG_CONFIGSPECIFIC;
                if SetupDiSetClassInstallParamsW(set, &dev, &params.ClassInstallHeader, std::mem::size_of::<SP_PROPCHANGE_PARAMS>() as u32) != 0 {
                    SetupDiCallClassInstaller(DIF_PROPERTYCHANGE, set, &dev);
                }
            }
            SetupDiDestroyDeviceInfoList(set);
        }
    }
}

fn install_driver(dir: &Path) -> Result<(), u32> {
    let inf = wide(&dir.join("HidHide.inf").to_string_lossy());
    let hwid = multi_sz(&[HARDWARE_ID.to_string()]);
    unsafe {
        // the root\HidHide device (like `devcon install`), then its driver
        if !root_devices().is_empty() {
            // left over from an earlier install: just (re)install the driver on it
        } else {
            let mut class = std::mem::zeroed::<GUID>();
            let mut name = [0u16; 64];
            if SetupDiGetINFClassW(inf.as_ptr(), &mut class, name.as_mut_ptr(), name.len() as u32, null_mut()) == 0 {
                return Err(GetLastError());
            }
            let set = SetupDiCreateDeviceInfoList(&class, null_mut());
            if set == INVALID_HANDLE_VALUE as isize {
                return Err(GetLastError());
            }
            let mut dev: SP_DEVINFO_DATA = std::mem::zeroed();
            dev.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
            let ok = SetupDiCreateDeviceInfoW(set, name.as_ptr(), &class, null(), null_mut(), DICD_GENERATE_ID, &mut dev) != 0
                && SetupDiSetDeviceRegistryPropertyW(set, &mut dev, SPDRP_HARDWAREID, hwid.as_ptr() as *const u8, (hwid.len() * 2) as u32) != 0
                && SetupDiCallClassInstaller(DIF_REGISTERDEVICE, set, &dev) != 0;
            let error = GetLastError();
            SetupDiDestroyDeviceInfoList(set);
            if !ok {
                return Err(error);
            }
        }
        let mut reboot = 0;
        if UpdateDriverForPlugAndPlayDevicesW(null_mut(), wide(HARDWARE_ID).as_ptr(), inf.as_ptr(), INSTALLFLAG_FORCE, &mut reboot) == 0 {
            return Err(GetLastError());
        }
    }
    for class in FILTER_CLASSES {
        set_class_filter(class, true);
    }
    if let Some(marker) = marker() {
        let _ = std::fs::create_dir_all(marker.parent().unwrap());
        let _ = std::fs::write(marker, "Installed by aLogs; removed by its uninstaller.\n");
    }
    Ok(())
}

fn remove() -> Result<(), u32> {
    if Path::new(NEFARIUS_DIR).exists() {
        return Ok(()); // the standalone HidHide is installed: leave its driver alone
    }
    for class in FILTER_CLASSES {
        set_class_filter(class, false);
    }
    unsafe {
        let set = SetupDiGetClassDevsW(null(), wide("ROOT").as_ptr(), null_mut(), DIGCF_ALLCLASSES);
        for index in root_devices() {
            let mut dev: SP_DEVINFO_DATA = std::mem::zeroed();
            dev.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
            if SetupDiEnumDeviceInfo(set, index, &mut dev) != 0 {
                DiUninstallDevice(null_mut(), set, &dev, 0, null_mut());
            }
        }
        SetupDiDestroyDeviceInfoList(set);
        // the driver package in Windows' driver store (oemNN.inf)
        if let Ok(entries) = std::fs::read_dir(r"C:\Windows\INF") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !(name.to_ascii_lowercase().starts_with("oem") && name.to_ascii_lowercase().ends_with(".inf")) {
                    continue;
                }
                let text: String = std::fs::read(entry.path()).unwrap_or_default().iter().filter(|&&b| b != 0).map(|&b| b as char).collect();
                if text.to_ascii_lowercase().contains("hidhide.sys") {
                    SetupUninstallOEMInfW(wide(&name).as_ptr(), SUOI_FORCEDELETE, null());
                }
            }
        }
        let scm = OpenSCManagerW(null(), null(), SC_MANAGER_CONNECT);
        if !scm.is_null() {
            let service = OpenServiceW(scm, wide("HidHide").as_ptr(), 0x0001_0000); // DELETE
            if !service.is_null() {
                DeleteService(service);
                CloseServiceHandle(service);
            }
            CloseServiceHandle(scm);
        }
    }
    if let Some(marker) = marker() {
        let _ = std::fs::remove_file(marker);
    }
    Ok(())
}

fn marker() -> Option<std::path::PathBuf> {
    std::env::var_os("ProgramData").map(|d| Path::new(&d).join(MARKER))
}

/// Indexes (in the ROOT device list) of root\HidHide devices.
fn root_devices() -> Vec<u32> {
    let mut found = Vec::new();
    unsafe {
        let set = SetupDiGetClassDevsW(null(), wide("ROOT").as_ptr(), null_mut(), DIGCF_ALLCLASSES);
        if set == INVALID_HANDLE_VALUE as isize {
            return found;
        }
        let mut index = 0;
        loop {
            let mut dev: SP_DEVINFO_DATA = std::mem::zeroed();
            dev.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
            if SetupDiEnumDeviceInfo(set, index, &mut dev) == 0 {
                break;
            }
            let mut buf = vec![0u16; 512];
            if SetupDiGetDeviceRegistryPropertyW(set, &dev, SPDRP_HARDWAREID, null_mut(), buf.as_mut_ptr() as *mut u8, (buf.len() * 2) as u32, null_mut()) != 0
                && split_multi_sz(&buf).iter().any(|id| id.eq_ignore_ascii_case(HARDWARE_ID))
            {
                found.push(index);
            }
            index += 1;
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    found
}

fn filter_on(class: &str) -> bool {
    let key = wide(&format!(r"SYSTEM\CurrentControlSet\Control\Class\{class}"));
    let mut buf = vec![0u16; 4096];
    let mut size = (buf.len() * 2) as u32;
    let read = unsafe {
        RegGetValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), wide("UpperFilters").as_ptr(), RRF_RT_REG_MULTI_SZ, null_mut(), buf.as_mut_ptr() as *mut c_void, &mut size)
    };
    read == 0 && split_multi_sz(&buf[..(size as usize / 2)]).iter().any(|f| f.eq_ignore_ascii_case("HidHide"))
}

/// Add HidHide to (or take it off) a device class's upper filters. A class that isn't on this PC yet is skipped.
fn set_class_filter(class: &str, on: bool) {
    let key = wide(&format!(r"SYSTEM\CurrentControlSet\Control\Class\{class}"));
    let value = wide("UpperFilters");
    let mut opened: HKEY = null_mut();
    if unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, key.as_ptr(), 0, KEY_READ, &mut opened) } != 0 {
        return;
    }
    unsafe { RegCloseKey(opened) };
    let mut buf = vec![0u16; 4096];
    let mut size = (buf.len() * 2) as u32;
    let read = unsafe {
        RegGetValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), value.as_ptr(), RRF_RT_REG_MULTI_SZ, null_mut(), buf.as_mut_ptr() as *mut c_void, &mut size)
    };
    let mut filters = if read == 0 { split_multi_sz(&buf[..(size as usize / 2)]) } else { Vec::new() };
    let has = filters.iter().any(|f| f.eq_ignore_ascii_case("HidHide"));
    if on == has {
        return;
    }
    if on {
        filters.push("HidHide".into());
    } else {
        filters.retain(|f| !f.eq_ignore_ascii_case("HidHide"));
    }
    let data = multi_sz(&filters);
    unsafe {
        RegSetKeyValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), value.as_ptr(), REG_MULTI_SZ, data.as_ptr() as *const c_void, (data.len() * 2) as u32);
    }
}

#[cfg(test)]
mod tests {
    // read-only: lists what setup would block, and the driver's current block list
    #[test]
    fn list_controllers() {
        println!("status: {:#}", super::status());
        std::thread::sleep(std::time::Duration::from_millis(1500));
        println!("readers still running (device opened for reading): {:?}", super::WATCHED.lock().unwrap());
        if super::installed() {
            let raw = super::control(super::IOCTL_GET_BLACKLIST, &[]).unwrap();
            let wide: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            println!("block list: {:#?}", super::split_multi_sz(&wide));
        }
    }
}

