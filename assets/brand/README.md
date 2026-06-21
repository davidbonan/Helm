# Helm visual identity

> **Concept** — A focus frame, representing control, review, and
> direction. You stay at the Helm. *Stay at the Helm.*

All files in this folder are **generated** by `scripts/gen_assets.py`
(logomark geometry + SF Pro Semibold for the wordmark). Do not edit them
by hand — modify the script then regenerate:

```sh
python3 scripts/gen_assets.py
```

## Files

| File | Usage |
|---|---|
| `helm-logo.svg` / `helm-logo-black.svg` | Main logo (mark + wordmark), white / black |
| `helm-mark.svg` | Logomark alone (white) |
| `helm-mark-white.svg` / `helm-mark-black.svg` | Variants for dark / light background |
| `icon.icns` | macOS icon — consumed by `scripts/bundle.sh` (`Contents/Resources/`, cf. `specs/update.md` §3) |
| `icon.ico` | Windows icon (16, 24, 32, 48, 64, 128, 256) |
| `icons/icon-{16…1024}.png` | PNG sizes (Linux, various) |
| `icons/icon-dock-512.png` | Runtime Dock icon (Apple margin) — embedded by `src/app.rs` |
| `favicon.svg` / `favicon.ico` | Favicon (ico: 16, 32, 64, 128, 256) |
| `github-avatar.png` | GitHub avatar 512×512 — to upload in the account/org settings |
| `github-social-preview.png` | Social preview 1200×630 — to upload in the repo's *Settings → Social preview* |
| `splash.png` | Splash screen 2000×1200 |

## Palette

| Color | Hex | Role |
|---|---|---|
| Background | `#0A0A0A` | Background |
| Logo / Text | `#FFFFFF` | Logo, text |
| Accent Purple | `#A78BFA` | Accent, tagline |
| Accent Blue | `#3B82F6` | Secondary accent |
| Git Added | `#22C55E` | Git status: added |
| Git Modified | `#EAB308` | Git status: modified |
| Git Removed | `#EF4444` | Git status: removed |
| Muted | `#64748B` | Secondary text |

## Logomark geometry

Measured on the brand board (units relative to the side of the mark):
stroke **15%**, arm **35%**, central gap **30%**. Each bracket is a
**filled L with small fillets** (no round caps): outer corner ≈ ⅓ of the
stroke, arm ends and inner corner ≈ 0.15 × stroke. In an icon tile: mark at **50%** of the side,
corners rounded at **22%**, background `#0A0A0A`. The icns and Dock icon embed
the Apple margin (824/1024 content).

## Usage on backgrounds

The white mark is used on dark or saturated colored backgrounds (black, purple,
midnight blue); the black mark on light backgrounds. Avoid the white mark on
light or medium-saturation backgrounds (insufficient contrast).
