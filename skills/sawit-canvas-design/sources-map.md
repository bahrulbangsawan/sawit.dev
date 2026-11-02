# Sources map

Where every rule in `SKILL.md` came from, which source rules were folded together as duplicates, how each conflict was resolved, and which source rules were left out and why.

## How to read it

- `a/ID` is a rule in `research/sources-a/rules.md` (unslop, antislop, Impeccable) and `b/ID` a rule in `research/sources-b/rules.md` (Interface Skills, Make Interfaces Feel Better, jakub.kr/skills). Both files were read in full for this work; they and their `raw/` folders were then deleted from the research folder by another process (at about 22:01 on 2026-11-02, while this map was being written), so each id below carries the primary citation its source file gave, in brackets.
- Citation keys inside brackets, as defined by those files:
  - `U S:n` is `skills/unslop/SKILL.md` line n; `U tokens:n`, `U signs:n`, `U visuals:n`, `U effects:n`, `U refs:n` the files in `skills/unslop/references/`; `U README:n`. Base: https://github.com/heydamir/unslop/blob/a6f121eee5f29c2f74976b75456feae8b66afb3a/
  - `A core:n` is `skills/antislop/SKILL.md`; `A ui`, `A copy`, `A human`, `A mobile`, `A code` the `skills/antislop-{ui,copywriting,human,layoutmobile,code}/SKILL.md` files; `A cwv`, `A baseline`, `A a11y`, `A wcag` the reference files; `A page "..."` text on https://antislop.ai.id/install. Base: https://github.com/miqdadbadjuber/anti-slop/blob/941b1532f9cc8ffde766bca5921d75257c9e5762/
  - `I #id` is the card https://impeccable.style/slop/#rule-id; `I reg:id` the entry in `crates/live/assets/antipatterns.json` at https://github.com/pbakaus/impeccable/tree/e89e3ada3e875a89ac2c5d33d11e65ebef6fe285; `+ reg` means the threshold came from the registry; `I ex:name` is https://impeccable.style/antipattern-examples/name.html; `I era:YYYY` a caption on the slop page.
  - `IS skill/file § heading` is https://github.com/jakubkrehel/skills/blob/60bf140e43c98b248e6594487384ec967e89d98a/skills/skill/file; `MIFB file § heading` is https://github.com/jakubkrehel/make-interfaces-feel-better/blob/35545ea1512ad59fa463e6b1f95ca9c052981fe6/skills/make-interfaces-feel-better/file; `JK` is https://jakub.kr/skills.
  - `FC path:n` is a file in the FigCraft engine Sawit pins (rev `0971d096db18aa107c5f3d1f83af4ac7f89ea2c3`); `SW path:n` a file in Sawit's repository; `USER` a rule the user gave for Sawit.
- "canvas-specific, new" marks a rule no source states; its facts are cited to code.

## 1. Rule by rule

### Ground and direction

