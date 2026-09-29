#![cfg(target_os = "windows")]

use arboard::Clipboard;

use std::{
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use windows::Win32::UI::{
    Input::KeyboardAndMouse::{
        GetAsyncKeyState,
        SendInput,
        INPUT,
        INPUT_0,
        INPUT_KEYBOARD,
        KEYBDINPUT,
        KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP,
        VK_C,
        VK_CONTROL,
        VK_SHIFT,
    },
    WindowsAndMessaging::{
        GetForegroundWindow,
        GetWindowTextW,
        GetWindowThreadProcessId,
    },
};

#[derive(Debug, Clone)]
pub struct SelectionData {
    pub text: String,
    pub window_title: String,
    pub source_path: Option<String>,
    pub source_type: String,
}

pub fn capture_selection() -> Result<SelectionData, String> {
    println!("Attempting to capture current selection...");

    let foreground_window = unsafe {
        GetForegroundWindow()
    };

    if foreground_window.is_invalid() {
        return Err(
            "Could not determine the foreground window.".into()
        );
    }

    let window_title =
        get_window_title(foreground_window);

    println!(
        "Foreground window: {}",
        window_title
    );

    let process_id =
        get_foreground_process_id(foreground_window)?;

    println!(
        "Foreground process ID: {}",
        process_id
    );

    let process_info =
        get_process_info(process_id)?;

    println!(
        "Process: {}",
        process_info.name
    );

    println!(
        "Command line: {}",
        process_info.command_line
    );

    /*
     * Release Ctrl + Shift from the Moneta hotkey
     * before sending our own keyboard input.
     */
    wait_for_hotkey_release();

    thread::sleep(
        Duration::from_millis(75)
    );

    /*
     * Capture the selected text first.
     */
    let selected_text =
        capture_selected_text()?;

    /*
     * Resolve the source after we have the selection.
     */
    let (source_path, source_type) =
        resolve_source(
            &process_info,
            &window_title,
            &selected_text,
        );

    println!(
        "Detected source path: {:?}",
        source_path
    );

    println!(
        "Detected source type: {}",
        source_type
    );

    Ok(SelectionData {
        text: selected_text,
        window_title,
        source_path,
        source_type,
    })
}

pub fn get_selected_text() -> Result<String, String> {
    let selection =
        capture_selection()?;

    Ok(selection.text)
}

/*
 * ============================================================
 * SELECTED TEXT
 * ============================================================
 */

fn capture_selected_text() -> Result<String, String> {
    {
        let mut clipboard =
            Clipboard::new()
                .map_err(|e| {
                    format!(
                        "Could not access clipboard: {e:?}"
                    )
                })?;

        clipboard
            .clear()
            .map_err(|e| {
                format!(
                    "Could not clear clipboard: {e:?}"
                )
            })?;
    }

    println!("Clipboard cleared.");

    thread::sleep(
        Duration::from_millis(50)
    );

    send_ctrl_c()?;

    println!(
        "Ctrl+C sent automatically."
    );

    let timeout =
        Duration::from_millis(1500);

    let poll_interval =
        Duration::from_millis(25);

    let start =
        Instant::now();

    loop {
        if start.elapsed() >= timeout {
            return Err(
                "Timed out waiting for selected text on clipboard."
                    .into(),
            );
        }

        let current_text = {
            let mut clipboard =
                match Clipboard::new() {
                    Ok(clipboard) => clipboard,

                    Err(_) => {
                        thread::sleep(
                            poll_interval
                        );

                        continue;
                    }
                };

            clipboard.get_text().ok()
        };

        if let Some(text) = current_text {
            let text =
                text.trim_end_matches('\0')
                    .to_string();

            if !text.trim().is_empty() {
                println!(
                    "Selected text captured successfully."
                );

                return Ok(text);
            }
        }

        thread::sleep(
            poll_interval
        );
    }
}

/*
 * ============================================================
 * SOURCE RESOLUTION
 * ============================================================
 */


fn resolve_source(
    process: &ProcessInfo,
    window_title: &str,
    selected_text: &str,
) -> (Option<String>, String) {
    let process_name =
        process.name.to_lowercase();

    /*
     * GENERAL FILE IDENTIFIER
     */
    if let Some(path) =
        find_any_file_in_command_line(
            &process.command_line,
        )
    {
        let source_type =
            get_source_type_from_path(&path);

        println!(
            "General file identifier found: {}",
            path
        );

        return (
            Some(path),
            source_type,
        );
    }

    /*
     * Microsoft Word
     */
    if process_name == "winword.exe"
        || process_name == "winword"
        || window_title
            .to_lowercase()
            .contains("word")
    {
        match get_word_document_path() {
            Ok(path) => {
                return (
                    Some(path),
                    "word".to_string(),
                );
            }

            Err(error) => {
                println!(
                    "Could not resolve Word document: {}",
                    error
                );
            }
        }
    }

    /*
     * Web browsers.
     *
     * Chrome, Edge, Firefox, Brave, Opera and Vivaldi
     * generally don't put the currently opened PDF/EPUB
     * into their process command line.
     *
     * We therefore read the current address bar.
     */
    if is_browser(&process_name) {
        match get_browser_document_path(
            selected_text,
        ) {
            Ok(Some((path, source_type))) => {
                return (
                    Some(path),
                    source_type,
                );
            }

            Ok(None) => {
                println!(
                    "Browser page is not a local PDF/EPUB."
                );
            }

            Err(error) => {
                println!(
                    "Could not resolve browser document: {}",
                    error
                );
            }
        }
    }

    /*
     * Native PDF/EPUB readers.
     *
     * Try the process command line.
     */
    if let Some(path) =
        find_document_in_command_line(
            &process.command_line,
        )
    {
        let extension =
            Path::new(&path)
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("")
                .to_lowercase();

        if extension == "pdf" {
            return (
                Some(path),
                "pdf".to_string(),
            );
        }

        if extension == "epub" {
            return (
                Some(path),
                "epub".to_string(),
            );
        }
    }

    (
        None,
        "unknown".to_string(),
    )
}

fn is_browser(
    process_name: &str,
) -> bool {
    matches!(
        process_name,
        "chrome.exe"
            | "msedge.exe"
            | "firefox.exe"
            | "brave.exe"
            | "opera.exe"
            | "vivaldi.exe"
    )
}

/*
 * ============================================================
 * BROWSER DOCUMENT PATH
 * ============================================================
 */

fn get_browser_document_path(
    selected_text: &str,
) -> Result<Option<(String, String)>, String> {
    println!(
        "Reading current browser URL..."
    );

    /*
     * Focus address bar.
     */
    send_ctrl_l()?;

    thread::sleep(
        Duration::from_millis(100)
    );

    /*
     * Copy URL.
     */
    send_ctrl_c()?;

    thread::sleep(
        Duration::from_millis(100)
    );

    let url =
        read_clipboard_text(
            Duration::from_millis(1000)
        )?;

    /*
     * Return focus to the document.
     */
    send_escape()?;

    thread::sleep(
        Duration::from_millis(75)
    );

    /*
     * Restore the selected text to the clipboard.
     */
    {
        let mut clipboard =
            Clipboard::new()
                .map_err(|e| {
                    format!(
                        "Could not access clipboard: {e:?}"
                    )
                })?;

        clipboard
            .set_text(selected_text)
            .map_err(|e| {
                format!(
                    "Could not restore selected text: {e:?}"
                )
            })?;
    }

    println!(
        "Browser URL: {}",
        url
    );

    /*
     * Local PDF/EPUB:
     *
     * file:///C:/Books/Syllabus.pdf
     */
    if let Some(path) =
        file_url_to_path(&url)
    {
        let extension =
            Path::new(&path)
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("")
                .to_lowercase();

        if extension == "pdf" {
            return Ok(Some((
                path,
                "pdf".to_string(),
            )));
        }

        if extension == "epub" {
            return Ok(Some((
                path,
                "epub".to_string(),
            )));
        }
    }

    Ok(None)
}

/*
 * ============================================================
 * FILE URL
 * ============================================================
 */

fn file_url_to_path(
    url: &str,
) -> Option<String> {
    let url =
        url.trim();

    if !url
        .to_lowercase()
        .starts_with("file:///")
    {
        return None;
    }

    let encoded_path =
        &url[8..];

    let decoded =
        percent_decode(encoded_path)?;

    let path =
        decoded.replace('/', "\\");

    let path =
        PathBuf::from(path);

    if !path.is_file() {
        println!(
            "File URL does not point to an existing file: {}",
            path.display()
        );

        return None;
    }

    Some(
        path.to_string_lossy()
            .into_owned()
    )
}

fn percent_decode(
    input: &str,
) -> Option<String> {
    let bytes =
        input.as_bytes();

    let mut output =
        Vec::with_capacity(
            bytes.len()
        );

    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }

            let high =
                hex_value(
                    bytes[index + 1]
                )?;

            let low =
                hex_value(
                    bytes[index + 2]
                )?;

            output.push(
                (high << 4) | low
            );

            index += 3;
        } else {
            output.push(
                bytes[index]
            );

            index += 1;
        }
    }

    String::from_utf8(output).ok()
}

