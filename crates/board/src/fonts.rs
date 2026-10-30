//! The fonts Sawit embeds for the canvas: Inter, the family every new FigCraft text, sticky and
//! label starts in (`figcraft_doc::DEFAULT_FONT`).
//!
//! FigCraft's own builds embed Inter at build time, but a git dependency never reads FigCraft's
//! build settings, so in Sawit `figcraft_text::CRAFT_FONTS` is empty. Without Inter, its text would
//! be drawn in the system's fallback face (Helvetica Neue Regular) whatever its style: bold,
//! italic and weights would change the document and not the canvas.
//!
//! The files are rsms/inter's variable fonts, 4.001, under the SIL Open Font License 1.1
//! (`fonts/OFL-Inter.txt` in this crate; credited in the repository's `NOTICE`).

use std::sync::{OnceLock, PoisonError};

use figcraft_text::fontdb::{Face, FontBytes};

/// The family the faces are added as. The files call themselves "Inter Variable" (their name ID
/// 1), which no document asks for: documents ask for "Inter".
const FAMILY: &str = "Inter";

/// Inter's upright and italic faces, each with every weight from Thin to Black as a named instance.
const FACES: [(&[u8], &str); 2] = [
    (include_bytes!("../fonts/InterVariable.ttf"), "Regular"),
    (
        include_bytes!("../fonts/InterVariable-Italic.ttf"),
        "Italic",
    ),
];

/// Adds Inter to FigCraft's font database, once per process, as figcraft-text adds the fonts a
/// FigCraft build embeds, and has text laid out again ([`figcraft_text::layout_generation`]
/// changes, so every canvas renders again). Call it before the system's fonts are scanned: a font
/// of the same family and style installed on the Mac is then left out, and the canvas looks the
/// same on every Mac.
pub fn register() {
    // Concurrent callers wait for the one registration, as for figcraft-text's one font scan.
    static REGISTERED: OnceLock<()> = OnceLock::new();
    REGISTERED.get_or_init(|| {
        // Read before the database is locked: each face reads its font's tables.
        let faces = FACES.iter().filter_map(|&(bytes, style)| {
            Face::from_bytes(FontBytes::Static(bytes), 0, Some(FAMILY), Some(style))
        });
        let faces: Vec<Face> = faces.collect();
        let mut db = figcraft_text::db()
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        for face in faces {
            db.add(face);
        }
        drop(db);
        figcraft_text::clear_layout_cache();
    });
}

#[cfg(test)]
mod tests {
    use figcraft_doc::TextData;

    use super::register;

    #[test]
    fn inter_has_its_regular_bold_and_italic() {
        register();
        let styles = figcraft_text::family_styles("Inter");
        for style in ["Regular", "Bold", "Italic"] {
            assert!(styles.iter().any(|s| s == style), "{style}: {styles:?}");
        }
    }

    #[test]
    fn text_in_inter_bold_lays_out_wider_than_in_inter_regular() {
        register();
        let width = |style: &str| {
            let mut text = TextData::new("Sawit draws Inter");
            text.style.font_family = "Inter".into();
            text.style.font_style = style.into();
            text.style.font_size = 16.0;
            figcraft_text::layout(&text, None, None).width
        };
        let (regular, bold) = (width("Regular"), width("Bold"));
        assert!(regular > 0.0, "no glyphs");
        assert!(bold > regular, "bold {bold}, regular {regular}");
    }
}
