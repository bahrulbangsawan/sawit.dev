//! The capture controls in the browser (design: `docs/design/browser-capture.sawit`): the
//! toolbar's camera button opens the capture menu (the shared menu look, B16): a screenshot of
//! the visible area or the full page, the page at a phone, tablet, laptop and desktop size at
//! once, a recording of the visible area or the full page, the size and the video settings, and
//! the captures folder. While a recording runs the button stops it, the tab shows a red dot and
//! the time, and the capture bar under the toolbar says what is recorded with Stop. After a
//! capture the bar shows it: a thumbnail, the file, its size, Copy, Reveal in Finder and Open.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::channel::oneshot;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::browser::capture::{ImageFormat, VideoFormat};
use sawit_core::browser::{DEVICES, DeviceClass};
use serde_json::Value;

use super::super::BrowserView;
use super::super::shots::MesurerShot;
use super::record::{DEFAULT_SECONDS, RecordAsk, RecordMode};
use super::{CaptureAsk, capture_dir};
use crate::menu_icons;

/// The sizes "Phone, tablet, laptop, desktop" takes.
pub const SEVERAL: [&str; 4] = ["iPhone 15", "iPad Air", "Laptop", "Full HD"];
/// The video resolutions the menu offers.
const RESOLUTIONS: [&str; 4] = ["720p", "1080p", "1440p", "2160p"];

/// What the capture controls remember for the tab.
pub struct CaptureUi {
    /// The size captures are taken at: a device's name, or the tab's own (`None`).
    pub size: Option<String>,
    pub resolution: &'static str,
    pub video: VideoFormat,
    /// What the capture bar shows.
    pub bar: Bar,
    /// Redraws the recording's timer.
    _tick: Option<Task<()>>,
}

impl Default for CaptureUi {
    fn default() -> Self {
        CaptureUi {
            size: None,
            resolution: "1080p",
            video: VideoFormat::Mp4,
            bar: Bar::Hidden,
            _tick: None,
        }
    }
}

/// The capture bar under the toolbar.
#[derive(Clone, Default)]
pub enum Bar {
    #[default]
    Hidden,
    Working(SharedString),
    Done(Done),
    Failed(SharedString),
}

/// A capture made, as the bar shows it.
#[derive(Clone)]
pub struct Done {
    pub path: PathBuf,
    pub detail: SharedString,
    pub thumb: Option<Arc<RenderImage>>,
    pub video: bool,
    /// More than one file was made (several sizes): the folder is revealed.
    pub several: usize,
}

fn megabytes(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.)
    } else {
        format!("{} KB", (bytes / 1000).max(1))
    }
}

/// A thumbnail of a PNG for the bar.
fn thumb(png: Option<&[u8]>) -> Option<Arc<RenderImage>> {
    let picture = image::load_from_memory(png?).ok()?;
    let picture = picture.thumbnail(160, 100).to_rgba8();
    let (w, h) = picture.dimensions();
    let mut bgra = picture.into_raw();
    for px in bgra.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    let pixels = image::RgbaImage::from_raw(w, h, bgra)?;
    Some(Arc::new(RenderImage::new([image::Frame::new(pixels)])))
}

impl BrowserView {
    fn capture_ask(&self, full_page: bool) -> CaptureAsk {
        CaptureAsk {
            full_page,
            device: self.capture_ui.size.clone(),
            format: ImageFormat::Png,
            mesurer: MesurerShot::Hide,
            ..Default::default()
        }
    }

    /// The menu's screenshot: the visible area or the full page, at the size chosen.
    pub(in crate::browser) fn ui_screenshot(&mut self, full_page: bool, cx: &mut Context<Self>) {
        let ask = self.capture_ask(full_page);
        self.ui_capture(vec![ask], cx);
    }

    /// The page at a phone, tablet, laptop and desktop size.
    pub(in crate::browser) fn ui_several(&mut self, cx: &mut Context<Self>) {
        let asks = SEVERAL
            .iter()
            .map(|d| CaptureAsk {
                device: Some(d.to_string()),
                ..self.capture_ask(false)
            })
            .collect();
        self.ui_capture(asks, cx);
    }

