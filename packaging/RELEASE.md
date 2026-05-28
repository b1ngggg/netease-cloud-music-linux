# Release Checklist

## Artifacts

Publish these files on GitHub Releases:

- `netease-cloud-music-linux.flatpak`
- `netease-cloud-music-linux_1.0.0-1_amd64.deb`

Git tag:

- `v1.0.0`

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
flatpak install --user --bundle ./netease-cloud-music-linux.flatpak
flatpak run io.github.b1ngggg.netease-cloud-music-linux
```

deb:

```bash
sudo apt install ./netease-cloud-music-linux_1.0.0-1_amd64.deb
```

## Release Notes Template

```text
NetEase Cloud Music Linux v1.0.0

Recommended install:
flatpak install --user --bundle ./netease-cloud-music-linux.flatpak

deb package:
Only for Ubuntu 24.04+ / newer Debian amd64.
Install with:
sudo apt install ./netease-cloud-music-linux_1.0.0-1_amd64.deb
```
