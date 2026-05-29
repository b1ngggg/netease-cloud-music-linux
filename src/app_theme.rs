//
// app_theme.rs
// UI theme overrides.
// Distributed under terms of the GPL-3.0-or-later license.
//

pub fn css(dark: bool) -> &'static str {
    if dark { DARK_CSS } else { LIGHT_CSS }
}

const DARK_CSS: &str = r#"
@define-color app_bg #000000;
@define-color app_panel #121212;
@define-color app_panel_raised #181818;
@define-color app_panel_hover #242424;
@define-color app_pressed #2a2a2a;
@define-color app_border rgba(255, 255, 255, 0.08);
@define-color app_text #ffffff;
@define-color app_text_muted #b3b3b3;
@define-color app_green #1db954;
@define-color app_green_hover #1ed760;
@define-color app_input #242424;
@define-color app_actionbar #181818;
@define-color app_control_hover rgba(255, 255, 255, 0.08);
@define-color app_overlay rgba(18, 18, 18, 0.96);
@define-color app_scale_trough #4d4d4d;
@define-color app_card_frame #242424;
@define-color app_hero_start #2b2b2b;
@define-color app_hero_mid #181818;
@define-color app_hero_end #101010;
@define-color app_lyrics_start #242424;
@define-color app_lyrics_mid #181818;
@define-color app_lyrics_end #121212;
@define-color app_lyrics_text rgba(255, 255, 255, 0.60);
@define-color app_shadow rgba(0, 0, 0, 0.38);

.app-brand-icon {
  color: @app_text;
}

.app-window, .app-root, .app-shell,
preferencesdialog, preferencespage, .app-preferences {
  background: @app_bg;
  color: @app_text;
  /* Keep Chinese UI text readable inside Flatpak font sandboxes. */
  font-family: "Noto Sans CJK SC", "Source Han Sans SC", "Microsoft YaHei", "PingFang SC", "WenQuanYi Micro Hei", "SimSun", sans-serif;
}

searchbar.app-search-bar,
searchbar.app-search-bar > revealer,
searchbar.app-search-bar > revealer > box {
  background: transparent;
  border: none;
  box-shadow: none;
}

headerbar.app-header,
preferencesdialog headerbar {
  background: @app_bg;
  color: @app_text;
}

.app-sidebar,
.app-content {
  background: @app_panel;
  color: @app_text;
}

.app-actionbar,
revealer.app-player-revealer,
revealer.app-player-revealer > contents,
revealer.app-player-revealer > contents > actionbar,
revealer.app-player-revealer > contents > actionbar > revealer,
revealer.app-player-revealer > contents > actionbar > revealer > contents,
revealer.app-player-revealer > contents > actionbar > revealer > contents > box,
actionbar.app-actionbar,
actionbar.app-actionbar > revealer,
actionbar.app-actionbar > revealer > contents,
actionbar.app-actionbar > revealer > contents > box,
actionbar.app-actionbar > revealer > box,
.app-actionbar > revealer,
.app-actionbar > revealer > contents,
.app-actionbar > revealer > contents > box,
.app-actionbar > revealer > box {
  background: @app_actionbar;
}

.app-search-panel {
  background: @app_input;
  color: @app_text;
  border-color: @app_border;
  box-shadow: 0 12px 32px @app_shadow;
}

.app-search-panel entry,
.app-search-menu {
  background: transparent;
  color: @app_text;
  border-color: @app_border;
}

menubutton.app-search-menu {
  padding: 0;
  border: none;
  background: transparent;
  box-shadow: none;
}

menubutton.app-search-menu > button,
.app-search-menu button {
  background: transparent;
  color: @app_text;
  border-color: transparent;
  box-shadow: none;
}

menubutton.app-search-menu > button:hover,
.app-search-menu button:hover {
  background: @app_control_hover;
}

popover, popover contents {
  background: @app_panel_raised;
  color: @app_text;
  border-color: @app_border;
}

.lyrics-stage {
  background: linear-gradient(180deg, @app_lyrics_start 0%, @app_lyrics_mid 42%, @app_lyrics_end 100%);
}

