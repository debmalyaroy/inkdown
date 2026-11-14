//! Installation and file association helpers.

use crate::app::InkApp;
pub use crate::settings::config_dir;

pub fn open_default_apps_settings_or_explain(app: &mut InkApp) {
    #[cfg(windows)]
    {
        crate::platform::shell_open("ms-settings:defaultapps");
        app.toast("Choose Inkdown for .md under Default apps");
    }
    #[cfg(not(windows))]
    {
        app.toast("Use your system settings to choose the default app for Markdown files");
    }
}
