# CloudMusicPlayer (网易云音乐)

CloudMusicPlayer 是面向网易云音乐的 Linux 音乐播放器，基于 
[netease-cloud-music-gtk](https://github.com/gmg137/netease-cloud-music-gtk) 
进行界面UI、交互、视觉及动画效果设计重构，评论区开放、互动等新功能，并包含播放队列、歌词页和列表性能优化等改动,体验更完善丝滑


## 主要特性

- 网易云音乐账号登录、歌单、专辑、榜单、搜索、每日推荐和收藏内容浏览
- 底部播放条、播放队列抽屉、循环模式、桌面媒体控制和 MPRIS 支持
- 全屏歌词详情页，包含唱片动画、歌词滚动、播放队列和歌曲评论区
- 歌曲评论加载、点赞、回复、删除和回复列表展示
- 大歌单和大播放队列分批渲染，减少大量歌曲一次性加载导致的卡顿
- 深色/浅色主题适配，自定义应用图标和整体视觉样式

## 安装

使用 Flatpak 安装

```bash
flatpak install flathub io.github.b1ngggg.CloudMusicPlayer
```

## 本地构建
```bash
meson setup _build --prefix="$PWD/_run"
ninja -C _build
ninja -C _build install
```
## 界面展示

![发现页](screenshots/discover.png)

![每日推荐](screenshots/daily-recommendation.png)

![歌词详情页](screenshots/lyrics.png)

![播放队列](screenshots/queue.png)

![评论区](screenshots/comments.png)

## 项目说明

本项目是非官方客户端，不隶属于网易云音乐。

版权和来源说明见 [NOTICE.md](NOTICE.md)。

## 许可证

本项目遵循 GNU General Public License v3.0 or later。详见 [LICENSE](LICENSE)。
