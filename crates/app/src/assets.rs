//! App assets: gpui-component's default icon set plus a few extra Lucide
//! icons embedded individually (the full catalog would add ~1 MiB).

use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        Database,
        Server,
        Terminal,
        FileCode,
        Download,
        Table,
        X,
        Save,
        Circle,
        Eye,
        Lock,
        ChevronsLeft,
        ChevronsRight,
        Pin,
        PanelTop,
        WandSparkles,
        Image,
        FileText,
        Film,
        Minus,
        Play,
        Pause,
        Volume2,
        VolumeX,
        Code,
        Columns2,
        File
    ]
);

/// Extra icons first, then the default `Assets` (registering an asset source
/// replaces the default one rather than adding to it).
pub struct AppAssets;

/// The PG Desk mark (title bar).
pub const LOGO_MARK: &str = "logo/pgdesk-mark.svg";

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(svg) = crate::file_icons::asset(path) {
            return Ok(Some(Cow::Borrowed(svg)));
        }
        if path == LOGO_MARK {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../../../assets/logo/pgdesk-mark.svg"
            ))));
        }
        match ExtraIcons.load(path)? {
            Some(bytes) => Ok(Some(bytes)),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut all = ExtraIcons.list(path)?;
        all.extend(gpui_kit::assets::Assets.list(path)?);
        all.sort();
        all.dedup();
        Ok(all)
    }
}
