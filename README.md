# annotate-edit

A simple one-shotted image annotation to help me annotate screenshots.

# About

This project is a "vibe-coded" Rust application to build a simple image
annotation tool I found was lacking in my setup.

The goal is both to solve a real user pain I had and spot-check AI-coding
capabilities in doing some GUI work.

## Screenshot

![screenshot](/imgs/screenshot-001.png)

## Build and Install for Gnome/Nautilus

Once installed in Nautilus' script dir, one can right-click on an image and
then execute a script on the filepath. Hence, allowing a light-weight version
of "open-with".

```
cargo build --release
cp target/release/annotate-edit ~/.local/share/nautilus/scripts
chmod +x ~/.local/share/nautilus/scripts/annotate-edit 
```