fn hex_value(
    byte: u8,
) -> Option<u8> {
    match byte {
        b'0'..=b'9' =>
            Some(byte - b'0'),

        b'a'..=b'f' =>
            Some(byte - b'a' + 10),

        b'A'..=b'F' =>
            Some(byte - b'A' + 10),

        _ => None,
    }
}

/*
 * ============================================================
 * PROCESS INFORMATION
 * ============================================================
 */

#[derive(Debug)]
struct ProcessInfo {
    name: String,
    command_line: String,
}

fn get_foreground_process_id(
    hwnd: windows::Win32::Foundation::HWND,
) -> Result<u32, String> {
    let mut process_id =
        0u32;

    unsafe {
        let thread_id =
            GetWindowThreadProcessId(
                hwnd,
                Some(&mut process_id),
            );

        if thread_id == 0 {
            return Err(
                "Could not determine foreground process."
                    .into()
            );
        }
    }

    if process_id == 0 {
        return Err(
            "Foreground process ID was zero."
                .into()
        );
    }

    Ok(process_id)
}

fn get_process_info(
    process_id: u32,
) -> Result<ProcessInfo, String> {
    let script =
        format!(
            r#"
$ErrorActionPreference = 'Stop'

$p = Get-CimInstance Win32_Process -Filter "ProcessId = {process_id}"

if ($null -eq $p) {{
    throw "Process not found."
}}

Write-Output $p.Name
Write-Output $p.CommandLine
"#
        );

    let output =
        Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &script,
            ])
            .output()
            .map_err(|e| {
                format!(
                    "Failed to start PowerShell: {e}"
                )
            })?;

    if !output.status.success() {
        let error =
            String::from_utf8_lossy(
                &output.stderr,
            )
            .trim()
            .to_string();

        return Err(
            if error.is_empty() {
                "Could not query foreground process."
                    .to_string()
            } else {
                error
            }
        );
    }

    let stdout =
        String::from_utf8_lossy(
            &output.stdout,
        );

    let mut lines =
        stdout.lines();

    let name =
        lines
            .next()
            .unwrap_or("")
            .trim()
            .to_string();

    let command_line =
        lines
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();

    if name.is_empty() {
        return Err(
            "Foreground process name was empty."
                .into()
        );
    }

    Ok(ProcessInfo {
        name,
        command_line,
    })
}

