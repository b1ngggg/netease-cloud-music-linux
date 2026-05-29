# Release Checklist

## Artifacts

Publish these files on GitHub Releases:

- `CloudMusicPlayer.flatpak`
- `cloudmusicplayer_1.0.3-1_amd64.deb`

Git tag:

- `v1.0.3`

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
sudo apt install ./cloudmusicplayer_1.0.3-1_amd64.deb
```

## Release Notes Template

```text
CloudMusicPlayer v1.0.3

Recommended install:
flatpak install --user --bundle ./CloudMusicPlayer.flatpak

deb package:
Only for Ubuntu 24.04+ / newer Debian amd64.
Install with:
sudo apt install ./cloudmusicplayer_1.0.3-1_amd64.deb
```
