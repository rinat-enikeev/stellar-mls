# IEEE Security & Privacy — Submission Bundle

## What's in this directory

| File | Purpose | Where to upload in IEEE Publishing Portal |
|---|---|---|
| `onym-ieee.tex` | Main manuscript LaTeX source | "Manuscript" → file type: TeX/LaTeX source |
| `onym-ieee.pdf` | **YOU MUST REBUILD THIS LOCALLY** | "Manuscript" → file type: PDF |
| `Fig1.tex` | Figure 1 (architecture) — TikZ source | "Figure" → file type: source |
| `Fig1.pdf` | Figure 1 — rendered, ready to embed | "Figure" → file type: PDF |
| `Fig2.tex` | Figure 2 (P1 before/after) — TikZ source | "Figure" → file type: source |
| `Fig2.pdf` | Figure 2 — rendered, ready to embed | "Figure" → file type: PDF |
| `cover-letter.txt` | Cover letter (edit before submitting) | "Cover Letter" — paste content into portal text box, or upload as PDF |
| `headshot.jpg` | **YOU MUST PROVIDE THIS** — high-res author photo | "Author Photo" or "Biography Image" |

## Required actions before submission

### 1. Rebuild `onym-ieee.pdf` on your Mac
The sandbox where this bundle was prepared does not have `IEEEcsmag.cls`
installed and has no network access, so the rendered manuscript PDF must
be built locally. From the directory containing the .tex:

    pdflatex onym-ieee.tex
    pdflatex onym-ieee.tex      # second pass for cross-references

Required: `IEEEcsmag.cls` and supporting files (you already have these
in /Users/programyzer/Developer/stellar-mls/paper/venue/ieee/). The
TikZ packages (tikz, arrows.meta, positioning, calc) ship with TeX Live.

After the rebuild, verify in the produced PDF:

  - The running footer reads "IEEE Security & Privacy" (not
    "Publication Title")
  - Figure 1 appears after the "In Plain Language" section
  - Figure 2 appears inside the P1 subsection
  - All citations render as superscript numbers and the numbering is
    sequential (no `??` artifacts)
  - Index Terms appear directly under the abstract
  - The Acknowledgments include the "Use of generative AI" paragraph

### 2. Provide your headshot
The IEEEcsmag class biography environment renders without an inline
photo (this is a known class limitation noted in the source). The
production team will composite your separately-uploaded photo into the
biography during typesetting. Per IEEE graphics requirements:

  - Format: high-resolution JPEG (also accepts PS, EPS, TIFF)
  - Resolution: ≥300 dpi
  - Aspect: standard headshot (taller than wide)

You already have headshot.jpg in your local IEEE submission directory —
add it to this bundle before uploading.

### 3. Edit the cover letter
`cover-letter.txt` is a draft. Before submitting:

  - Add today's date
  - Optionally add 2–3 suggested reviewer names with affiliations
    (currently deferred to editor)
  - Adjust the wording on AI use to your comfort level
  - Convert to PDF if the portal requires PDF (most portals accept
    pasted text directly)

### 4. ORCID
The portal will require an ORCID at submission. If you do not yet have
one: https://orcid.org/register (takes ~3 minutes). Add it to your
ScholarOne / IEEE Author Portal account before starting submission.

## What I removed from the original .tex (and why)

- `\jname{IEEE Computer Society Magazine}` → `IEEE Security & Privacy`
  (was a generic placeholder; this is what makes the running footer correct)
- `\jtitle{Publication Title}` → `IEEE Security & Privacy`
  (same reason)
- 17 manual `\textsuperscript{N}` citations → `\cite{key}` calls
  (manual numbering would silently desync if any reference is
  added/removed; \cite auto-renumbers)
- Index Terms moved from mid-introduction into the abstract block,
  alphabetized, hyphenated "zero-knowledge proof", em-dash separator
  (matches IEEE house style)
- Two unused references (`zcash`, `barbulescu2019`) commented out
  — restore by uncommenting in the bibliography if you want them as
  background cites
- `cap0059` formally cited in Acknowledgments where CAP-0059 is
  mentioned in prose

## What I left for you to decide

- **Audience accessibility of the abstract.** Currently opens with
  Groth16/BLS12-381 jargon. The CFP says "Authors should not assume
  that the audience will have specialized experience in a particular
  subfield." A plainer opening sentence would help, but rewriting an
  abstract is a content choice, not a formatting fix.

- **AI disclosure citation form.** IEEE policy reads "citation to the
  AI system used to generate the text." Your prose disclosure is more
  thorough than most submissions. The conservative move is adding a
  formal bibliography entry for Claude — something like:

      \bibitem{claude2026}
      Anthropic, ``Claude (Opus 4.x),'' [Online]. Available:
      \url{https://claude.ai}, accessed 2026.

  and citing it in the AI paragraph. Whether this is needed depends on
  your read of how strictly the desk editor will interpret the policy.

- **Word count.** Currently ~2,800 words against the 5,500 ceiling.
  This is on the short side for a feature article; editors sometimes
  desk-reject pieces that look more like extended abstracts. The
  natural way to lengthen without padding is to expand the
  "In Plain Language" framing and add operational detail to the
  Construction section.

## IEEE policies you should re-read before submitting

- AI disclosure: https://magazines.ieeeauthorcenter.ieee.org/get-started-with-ieee-magazines/publishing-ethics/
- Submission requirements (the page I worked from):
  https://magazines.ieeeauthorcenter.ieee.org/create-your-ieee-magazine-article/article-submission-requirements/
- IEEE Magazine Style Guide (linked from):
  https://magazines.ieeeauthorcenter.ieee.org/your-role-in-article-production/ieee-magazines-editorial-style/
- S&P Call for Papers (scope check):
  https://www.computer.org/digital-library/magazines/sp/cfp-ieee-security-and-privacy