fn find_any_file_in_command_line(
    command_line: &str,
) -> Option<String> {
    /*
     * First inspect quoted arguments.
     *
     * Example:
     *
     * "C:\Users\Vivekananda\Documents\test.pptx"
     */
    let mut in_quotes = false;
    let mut current = String::new();

    for character in command_line.chars() {
        if character == '"' {
            if in_quotes {
                if let Some(path) =
                    valid_local_file(&current)
                {
                    return Some(path);
                }

                current.clear();
                in_quotes = false;
            } else {
                in_quotes = true;
                current.clear();
            }

            continue;
        }

        if in_quotes {
            current.push(character);
        }
    }

    /*
     * Then try unquoted arguments.
     */
    for argument in command_line.split_whitespace() {
        if let Some(path) =
            valid_local_file(argument)
        {
            return Some(path);
        }
    }

    None
}

fn valid_local_file(
    argument: &str,
) -> Option<String> {
    let argument =
        argument.trim_matches('"');

    /*
     * Ignore command-line switches.
     */
    if argument.starts_with('-') {
        return None;
    }

    let path =
        PathBuf::from(argument);

    /*
     * It must actually exist as a file.
     */
    if !path.is_file() {
        return None;
    }

    Some(
        path.to_string_lossy()
            .into_owned()
    )
}

