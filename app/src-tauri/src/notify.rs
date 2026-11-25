//! Desktop notifications, as the `Notifier` port. Lives in the driver: it depends on Tauri.

use sprawlr_app::Notifier;
use sprawlr_domain::status::Notice;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

pub struct TauriNotifier(pub AppHandle);

impl Notifier for TauriNotifier {
    fn notify(&self, notice: &Notice) -> anyhow::Result<()> {
        self.0.notification().builder().title(&notice.title).body(&notice.body).show()?;
        Ok(())
    }
}
