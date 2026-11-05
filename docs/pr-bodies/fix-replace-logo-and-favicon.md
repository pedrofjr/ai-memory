> **Title:** `fix(web): replace bloated logo PNG and serve favicon`

## Summary

- Replace the mislabeled, oversized `docs/logo.png` (was effectively a ~992 KB JPEG) with a proper transparent PNG (~126 KB) for the **embedded web UI** (header + favicon only).
- Serve that asset as the browser tab icon via `<link rel="icon">` and a new `GET /favicon.ico` route.
- Simplify the web static handlers: one `LOGO` blob, `logo()` + `favicon()` helpers, no separate light/dark routes on the HTTP surface.
- **Rename the former `docs/logo.png` to `docs/logo-light.png`** and point the README light-mode `<img>` at it, alongside the existing `docs/logo-dark.png` dark-mode asset. README branding stays on the legacy pair; the web surface uses the new PNG.

### Logo preparation (author workflow)

The new `docs/logo.png` (web only) was prepared outside the repo:

1. Uploaded the source image to [iloveIMG](https://www.iloveimg.com/) for basic cleanup.
2. Cropped to **768×768** for a square, consistent header/favicon aspect ratio.
3. Ran the result through iloveIMG’s **background remover** to produce a transparent PNG suitable for the web header and favicon.

The README continues to use the original maintainer assets (`logo-light.png` / `logo-dark.png`) via `<picture>`; only the compiled web UI and favicon ship the new file.

## Changes

- `docs/logo.png` — new transparent PNG (web UI + favicon).
- `docs/logo-light.png` — former `logo.png` content, used only by the README light branch.
- `docs/logo-dark.png` — unchanged (~1.5 MB).
- `README.md` — `<picture>` uses `logo-light.png` (light) and `logo-dark.png` (dark).
- `crates/ai-memory-web/templates/base.html` — favicon link + header logo (`static/logo.png`).
- `crates/ai-memory-web/src/routes/mod.rs` — `GET /favicon.ico`; single `logo.png` route.
- `crates/ai-memory-web/src/routes/statics.rs` — one embedded logo, shared by header and favicon.

## Test plan

- [x] `cargo fmt --all -- --check`
- [x] `TAILWIND_SKIP=1 cargo clippy --workspace --all-targets -- -D warnings`
- [x] `TAILWIND_SKIP=1 cargo test -p ai-memory-web`
- [ ] `cargo test --workspace` (full suite on CI/Linux)
- [ ] Manual: `cargo build -p ai-memory-cli && ai-memory serve --enable-web`, open `/web`, confirm header logo and tab favicon (new transparent PNG)
- [ ] Manual: verify README on GitHub shows `logo-light.png` in light mode and `logo-dark.png` in dark mode via `<picture>`

## gh command

```powershell
gh pr create --repo akitaonrails/ai-memory `
  --head pedrofjr:fix/replace-logo-and-favicon `
  --base main `
  --title "fix(web): replace bloated logo PNG and serve favicon" `
  --body-file docs/pr-bodies/fix-replace-logo-and-favicon.md
```

When opening the PR manually, omit the **Title** blockquote and **gh command** section from the pasted body (or strip lines 1–2 and the final section).