fn get_source_type_from_path(
    path: &str,
) -> String {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase())
        .unwrap_or_else(|| "file".to_string())
}
/*
 * ============================================================
 * PDF / EPUB COMMAND LINE
 * ============================================================
 */

fn find_document_in_command_line(
    command_line: &str,
) -> Option<String> {
    /*
     * Handle quoted arguments.
     *
     * Example:
     *
     * "C:\Books\My Book.pdf"
     */
    let mut in_quotes =
        false;

    let mut current =
        String::new();

    for character
        in command_line.chars()
    {
        if character == '"' {
            if in_quotes {
                if let Some(path) =
                    document_path_from_argument(
                        &current,
                    )
                {
                    return Some(path);
                }

                current.clear();
                in_quotes = false;
            } else {
                in_quotes = true;
                current.clear();
            }

            continue;
        }

        if in_quotes {
            current.push(character);
        }
    }

    /*
     * Fallback for unquoted arguments.
     */
    for argument
        in command_line.split_whitespace()
    {
        if let Some(path) =
            document_path_from_argument(
                argument,
            )
        {
            return Some(path);
        }
    }

    None
}

fn document_path_from_argument(
    argument: &str,
) -> Option<String> {
    let argument =
        argument.trim_matches('"');

    let path =
        PathBuf::from(argument);

    let extension =
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_lowercase();

    if extension != "pdf"
        && extension != "epub"
    {
        return None;
    }

    if path.is_file() {
        return Some(
            path.to_string_lossy()
                .into_owned()
        );
    }

    None
}

/*
 * ============================================================
 * MICROSOFT WORD
 * ============================================================
 */

fn get_word_document_path()
    -> Result<String, String>
{
    let script = r#"
$ErrorActionPreference = 'Stop'

$word = [Runtime.InteropServices.Marshal]::GetActiveObject('Word.Application')

if ($null -eq $word) {
    throw 'No active Microsoft Word instance found.'
}

if ($word.Documents.Count -eq 0) {
    throw 'Word has no open documents.'
}

$document = $word.ActiveDocument

if ($null -eq $document) {
    throw 'Word has no active document.'
}

$document.FullName
"#;

    let output =
        Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ])
            .output()
            .map_err(|e| {
                format!(
                    "Failed to start PowerShell: {e}"
                )
            })?;

    if !output.status.success() {
        let error =
            String::from_utf8_lossy(
                &output.stderr,
            )
            .trim()
            .to_string();

        return Err(
            if error.is_empty() {
                "PowerShell could not access Word."
                    .to_string()
            } else {
                error
            }
        );
    }

    let path =
        String::from_utf8_lossy(
            &output.stdout,
        )
        .trim()
        .to_string();

    if path.is_empty() {
        return Err(
            "Word returned an empty document path."
                .into()
        );
    }

    Ok(path)
}

/*
 * ============================================================
 * WINDOW TITLE
 * ============================================================
 */

fn get_window_title(
    hwnd: windows::Win32::Foundation::HWND,
) -> String {
    unsafe {
        let mut buffer =
            [0u16; 512];

        let length =
            GetWindowTextW(
                hwnd,
                &mut buffer,
            );

        if length == 0 {
            return "<unknown window>"
                .to_string();
        }

        String::from_utf16_lossy(
            &buffer[..length as usize]
        )
    }
}

/*
 * ============================================================
 * HOTKEY RELEASE
 * ============================================================
 */

