# Flatpak Packaging

This packaging target builds a local Flatpak bundle for distribution on Ubuntu
20.04 / 22.04 / 24.04 and other Linux distributions with Flatpak support. It
uses the GNOME runtime, so users do not need a new enough system GTK4/libadwaita
stack from their package manager.

## Build Requirements

```bash
sudo apt install flatpak flatpak-builder
flatpak --user remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
```

## Build

```bash
./scripts/build-flatpak.sh
```

The generated bundle is written to:

```text
_build/flatpak/CloudMusicPlayer.flatpak
```

## Install And Run

```bash
flatpak install --user --bundle _build/flatpak/CloudMusicPlayer.flatpak
flatpak run io.github.b1ngggg.CloudMusicPlayer
```

The local helper manifest in this directory uses the working tree as its source
and allows network access during the build. The top-level
`io.github.b1ngggg.CloudMusicPlayer.yml` manifest is the Flathub-oriented
manifest and uses `cargo-sources.json` for offline Rust dependencies.
