//! Handing things to Windows: opening files and folders, Explorer with a file selected,
//! "Open with", the folder picker and the Save as dialog, and the clipboard.

use super::App;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
                                  COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE};
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const CF_UNICODETEXT: u32 = 13;

/// Windows' shell calls return once the other program has started (it can take a second or
/// more): they run on a thread of their own, so no window waits for them.
fn in_background(action: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        // SAFETY: COM for this thread, as the shell wants it; ended with the thread
        let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        action();
        if com.is_ok() {
            unsafe { CoUninitialize() };
        }
    });
}

/// Open with its default program, like a double-click in Explorer.
pub fn open(path: &Path) {
    let path = HSTRING::from(path.as_os_str());
    // SAFETY: plain shell call
    in_background(move || unsafe { ShellExecuteW(None, w!("open"), &path, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL); });
}

pub fn open_download(app: &Rc<App>, id: &str) {
    if let Some(s) = app.manager.get(id) {
        open(&s.path);
    }
}

/// Explorer with the download selected (or its folder, if the file is gone).
pub fn show_download(app: &Rc<App>, id: &str) {
    let Some(s) = app.manager.get(id) else { return };
    if !s.path.exists() {
        return open(&s.directory);
    }
    let path = HSTRING::from(s.path.as_os_str());
    // SAFETY: an item list for the path, freed after use
    in_background(move || unsafe {
        let item = ILCreateFromPathW(&path);
        if !item.is_null() {
            let _ = SHOpenFolderAndSelectItems(item, None, 0);
            ILFree(Some(item));
        }
    });
}

/// Windows' "Open with" chooser.
pub fn open_with(path: &Path) {
    let _ = std::process::Command::new("rundll32.exe").arg("shell32.dll,OpenAs_RunDLL").arg(path).spawn();
}

/// Windows' folder picker, starting at `initial`.
pub fn choose_folder(owner: HWND, title: &str, initial: &Path) -> Option<PathBuf> {
    // SAFETY: Windows' file dialog (COM, initialized on this thread at start)
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        dialog.SetOptions(FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST).ok()?;
        let _ = dialog.SetTitle(&HSTRING::from(title));
        if let Ok(folder) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(initial.as_os_str()), None) {
            let _ = dialog.SetFolder(&folder);
        }
        dialog.Show(Some(owner)).ok()?; // cancelled: an error
        let name = dialog.GetResult().ok()?.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = PathBuf::from(name.to_string().ok()?);
        CoTaskMemFree(Some(name.0 as *const _));
        Some(path)
    }
}

/// Windows' Open dialog for one file, starting in `folder`, showing files of `kind` (its name,
/// its pattern: "Text files", "*.txt") or all.
pub fn choose_open(owner: HWND, title: &str, folder: &Path, kind: (&str, &str)) -> Option<PathBuf> {
    // SAFETY: Windows' file dialog (COM, initialized on this thread at start)
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        dialog.SetOptions(FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST).ok()?;
        let _ = dialog.SetTitle(&HSTRING::from(title));
        let (name, pattern) = (HSTRING::from(kind.0), HSTRING::from(kind.1));
        let (all, every) = (HSTRING::from("All files"), HSTRING::from("*.*"));
        let types = [COMDLG_FILTERSPEC { pszName: PCWSTR(name.as_ptr()), pszSpec: PCWSTR(pattern.as_ptr()) },
                     COMDLG_FILTERSPEC { pszName: PCWSTR(all.as_ptr()), pszSpec: PCWSTR(every.as_ptr()) }];
        let _ = dialog.SetFileTypes(&types);
        if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(folder.as_os_str()), None) {
            let _ = dialog.SetFolder(&item);
        }
        dialog.Show(Some(owner)).ok()?; // cancelled: an error
        let name = dialog.GetResult().ok()?.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = PathBuf::from(name.to_string().ok()?);
        CoTaskMemFree(Some(name.0 as *const _));
        Some(path)
    }
}

/// Windows' "Save as" dialog, starting at `initial` (a file's whole path); the path picked.
/// (No "replace it?" question: a name that's taken gets a number added, like "file (1).zip".)
pub fn choose_save(owner: HWND, title: &str, initial: &Path) -> Option<PathBuf> {
    // SAFETY: Windows' file dialog (COM, initialized on this thread at start)
    unsafe {
        let dialog: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        dialog.SetOptions(FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOREADONLYRETURN).ok()?;
        let _ = dialog.SetTitle(&HSTRING::from(title));
        // the folder (or the nearest one above it that exists yet)
        if let Some(folder) = initial.parent().and_then(|p| p.ancestors().find(|a| a.is_dir()))
            && let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(folder.as_os_str()), None) {
            let _ = dialog.SetFolder(&item);
        }
        if let Some(name) = initial.file_name() {
            let _ = dialog.SetFileName(&HSTRING::from(name));
        }
        dialog.Show(Some(owner)).ok()?; // cancelled: an error
        let name = dialog.GetResult().ok()?.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = PathBuf::from(name.to_string().ok()?);
        CoTaskMemFree(Some(name.0 as *const _));
        Some(path)
    }
}

pub fn copy_text(text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    // SAFETY: the clipboard takes ownership of the memory once set
    unsafe {
        if OpenClipboard(None).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(memory) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
            let target = GlobalLock(memory) as *mut u16;
            if !target.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), target, wide.len());
                let _ = GlobalUnlock(memory);
                let _ = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(memory.0)));
            }
        }
        let _ = CloseClipboard();
    }
}

pub fn clipboard_text() -> Option<String> {
    // SAFETY: reads the clipboard's text while it's open; the memory stays the clipboard's
    unsafe {
        OpenClipboard(None).ok()?;
        let text = GetClipboardData(CF_UNICODETEXT).ok().and_then(|data| {
            let memory = HGLOBAL(data.0);
            let source = GlobalLock(memory) as *const u16;
            if source.is_null() {
                return None;
            }
            let length = (0..).take_while(|&i| *source.add(i) != 0).count();
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(source, length));
            let _ = GlobalUnlock(memory);
            Some(text)
        });
        let _ = CloseClipboard();
        text
    }
}