fn wait_for_hotkey_release() {
    let timeout =
        Duration::from_millis(1000);

    let start =
        Instant::now();

    loop {
        let ctrl_down =
            unsafe {
                (
                    GetAsyncKeyState(
                        VK_CONTROL.0 as i32,
                    ) as u16
                        & 0x8000
                ) != 0
            };

        let shift_down =
            unsafe {
                (
                    GetAsyncKeyState(
                        VK_SHIFT.0 as i32,
                    ) as u16
                        & 0x8000
                ) != 0
            };

        if !ctrl_down
            && !shift_down
        {
            return;
        }

        if start.elapsed()
            >= timeout
        {
            println!(
                "Warning: Ctrl/Shift still appear to be held."
            );

            return;
        }

        thread::sleep(
            Duration::from_millis(10)
        );
    }
}

/*
 * ============================================================
 * SEND CTRL+C
 * ============================================================
 */

fn send_ctrl_c()
    -> Result<(), String>
{
    unsafe {
        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_CONTROL,
                        wScan: 0,
                        dwFlags:
                            KEYBD_EVENT_FLAGS(0),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },

            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_C,
                        wScan: 0,
                        dwFlags:
                            KEYBD_EVENT_FLAGS(0),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },

            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_C,
                        wScan: 0,
                        dwFlags:
                            KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },

            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_CONTROL,
                        wScan: 0,
                        dwFlags:
                            KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        let sent =
            SendInput(
                &inputs,
                std::mem::size_of::<INPUT>()
                    as i32,
            );

        if sent != inputs.len() as u32 {
            return Err(
                format!(
                    "SendInput failed. Sent {} of {} keyboard events.",
                    sent,
                    inputs.len()
                )
            );
        }
    }

    Ok(())
}

/*
 * ============================================================
 * SEND CTRL+L
 * ============================================================
 */

fn send_ctrl_l()
    -> Result<(), String>
{
    unsafe {
        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_CONTROL,
                        wScan: 0,
                        dwFlags:
                            KEYBD_EVENT_FLAGS(0),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },

            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: windows::Win32::UI::Input::KeyboardAndMouse::VK_L,
                        wScan: 0,
                        dwFlags:
                            KEYBD_EVENT_FLAGS(0),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },

            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: windows::Win32::UI::Input::KeyboardAndMouse::VK_L,
                        wScan: 0,
                        dwFlags:
                            KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },

            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_CONTROL,
                        wScan: 0,
                        dwFlags:
                            KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        let sent =
            SendInput(
                &inputs,
                std::mem::size_of::<INPUT>()
                    as i32,
            );

        if sent != inputs.len() as u32 {
            return Err(
                format!(
                    "SendInput failed while sending Ctrl+L. Sent {} of {} events.",
                    sent,
                    inputs.len()
                )
            );
        }
    }

    Ok(())
}

/*
 * ============================================================
 * SEND ESCAPE
 * ============================================================
 */

fn send_escape()
    -> Result<(), String>
{
    unsafe {
        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE,
                        wScan: 0,
                        dwFlags:
                            KEYBD_EVENT_FLAGS(0),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },

            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE,
                        wScan: 0,
                        dwFlags:
                            KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        let sent =
            SendInput(
                &inputs,
                std::mem::size_of::<INPUT>()
                    as i32,
            );

        if sent != inputs.len() as u32 {
            return Err(
                format!(
                    "SendInput failed while sending Escape. Sent {} of {} events.",
                    sent,
                    inputs.len()
                )
            );
        }
    }

    Ok(())
}

/*
 * ============================================================
 * READ CLIPBOARD
 * ============================================================
 */

fn read_clipboard_text(
    timeout: Duration,
) -> Result<String, String> {
    let start =
        Instant::now();

    loop {
        if start.elapsed()
            >= timeout
        {
            return Err(
                "Timed out reading clipboard."
                    .into()
            );
        }

        if let Ok(mut clipboard) =
            Clipboard::new()
        {
            if let Ok(text) =
                clipboard.get_text()
            {
                let text =
                    text.trim_end_matches('\0')
                        .trim()
                        .to_string();

                if !text.is_empty() {
                    return Ok(text);
                }
            }
        }

        thread::sleep(
            Duration::from_millis(25)
        );
    }
}