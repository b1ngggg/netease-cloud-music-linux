# Release Checklist

## Artifacts

Publish these files on GitHub Releases:

- `CloudMusicPlayer.flatpak`
- `cloudmusicplayer_1.0.4-1_amd64.deb`

Git tag:

- `v1.0.4`

## Changes

- Fix the song list background when switching to light mode ([#2](https://github.com/b1ngggg/CloudMusicPlayer/issues/2)).

## Compatibility

- Flatpak: recommended for Ubuntu 20.04, Ubuntu 22.04, Ubuntu 24.04 and other Flatpak-capable Linux distributions.
- deb: Ubuntu 24.04+ / newer Debian amd64 only.

## Build

```bash
./scripts/build-flatpak.sh
./scripts/build-deb.sh
```

## User Install Commands

Flatpak:

```bash
flatpak install --user --bundle ./CloudMusicPlayer.flatpak
flatpak run io.github.b1ngggg.CloudMusicPlayer
```

deb:

```bash
sudo apt install ./cloudmusicplayer_1.0.4-1_amd64.deb
```

## Release Notes Template

```text
CloudMusicPlayer v1.0.4

Fixed the song list background when switching to light mode (#2).

Recommended install:
flatpak install --user --bundle ./CloudMusicPlayer.flatpak

deb package:
Only for Ubuntu 24.04+ / newer Debian amd64.
Install with:
sudo apt install ./cloudmusicplayer_1.0.4-1_amd64.deb
```
