//! Asset naming, data-URL decoding and capped downloads for the design export.

mod curl;
mod data_url;
mod download;
mod hosts;
mod naming;
mod policy;
#[cfg(test)]
mod tests;

use super::scan::{AssetKind, AssetRef, FontFace};

pub use curl::{Fetched, SAFARI_UA, curl_args, fetch_text};
pub use data_url::decode_data_url;
pub use download::{MAX_TOTAL, VIDEO_SKIPPED, download_all, is_skipped_video};
pub use hosts::{is_local_host, is_local_ip, local_page, page_is_local, url_host};
pub use naming::{file_name, sub_dir};
pub use policy::{answer_allowed, download_allowed, is_css, may_fetch_for, sheet_for_page};

pub fn font_assets(faces: &[FontFace]) -> Vec<AssetRef> {
    faces
        .iter()
        .flat_map(|f| {
            f.src.iter().map(move |u| AssetRef {
                kind: AssetKind::Font,
                url: Some(u.clone()),
                svg: None,
                label: format!("{}-{}-{}", f.family, f.weight, f.style),
                width: None,
                height: None,
                used_on: vec![],
                logo: false,
            })
        })
        .collect()
}