textview.lyrics-text-view,
textview.lyrics-text-view text {
  color: @app_lyrics_text;
  font-family: "Noto Sans CJK SC", "Source Han Sans SC", "Microsoft YaHei", "PingFang SC", "WenQuanYi Micro Hei", "SimSun", sans-serif;
}
"#;

const LIGHT_CSS: &str = r#"
@define-color app_bg #eef1ef;
@define-color app_panel #fbfcfb;
@define-color app_panel_raised #f5f8f6;
@define-color app_panel_hover #e8eeeb;
@define-color app_pressed #dbe4df;
@define-color app_border rgba(14, 21, 17, 0.10);
@define-color app_text #111513;
@define-color app_text_muted #5f6863;
@define-color app_green #16883d;
@define-color app_green_hover #1aa34a;
@define-color app_input #f6f8f7;
@define-color app_actionbar #fbfcfb;
@define-color app_control_hover rgba(14, 21, 17, 0.075);
@define-color app_overlay rgba(251, 252, 251, 0.97);
@define-color app_scale_trough #d7dfdb;
@define-color app_card_frame #edf2ef;
@define-color app_hero_start #fbfcfb;
@define-color app_hero_mid #eef4f0;
@define-color app_hero_end #dfeae4;
@define-color app_lyrics_start #fbfcfb;
@define-color app_lyrics_mid #f0f6f2;
@define-color app_lyrics_end #e3ece7;
@define-color app_lyrics_text rgba(17, 21, 19, 0.58);
@define-color app_shadow rgba(15, 23, 18, 0.14);

.app-brand-icon {
  color: @app_text;
}

.app-window, .app-root, .app-shell,
preferencesdialog, preferencespage, .app-preferences {
  background: @app_bg;
  color: @app_text;
  /* Keep Chinese UI text readable inside Flatpak font sandboxes. */
  font-family: "Noto Sans CJK SC", "Source Han Sans SC", "Microsoft YaHei", "PingFang SC", "WenQuanYi Micro Hei", "SimSun", sans-serif;
}

searchbar.app-search-bar,
searchbar.app-search-bar > revealer,
searchbar.app-search-bar > revealer > box {
  background: transparent;
  border: none;
  box-shadow: none;
}

headerbar.app-header,
preferencesdialog headerbar {
  background: @app_bg;
  color: @app_text;
  border-bottom: 1px solid @app_border;
  box-shadow: none;
}

.app-sidebar,
.app-content {
  background: @app_panel;
  color: @app_text;
  border: 1px solid @app_border;
  box-shadow: 0 12px 32px rgba(15, 23, 18, 0.08);
}

.app-actionbar,
revealer.app-player-revealer,
revealer.app-player-revealer > contents,
revealer.app-player-revealer > contents > actionbar,
revealer.app-player-revealer > contents > actionbar > revealer,
revealer.app-player-revealer > contents > actionbar > revealer > contents,
revealer.app-player-revealer > contents > actionbar > revealer > contents > box,
actionbar.app-actionbar,
actionbar.app-actionbar > revealer,
actionbar.app-actionbar > revealer > contents,
actionbar.app-actionbar > revealer > contents > box,
actionbar.app-actionbar > revealer > box,
.app-actionbar > revealer,
.app-actionbar > revealer > contents,
.app-actionbar > revealer > contents > box,
.app-actionbar > revealer > box {
  background: @app_actionbar;
  border-top: 1px solid @app_border;
  box-shadow: 0 -8px 28px rgba(15, 23, 18, 0.08);
}

.app-search-panel {
  background: @app_input;
  color: @app_text;
  border-color: @app_border;
  box-shadow: 0 10px 28px rgba(15, 23, 18, 0.10);
}

.app-search-panel entry,
.app-search-menu {
  background: transparent;
  color: @app_text;
  border-color: @app_border;
}

menubutton.app-search-menu {
  padding: 0;
  border: none;
  background: transparent;
  box-shadow: none;
}

