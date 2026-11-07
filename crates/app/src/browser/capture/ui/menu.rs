//! The capture menu the toolbar's camera button opens.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::*;
use sawit_core::browser::capture::VideoFormat;
use sawit_core::browser::{DEVICES, DeviceClass};

use super::super::super::BrowserView;
use super::super::capture_dir;
use super::RESOLUTIONS;
use crate::menu_icons;

/// The capture menu.
pub(super) fn capture_menu(
    menu: PopupMenu,
    view: WeakEntity<BrowserView>,
    size: Option<String>,
    resolution: &'static str,
    video: VideoFormat,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let act = |f: fn(&mut BrowserView, &mut Context<BrowserView>)| {
        let view = view.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let _ = view.update(cx, f);
        }
    };
    let size_label = size.clone().unwrap_or_else(|| "This tab".into());
    let sizes = {
        let view = view.clone();
        let current = size.clone();
        PopupMenu::build(window, cx, move |mut m, _, _| {
            let pick = |name: Option<String>| {
                let view = view.clone();
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    let name = name.clone();
                    let _ = view.update(cx, |this, cx| {
                        this.capture_ui.size = name;
                        cx.notify();
                    });
                }
            };
            m = m.item(
                PopupMenuItem::new("This tab's size")
                    .icon(Lucide::MonitorSmartphone)
                    .checked(current.is_none())
                    .on_click(pick(None)),
            );
            for class in DeviceClass::ALL {
                m = m.separator().label(class.name());
                for d in DEVICES.iter().filter(|d| d.class == class) {
                    m = m.item(
                        PopupMenuItem::new(format!("{} · {} × {}", d.name, d.width, d.height))
                            .icon(super::super::super::device::class_icon(class))
                            .checked(current.as_deref() == Some(d.name))
                            .on_click(pick(Some(d.name.to_string()))),
                    );
                }
            }
            m.scrollable(true).max_h(px(480.))
        })
    };
    let videos = {
        let view = view.clone();
        PopupMenu::build(window, cx, move |mut m, _, _| {
            m = m.label("Resolution");
            for r in RESOLUTIONS {
                let view = view.clone();
                m = m.item(
                    PopupMenuItem::new(r)
                        .icon(Lucide::Ratio)
                        .checked(r == resolution)
                        .on_click(move |_, _, cx| {
                            let _ = view.update(cx, |this, cx| {
                                this.capture_ui.resolution = r;
                                cx.notify();
                            });
                        }),
                );
            }
            m = m.separator().label("Format");
            for (f, label) in [
                (VideoFormat::Mp4, "MP4 (H.264)"),
                (VideoFormat::Mov, "MOV (HEVC, smaller)"),
                (VideoFormat::Gif, "GIF (short clips)"),
            ] {
                let view = view.clone();
                m = m.item(
                    PopupMenuItem::new(label)
                        .icon(Lucide::Film)
                        .checked(f == video)
                        .on_click(move |_, _, cx| {
                            let _ = view.update(cx, |this, cx| {
                                this.capture_ui.video = f;
                                cx.notify();
                            });
                        }),
                );
            }
            m
        })
    };
    let folder = {
        let view = view.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let project = view
                .read_with(cx, |this, _| this.project.clone())
                .ok()
                .flatten();
            if let Ok(dir) = capture_dir(project.as_deref()) {
                cx.reveal_path(&dir);
            }
        }
    };
    menu.label("Screenshot")
        .item(
            PopupMenuItem::new("Visible area")
                .icon(Lucide::Camera)
                .on_click(act(|this, cx| this.ui_screenshot(false, cx))),
        )
        .item(
            PopupMenuItem::new("Full page")
                .icon(Lucide::ScrollText)
                .on_click(act(|this, cx| this.ui_screenshot(true, cx))),
        )
        .item(
            PopupMenuItem::new("Phone, tablet, laptop, desktop")
                .icon(Lucide::LayoutGrid)
                .on_click(act(|this, cx| this.ui_several(cx))),
        )
        .separator()
        .label("Record")
        .item(
            PopupMenuItem::new("Record visible area")
                .icon(Lucide::Video)
                .on_click(act(|this, cx| this.ui_record(false, cx))),
        )
        .item(
            PopupMenuItem::new("Record full page")
                .icon(Lucide::Clapperboard)
                .on_click(act(|this, cx| this.ui_record(true, cx))),
        )
        .separator()
        .item(
            PopupMenuItem::submenu(format!("Size: {size_label}"), sizes).icon(Lucide::Proportions),
        )
        .item(
            PopupMenuItem::submenu(
                format!("Video: {resolution} {}", video.extension().to_uppercase()),
                videos,
            )
            .icon(Lucide::Film),
        )
        .item(
            PopupMenuItem::new("Open Captures Folder")
                .icon(menu_icons::REVEAL)
                .on_click(folder),
        )
}
