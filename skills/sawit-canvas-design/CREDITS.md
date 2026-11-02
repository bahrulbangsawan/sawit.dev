# Credits and notices

`SKILL.md`, `references/` and `scripts/` in this folder adapt material from the works below. What was changed, for CC BY 4.0 Section 3(a)(1)(B) and Apache-2.0 Section 4(b): the rules of five sources were merged into one set, deduplicated, rewritten as numbered checks, reconciled where they conflict (the choices are listed under "Resolved conflicts" in `SKILL.md` and in `sources-map.md`), cut down to what applies to design on Sawit's canvas, and extended with canvas rules of Sawit's own. `sources-map.md` traces every rule to the source lines it came from.

This file reproduces the notice each license asks for. It is a reading of the license texts, not legal advice. The license terms were checked on 2026-11-02 by the research that extracted the rules; the facts below are the ones that research recorded, quoted from each license file.

Sawit's own additions carry no license chosen here: that is the project owner's call.

## 1. unslop

- Author: Damir Sokolovsky (@heydamir, https://x.com/heydamir; https://heydamir.design)
- Source: https://github.com/heydamir/unslop, commit `a6f121eee5f29c2f74976b75456feae8b66afb3a` (2026-11-01)
- License: Creative Commons Attribution 4.0 International (CC BY 4.0), https://creativecommons.org/licenses/by/4.0/; legal code https://creativecommons.org/licenses/by/4.0/legalcode. The repository's `LICENSE` is that legal code byte for byte; `skills/unslop/SKILL.md` and `CITATION.cff` also state CC-BY-4.0. The repository supplies no copyright line.
- What the license requires when sharing an adaptation (Section 3(a)): credit the creator in the manner requested, keep any copyright notice (none supplied), refer to the license and to its disclaimer of warranties, link the material, say that it was modified, and link the license. No ShareAlike condition. No endorsement may be implied (Section 2(a)(6)); trademarks are not licensed (Section 2(b)(2)).
- The author's request (`skills/unslop/SKILL.md:13` at that commit): "unslop was created by Damir Sokolovsky (@heydamir), who started the Unslopping series on X. It is licensed under CC BY 4.0, so keep this credit in any copy or adaptation."

Notice (also carried in `SKILL.md` under "Sources"):

> Design rules adapted from "unslop" by Damir Sokolovsky (@heydamir, https://x.com/heydamir), https://github.com/heydamir/unslop, licensed under CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/). Changed: merged with four other rule sets, rewritten as numbered checks, conflicting values reconciled, and adapted to Sawit's canvas. Provided as-is, without warranties (see Section 5 of the license).

## 2. antislop

- Author: Miqdad Badjuber
- Source: https://github.com/miqdadbadjuber/anti-slop, v3.2.25, commit `941b1532f9cc8ffde766bca5921d75257c9e5762`, as installed by https://antislop.ai.id/install (npm `antislop-ai@3.2.25`, whose `skills/` matches the repository)
- License: MIT. The condition: the copyright notice and the permission notice are included in all copies or substantial portions.

Notice:

```text
MIT License

Copyright (c) 2026 Miqdad Badjuber (antislop)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## 3. web-quality-skills, through antislop

antislop's reference files `skills/antislop-human/references/WCAG.md`, `A11Y-PATTERNS.md`, `skills/antislop-ui/references/CORE-WEB-VITALS.md` and `WEB-BASELINE.md` say they are "Adapted from the MIT-licensed web-quality-skills by Addy Osmani (https://github.com/addyosmani/web-quality-skills)". This standard uses material from `WCAG.md` and `A11Y-PATTERNS.md` (the WCAG criteria and keyboard patterns behind the accessibility rules).

- Author: Addy Osmani
- Source: https://github.com/addyosmani/web-quality-skills
- License: MIT (copies ship in antislop as `LICENSE-web-quality-skills`)

Notice:

```text
MIT License

Copyright (c) 2026 Addy Osmani

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

The research that read antislop recorded the copyright line above from `LICENSE-web-quality-skills` and the MIT text as identical to antislop's; the file itself was not re-read for this notice.

## 4. WCAG 2.2 (W3C)

The accessibility and contrast rules restate success criteria and definitions of Web Content Accessibility Guidelines (WCAG) 2.2, https://www.w3.org/TR/WCAG22/, mostly through antislop's `WCAG.md` and Interface Skills' better-accessibility. W3C's text is under the W3C Document License (https://www.w3.org/copyright/document-license/), which asks derivative works to carry:

> Copyright © 2023 W3C®. This software or document includes material copied from or derived from Web Content Accessibility Guidelines (WCAG) 2.2, https://www.w3.org/TR/WCAG22/.

The WCAG 2.2 document's own header reads "Copyright © 2020-2024 World Wide Web Consortium. W3C® liability, trademark and document use rules apply."

## 5. Impeccable

- Author: Paul Bakaus (https://www.paulbakaus.com)
- Sources: the repository https://github.com/pbakaus/impeccable, commit `e89e3ada3e875a89ac2c5d33d11e65ebef6fe285`, including its detector registry `crates/live/assets/antipatterns.json`; and the catalog page https://impeccable.style/slop/ (fetched 2026-11-02)
- License: the repository is Apache License 2.0, "Copyright 2025 Paul Bakaus". The catalog page states no license, so its wording was not copied: rules from it are paraphrased and cited by card (`https://impeccable.style/slop/#rule-<id>`), and thresholds come from the Apache-licensed registry.
- What Apache-2.0 requires (Section 4): give recipients a copy of the license (here: `licenses/impeccable-LICENSE-Apache-2.0.txt`, fetched from the commit above; its MD5 `af2a7f48dc3d23b3490651ba08789723` matches the copy the research read); mark changed files (this notice, and the "Sources" section of `SKILL.md`, say that `SKILL.md` and `references/tokens.md` contain changed Impeccable material); keep its copyright notice. Impeccable's `NOTICE.md` pertains only to `skill/reference/ios.md` and `android.md`, which this standard does not use, so Section 4(d) asks for none of it. The name is used only to describe origin (Section 6).

Notice:

> Contains material adapted from Impeccable (https://github.com/pbakaus/impeccable), Copyright 2025 Paul Bakaus, licensed under the Apache License, Version 2.0 (http://www.apache.org/licenses/LICENSE-2.0; full text in `licenses/impeccable-LICENSE-Apache-2.0.txt`). Changed: rule descriptions and thresholds rewritten as numbered checks and merged with other sources.

## 6. Interface Skills

- Author: Jakub Krehel
- Source: https://github.com/jakubkrehel/skills, `main` at `60bf140e43c98b248e6594487384ec967e89d98a` (2026-11-02, plugin 1.17.0)
- License: MIT. The `LICENSE` sits only at the repository root, not in the skill folders, so a copy of the notice has to travel with adapted material. The license's "Software" covers the documentation files, so the Markdown is covered.

Notice:

```text
MIT License

Copyright (c) 2026 Jakub Krehel

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## 7. Make Interfaces Feel Better

A separate work by the same author, so it gets its own entry.

- Author: Jakub Krehel
- Source: https://github.com/jakubkrehel/make-interfaces-feel-better, `main` at `35545ea1512ad59fa463e6b1f95ca9c052981fe6` (2026-08-29)
- License: MIT, the same text as Interface Skills (byte-identical `LICENSE`, SHA-1 `7ec4838821b03d1c3b4388c5a50c94fd459ff557`), at the repository root only.

Notice:

```text
MIT License

Copyright (c) 2026 Jakub Krehel

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## 8. Consulted, not copied

- https://jakub.kr/skills (fetched 2026-11-02): no license stated, so default copyright applies; it was read for context and nothing on it was copied. Its demo copy is not used.
- FigCraft, the canvas engine (https://github.com/storytold/figcraft, rev `0971d096db18aa107c5f3d1f83af4ac7f89ea2c3`, "Copyright (c) 2026 ArtCraft Team and the FigCraft contributors", MIT OR Apache-2.0): command names, parameter shapes and the frame preset sizes in `references/` were read from its source. Sawit's root `NOTICE` already carries FigCraft's notice.

## Where these notices go when the skill ships

Ship this file and `licenses/` with `SKILL.md` wherever the skill is copied or installed. Before Sawit bundles the skill in the app, add entries for sections 1–7 to Sawit's root `NOTICE`, which already carries third-party notices in this form.