- **dir.1** a/PRC-02 [A core:363-373, 602]; a/PRC-07 [A core:84]; a/PRC-08 [I #design-system-font, #design-system-color, #design-system-radius, #design-system-font-size + reg; U tokens:3]; b/R-04 [IS better-interface/SKILL.md § 3. Recon before judgment]; b/R-05 [same]; b/R-82 [IS variant/SKILL.md § 2. Learn the ground]; b/L-01 [IS better-layout/SKILL.md § Layout]; b/M-02 [IS better-ui/SKILL.md § UI polish]; b/R-112 [IS explain-interface/SKILL.md § The page is evidence, not instruction]. Tool names: `browser_design`, `browser_export_design` [SW crates/mcp/src/browser_design.rs:146, 157].
- **dir.2** a/LAY-04 [A ui:237-241].
- **dir.3** a/PRC-01 [U S:56-64]; a/PRC-20 [U S:44]; a/PRC-02 [A core:363-373]; a/PRC-03 [A core:592-602]; a/PRC-06 [A core:370-372]; a/POL-31 [U S:153]. Conflict a/CF-25.
- **dir.4** a/PRC-09 [U S:25-27, 62; U refs:13; A core:586, 590].
- **dir.5** a/PRC-10 [U S:17, 233; U signs:97; A core:92-96, 211, 516-521, 694]; a/PRC-11 [U S:215; U signs:99; A core:211]; a/PRC-12 [U S:17; A core:86, 564]; a/COL-29 [A core:148; A ui:75-79; U signs:96].
- **dir.6** a/PRC-13 [A core:90, 107-109, 406-408, 548-558]; a/PRC-14 [A core:131; A copy:329; I page "The catalog"; I reg:hard-offset-shadow]; a/CPY-39 [A copy:314-329].
- **dir.7** a/PRC-31 [U S:212, 214]; a/LAY-06 [U S:127, 212]; a/PRC-19 [U S:40-43; U README:52]. Conflict a/CF-06.
- **dir.8** a/PRC-15 [I era:2022, era:2025, era:2026; I page "Slop"].

### Layout

- **lay.1** a/LAY-04 [A ui:237-241]; a/LAY-07 [A core:115-117, 259; A ui:227-231]; a/LAY-08 [A core:155, 495; A ui:83-87; I ex:layout-templates]; a/LAY-09 [A core:486]; a/LAY-16 [A core:166, 492; A ui:132-136]; a/CPY-44 [A core:202, 318-323]. Conflicts a/CF-05, a/CF-06.
- **lay.2** b/L-25 [IS better-layout/SKILL.md § Order by importance]; b/L-27 [IS better-layout/grouping-and-alignment.md § Order by importance].
- **lay.3** b/L-28 [IS better-layout/SKILL.md § One primary action per view]; b/C-34 [IS better-colors/SKILL.md § Fill exactly one action per view]; a/LAY-28 [U S:28, 134; U signs:70].
- **lay.4** b/L-10, b/L-11, b/L-12 [IS better-layout/SKILL.md § Group with space, not lines; grouping-and-alignment.md].
- **lay.5** a/SPC-13 [U S:73, 221; U signs:30]; b/L-15 [IS better-layout/SKILL.md § Align to shared edges]; b/L-16 [IS better-layout/grouping-and-alignment.md § Align to shared edges].
- **lay.6** a/LAY-10 [A core:167-168, 493; A ui:138-142; I #identical-card-grids]; a/LAY-11 [A core:496, 573]; a/LAY-12 [A core:157, 488; A ui:95-99; U S:213]; a/LAY-13 [A core:162, 487; A ui:107-111; I #numbered-section-labels]; a/LAY-14 [A core:164-165, 490; A ui:119-124].
- **lay.7** a/LAY-18 [I #nested-cards; I ex:cardocalypse].
- **lay.8** a/LAY-19 [I ex:modal-abuse].
- **lay.9** a/LAY-21 [I #line-length + reg; U tokens:92]; b/T-29 [IS better-typography/SKILL.md § Cap the measure]. Discrepancy a/DS-05.
- **lay.10** a/LAY-17 [A core:212, 538-546; U refs:3; U S:164]; a/PRC-30 [U S:164; U refs:3].
- **lay.11** a/LAY-01 [U S:128; U tokens:18]; a/LAY-02 [U S:128]; a/LAY-03 [U tokens:19]; a/LAY-05 [U tokens:22; U S:145]; the user's rule for Sawit pages [USER: Sawit's pages fill the window]. Conflicts a/CF-05, N-5.
- **lay.12** a/LAY-22 [U S:129, 161-162; U visuals:8]; a/LAY-23 [U S:130; U signs:69]; a/LAY-24 [U S:131]; a/LAY-25 [U S:132]; a/LAY-27 [U S:28; U refs:13]; a/LAY-29 [I #oversized-h1 + reg]; a/LAY-30 [U S:136, 231]; a/LAY-31 [A mobile:59-63]; a/LAY-20 [I #first-viewport-column-overflow + reg]. Conflict a/CF-11.
- **lay.13** a/LAY-26 [U S:133].
- **lay.14** a/LAY-32 [U S:135].
- **lay.15** b/L-30, b/L-31 [IS better-layout/SKILL.md § Hint at hidden content; spacing-and-adaptivity.md]; b/L-33 [spacing-and-adaptivity.md § Hint at hidden content]; b/S-05 [same].
- **lay.16** b/L-32 [IS better-layout/SKILL.md § Hint at hidden content]; b/T-41 [IS better-typography/SKILL.md § Truncate with a way back].
- **lay.17** b/L-35, b/L-36, b/L-37, b/L-39 [IS better-layout/SKILL.md § Content bleeds, controls float; § Inset buttons from the edges]; b/S-04, b/S-06 [IS better-layout/spacing-and-adaptivity.md].
- **lay.18** b/L-45 [IS better-layout/SKILL.md § Plan for growth and clipping]; b/L-46, b/L-49 [§ Before you finish]; b/A-131 [IS better-accessibility/SKILL.md § Survive zoom and text resize]; a/LAY-44 [A mobile:99-103; I #clipped-overflow-container]; a/CMP-47 [I #clipped-overflow-container + reg] (menus and popovers drawn inside a clipping frame).
- **lay.19** b/L-50, b/L-51 [IS better-layout/SKILL.md § Plan for growth and clipping]; a/LAY-49 [A human:135-139].
- **lay.20** a/LAY-34 [A core:239-248; A mobile:16, 21-25; U S:137]; a/LAY-37 [A mobile:27-31]; a/LAY-38 [A mobile:39-43]; a/LAY-39 [A mobile:33-37]; a/LAY-40 [A mobile:73-77]; a/LAY-42 [A mobile:85-89]; b/L-40, b/L-41 [IS better-layout/SKILL.md § Hold structure until it breaks].
- **lay.21** a/LAY-35 [U S:137; U tokens:95]; a/LAY-36 [U S:137]; a/LAY-45 [A mobile:133-137; U S:135]; a/LAY-46 [A mobile:139-143]; a/LAY-47 [A mobile:145-149]; a/LAY-48 [A mobile:151-155].
- **lay.22** a/LAY-43 [A core:242-244; A mobile:93-97; A wcag:54; I #text-overflow]; a/LAY-41 [A mobile:79-83, 105-109]; a/W-1.4.10 [A wcag:54]; b/A-130 [IS better-accessibility/SKILL.md § Survive zoom and text resize]; b/L-47 [IS better-layout/SKILL.md § Before you finish].

### Typography

- **typ.1** a/TYP-01 [U S:117; A core:193, 426-427; I #overused-font]; a/TYP-02 [A core:426; A ui:199; I #overused-font + reg; I ex:inter-everywhere; U S:117]; b/T-02 [IS better-typography/SKILL.md § Measured, not preferred; MIFB typography.md § Font Family Scope]; b/T-03 [MIFB typography.md § Font Family Scope]; b/T-07 [IS better-typography/choosing-fonts.md § Choosing a typeface]; b/T-09 [choosing-fonts.md § Fallback stacks]. Conflict a/CF-03.
- **typ.2** a/TYP-03 [U S:117; U tokens:73-80]; a/TYP-04 [I #single-font]; b/T-05 [IS better-typography/SKILL.md § Fewer fonts, sizes and weights].
- **typ.3** a/TYP-05 [U tokens:82]; a/TYP-06 [I #italic-serif-display + reg].
- **typ.4** a/TYP-11 [U tokens:86-95]; b/T-20 [IS better-typography/SKILL.md § Use a type scale with semantic names]; b/T-21 [IS better-typography/spacing-and-sizing.md § Type scale]. Conflict a/CF-08.
- **typ.5** a/TYP-08 [U S:118, 226; U signs:18, 20]; a/TYP-09 [U S:119]; b/T-22 [IS better-typography/spacing-and-sizing.md § Type scale]. Conflict a/CF-07.
- **typ.6** a/TYP-10 [I #flat-type-hierarchy + reg]; b/T-23 [IS better-typography/SKILL.md § Heading sizes descend with level].
- **typ.7** a/TYP-23 [I #tight-leading + reg]; b/T-25, b/T-26 [IS better-typography/SKILL.md § Line-height by role]. Conflict a/CF-08.
- **typ.8** a/TYP-16 [U tokens:97]; a/TYP-17 [U S:121, 226; U signs:22]; a/TYP-18 [I #extreme-negative-tracking + reg]; a/TYP-19 [I #wide-tracking + reg]; b/T-27, b/T-28 [IS better-typography/SKILL.md § Letter-spacing by size]. Canvas syntax [FC engine/src/cmd/props.rs:27]. Conflict N-4.
- **typ.9** a/TYP-12 [U S:120, 226; U signs:19]; a/TYP-13 [A core:192, 425; A ui:194-198]; b/W-23 [IS better-writing/SKILL.md § One capitalization policy]; b/T-50 [IS better-typography/SKILL.md § Natural case, typographic punctuation]. Conflicts a/CF-04, a/CF-16.
- **typ.10** a/TYP-14 [I #kicker-above-heading + reg; A ui:182-186; U signs:65]; a/CMP-14 [U signs:65; A core:191; A ui:182-186; I #hero-eyebrow-chip + reg]. Conflict a/CF-28.
- **typ.11** a/TYP-21 [I #tiny-text + reg; U tokens:89-90]; a/TYP-22 [I #undersized-ui-text + reg; U S:137]; b/T-06 [IS better-typography/SKILL.md § Fewer fonts, sizes and weights]; b/T-59 [§ Inputs at 16px on mobile]; b/T-61 [§ Size floors]. Discrepancy a/DS-05.
- **typ.12** a/TYP-20 [U S:122, 228; U signs:21]; b/T-45 [IS better-typography/SKILL.md § Tabular numbers on changing values; MIFB SKILL.md § 9]; b/T-46 [MIFB typography.md § When to Use, § Caveat]. Canvas key `features` [FC engine/src/cmd/props.rs:32].
- **typ.13** a/TYP-24 [I #justified-text + reg]; a/TYP-25 [I #straggler + reg]; a/TYP-26 [I #unbalanced-heading + reg]; b/T-30, b/T-31 [IS better-typography/SKILL.md § Wrap deliberately]; b/T-32 [MIFB typography.md § text-wrap: pretty]; b/T-34 [IS better-typography/SKILL.md § Wrap deliberately]. Canvas: `text.fit` shrinks, then falls back to a no-break space [FC engine/src/content/textfit.rs:1-6, 68-71, 115]. Conflict b/CF-03.
- **typ.14** a/TYP-07 [U S:123, 226; U signs:23]; a/TYP-15 [I #all-caps-body + reg]; a/CPY-19 [A copy:103-112]; b/T-63 [IS better-typography/SKILL.md § Decorative text in CSS, not images]; a/W-1.4.5 [A wcag:53].
- **typ.15** b/T-51 [IS better-typography/SKILL.md § Natural case, typographic punctuation; wrapping-and-punctuation.md § Smart punctuation].
- **typ.16** b/T-40 [IS better-typography/SKILL.md § Truncate with a way back]; b/T-33 [§ Wrap deliberately]. Canvas keys `textAutoResize`, `maxLines` [FC engine/src/cmd/props.rs:27-32; engine/src/cmd/text.rs:303].

### Color

- **col.1** a/COL-02 [U S:80; U tokens:26]; a/COL-03 [U S:81; U tokens:26]; a/COL-09 [A ui:24, 329; A page "Skill: Visual & UI Design"; I #ai-color-palette]; b/C-15 [IS better-colors/SKILL.md § A system is ramps, not colors]; b/C-16, b/C-17 [IS better-colors/palette-structure.md § Every step has a job]; b/C-19 [palette-structure.md § Neutrals].
- **col.2** a/COL-01 [U S:19, 26].
- **col.3** a/COL-07 [U S:83; U tokens:62; U signs:13; A core:147, 589; A ui:69-73]; b/C-15 (second accent clause).
- **col.4** a/COL-10 [A core:146, 531-536; A ui:63-67]; a/COL-11 [U S:84]; b/C-41 [IS better-colors/palette-structure.md § Auditing an existing palette]. Canvas `paint.documentColors` [FC engine/src/cmd/styles.rs:18]. Conflict a/CF-10.
- **col.5** a/COL-08 [U S:85; U tokens:59-60]; a/PRC-05 [U S:66; U tokens:59]; b/C-39 [IS better-colors/palette-generation.md § Start from the brand color].
- **col.6** a/COL-12 [U S:89; U signs:7; A core:137, 142, 412; A ui:20-25; I #ai-color-palette + reg; I ex:purple-gradients; I era:2022]; a/COL-13 [U S:92]. Conflict a/CF-01.
- **col.7** a/COL-14 [U S:89, 118, 223; U signs:8, 67; I #gradient-text + reg]; a/COL-15 [U S:89, 223; U signs:9; I #dark-glow + reg; A core:141, 456-459]; a/COL-16 [U S:89, 223; U signs:10, 68; A core:142, 412; I #radial-halo + reg; I #radial-spotlight-glow + reg]; a/POL-09 [A core:141, 456-459; A ui:45-49]. Conflict a/CF-02; discrepancy a/DS-06.
- **col.8** a/COL-05 [U S:82, 222; U signs:12]; a/COL-06 [U tokens:53]; b/C-27 [IS better-colors/token-naming.md § The role inventory].
- **col.9** a/COL-17 [U S:90; U signs:11; I #dark-glow; A page "Skill: Visual & UI Design"].
- **col.10** a/COL-20 [A core:285-292; A human:22-26, 60-83; I #low-contrast + reg]; a/COL-21 [A core:218, 290; A human:34-38, 69-81; I ex:bad-contrast]; a/COL-24 [A human:40-44; A ui:259; A wcag:55]; a/W-1.4.3 [A wcag:51]; a/W-1.4.11 [A wcag:55]; b/A-111 [IS better-accessibility/SKILL.md § Pair color with a second cue]; b/A-34 [IS better-accessibility/semantics-and-aria.md § Disabled states]; b/C-01 [IS better-colors/SKILL.md § Colors]; b/C-46 [§ Measure the rendered pair, then report]; b/C-48 [IS better-colors/contrast.md § WCAG 2 thresholds]; b/C-49, b/C-50, b/C-52 [contrast.md § APCA thresholds, § Contrast, § Pick text polarity]; b/C-53 [contrast.md § What to check]; b/C-51 [contrast.md § Fix a failing pair only on request]; b/A-112 [IS better-accessibility/SKILL.md § Pair color with a second cue]. Large-text values from https://www.w3.org/TR/WCAG22/#dfn-large-scale. Discrepancy a/DS-03; conflicts a/CF-09, b/CF-25.
- **col.11** a/COL-22 [A core:291-292; A human:28-32; U S:28]; b/C-44 [IS better-colors/color-usage.md § Gradients]; b/C-55 [IS better-colors/token-naming.md § In Tailwind projects].
- **col.12** a/COL-23 [I #gray-on-color + reg].
- **col.13** a/COL-25 [A core:389; A human:113-117; A wcag:18]; a/W-1.4.1 [A wcag:18]; a/W-1.3.3 [A wcag:17]; b/A-110 [IS better-accessibility/SKILL.md § Pair color with a second cue]; b/C-33 [IS better-colors/SKILL.md § One color, one meaning].
- **col.14** a/COL-18 [U S:88; U tokens:55]; b/C-20 [IS better-colors/palette-structure.md § Status colors]; b/C-45 [IS better-colors/color-usage.md § Color across cultures]; b/W-35 [IS better-writing/SKILL.md § Undo beats confirmation].
- **col.15** a/COL-19 [U S:86; U tokens:61]; b/C-18 [IS better-colors/palette-structure.md § Every step has a job]; b/C-39 (white on `#3b82f6` = 3.68:1).
- **col.16** b/C-05 [IS better-colors/SKILL.md § Match the project's color system]; b/C-25 [§ Name primitives by hue, semantics by role]; b/C-26, b/C-27, b/C-28, b/C-29, b/C-30 [IS better-colors/token-naming.md]; b/C-31 [SKILL.md § Use a token only in its role]; b/C-32 [SKILL.md § Before you finish]; b/C-54 [token-naming.md § In Tailwind projects].
- **col.17** b/C-35, b/C-36, b/C-37 [IS better-colors/SKILL.md § Hold the hue across the ramp; palette-generation.md]; b/C-38, b/C-40 [palette-generation.md]; b/C-10, b/C-11 [color-formats.md § Gamut]. Canvas `view.proofSrgb` [FC engine/src/cmd/view.rs, listed by `canvas_commands`].
- **col.18** a/COL-27 [A core:145, 523-529; A ui:57-61]; a/COL-28 [A core:341-345]; a/PRC-36 [A core:528]; b/C-56 [IS better-colors/color-usage.md § Light, dark and increased contrast]; b/C-57 [palette-generation.md § Dark mode].
- **col.19** a/COL-26 [I #cream-palette + reg; I era:2025]; a/COL-30 [U signs:96]; a/COL-04 [U tokens:30-51] (ramps, in `references/tokens.md` with contrast computed by `scripts/contrast.py`). Conflicts a/CF-09, a/CF-13.

### Spacing

- **spc.1** a/SPC-01 [U S:70, 220]; a/SPC-02 [U tokens:7]; a/SPC-03 [U S:76; U signs:35]; a/SPC-20 [A core:247].
- **spc.2** a/SPC-08 [U S:72; U signs:37; A ui:101-105; I #monotonous-spacing + reg]; a/SPC-09 [A core:159, 588; A ui:101-105; I #monotonous-spacing]; b/S-01 [IS better-layout/SKILL.md § Group with space, not lines]; b/L-11.
- **spc.3** a/SPC-04 [U tokens:9-12]; a/SPC-18 [A mobile:47-51, 65-69].
- **spc.4** a/SPC-10 [U S:75; U tokens:20; I #heading-rhythm + reg]; a/SPC-14 [U signs:39].
- **spc.5** a/SPC-11 [U S:74; U tokens:18]; a/SPC-15 [I #cramped-padding + reg; U tokens:9]; a/SPC-16 [I #body-text-viewport-edge + reg; U S:74].
- **spc.6** a/SPC-07 [U S:71, 220; U signs:36, 38]; b/S-02 [IS better-layout/SKILL.md § Align to shared edges].
- **spc.7** a/SPC-05 [U tokens:13]; a/SPC-12 [U S:75; U tokens:20]. Conflict a/CF-14.
- **spc.8** b/S-03 [IS better-layout/SKILL.md § Borderless controls need more clearance].
- **spc.9** a/SPC-17 [I #edge-flush-cards + reg].
- **spc.10** a/SPC-06 [U tokens:14; U S:132].

### Copywriting

- **cpy.1** a/CPY-01 [U S:104; U signs:66, 97; A core:94-96; I ex:inter-everywhere].
- **cpy.2** b/W-01 [IS better-writing/SKILL.md § Writing]; b/W-02 [§ Inventory the existing strings first]; b/W-05 [§ One voice, one vocabulary]; b/W-09 [§ Plain words over clever ones]; a/CPY-26 [A copy:178-186]; a/CPY-38 [A copy:18, 341-349]. Conflict N-6.
- **cpy.3** a/CPY-07 [U S:110; U signs:47; A core:176, 510-514; A copy:23-31; A page "Skill: Tone, Voice & Prose"; I #marketing-buzzword + reg]; a/CPY-08 [U S:110; U signs:31; A core:186, 442].
- **cpy.4** a/CPY-10 [A copy:33-41]; a/CPY-14 [A copy:53-61]; a/CPY-15 [A copy:63-71]; a/CPY-21 [A copy:125-134]; a/CPY-22 [A copy:138-146]; a/CPY-23 [A copy:148-156; I #aphoristic-cadence + reg]; a/CPY-24 [A copy:158-166]; a/CPY-25 [A copy:168-176]; a/CPY-27 [A copy:188-196]; a/CPY-43 [I #theater-slop-phrase + reg].
- **cpy.5** a/CPY-11 [U S:111, 227; U signs:71; A core:177-179; A copy:43-51; I #marketing-buzzword]; a/CPY-12 [A core:178, 256-260]; a/CPY-13 [A core:179, 357-361]; a/CPY-28 [A copy:17, 200-208; A core:250-260, 357-361, 375-380; U S:104, 214]; a/CPY-29 [A copy:210-218]; a/LAY-15 [A core:163, 491; A ui:113-117; U signs:71]; a/CMP-23 [I #hero-metric-layout; A core:177]; a/PRC-23 [U S:47, 111]; a/POL-32 [A core:180; A ui:126-130].
- **cpy.6** b/W-20 [IS better-writing/SKILL.md § Verb-first buttons]; b/W-22 [§ Links describe their destination]; a/CPY-09 [A core:175, 504-508]; a/W-2.4.4 [A wcag:29]. Conflicts a/CF-16, N-6.
- **cpy.7** a/CPY-02 [U S:105, 227; U signs:44]; a/CPY-03 [U S:106, 227; U signs:43]; b/W-11 [IS better-writing/SKILL.md § Before you finish]; b/W-31 [§ Errors say how to fix].
- **cpy.8** a/CPY-04 [U S:107; U signs:46]; b/W-26 [IS better-writing/SKILL.md § Placeholders show an example]. Conflict a/CF-15.
- **cpy.9** a/CPY-05 [U S:108; U signs:45; A ui:247].
- **cpy.10** a/CPY-06 [U S:109, 227; U signs:48].
- **cpy.11** a/CPY-41 [I #repeated-container-text + reg]; a/CPY-42 [I ex:redundant-ux-writing].
- **cpy.12** a/CPY-35 [A copy:283-292; A ui:158-162; U S:224].
- **cpy.13** b/W-07, b/W-08 [IS better-writing/SKILL.md § Address the reader directly]; b/W-10 [§ Plain words over clever ones].
- **cpy.14** b/W-24, b/W-25 [IS better-writing/SKILL.md § Settings describe the ON state].
- **cpy.15** b/W-30, b/W-31 [IS better-writing/SKILL.md § Errors say how to fix, next to where it broke]; b/A-88 [IS better-accessibility/forms.md § Error messaging]; a/W-3.3.1 [A wcag:38]; a/W-3.3.3 [A wcag:67].
- **cpy.16** b/W-32, b/W-33 [IS better-writing/SKILL.md § Undo beats confirmation]; b/W-34 [IS better-writing/patterns.md § Destructive flows]; a/W-3.3.4 [A wcag:68]. Conflicts b/CF-20, b/CF-30.
- **cpy.17** b/ST-15, b/ST-25, b/ST-26, b/ST-27, b/ST-28 [IS better-writing/patterns.md § Status copy]. Conflict b/CF-35.
- **cpy.18** a/CPY-31 [A core:174, 231-237; A copy:232-246, 357; I #em-dash-overuse + reg]. Conflict a/CF-17.
- **cpy.19** a/CPY-46 [U S:112; U signs:49]; a/CPY-47 [A core:250-254, 274; A ui:267-271]; a/CMP-22 [A ui:243-247]; b/R-73 [IS state-machine/SKILL.md § 4. Feed each state at the data boundary]; b/R-84 [IS variant/SKILL.md § 4. Build it into the real page]; b/R-95 [IS build-design/SKILL.md § 2. Map the design onto the project]. Conflict a/CF-18.
- **cpy.20** a/CPY-16 [A copy:73-81]; a/CPY-17 [A copy:83-91]; a/CPY-18 [A copy:93-101]; a/CPY-20 [A copy:114-123]; a/CPY-30 [A copy:220-228]; a/CPY-32 [A copy:248-257]; a/CPY-33 [A copy:259-268]; a/CPY-34 [A copy:270-281]; a/CPY-36 [A copy:294-302]; a/CPY-37 [A copy:304-312]; a/CPY-40 [A copy:331-339]; a/CPY-48 [A copy:351-357, 372].
- **cpy.21** a/CPY-45 [U S:211].
- **cpy.22** b/W-06 [IS better-writing/SKILL.md § One voice, one vocabulary].

### Components

- **cmp.1** a/CMP-01 [U S:96, 224; U signs:27; A ui:158-162]; a/CMP-07 [A core:187, 419; A ui:152-156]; b/P-22 [IS better-ui/SKILL.md § Match icon stroke to text weight; MIFB SKILL.md § 17]. Conflict a/CF-19.
- **cmp.2** a/CMP-02 [U S:97, 225; U tokens:112; U signs:29]; b/P-21 [IS better-ui/SKILL.md § Match icon stroke to text weight; icons.md]; b/P-23, b/S-09 [IS better-ui/icons.md]; b/P-26 [icons.md § Design at render size]. Conflicts b/CF-06, b/CF-07.
- **cmp.3** a/CMP-03 [U S:98; U tokens:112]; b/P-24 [IS better-ui/SKILL.md § One SVG, recolored per state]; b/P-25 [icons.md § Outline default, fill active]; b/K-08 [icons.md § One SVG, recolored per state].
- **cmp.4** a/CMP-04 [U S:99; U tokens:112; U signs:30].
- **cmp.5** a/CMP-05 [U S:100; A core:421; A ui:150]; a/CMP-06 [A core:186, 418; A ui:146-150; U signs:31]; a/CMP-08 [I #icon-tile-stack + reg]; a/CMP-09 [I ex:massive-icons].
- **cmp.6** a/CMP-12 [U S:113; U signs:54; A core:190, 440-443; A ui:176-180]; a/CMP-13 [A core:190, 444; A ui:178]; b/L-14 [IS better-layout/grouping-and-alignment.md § Keep controls distinct from content].
- **cmp.7** a/CMP-11 [A ui:188-192; I #pulsing-dot + reg]; a/CMP-10 [U S:100]. Conflict a/CF-20.
- **cmp.8** a/CMP-15 [U S:142; U signs:28, 53]; a/CMP-16 [U S:143; U signs:53; U tokens:103]; a/CMP-17 [A core:189, 434-438; A ui:164-168].
- **cmp.9** a/CMP-18 [U S:148; U signs:59; I #identical-card-grids]; a/CMP-19 [A core:156, 461-465; A ui:89-93; U signs:73; I #identical-card-grids].
- **cmp.10** a/CMP-20 [U S:147; U signs:58]; a/CMP-21 [U tokens:21].
- **cmp.11** a/CMP-24 [A core:188; A ui:170-174; I #side-tab + reg; I ex:thick-border-cards]; a/CMP-25 [I #border-accent-on-rounded + reg]; a/CMP-26 [I #gpt-thin-border-wide-shadow + reg].
- **cmp.12** a/CMP-28 [U S:145; U tokens:22]; a/CMP-29 [U S:149]; a/CMP-32 [U S:144; A ui:255-259].
- **cmp.13** a/CMP-30 [U S:144; U signs:56]; a/CMP-31 [U S:146; U signs:57].
- **cmp.14** a/CMP-33 [A ui:261-265]; a/CMP-34 [A ui:249-253].
- **cmp.15** a/CMP-35 [U tokens:66]; a/CMP-36 [U S:87; U tokens:67-68].
- **cmp.16** b/L-13 [IS better-layout/SKILL.md § Keep controls distinct from content]; a/CMP-39 [A core:111-113, 200, 294-307, 604-615; A ui:221-225]; a/LAY-33 [A core:204, 278-283; A ui:215-219].
- **cmp.17** a/CMP-42 [A core:388; A human:175-179; A a11y:140-166]; a/W-3.3.2 [A wcag:39]; b/A-80, b/A-81, b/A-82 [IS better-accessibility/SKILL.md § Label and type every control; forms.md § Labels].
- **cmp.18** a/CMP-43 [A a11y:26-65; A core:299, 330]; b/A-55 [IS better-accessibility/focus-and-keyboard.md § Keyboard patterns]; b/A-60, b/A-61 [IS better-accessibility/SKILL.md § Trap and restore focus].
- **cmp.19** a/CMP-49 [U S:152, 211; U signs:60-61]; a/CMP-50 [U S:211; A core:203, 262-276]; a/PRC-27 [A core:262-276]. Conflict a/CF-21.
- **cmp.20** a/CMP-48 [A core:158, 489; A ui:201-205; I #blinking-cursor].

### States

- **sta.1** a/STA-01 [A core:119-121, 201, 309-316; A human:119-123]; a/STA-05 [A ui:277]; b/ST-01 [IS better-interface/SKILL.md § 1. Resolve the scope first]; b/ST-03 [IS interface-review/SKILL.md § 6]; b/ST-04 [IS state-machine/SKILL.md § 2. Find the states in the code]; b/ST-05 [IS build-design/SKILL.md § 3]; b/ST-13 [IS break/scenarios.md § Quantity]; b/R-62 [IS break/scenarios.md].
- **sta.2** a/STA-02 [A ui:273-277]; b/ST-10, b/ST-11, b/ST-12 [IS better-writing/SKILL.md § Empty states point forward].
- **sta.3** a/STA-03 [A ui:275-277; A human:121-123]; a/POL-23 [A core:401; A ui:297-301; U visuals:80]; b/ST-16, b/A-106 [IS better-accessibility/screen-readers.md § Live regions]; b/ST-17, b/A-90 [IS better-accessibility/forms.md § Submit behavior]; b/ST-19 [IS break/scenarios.md § State].
- **sta.4** a/STA-04 [A ui:277; A human:117, 123; A a11y:101-136]; b/ST-26 [IS better-writing/patterns.md § Status copy]; b/A-87 [IS better-accessibility/SKILL.md § Errors that announce].
- **sta.5** a/STA-12 [U S:151; A core:331]; a/STA-13 [A core:389; A human:163-167]; b/ST-02 [IS better-ui/SKILL.md § Reporting; MIFB SKILL.md § Verification and Verdict].
- **sta.6** a/STA-14 [U S:150; U tokens:116]; a/STA-15 [U S:172; U tokens:130; A mobile:129]; a/CMP-37 [U tokens:69, 116]; b/ST-30 [IS better-ui/icons.md § Outline default, fill active; better-colors/color-usage.md]. Conflict a/CF-12.
- **sta.7** b/A-30, b/A-31, b/A-32 [IS better-accessibility/SKILL.md § Disabled means unavailable; semantics-and-aria.md § Disabled states]; b/ST-29 [IS break/scenarios.md § State]; b/ST-31 [IS better-ui/icons.md]. Conflicts a/CF-12, b/CF-20, b/CF-26.
- **sta.8** a/STA-16 [A core:331-332; A human:87-91, 107-111]; a/W-2.4.7 [A wcag:60]; a/W-2.4.11 [A wcag:61]; b/A-40, b/A-41, b/A-43, b/A-44 [IS better-accessibility/SKILL.md § Visible focus rings; focus-and-keyboard.md § Focus rings]; b/A-48 [focus-and-keyboard.md § Focus not obscured]; b/L-38 [IS better-layout/SKILL.md § Content bleeds, controls float].
- **sta.9** a/STA-07 [A core:149]; a/STA-08 [I #content-hidden-at-rest + reg]; a/STA-09 [I #broken-image + reg]; a/STA-11 [I #text-occlusion + reg]; b/M-23 [IS better-ui/enter-exit.md § CSS].

### Accessibility

- **a11y.1** b/A-01, b/A-02 [IS better-accessibility/SKILL.md § Criteria, not conventions]. Conflict b/CF-24.
- **a11y.2** a/SPC-19 [A core:246; A mobile:113-123]; a/W-2.5.8 [A wcag:63]; a/W-AAA (2.5.5) [A wcag:76-106]; b/A-70, b/A-71, b/A-72, b/A-73, b/A-75, b/A-76 [IS better-accessibility/SKILL.md § Minimum hit area; hit-areas.md]. Conflicts a/CF-14, b/CF-04.
- **a11y.3** a/STA-17 [A core:325-333; A human:93-103; A a11y:69-97]; a/POL-37 [A core:392-394; A human:181-185; A baseline:9-12; I #skipped-heading + reg]; a/W-1.3.2 [A wcag:16]; a/W-2.4.3 [A wcag:28]; a/W-2.4.6 [A wcag:59]; b/L-26 [IS better-layout/SKILL.md § Order by importance]; b/A-16 [IS better-accessibility/semantics-and-aria.md § Landmarks and headings]; b/A-18 [IS better-accessibility/SKILL.md § Structure is navigation]; b/T-24 [IS better-typography/SKILL.md § Typography]. Conflict b/CF-24.
- **a11y.4** a/CMP-45 [A a11y:170-196; A human:95]; a/CMP-46 [A mobile:125-129; A human:93-97]; a/W-2.5.1 [A wcag:30]; a/W-2.5.7 [A wcag:62]; a/W-1.4.13 [A wcag:57]; b/A-78 [IS better-accessibility/hit-areas.md § Gestures and dragging].
- **a11y.5** a/STA-19 [A human:129-133; A wcag:52]; a/W-1.4.4 [A wcag:52].
- **a11y.6** b/A-123, b/A-124, b/A-125 [IS better-accessibility/SKILL.md § Nothing the user needs runs on a timer; motion-and-zoom.md § Autoplay and timed UI]; a/W-2.2.1 [A wcag:23].
- **a11y.7** a/CMP-41 [A core:388; A human:157-161]; a/POL-22 [U visuals:76; A core:388]; a/STA-18 [A a11y:223-249; A wcag:70]; a/W-1.1.1 [A wcag:11]; a/W-2.1.1 [A wcag:20]; a/W-2.5.3 [A wcag:32]; a/W-4.1.2 [A wcag:41]; a/W-4.1.3 [A wcag:70]; b/A-20, b/A-21 [IS better-accessibility/SKILL.md § Accessible names everywhere]; b/A-50 [§ Full keyboard support]; b/A-101, b/A-102 [screen-readers.md § Choosing how to announce a change, § Live regions]; b/A-107 [SKILL.md § Alt text by purpose]; b/W-21 [IS better-writing/SKILL.md § Verb-first buttons]. Canvas `annotation.add` [FC engine/src/dev/annotations.rs:247].
- **a11y.8** a/CMP-38 [A core:387, 391; A human:151-155]; a/CMP-40 [A core:386; A human:145-149; A a11y:9-24]; a/CMP-44 [A a11y:200-219]; b/A-10, b/A-11, b/A-12, b/A-14 [IS better-accessibility/SKILL.md § Native elements first; semantics-and-aria.md]; b/A-45 [SKILL.md § Visible focus rings]; b/A-120 [SKILL.md § Honor prefers-reduced-motion]; b/A-132 [SKILL.md § Survive zoom and text resize]; b/A-04 [SKILL.md § Accessibility: the platform default over a custom rebuild]. Medium from the user's direction [USER: UI screens on the canvas are HTML/React; everything else stays vector].

### Motion

- **mot.1** a/MOT-10 [U S:172]; a/MOT-11 [A core:161, 467-472, 574]; a/MOT-12 [A ui:287-291]; b/M-17 [MIFB animations.md § Motion Restraint].
- **mot.2** b/M-15 [IS better-ui/SKILL.md § High-frequency interactions respond instantly; MIFB SKILL.md § 19]. Conflicts b/CF-22, b/CF-34.
- **mot.3** a/MOT-01 [U S:170, 232; U signs:85; A ui:281-285; I #pulsing-dot + reg]; a/MOT-02 [U S:170; U signs:72, 89; U visuals:78]; a/MOT-14 [I #marquee + reg]; a/MOT-15 [I #blinking-cursor + reg]; a/MOT-16 [I ex:lazy-impact]; a/W-2.2.2 [A wcag:24]. Conflict a/CF-22.
- **mot.4** a/MOT-04 [U signs:87; I #bounce-easing + reg]; a/MOT-05 [U tokens:121; U S:174, 232]; b/M-01 [IS better-ui/SKILL.md § Exact values, optional polish]. Conflict N-2.
- **mot.5** a/MOT-06 [U tokens:123-128]; b/M-20 [IS better-ui/SKILL.md § Stagger infrequent entrances by 100ms]; b/M-21 [enter-exit.md § Enter animations]; b/M-24 [MIFB animations.md § CSS-Only Stagger]; b/M-25, b/M-26, b/M-27 [IS better-ui/SKILL.md § Exits are shorter and smaller than enters]. Conflicts N-3, b/CF-10.
- **mot.6** a/MOT-09 [U S:175; U signs:88; A core:161, 470]; a/MOT-07 [U tokens:130]; b/M-20; b/M-28 [IS better-ui/SKILL.md § Skip state animations on first render]. Conflict a/CF-23.
- **mot.7** a/STA-15 [U S:172; U tokens:130]; a/MOT-07 [U tokens:130]; b/M-10, b/M-11 [IS better-ui/SKILL.md § Press scales to 0.96]; b/M-12 [MIFB SKILL.md § 12]; b/K-07 [IS better-ui/animations.md § Static prop pattern]. Conflicts N-1, b/CF-01, b/CF-02.
- **mot.8** a/MOT-03 [U S:171; U signs:86]; a/MOT-08 [U S:173, 232; U tokens:131; U signs:90]; b/M-36 [IS better-ui/SKILL.md § Transition only what changes].
- **mot.9** b/M-30, b/M-31 [IS better-ui/SKILL.md § Cross-fade contextual icons with exact values; icon-transitions.md; MIFB SKILL.md § 7; JK]; b/M-32 [icon-transitions.md § CSS]. Conflicts b/CF-08, b/CF-32, b/CF-33.
- **mot.10** b/M-16 [IS better-ui/SKILL.md § High-frequency interactions respond instantly; better-interface/SKILL.md § Rank by user impact].
- **mot.11** a/MOT-13 [A core:390; A human:169-173]; a/W-2.3.1 [A wcag:25]; b/M-18 [IS better-ui/SKILL.md § Gate motion behind the reduced-motion preference]; b/M-19 [MIFB animations.md § Motion Restraint]; b/A-121, b/A-122 [IS better-accessibility/SKILL.md § Honor prefers-reduced-motion; § Nothing the user needs runs on a timer]; b/ST-18 [IS better-accessibility/motion-and-zoom.md § What to disable vs reduce]. Conflicts b/CF-09, b/CF-27, b/CF-36.
- **mot.12** canvas-specific, new; a/MOT-17 [U visuals:78]. Canvas `proto.interaction.add` [FC engine/src/cmd/proto.rs:16], refused on boards [FC engine/src/boards/board.rs:209-217].

### Polish

- **pol.1** a/POL-02 [U S:141; U tokens:101-106]; a/POL-03 [U S:141; U signs:55; A core:139, 498-502; A ui:33-37]; a/POL-04 [I #over-round; A page "Skill: Visual & UI Design"]. Discrepancy a/DS-02.
- **pol.2** a/POL-01 [U S:141, 229; U tokens:108]; b/P-01, b/P-02 [IS better-ui/SKILL.md § Outer radius equals inner radius plus padding; MIFB SKILL.md § 1; JK]. Conflict b/CF-13.
- **pol.3** a/POL-05 [U S:91; U tokens:116]; a/POL-06 [U S:91; U tokens:117]; a/POL-07 [A core:140, 451-454; A ui:39-43]. Engine default shadow [FC doc/src/props.rs:514-517].
- **pol.4** b/P-10 [IS better-ui/SKILL.md § Shadows for elevation, borders for structure; MIFB SKILL.md § 3]; b/P-11, b/P-12, b/P-13 [IS better-ui/surfaces.md]; b/P-15 [MIFB surfaces.md § When to Use Shadows vs. Borders]. Conflicts b/CF-11, b/CF-12.
- **pol.5** a/POL-08 [A core:138, 446-449; A ui:27-31; I #glassmorphism]; a/POL-10 [U S:157; U signs:68; A core:143, 429-432; A ui:51-55; I #codex-grid-background + reg]; a/POL-11 [I #repeating-stripes-gradient + reg]; a/POL-12 [A core:144; U S:213; I ex:lazy-cool]; a/CMP-27 [U signs:14]. Conflict a/CF-24.
- **pol.6** a/POL-13 [I reg:hard-offset-shadow; I era:2026]. Discrepancy a/DS-04.
- **pol.7** a/POL-14 [U S:157; U visuals:19, 38; U signs:68, 77-78]; a/POL-15 [A core:194, 474-478; A ui:207-211; I #sketchy-svg; I #shape-assembled-illustration + reg]; a/POL-16 [U S:158; U signs:81]; a/POL-17 [U S:159; U visuals:7; U README:59]; a/POL-18 [U S:29, 162-163; U visuals:9]; a/STA-06 [U S:160].
- **pol.8** a/POL-20 [U signs:79; U visuals:71]; a/POL-21 [U visuals:77; U signs:80]; a/POL-25 [I #organic-clip-path + reg]; a/POL-26 [I #buried-raster + reg].
- **pol.9** b/P-16 [IS better-ui/SKILL.md § Outline images in pure black or white; MIFB SKILL.md § 11; JK]; b/P-17 [MIFB surfaces.md § Image Outlines]. Conflict b/CF-14.
- **pol.10** b/P-05, b/P-06, b/P-07 [IS better-ui/SKILL.md § Align optically where geometry looks off; surfaces.md]; b/S-08. Conflict b/CF-16.
- **pol.11** b/T-62 [IS better-typography/SKILL.md § Trim text boxes in tight containers]. Canvas key `leadingTrim` [FC engine/src/cmd/props.rs:32].
- **pol.12** a/POL-27 [U visuals:10, 23, 35-37]; a/POL-29 [U visuals:64-72]; a/PRC-28 [U visuals:54-58; U S:50].
- **pol.13** a/PRC-29 [U visuals:60; U S:165]; a/POL-19 [U S:230; U signs:98]. Conflict a/CF-27.

### Canvas craft (canvas-specific, new unless a source is named)

- **cnv.1** Facts: `canvas_add_diagram` [SW crates/mcp/src/app/canvas/mod.rs:32-52]; `canvas_add_elements` colors and types [SW crates/mcp/src/app/canvas/args.rs:25]; its `frame` is made a section [SW crates/board/src/skeleton/layers.rs:124-133] and its shapes `board.shape` in the sticky color [layers.rs:25-28, 61-78].
- **cnv.2** Facts: board or design file [SW crates/board/src/doc/mod.rs:20-26]; `proto.*`, `dev.*` refused on boards and `board.*` in design files [FC engine/src/boards/board.rs:204-217]; `add` in a design file [SW crates/board/src/ops/elements.rs:67-69]; the project canvas is a board [SW README.md:500-502]; `file` [SW crates/mcp/src/app/canvas/args.rs:7].
- **cnv.3** The user's direction [USER: UI screens on the canvas are HTML/React; everything else stays vector]; no HTML or web layer in the canvas code (searched `crates/board/src` and `crates/app/src/board` for html and webview: only comments about the old Excalidraw webview, `crates/app/src/board/shown.rs:12`); `inspect.code` [FC engine/src/dev/codegen.rs:40].
- **cnv.4** Presets [FC engine/src/cmd/create.rs:15-51, 368-374]; Sawit's window sizes [USER: Sawit's pages fill the window]; a/LAY-38 [A mobile:39-43]; a/W-1.4.10 [A wcag:54].
- **cnv.5** a/LAY-30 [U S:136, 231]; a/LAY-01 [U S:128].
- **cnv.6** `page.add` dividers [FC engine/src/cmd/page.rs:39-60].
- **cnv.7** The engine's own 100 gap for new and duplicated frames [FC engine/src/cmd/create.rs:395-411; engine/src/cmd/edit.rs:111]; `distribute.horizontal`, `align.top` [FC engine/src/cmd/arrange.rs:17-20]; `object.wrapInSection` [FC engine/src/cmd/structure.rs:45].
- **cnv.8** Default names, including text named after its first line [FC doc/src/lib.rs:1009-1030; engine/src/cmd/create.rs:317-335]; `node.find` [FC engine/src/cmd/edit.rs:312-318].
- **cnv.9** Variant names [FC engine/src/assets/components/variants.rs:28-46]; slash folders [FC engine/src/assets/library.rs:68]; `inspect.code` pairs frames by layer name [FC engine/src/dev/codegen.rs:40]; `data.fill` fills by layer name [FC engine/src/content/datafill.rs:17].
- **cnv.10** b/C-25, b/C-28 [IS better-colors/token-naming.md § Two tiers, § Naming grammar], adapted to slash-grouped variable names; b/T-21 [IS better-typography/spacing-and-sizing.md § Type scale] for role names.
- **cnv.11** b/R-100 [IS build-design/figma.md § What to trust: auto layout over absolute positions].
- **cnv.12** `layout.add` [FC engine/src/cmd/structure.rs:73, 612-656]; layout keys [FC engine/src/cmd/props.rs:22-26]; `layout.setTrack` [FC engine/src/cmd/layout.rs:12-19]; what the canvas doesn't draw [SW crates/board/src/overlay/mod.rs:11-17; crates/board/src/policy.rs:44-51]; `measure.padding` [FC engine/src/canvas/distance.rs:400].
- **cnv.13** b/R-93 [IS build-design/SKILL.md § 2. Map the design onto the project]; `component.create`, `instance.create`, `instance.place` [FC engine/src/cmd/component.rs:58-64, 89-104].
- **cnv.14** Default variant top-left and unique combinations [FC engine/src/assets/components/variants.rs:3-27]; `component.combineVariants`, `addProperty`, `instance.setProperties`, `detach` [FC engine/src/cmd/component.rs:75, 233, 226, 179].
- **cnv.15** b/C-25, b/C-32 [IS better-colors/SKILL.md]; b/T-20 [IS better-typography/SKILL.md § Use a type scale with semantic names]; `$var` fills [FC engine/src/cmd/props.rs:20]; variables and styles [FC engine/src/cmd/styles.rs:31-38, 115-203]; `variable.export` [FC engine/src/cmd/variables_io.rs:18].
- **cnv.16** Default text style [FC doc/src/text.rs:273-293]; text fill on dark pages and wrapping [FC engine/src/cmd/create.rs:317-342].
- **cnv.17** b/R-80 [IS variant/SKILL.md § Different answers, not different tints]; b/R-81 [§ The floor every variant clears]; b/R-83 [§ 3. Name the axis before writing code].
- **cnv.18** b/R-84 [IS variant/SKILL.md § 4. Build it into the real page]; b/R-85 [§ 5. Present the tradeoffs and stop]; b/R-86 [§ 6. Promote one, delete the rest]. The canvas layout (one row, a section, the `Explorations` page) is new. Conflict N-7.
- **cnv.19** b/ST-04 [IS state-machine/SKILL.md § 2]; b/R-71 [same]. Variant names [FC engine/src/assets/components/variants.rs:28-46].
- **cnv.20** [SW crates/mcp/src/lib.rs `INSTRUCTIONS`; crates/mcp/src/app/canvas/mod.rs:54, 245]; batch references [FC engine/src/automation/batch.rs:1-6; engine/src/cmd/edit.rs:463-470].
- **cnv.21** a/PRC-16 [I page "How Impeccable checks these patterns"]; a/PRC-17 [U S:35-38]; a/PRC-22 [U S:46, 50]; a/PRC-33 [A core:347-355]; b/R-08 [IS better-interface/SKILL.md § 5. Verify what can be verified]; b/R-97 [IS build-design/SKILL.md § 4. Compare against the design]; b/TD-43 [IS teardown/SKILL.md § 5. Render, inspect and correct]; b/C-03 [IS better-colors/SKILL.md § Reporting]; b/T-04 [IS better-typography/SKILL.md § Reporting]; b/L-02 [IS better-layout/SKILL.md § Reporting]; b/W-03 [IS better-writing/SKILL.md § Reporting]. Tools [SW crates/mcp/src/app/canvas/mod.rs:180; FC engine/src/cmd/file.rs:118, engine/src/dev/devinspect.rs:318]. Conflict a/CF-26.
- **cnv.22** [SW the user's approved canvas plan: canvas_begin and canvas_progress announce each component and report its steps, so the user watches it being built]
- **cnv.23** a/PRC-23 [U S:47, 111]; b/R-18 [IS better-interface/review-format.md § Verification]; b/R-95 [IS build-design/SKILL.md § 2]; b/R-98 [§ 5. Report and stop].

### Reviewing

- **rev.1** b/R-01 [IS better-interface/SKILL.md § Evidence, not taste]; b/R-05; b/T-01 [IS better-typography/SKILL.md § Measured, not preferred]; b/A-112 (failing pairs are reported, colors left alone unless asked); b/R-12 [§ Require evidence]; b/R-15 [§ Review without mutating by default]; b/A-05 [IS better-accessibility/SKILL.md § Reporting]; b/C-02 [IS better-colors/SKILL.md § Colors]; b/L-01; b/M-02; b/W-01.
- **rev.2** b/R-09, b/R-10, b/R-13 [IS better-interface/SKILL.md § Rank by user impact]; b/R-22, b/R-23, b/R-24, b/R-25, b/R-26, b/R-27 [each better-* SKILL.md § Reporting]; b/ST-32 [IS better-ui/SKILL.md § Hover effects only on hover-capable pointers].
- **rev.3** b/R-02 [IS better-interface/SKILL.md § 1. Resolve the scope first]; b/R-07 [§ 4]; b/R-11 [§ Prefer the cheaper fix]; b/R-14 [§ Consolidate systemic findings]; b/R-16, b/R-17, b/R-19 [IS better-interface/review-format.md]; b/R-21 [every better-* SKILL.md § Reporting]. Conflicts b/CF-18, b/CF-19.

## 2. Duplicates folded together

Each line names the source rules that said the same thing, the one whose wording or values were kept, and the copies dropped as duplicates.

| Kept in | Kept wording or values from | Dropped as duplicates |
| --- | --- | --- |
| col.6 | a/COL-12 (U, A and I agree) | a/COL-13's hue-to-hue ban folded in |
| col.7 | a/COL-14, a/COL-16 | a/COL-15 (glow), a/POL-09 (glow dose) |
| pol.5 | a/POL-10, a/POL-12, with a/POL-08's count of 1–2 | a/POL-11, a/CMP-27 |
| typ.10 | a/TYP-14 (registry ban) | a/CMP-14 (same eyebrow rule on the hero) |
| cpy.3 | union of the word lists in a/CPY-07 | a/CPY-08 |
| cpy.5 | a/CPY-11 | a/CPY-12, a/CPY-13, a/LAY-15, a/CMP-23 |
| cmp.9 | a/CMP-19 | a/CMP-18, a/LAY-10's card clause |
| mot.3 | a/MOT-01 | a/CMP-11's pulse clause, a/MOT-16 |
| spc.2 | b/S-01 (2× value) | a/SPC-08, a/SPC-09, b/L-11's number |
| pol.7 | a/POL-15 | a/POL-14, a/POL-16 |
| typ.1 | b/T-02, a/TYP-01 | a/TYP-02's list kept as the union of lists |
| col.10 | b/C-48 values (WCAG 2.2) | a/COL-20, a/COL-24, b/A-111 (same thresholds; A-111 and C-48 are a by-design pair in IS), a/W-1.4.3, a/W-1.4.11 |
| col.13 | b/A-110 | b/C-33's no-color-alone clause, a/COL-25, a/W-1.4.1 |
| lay.9 | b/T-29 (60–75 and px) | a/LAY-21 |
| lay.16 | b/L-32 (owner) | b/T-41 (IS says better-layout owns it) |
| lay.18 | b/L-49 | b/A-131 (IS says better-layout owns it) |
| lay.3 | b/L-28 | b/C-34's one-action clause (structure kept in layout, color side in cmp.15) |
| lay.20 | a/LAY-34, b/L-40 | a/LAY-37, a/LAY-39, a/LAY-40, a/LAY-42 |
| lay.22 | a/LAY-43 | a/W-1.4.10, b/A-130's reflow clause, a/LAY-41 |
| typ.8 | a/TYP-16 and b/T-27 values | a/TYP-17, a/TYP-18, a/TYP-19 |
| typ.12 | b/T-45 | a/TYP-20's tabular clause, b/T-46 |
| typ.13 | b/T-30, b/T-34 | a/TYP-24, a/TYP-25, a/TYP-26, b/T-31, b/T-32 |
| a11y.2 | b/A-70 (24 required) | a/W-2.5.8, a/SPC-19's 44 kept for touch |
| a11y.3 | a/STA-17 | a/W-1.3.2, a/W-2.4.3, b/L-26, b/A-16, b/T-24 |
| a11y.7 | b/A-20, b/W-21 | a/CMP-41, b/A-21 |
| sta.2 | b/ST-10 | a/STA-02 |
| sta.8 | b/A-41, b/A-43 values | a/STA-16, a/W-2.4.7 |
| cmp.17 | b/A-80 | a/CMP-42, a/W-3.3.2 |
| cmp.1 | b/P-22 | a/CMP-01's set clause |
| cmp.2 | b/P-21, b/P-23 | a/CMP-02's single stroke, b/S-09 |
| cmp.3 | b/P-24, b/P-25 | b/K-08, a/CMP-03 |
| pol.2 | b/P-01 | a/POL-01 |
| pol.4 | b/P-11, b/P-12 (IS and MIFB identical) | b/P-13, b/P-15 |
| pol.9 | b/P-16 | b/P-17 |
| pol.10 | b/P-05 | b/S-08 |
| mot.2 | b/M-15 | MIFB SKILL.md § 19 and animations.md § Motion Restraint (the same rule) |
| mot.5 | b/M-25 | b/M-26, b/M-27, MIFB § 6 |
| mot.7 | b/M-10 | b/M-12, b/K-07, a/MOT-07's press value |
| mot.8 | b/M-36 | a/MOT-08's transition clause, MIFB § 14 |
| mot.9 | b/M-30 | b/M-32, MIFB § 7, JK-06 |
| mot.11 | b/M-18 | b/A-120, b/A-121, a/MOT-13 |
| cpy.15 | b/W-30 | b/A-88, a/W-3.3.1, a/W-3.3.3 |
| cpy.19 | b/R-73, a/CPY-46 | b/R-84's content clause, a/CPY-47's placeholder clause |
| sta.3 | a/STA-03 | b/ST-16, b/ST-17, b/A-90, b/A-106 |
| rev.2 | b/R-09 | b/R-22 to b/R-27 (each restates part of the trigger list) |
| rev.3 | b/R-17, b/R-19 | b/R-21, b/R-14's cap |
| dir.5 | a/PRC-10 | a/PRC-11, a/PRC-12 |
| dir.6 | a/PRC-13, a/PRC-14 | a/CPY-39 |
| lay.11 | a/LAY-01 | a/LAY-02 |
| lay.21 | a/LAY-45 | U S:135's menu clause |
| dir.1 | b/R-04, b/R-05 | b/L-01, b/M-02, a/PRC-08 |
| cnv.21 | a/PRC-17, b/R-97 | a/PRC-22, a/PRC-33, b/R-08, b/C-03, b/T-04, b/L-02, b/TD-43 |
| JK statements | the IS rules they repeat | b/JK-03 (A-70, T-31, A-111, P-01), b/JK-04 (R-17), b/JK-05 (W-33, ST-10), b/JK-06 (M-28, M-30), b/JK-07 (C-06, C-37) |

## 3. Conflicts resolved

### From sources-a (unslop, antislop, Impeccable)

- a/CF-01 Purposeful hue gradients: no hue-to-hue gradient (U, the strictest; I calls a purple or cyan gradient the tell on its own); a brand gradient counts only if the user says it is the brand (U S:66). col.6
- a/CF-02 Glow: none (U, I). A's dose of glow on 1–2 focus elements is dropped because the focus ring (sta.8) does that job without a halo. col.7
- a/CF-03 Default fonts: the project's font wins; a new pick needs a written reason; U's recommended faces are allowed only as stated choices. typ.1
- a/CF-04 Uppercase tracked labels: sentence case, uppercase only on short labels with a purpose (I's convention for short labels, A's purpose gate). typ.9
- a/CF-05 Dashboard shell: A's test (derive the layout from the screen's job) decides; U's shell values are starting points used only when the job needs a sidebar app. lay.1, lay.11
- a/CF-06 Keep or cut sections: keep (U), except template filler with nothing real in it (A), reported. dir.7
- a/CF-07 Weight: 400–600, size and color first (U), weight second (A, I, IS). typ.5
- a/CF-08 Line height: IS's roles, I's floor raised to IS's 1.4 for wrapping text, U's 1.02–1.08 only on one- or two-line heroes. typ.7
- a/CF-09 U's muted tokens fail 4.5:1 on its raised surface: kept in `references/tokens.md` with the measured ratios and where muted may sit. col.10, col.19
- a/CF-10 Number of hues: A's cap for chrome, U's richer moments when each hue means something. col.4
- a/CF-11 Hero height: composed at the preset height on the canvas; in code a dvh-based minimum height, never rigid 100vh. lay.12
- a/CF-12 Color-only states: hover may be a fill step (U; the pointer is the second cue), disabled and selected differ by more than hue (A). sta.6, sta.7
- a/CF-13 Cream pages: allowed when they belong to the product's world, with a reason (U's ramp, I's own exception). col.19
- a/CF-14 Target size: 24 everywhere, 44 on touch frames, 40 on desktop where density allows; U's 32–36 rows on desktop only. a11y.2, spc.7
- a/CF-15 Placeholder wording: IS's format example and U's brevity; A's "Drop your message here..." style dropped. cpy.8
- a/CF-16 Title Case CTAs: sentence case; A's examples were about naming the action. typ.9, cpy.6
- a/CF-17 Em dashes: none in interface copy (A, the strictest; IS's own authoring rule b/R-136 agrees); en dashes for numeric ranges only. cpy.18
- a/CF-18 Mock data: product-shaped sample data in UI (U, IS b/R-73, b/R-84) because layouts must be judged on realistic lengths; never as a claim (A), always listed as sample (IS b/R-95). cpy.19, cpy.5
- a/CF-19 Lucide: the project's set wins; a new set needs a reason fitting the character (A), so Lucide is fine when chosen, not when defaulted. cmp.1
- a/CF-20 Category dots: a dot marks a real state or a labeled data category. cmp.7
- a/CF-21 Redrawing a logo: ask first (A's hard gate); U's flat redraw only after the user agrees. cmp.19
- a/CF-22 Parallax: none (U; IS turns it off under reduced motion). mot.3
- a/CF-23 Entrance and scroll motion: one entrance (U), content visible at rest (I). mot.6, sta.9
- a/CF-24 Glass: only to solve a layering problem (I), on at most 1–2 elements (A). pol.5
- a/CF-25 Who writes the direction: the agent drafts from the product (U), the user approves (A), else a labeled draft. dir.3
- a/CF-26 Verification effort: every delivered frame is looked at after the last change (A); stop after three fix rounds and report (U's cap as a stop rule). cnv.21
- a/CF-27 Empty visual slot: no text inside (U) but labeled by layer name and annotation (A). pol.13
- a/CF-28 Eyebrow strictness: banned outright (I's registry, the strictest). typ.10
- a/DS-01 antislop's install page lags its repo: the repo (v3.2.25) was used.
- a/DS-02 "Radius over 24 without reason" exists only on antislop's page: kept, as it agrees with I #over-round. pol.1
- a/DS-03 antislop's large text at 18px: replaced by WCAG's 24px or 18.67px bold. col.10
- a/DS-04 The 68th Impeccable pattern exists only in the registry: kept. pol.6
- a/DS-05 Impeccable page and registry thresholds differ: the page's targets (65–75 characters, 16px body) with the registry's floors (80 characters, 12px). lay.9, typ.11
- a/DS-06 unslop's reference dashboard glows: the rule (no glow) wins over the image. col.7
- a/DS-07 antislop's bundle and skill carry the same rules: cited once.

### From sources-b (Interface Skills, Make Interfaces Feel Better, jakub.kr)

- b/CF-01 Press-scale floor: 0.96 exactly, as IS HEAD says; MIFB's 0.95 tolerance dropped (IS counts `scale-95` as a finding). mot.7
- b/CF-02 Disabled press: disabled never scales (IS). mot.7
- b/CF-03 `text-wrap: pretty` on long text: no lone last word in any paragraph (IS HEAD); on the canvas this is an edit or `text.fit`. typ.13
- b/CF-04 Minimum target: 24 required, 44 touch, 40 desktop where density allows. a11y.2
- b/CF-05 Where a hit-area pseudo-element goes: code-level, not carried (a11y.2 keeps the rule that hit areas may exceed the visual).
- b/CF-06, b/CF-07 Icon stroke: stroke by adjacent text weight in rendered pixels (IS HEAD); one library per surface. cmp.2, cmp.1
- b/CF-08 Which icon swaps animate: IS HEAD (tab and hover-revealed swaps are instant). mot.9
- b/CF-09 Reduced-motion exits: opacity cross-fade (IS HEAD). mot.11
- b/CF-10 CSS stagger recipe: hidden state never left in a base rule (IS); content visible at rest. sta.9, mot.5
- b/CF-11 Shadow ring in forced colors: code-level (a transparent border under the ring), not carried; pol.4 keeps real borders where structure needs them.
- b/CF-12 Focus via shadows: focus is a ring (sta.8), never a hover-style lift. pol.4, sta.8
- b/CF-13 Concentric formula: IS's, with border width. pol.2
- b/CF-14 Image outline scope: content images only (IS). pol.9
- b/CF-15 Literal colors in icon recipes: tokens only (IS). cmp.3, col.16
- b/CF-16 Physical vs logical properties: code-level; pol.10 keeps the 2px values.
- b/CF-17 `will-change`: code-level, not carried.
- b/CF-18 Verdict and rejected candidates: IS's two verdicts; no "considered but rejected" table. rev.3
- b/CF-19 LOW findings: one cap of 15, LOW allowed (IS). rev.3
- b/CF-20 Disabled type-to-confirm button: enabled, errors on press (better-accessibility over better-writing). sta.7, cpy.16
- b/CF-21, b/CF-22, b/CF-23 The variant picker and state switcher's own CSS: not carried; the canvas shows variants side by side instead (cnv.18).
- b/CF-24 One-`<h1>` outline: a11y.3 states it as the design rule; a11y.1 keeps WCAG A and AA as the requirement, so it is never a blocker on its own.
- b/CF-25 Contrast standard: WCAG 2.2 decides pass or fail (IS HEAD); APCA is optional extra information. col.10
- b/CF-26 Pending submit: label plus spinner, not natively disabled (IS HEAD). sta.3, sta.7
- b/CF-27 Global reduced-motion override: IS HEAD's opt-in gating and cross-fade. mot.11
- b/CF-28 Change-review status: not carried (change reviews are out of scope, section 4).
- b/CF-29 jakub.kr page vs repository: the repository at HEAD was used.
- b/CF-30 Destructive dialog on the demo page: verb plus object (better-writing). cpy.16
- b/CF-31 Ungated `:hover` in IS recipes: code-level, not carried.
- b/CF-32, b/CF-33 Blur and `initial={false}` in Motion recipes: code-level; mot.6 keeps "the default state never animates in", mot.11 the reduced-motion cross-fade.
- b/CF-34 High-frequency transition property: opacity or fill (IS). mot.2
- b/CF-35 Done-state wording: object plus past tense (better-writing). cpy.17
- b/CF-36 Press scale under reduced motion: press feedback stays as a fill step when scale is turned off. mot.11
- b/CF-37 Teardown viewer code vs prose: not carried (teardown is out of scope).

### Between the two sets, and against the user's own rules (found while merging)

- N-1 Press scale: U 0.97–0.98 vs IS 0.96 exact and MIFB 0.96. Kept 0.96: two packs agree, IS states it as exact. mot.7
- N-2 Easing: U one curve `cubic-bezier(0.22, 1, 0.36, 1)` vs IS's several exact curves (`ease-out`, `cubic-bezier(0.2, 0, 0, 1)`). Kept one ease-out curve: a single curve is checkable and matches I's exponential ease-out advice (a/MOT-04). mot.4
- N-3 Menu and popover timing: U 160–220 vs IS's popover example (300 in, 150 out). Kept U for menus, IS's 300 for larger entrances and 150 for exits. mot.5
- N-4 Letter spacing ranges: U −0.02em from 40px and −0.03em from 56px vs IS −0.01 to −0.02em from 24px. Kept both: IS's from 24px, U's −0.03em from 56px. typ.8
- N-5 Content width: U caps app content at 1440–1600 (a/LAY-01) vs the user's rule that Sawit pages fill the window with caps only on prose, launchers and sign-in cards [USER: Sawit's pages fill the window]. Kept the user's rule. lay.11
- N-6 "Get started": A bans it as a generic CTA (a/CPY-09) vs IS using it to enter a flow (b/W-05). Banned as a vague marketing CTA, fine as a flow's first button. cpy.2, cpy.6
- N-7 Variant presentation: IS shows one variant at a time at full size in the real page through a picker (b/R-84, b/K-01 to b/K-06); a canvas shows them side by side. Kept side by side at full size (never scaled down, the reason IS gives against thumbnails). cnv.18

## 4. Source rules not carried over

Read and judged out of scope for a design standard on the canvas. Nothing here contradicts `SKILL.md`; it is code, tooling or procedure for other jobs.

- Web implementation details with no canvas counterpart (HTML, CSS, JS, browser behavior): a/LAY-31's literal `100vh` (resolved in a/CF-11), a/TYP-27 (`clamp()` type), a/POL-24 (hero `<img>` loading), a/POL-33, a/POL-34, a/POL-35, a/POL-36, a/POL-38, a/POL-39 (scripts, Web Vitals, document hygiene, security headers), a/STA-10 (script errors), a/MOT-18 (canvas pixel effects in code); b/A-13, b/A-15, b/A-17, b/A-19, b/A-22, b/A-23, b/A-24 (ARIA and title mechanics), b/A-33, b/A-42, b/A-46, b/A-47 (attribute and CSS details of disabled and focus), b/A-51, b/A-52, b/A-53, b/A-54, b/A-56, b/A-57 (tabindex and key maps; a11y.7 asks for the keyboard path instead), b/A-62, b/A-63, b/A-64, b/S-07 (route changes, skip link, scroll margin), b/A-74 (pseudo-element recipe), b/A-77, b/A-79 (pointer-events, touch-action), b/A-83, b/A-84, b/A-85, b/A-86, b/A-89, b/A-91, b/A-92 (input types, autocomplete, paste, validation timing, announcements of results, unsaved-change guards), b/A-100, b/A-103, b/A-104, b/A-105, b/A-108, b/A-109 (screen-reader markup and media), b/A-133 (rem media queries); b/L-20, b/L-21, b/L-22, b/L-42, b/L-48, b/L-52 (logical properties, RTL CSS, container queries, `dvh`); b/T-08, b/T-10, b/T-11, b/T-12, b/T-13, b/T-14, b/T-15, b/T-16 (font files, synthesis, OpenType properties), b/T-52, b/T-53 (lang and bidi markup), b/T-55, b/T-56, b/T-57, b/T-58, b/T-60, b/T-64, b/T-65 (smoothing, underlines, caret, selection); b/C-06, b/C-07, b/C-08, b/C-12 (CSS color notation and fallbacks), b/C-42, b/C-43 (gradient interpolation and banding), b/C-47, b/C-58 (theme switching CSS, `prefers-contrast`); b/M-05 (CSS transitions vs keyframes), b/M-22 (`@starting-style`), b/M-33 (Motion library imports), b/M-35 (theme-switch transitions), b/M-37, b/M-38 (`will-change`); b/P-14 (forced-colors border), b/P-18, b/P-19, b/P-20 (hover media query, tap highlight, overscroll), b/P-27 (RTL icon flipping); b/W-15, b/W-16 (ICU messages and `Intl` formatting).
- antislop's process machinery: a/PRC-04 (the three dials; the direction in dir.3 replaces them), a/PRC-18, a/PRC-21 (unslop's audit list and nine-pass order), a/PRC-24, a/PRC-25, a/PRC-26 (antislop's modes, audit files and three fixed steps), a/PRC-32 (no string-replacement UI builds: a code rule), a/PRC-34, a/PRC-35 (the Delivery Gate report and its definition of done; the checklist in `SKILL.md` replaces it), a/PRC-37, a/PRC-38 (unslop's story videos and shipping without story classes), a/PRC-39 (the single-file installer), a/PRC-40 to a/PRC-43 (code comment cleanup).
- Image-generation art direction beyond pol.12: a/POL-28 (objects for moments), a/POL-30 (glyph and dithering treatments).
- The WCAG list antislop reproduces, where `SKILL.md` restates no criterion: a/W-1.2.1, a/W-1.2.2, a/W-1.2.3, a/W-1.2.4, a/W-1.2.5 (captions and audio description), a/W-1.3.1, a/W-1.3.4, a/W-1.3.5, a/W-1.4.2, a/W-1.4.12, a/W-2.1.2, a/W-2.1.4, a/W-2.4.1, a/W-2.4.2, a/W-2.4.5, a/W-2.5.2, a/W-2.5.4, a/W-3.1.1, a/W-3.1.2, a/W-3.2.1, a/W-3.2.2, a/W-3.2.3, a/W-3.2.4, a/W-3.2.6, a/W-3.3.7, a/W-3.3.8. a11y.1 makes all of WCAG 2.2 A and AA the requirement, so these still apply in full; they are only not restated.
- Review and workbench procedures of other skills: b/R-03, b/R-06, b/R-20 (routing between better-interface and interface-review), b/A-03 (the two-walk review of built UI with keyboard and screen reader), b/R-30 to b/R-35 (MIFB's review modes and format; resolved in b/CF-18, b/CF-19), b/R-40 to b/R-55 (interface-review: change scopes, git ranges, blast radius, removed signals), b/R-60, b/R-61, b/R-63 to b/R-66 (break's harness pages), b/R-70, b/R-72, b/R-74 (state-machine's throwaway pages), b/ST-20, b/ST-33 (harness-only state rules), b/K-01 to b/K-06, b/K-09 (picker and switcher component specs), b/R-90, b/R-91, b/R-92, b/R-94, b/R-96, b/R-99 (build-design's Figma route and rounding rules), b/R-110, b/R-111, b/R-113 to b/R-121 (explain-interface), b/R-130 to b/R-139 (the packs' own authoring and repository conventions), b/TD-01 to b/TD-03, b/TD-05 to b/TD-10, b/TD-12 to b/TD-21, b/TD-25 to b/TD-42, b/TD-44 to b/TD-47 (teardown technical illustration), b/M-03 (replaying running motion at 10% speed: for built UI), b/JK-01, b/JK-02, b/JK-08 (install notes and the article link).

## 5. Coverage check

Every rule id of both source files is named in sections 1–4 above. The check rebuilt the id lists from the ranges the two files used (sources-a: 343 design rules, 56 WCAG lines and 7 discrepancies; sources-b: 498 ids), because the files themselves had been deleted, and compared them with the ids this map mentions; the result is recorded at the end of this file.

Result of the last run (2026-11-02): source ids rebuilt, sources-a 434 (343 design rules, 56 WCAG lines, 7 discrepancies, 28 conflicts) and sources-b 535 (498 rules and statements, 37 conflicts); all 969 are named in sections 1–4, none is unknown; all 184 rules of `SKILL.md` have an entry in section 1 and no rule id repeats.