    fn ui_capture(&mut self, asks: Vec<CaptureAsk>, cx: &mut Context<Self>) {
        let n = asks.len();
        self.capture_ui.bar = Bar::Working(if n > 1 {
            format!("Taking {n} screenshots…").into()
        } else if asks[0].full_page {
            "Taking a full-page screenshot…".into()
        } else {
            "Taking a screenshot…".into()
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut last: Option<Value> = None;
            let mut error = None;
            for ask in asks {
                let (tx, rx) = oneshot::channel();
                let full = ask.full_page;
                if this
                    .update(cx, |this, cx| this.capture_op(ask, tx, cx))
                    .is_err()
                {
                    return;
                }
                match rx.await {
                    Ok(Ok(mut v)) => {
                        v["full"] = full.into();
                        last = Some(v)
                    }
                    Ok(Err(e)) => {
                        error = Some(e);
                        break;
                    }
                    Err(_) => {
                        error = Some("The capture was cancelled.".into());
                        break;
                    }
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.capture_ui.bar = match (error, last) {
                    (Some(e), _) => Bar::Failed(e.into()),
                    (None, Some(v)) => Bar::Done(done_of(&v, n)),
                    (None, None) => Bar::Hidden,
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// The menu's recording: the visible area or the full page, at the video settings chosen.
    pub(in crate::browser) fn ui_record(&mut self, full_page: bool, cx: &mut Context<Self>) {
        let ask = RecordAsk {
            device: self.capture_ui.size.clone(),
            resolution: Some(self.capture_ui.resolution.to_string()),
            format: self.capture_ui.video,
            mode: if full_page {
                RecordMode::FullPage
            } else {
                RecordMode::View
            },
            max_seconds: if full_page { 120 } else { DEFAULT_SECONDS * 4 },
            ..Default::default()
        };
        let (tx, rx) = oneshot::channel();
        self.record_start(ask, tx, cx);
        self.capture_ui.bar = Bar::Working("Starting the recording…".into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let started = rx.await;
            let _ = this.update(cx, |this, cx| {
                match started {
                    Ok(Ok(_)) => {
                        this.capture_ui.bar = Bar::Hidden;
                        this.watch_recording(cx);
                    }
                    Ok(Err(e)) => this.capture_ui.bar = Bar::Failed(e.into()),
                    Err(_) => this.capture_ui.bar = Bar::Hidden,
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Redraws the timer while the recording runs, and shows its file when it ends (on its own
    /// or by Stop).
    fn watch_recording(&mut self, cx: &mut Context<Self>) {
        self.capture_ui._tick = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let Ok(running) = this.update(cx, |this, cx| {
                    cx.notify();
                    this.recorder.current.is_some()
                }) else {
                    return;
                };
                if !running {
                    break;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.capture_ui.bar = match this.recorder.last.clone() {
                    Some(Ok(v)) => Bar::Done(done_of(&v, 1)),
                    Some(Err(e)) => Bar::Failed(e.into()),
                    None => Bar::Hidden,
                };
                cx.notify();
            });
        }));
    }

    /// Stop: the recording ends and its file is finished.
    pub(in crate::browser) fn ui_stop(&mut self, cx: &mut Context<Self>) {
        let (tx, _rx) = oneshot::channel();
        self.record_stop(tx, cx);
        self.capture_ui.bar = Bar::Working("Finishing the video…".into());
        cx.notify();
    }

    /// Stops the recording (its tab is closing); the file is finished in the background.
    pub fn stop_recording(&mut self, cx: &mut Context<Self>) {
        let (tx, _rx) = oneshot::channel();
        self.record_stop(tx, cx);
    }

    /// Whether this tab is recording.
    pub fn is_recording(&self) -> bool {
        self.recorder.current.is_some()
    }

    /// The recording's time, `0:12`, for the tab.
    pub fn recording_time(&self) -> Option<SharedString> {
        self.recorder.current.as_ref().map(|r| {
            let s = r.started.elapsed().as_secs();
            format!("{}:{:02}", s / 60, s % 60).into()
        })
    }

    /// The toolbar's capture button: the menu, or Stop while recording.
    pub(in crate::browser) fn capture_button(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.is_recording() {
            return Button::new("browser-capture-stop")
                .ghost()
                .xsmall()
                .icon(Icon::new(Lucide::Square).text_color(cx.theme().danger))
                .accessibility_label("Stop Recording")
                .tooltip("Stop recording")
                .on_click(cx.listener(|this, _, _, cx| this.ui_stop(cx)))
                .into_any_element();
        }
        let weak = cx.entity().downgrade();
        let size = self.capture_ui.size.clone();
        let resolution = self.capture_ui.resolution;
        let video = self.capture_ui.video;
        let opened = weak.clone();
        Button::new("browser-capture")
            .debug_selector(|| "browser-capture".into())
            .ghost()
            .xsmall()
            .icon(Lucide::Camera)
            .accessibility_label("Capture")
            .tooltip("Screenshot or record the page")
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, cx| {
                capture_menu(
                    menu,
                    weak.clone(),
                    size.clone(),
                    resolution,
                    video,
                    window,
                    cx,
                )
            })
            .on_open_change(move |open, _, cx| {
                let _ = opened.update(cx, |this, cx| this.set_menu_open(*open, cx));
            })
            .into_any_element()
    }

    /// The capture bar: what is being captured, the recording with Stop, or the capture made.
    pub(in crate::browser) fn capture_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme();
        let (muted, border, bg, danger) = (
            theme.muted_foreground,
            theme.border,
            theme.secondary,
            theme.danger,
        );
        let bar = || {
            h_flex()
                .id("browser-capture-bar")
                .h(px(44.))
                .flex_none()
                .pl_3()
                .pr_2()
                .gap_2()
                .items_center()
                .text_xs()
                .bg(bg)
                .border_b_1()
                .border_color(border)
        };
        let meta = |title: SharedString, detail: Option<SharedString>| {
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .truncate()
                        .child(title),
                )
                .children(detail.map(|d| div().text_color(muted).truncate().child(d)))
        };
        let close = Button::new("browser-capture-close")
            .ghost()
            .xsmall()
            .icon(menu_icons::CLOSE)
            .accessibility_label("Close the Capture Bar")
            .tooltip("Close")
            .on_click(cx.listener(|this, _, _, cx| {
                this.capture_ui.bar = Bar::Hidden;
                cx.notify();
            }));
        if let Some(r) = &self.recorder.current {
            let what = match r.mode {
                RecordMode::FullPage => "Recording the full page",
                RecordMode::Element => "Recording an element",
                RecordMode::Region => "Recording a region",
                RecordMode::View => "Recording the visible area",
            };
            let time = self.recording_time().unwrap_or_default();
            let cap = r.max.as_secs();
            return Some(
                bar()
                    .child(div().size(px(8.)).flex_none().rounded_full().bg(danger))
                    .child(meta(
                        format!("{what} · {time}").into(),
                        Some(
                            format!(
                                "{} × {} {} · stops by itself at {}:{:02}",
                                r.width,
                                r.height,
                                r.format.extension().to_uppercase(),
                                cap / 60,
                                cap % 60
                            )
                            .into(),
                        ),
                    ))
                    .child(
                        Button::new("browser-capture-bar-stop")
                            .small()
                            .danger()
                            .icon(Lucide::Square)
                            .label("Stop Recording")
                            .on_click(cx.listener(|this, _, _, cx| this.ui_stop(cx))),
                    )
                    .into_any_element(),
            );
        }
        match &self.capture_ui.bar {
            Bar::Hidden => None,
            Bar::Working(text) => Some(
                bar()
                    .child(
                        gpui_kit::component::spinner::Spinner::new()
                            .xsmall()
                            .color(muted),
                    )
                    .child(meta(text.clone(), None))
                    .into_any_element(),
            ),
            Bar::Failed(e) => Some(
                bar()
                    .child(Icon::new(Lucide::CircleX).small().text_color(danger))
                    .child(meta(
                        "The capture didn't work".into(),
                        Some(format!("{e} Try again from the camera menu.").into()),
                    ))
                    .child(close)
                    .into_any_element(),
            ),
            Bar::Done(done) => {
                let name: SharedString = if done.several > 1 {
                    format!("{} screenshots", done.several).into()
                } else {
                    done.path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default()
                        .into()
                };
                let path = done.path.clone();
                let reveal = path.clone();
                let open = path.clone();
                let video = done.video;
                Some(
                    bar()
                        .child(match &done.thumb {
                            Some(image) => div()
                                .w(px(48.))
                                .h(px(30.))
                                .flex_none()
                                .rounded(px(4.))
                                .overflow_hidden()
                                .border_1()
                                .border_color(border)
                                .child(img(image.clone()).size_full().object_fit(ObjectFit::Cover))
                                .into_any_element(),
                            None => Icon::new(if video {
                                Lucide::Film
                            } else {
                                menu_icons::IMAGE
                            })
                            .small()
                            .text_color(muted)
                            .into_any_element(),
                        })
                        .child(meta(name, Some(done.detail.clone())))
                        .when(done.several <= 1, |el| {
                            el.child(
                                Button::new("browser-capture-copy")
                                    .xsmall()
                                    .outline()
                                    .icon(menu_icons::COPY)
                                    .label("Copy")
                                    .tooltip(if video {
                                        "Copy the file's path"
                                    } else {
                                        "Copy the image"
                                    })
                                    .on_click(move |_, _, cx| copy(&path, video, cx)),
                            )
                        })
                        .child(
                            Button::new("browser-capture-reveal")
                                .xsmall()
                                .outline()
                                .icon(menu_icons::REVEAL)
                                .label("Reveal in Finder")
                                .on_click(move |_, _, cx| cx.reveal_path(&reveal)),
                        )
                        .when(done.several <= 1, |el| {
                            el.child(
                                Button::new("browser-capture-open")
                                    .xsmall()
                                    .outline()
                                    .icon(menu_icons::OPEN_EXTERNAL)
                                    .label("Open")
                                    .on_click(move |_, _, cx| cx.open_with_system(&open)),
                            )
                        })
                        .child(close)
                        .into_any_element(),
                )
            }
        }
    }
}

/// The capture bar's view of what a capture or a recording answered.
fn done_of(v: &Value, several: usize) -> Done {
    let path = PathBuf::from(v["path"].as_str().unwrap_or_default());
    let video = v["codec"].is_string();
    let size = format!("{} × {} px", v["width"], v["height"]);
    let bytes = megabytes(v["bytes"].as_u64().unwrap_or(0));
    let detail = if video {
        format!(
            "{size} · {bytes} · {} · {:.1} s at {} fps",
            v["codec"].as_str().unwrap_or(""),
            v["duration_seconds"].as_f64().unwrap_or(0.),
            v["fps_achieved"]
        )
    } else if several > 1 {
        format!("Phone, tablet, laptop and desktop · the last {size}")
    } else {
        let device = v["device"].as_str().unwrap_or("");
        let full = if v["full"] == true { ", full page" } else { "" };
        format!("{size} · {bytes} · {device}{full}")
    };
    let preview = v["preview_png_base64"]
        .as_str()
        .and_then(sawit_core::browser::base64_decode);
    Done {
        path,
        detail: detail.into(),
        thumb: thumb(preview.as_deref()),
        video,
        several,
    }
}

/// Copy: the image itself, or a video's path.
fn copy(path: &std::path::Path, video: bool, cx: &mut App) {
    if !video && let Ok(bytes) = std::fs::read(path) {
        let format = if path.extension().is_some_and(|e| e == "jpg") {
            ImageFormat::Jpeg
        } else {
            ImageFormat::Png
        };
        let kind = match format {
            ImageFormat::Jpeg => gpui_kit::ImageFormat::Jpeg,
            _ => gpui_kit::ImageFormat::Png,
        };
        cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(kind, bytes)));
        return;
    }
    cx.write_to_clipboard(ClipboardItem::new_string(path.display().to_string()));
}

/// The capture menu.
fn capture_menu(
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
                            .icon(super::super::device::class_icon(class))
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
