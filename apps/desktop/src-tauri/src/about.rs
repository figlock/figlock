//! The About screen's in-app browser and the "About FigLock" menu item (FIG-305).
//!
//! The window only ever shows pages on figlock.com over https. The main window's CSP and capabilities stay as
//! they are: the page opens in its own window with no IPC access, and any link that leaves figlock.com goes to
//! the default browser instead.

use tauri::{AppHandle, Emitter, Manager, Url, WebviewUrl, WebviewWindowBuilder};

/// The person chose About FigLock in the app menu; the main window shows its About screen.
pub const OPEN_ABOUT_EVENT: &str = "figlock://open-about";

const HOST: &str = "figlock.com";
/// Navigating here closes the page window. The page's Escape handler (below) is the only thing that uses it.
const CLOSE_URL: &str = "figlock-close://now/";

/// The pages the About screen links to: id, window title, address. The window asks for an id, never an address,
/// so it cannot point this window anywhere else.
const PAGES: [(&str, &str, &str); 4] = [
    (
        "protects",
        "How FigLock protects your data",
        "https://figlock.com/faq/",
    ),
    ("privacy", "Privacy policy", "https://figlock.com/privacy/"),
    ("terms", "Terms", "https://figlock.com/terms/"),
    ("support", "Support", "https://figlock.com/support/"),
];

/// Escape closes the window, like the sheet on iOS. Links that want a new tab stay in this window.
const PAGE_SCRIPT: &str = r#"
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") window.location.href = "figlock-close://now/";
});
document.addEventListener("click", (e) => {
  const a = e.target instanceof Element ? e.target.closest("a[target=_blank]") : null;
  if (a) a.removeAttribute("target");
}, true);
"#;

fn page(id: &str) -> Option<(&'static str, &'static str)> {
    PAGES
        .iter()
        .find(|(known, _, _)| *known == id)
        .map(|(_, title, url)| (*title, *url))
}

/// Whether the in-app window may show `url`: https on figlock.com, nothing else.
fn allowed(url: &Url) -> bool {
    url.scheme() == "https" && url.host_str() == Some(HOST) && url.port().is_none()
}

/// Hands a link to the default browser.
fn open_outside(url: &Url) {
    // Only web links: never a file, mail or app URL that a page on figlock.com could be made to carry.
    if !matches!(url.scheme(), "https" | "http") {
        return;
    }
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(target_os = "windows")]
    let opener = "explorer";
    #[cfg(all(unix, not(target_os = "macos")))]
    let opener = "xdg-open";
    let _ = std::process::Command::new(opener).arg(url.as_str()).spawn();
}

/// Show one of the About pages in its own window, or bring it forward if it is already open.
#[tauri::command]
pub fn open_about_page(app: AppHandle, page_id: String) -> Result<(), String> {
    let Some((title, address)) = page(&page_id) else {
        return Err("That page is not part of FigLock.".into());
    };
    let label = format!("about-{page_id}");
    if let Some(open) = app.get_webview_window(&label) {
        let _ = open.show();
        return open.set_focus().map_err(|e| e.to_string());
    }
    let url = Url::parse(address).map_err(|e| e.to_string())?;
    let handle = app.clone();
    let closing = label.clone();
    WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(url))
        .title(title)
        .inner_size(920.0, 720.0)
        .min_inner_size(480.0, 400.0)
        .initialization_script(PAGE_SCRIPT)
        .on_navigation(move |to| {
            if to.as_str() == CLOSE_URL {
                // Not on this thread: the window cannot close itself from inside its own navigation callback.
                let handle = handle.clone();
                let label = closing.clone();
                std::thread::spawn(move || {
                    if let Some(window) = handle.get_webview_window(&label) {
                        let _ = window.close();
                    }
                });
                return false;
            }
            if allowed(to) {
                return true;
            }
            open_outside(to);
            false
        })
        .build()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Replace the app menu's stock About item with one that shows FigLock's own About screen.
pub fn install_menu(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, MenuItemKind};

    let menu = Menu::default(app)?;
    if let Some(MenuItemKind::Submenu(first)) = menu.items()?.into_iter().next() {
        if let Some(stock) = first.items()?.into_iter().next() {
            first.remove(&stock)?;
        }
        let about = MenuItem::with_id(app, "about", "About FigLock", true, None::<&str>)?;
        first.insert(&about, 0)?;
    }
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| {
        if event.id() == "about" {
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.show();
                let _ = main.set_focus();
            }
            let _ = app.emit(OPEN_ABOUT_EVENT, ());
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn every_page_is_on_figlock_com_over_https() {
        for (id, _, address) in PAGES {
            assert!(allowed(&url(address)), "{id} must be allowed");
        }
    }

    #[test]
    fn only_known_page_ids_resolve() {
        assert!(page("privacy").is_some());
        assert!(page("https://evil.example/").is_none());
        assert!(page("").is_none());
    }

    #[test]
    fn nothing_but_https_figlock_com_is_allowed() {
        assert!(!allowed(&url("http://figlock.com/privacy/")));
        assert!(!allowed(&url("https://figlock.com.evil.example/")));
        assert!(!allowed(&url("https://evil.example/figlock.com")));
        assert!(!allowed(&url("https://www.figlock.com/")));
        assert!(!allowed(&url("https://figlock.com:8443/")));
        assert!(!allowed(&url("file:///etc/passwd")));
        assert!(!allowed(&url(CLOSE_URL)));
    }
}
