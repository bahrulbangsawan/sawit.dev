//! Changing the saved instance list: the helper every change goes through, and forgetting one.

use gpui_kit::Context;
use sawit_core::Result as CoreResult;
use sawit_core::instances::Registry;

use super::ServerStore;

impl ServerStore {
    /// Changes the list on disk as it is now (what `sawit-mcp` saved is kept; an unreadable file is
    /// never saved over), then takes the result.
    pub(super) fn update_registry(
        &mut self,
        change: impl FnOnce(&mut Registry) -> CoreResult<()>,
    ) -> Result<(), String> {
        let (registry, ()) =
            Registry::update(&self.paths.instances_file(), change).map_err(|e| e.to_string())?;
        self.registry = registry;
        Ok(())
    }

    pub fn forget(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Err(e) = self.update_registry(|r| {
            r.remove(name);
            Ok(())
        }) {
            self.notice = Some(format!("Unable to save the instance list. {e}"));
        }
        self.statuses.remove(name);
        if self.selected.as_deref() == Some(name) {
            self.selected = None;
            self._log_task = None;
            self.log.clear();
        }
        cx.notify();
    }
}
