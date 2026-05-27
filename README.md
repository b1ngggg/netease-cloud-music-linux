# NetEase Cloud Music Linux

这是一个基于 Rust、GTK4 和 libadwaita 的网易云音乐桌面客户端。

基于原项目：https://github.com/gmg137/netease-cloud-music-gtk

## 主要特性

- 网易云音乐账号登录、歌单、专辑、榜单、搜索、每日推荐和收藏内容浏览
- 底部播放条、播放队列抽屉、循环模式、桌面媒体控制和 MPRIS 支持
- 全屏歌词详情页，包含唱片动画、歌词滚动、播放队列和歌曲评论区
- 歌曲评论加载、点赞、回复、删除和回复列表展示
- 大歌单和大播放队列的分批渲染，减少一次性创建大量 GTK 行导致的卡顿
- 深色/浅色主题适配，自定义应用图标和整体视觉样式

## 开发说明

本仓库基于原项目进行了界面重构、交互优化、评论区、列表性能优化、动态图标和主题等改动。

## 下载安装

推荐普通用户优先使用 Flatpak。Flatpak 版本适合 Ubuntu 20.04 / 22.04 / 24.04 以及其他支持 Flatpak 的 Linux 发行版。

首次使用 Flatpak 时，先安装 Flatpak 并添加 Flathub：

```bash
sudo apt install flatpak
flatpak --user remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
```

然后从 GitHub Releases 下载 `netease-cloud-music-linux.flatpak`，在文件所在目录执行：

```bash
flatpak install --user --bundle ./netease-cloud-music-linux.flatpak
flatpak run io.github.b1ngggg.netease_cloud_music_linux
```

如果桌面环境没有立刻显示应用图标，注销后重新登录即可。

### Debian / Ubuntu deb 包

deb 包仅面向 Ubuntu 24.04+ / Debian 新版 amd64 系统。Ubuntu 20.04 和 22.04 用户请使用 Flatpak。

从 GitHub Releases 下载 `netease-cloud-music-linux_1.0.0-1_amd64.deb` 后安装：

```bash
sudo apt install ./netease-cloud-music-linux_1.0.0-1_amd64.deb
```

如果依赖没有自动修复，可以执行：

```bash
sudo apt -f install
```

## 运行依赖

常见依赖包括：

- Rust toolchain
- GTK4
- libadwaita-1
- OpenSSL
- GStreamer 及 good/bad/ugly/base 插件
- Meson 和 Ninja

不同发行版包名可能不同，请按发行版实际包名安装。

普通用户不需要手动安装这些开发依赖，直接使用 Flatpak 或 deb 包即可。

## 发布构建

构建 deb：

```bash
cargo install cargo-deb
./scripts/build-deb.sh
```

生成路径：

```text
_build/target/debian/netease-cloud-music-linux_1.0.0-1_amd64.deb
```

构建 Flatpak：

```bash
sudo apt install flatpak flatpak-builder
flatpak --user remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
./scripts/build-flatpak.sh
```

生成路径：

```text
_build/flatpak/netease-cloud-music-linux.flatpak
```

## 本地构建

项目当前使用本地 vendor GStreamer 路径构建。建议在仓库根目录执行：

```bash
PATH="$HOME/.cargo/bin:$PATH" \
PKG_CONFIG_PATH="$PWD/_vendor/gstreamer-bad-dev/usr/lib/x86_64-linux-gnu/pkgconfig:$PWD/_vendor/gstreamer-good-dev/usr/lib/x86_64-linux-gnu/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}" \
RUSTFLAGS="-L native=$PWD/_vendor/gstreamer-bad-dev/usr/lib/x86_64-linux-gnu -L native=$PWD/_vendor/gstreamer-good-dev/usr/lib/x86_64-linux-gnu" \
CARGO_NET_GIT_FETCH_WITH_CLI=true \
CARGO_TARGET_DIR="$PWD/_build/target" \
cargo build --manifest-path "$PWD/Cargo.toml" --release
```

## 安装到本地运行目录

```bash
cp "$PWD/_build/target/release/netease-cloud-music-linux" "$PWD/_build/src/netease-cloud-music-linux"

PATH="$HOME/.cargo/bin:$PATH" \
PKG_CONFIG_PATH="$PWD/_vendor/gstreamer-bad-dev/usr/lib/x86_64-linux-gnu/pkgconfig:$PWD/_vendor/gstreamer-good-dev/usr/lib/x86_64-linux-gnu/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}" \
RUSTFLAGS="-L native=$PWD/_vendor/gstreamer-bad-dev/usr/lib/x86_64-linux-gnu -L native=$PWD/_vendor/gstreamer-good-dev/usr/lib/x86_64-linux-gnu" \
CARGO_TARGET_DIR="$PWD/_build/target" \
ninja -C _build install
```

## 常用校验

```bash
cargo clippy --manifest-path "$PWD/Cargo.toml" --release --all-targets
cargo test --manifest-path "$PWD/Cargo.toml" --release
xmllint --noout data/gtk/*.ui data/netease_cloud_music_linux.gresource.xml data/io.github.b1ngggg.netease_cloud_music_linux.metainfo.xml.in
glib-compile-resources --sourcedir=data --generate-dependencies data/netease_cloud_music_linux.gresource.xml
git diff --check
```

## 目录说明

- `src/window.rs`：主窗口、页面切换、抽屉、主题入口
- `src/gui/player_controls.rs`：底部播放条和播放控制
- `src/gui/playlist_lyrics.rs`：歌词详情页、唱片动画、评论区和歌词页队列
- `src/gui/songlist_view.rs`：歌曲列表组件和分批渲染逻辑
- `src/ncmapi.rs`：网易云音乐接口封装
- `src/app_theme.rs`：应用主题覆盖样式
- `data/gtk/`：GTK 模板
- `data/themes/`：主样式文件
- `data/icons/`：应用图标资源

## 许可证

本项目遵循 GNU General Public License v3.0 or later。详见 [LICENSE](LICENSE)。
