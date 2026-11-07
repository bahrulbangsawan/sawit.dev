# Sawit's patch of figcraft-text

FigCraft's `crates/text` at the rev the workspace pins (`8eb0b87646c5aef712326d6c431ccde5408630de`,
MIT OR Apache-2.0, its licences beside this file), used through `[patch."https://github.com/storytold/figcraft"]`
in the workspace `Cargo.toml`. `Cargo.toml` writes out the versions FigCraft's workspace gives;
`rustfmt.toml` is FigCraft's, so its code keeps its own format. Changes (search for "Sawit patch"):

- `src/fontdb.rs`, `system_font_bytes`: a font file in `/System/Library/Fonts` is mapped into
  memory (memmap2) once and kept for the process, as `FontBytes::Static`, instead of read into a
  `Vec` (`FontDb::read_faces`). The first text the canvas lays out reads one face of each fallback
  family (Heiti SC, Apple SD Gothic Neo, Hiragino Sans GB, … files of 20-55 MB), and emoji read
  Apple Color Emoji (180 MB): read, that was 250 MB and more of Sawit's own memory per style used
  (B6, docs/notes/2026-11-07-size-and-ram.md); mapped, it is the file's pages, clean and shared.
  The folder is on the sealed system volume, so a mapped file never changes. Other folders (the
  user's fonts, which a font manager may rewrite) are still read.

Upstream has no option for this. To drop the patch, delete this folder and the `[patch]` entry;
bump it with the workspace's FigCraft rev (copy `crates/text` again and re-apply the change).