menubutton.app-search-menu > button,
.app-search-menu button {
  background: transparent;
  color: @app_text;
  border-color: transparent;
  box-shadow: none;
}

menubutton.app-search-menu > button:hover,
.app-search-menu button:hover {
  background: @app_control_hover;
}

popover, popover contents {
  background: @app_panel_raised;
  color: @app_text;
  border-color: @app_border;
  box-shadow: 0 18px 42px rgba(15, 23, 18, 0.16);
}

button.app-nav-item.active,
button.app-library-item.active {
  color: @app_text;
  background: rgba(22, 136, 61, 0.11);
}

button.app-nav-item:hover,
button.app-library-item:hover,
flowboxchild:hover,
.songlist_grid_page gridview > child:hover,
.app-singer-card:hover,
.my-action-card:hover,
row.song_row:hover,
row.song_row:selected,
listbox.toplist-sidebar row:hover,
listbox.toplist-sidebar row:selected {
  background: @app_panel_hover;
}

.app-hero-carousel,
carousel.card,
.app-detail-hero,
.app-search-results-hero {
  border: 1px solid @app_border;
  background: linear-gradient(135deg, @app_hero_start 0%, @app_hero_mid 52%, @app_hero_end 100%);
  box-shadow: 0 12px 32px rgba(15, 23, 18, 0.10);
}

flowboxchild,
.songlist_grid_page gridview > child,
.app-singer-card,
.my-action-card,
preferencesgroup row,
preferencesgroup comborow,
preferencesgroup actionrow,
.app-preferences row,
.app-preferences comborow,
.app-preferences actionrow {
  border: 1px solid @app_border;
  background: @app_panel_raised;
  box-shadow: 0 8px 24px rgba(15, 23, 18, 0.06);
}

flowboxchild frame,
gridview > child frame,
.songlist-detail-page frame,
.toplist-page frame,
.app-cover-frame {
  background: @app_card_frame;
  box-shadow: 0 8px 22px rgba(15, 23, 18, 0.10);
}

.toplist-sidebar-panel,
.toplist-sidebar-panel viewport {
  border: 1px solid @app_border;
  background: @app_panel_raised;
}

row.toplist-row:selected .subtitle,
row.toplist-row:hover .subtitle,
listbox.toplist-sidebar row.toplist-row:selected .subtitle,
listbox.toplist-sidebar row.toplist-row:hover .subtitle {
  color: @app_text_muted;
}

button.circular,
.app-now-playing-cover {
  background: @app_panel_hover;
}

button.app-play-button {
  color: #ffffff;
  background: @app_green;
}

button.app-play-button:hover {
  color: #ffffff;
  background: @app_green_hover;
}

button.app-secondary-action {
  color: @app_green;
  background: rgba(22, 136, 61, 0.11);
}

button.app-secondary-action:hover,
button.player-repeat-button.active:hover {
  background: rgba(22, 136, 61, 0.16);
}

scale slider {
  background: @app_panel;
  border: 1px solid rgba(14, 21, 17, 0.14);
  box-shadow: 0 2px 8px rgba(15, 23, 18, 0.15);
}

.my-action-card avatar {
  color: @app_green;
  background: #e7f0ea;
}

textview,
textview text {
  background: @app_panel;
  color: @app_text;
}

.lyrics-stage {
  border: 1px solid @app_border;
  background: linear-gradient(180deg, @app_lyrics_start 0%, @app_lyrics_mid 42%, @app_lyrics_end 100%);
  box-shadow: inset 0 1px rgba(255, 255, 255, 0.72), 0 14px 40px rgba(15, 23, 18, 0.10);
}

textview.lyrics-text-view,
textview.lyrics-text-view text {
  color: @app_lyrics_text;
  background: transparent;
  font-family: "Noto Sans CJK SC", "Source Han Sans SC", "Microsoft YaHei", "PingFang SC", "WenQuanYi Micro Hei", "SimSun", sans-serif;
}

.lyrics-queue-panel {
  border: 1px solid @app_border;
  background: @app_overlay;
  box-shadow: -18px 0 44px rgba(15, 23, 18, 0.15);
}
"#;
