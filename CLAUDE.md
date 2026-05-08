# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`annotate-edit` is a single-file Rust GUI application for annotating images (arrows, rectangles, text). It integrates with GNOME Nautilus via a right-click script. Annotations are stored as JSON in `.annotz` sidecar files and exported as `{name}_annotated.png`.

## Commands

```bash
cargo build --release
cargo check          # fast type-checking without full build
cargo clippy         # linting
```

Install for Nautilus:
```bash
cp target/release/annotate-edit ~/.local/share/nautilus/scripts/
chmod +x ~/.local/share/nautilus/scripts/annotate-edit
```

Run directly:
```bash
./target/release/annotate-edit /path/to/image.png
```

There are no tests.

## Architecture

Everything is in `src/main.rs` (~894 lines). Key sections:

**Data model & persistence** (lines 6–112): `AnnotationKind` enum (Arrow, Rectangle, Text), `Annotation` wrapper, `AnnotationFile` for serde. Sidecar path: `image.png` → `image.png.annotz`.

**App state** (`AnnotateApp`, line 131): Holds image texture, annotation list, undo/redo stacks, current tool, drag state, pan/zoom, and text overlay state.

**Coordinate system** (lines ~200–240): `image_to_screen()` / `screen_to_image()` apply pan+zoom transformations between image pixel space and egui screen space.

**Rendering** (`draw_annotations`, line ~280): Iterates annotations and draws them via egui `Painter`. Hit testing via `hit_test()` uses point-to-segment distance for arrows/lines.

**Export** (`export_annotated`, line ~450): Renders annotations onto a pixel buffer using Bresenham-style line drawing, saves as `{name}_annotated.png`. **Text annotations are not rendered to the exported PNG** (no font rasterization implemented).

**egui update loop** (`impl eframe::App`, line 577): Handles keyboard shortcuts (`Ctrl+Z/Y`, `Ctrl+S`, `Delete`), toolbar widgets, and canvas input (drag to draw, middle-mouse pan, scroll-wheel zoom).

## Key Behaviors

- Auto-saves annotations to disk after every mutation (`auto_save()`)
- Undo/redo uses full state snapshots of the annotation list
- Zoom is centered on cursor position
- Text input uses a floating overlay triggered by click, confirmed with Enter
