//
// playlist_lyrics.rs
// Copyright (C) 2022 gmg137 <gmg137 AT live.com>
// Copyright (C) 2026 b1ngggg
// Distributed under terms of the GPL-3.0-or-later license.
//
use adw::subclass::prelude::BinImpl;
use async_channel::Sender;
use gettextrs::gettext;
use glib::{ParamSpec, Value, closure_local};
use gtk::{CompositeTemplate, gdk, gdk_pixbuf, glib, prelude::*, subclass::prelude::*, *};
use ncm_api::SongInfo;
use once_cell::sync::Lazy;
use once_cell::sync::OnceCell;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::{
    application::Action,
    gui::{app_menu, songlist_row::SonglistRow, songlist_view::SongListView},
    model::ImageDownloadImpl,
    ncmapi::{CommentReply, SongComment, SongCommentReplies, SongComments},
    path::CACHE,
};

const RECORD_SPIN_DURATION_SECONDS: f64 = 42.0;
const RECORD_SPIN_DEGREES_PER_SECOND: f64 = 360.0 / RECORD_SPIN_DURATION_SECONDS;
const COMMENTS_PRELOAD_MARGIN: f64 = 180.0;
const COMMENTS_BOTTOM_LOAD_MARGIN: f64 = 360.0;
const COMMENTS_LOAD_COOLDOWN: Duration = Duration::from_millis(900);

#[derive(Clone, Copy)]
struct CommentDeleteTarget {
    song_id: u64,
    parent_comment_id: u64,
    comment_id: u64,
}

glib::wrapper! {
    pub struct PlayListLyricsPage(ObjectSubclass<imp::PlayListLyricsPage>)
        @extends adw::Bin, Widget, Paned,
        @implements Accessible, Orientable, ConstraintTarget,Buildable;
}

impl PlayListLyricsPage {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn set_sender(&self, sender_: Sender<Action>) {
        let sender = sender_.clone();
        self.imp().sender.set(sender).unwrap();

        let sender = sender_;
        self.imp()
            .songs_list
            .get()
            .connect_row_activated(closure_local!(move |_: SongListView, row: SonglistRow| {
                if let Some(si) = row.get_song_info() {
                    sender.send_blocking(Action::UpdateLyrics(si, 0)).unwrap();
                }
            }));
    }

    pub fn set_comment_user_info(&self, uid: u64, nickname: String, avatar_url: String) {
        let imp = self.imp();
        imp.current_user_id.set(uid);
        imp.current_user_nickname.replace(nickname);
        imp.current_user_avatar_url.replace(avatar_url);
    }

    pub fn playlist_changed(&self, sis: &[SongInfo]) -> bool {
        let playlist = self.imp().playlist.borrow();
        playlist.len() != sis.len()
            || playlist
                .iter()
                .zip(sis.iter())
                .any(|(old, new)| old.id != new.id)
    }

    pub fn init_page(
        &self,
        sis: &[SongInfo],
        si: SongInfo,
        likes: &[bool],
        playlist_changed: bool,
    ) -> bool {
        let imp = self.imp();
        let old_song_id = imp.current_song_id.get();

        if playlist_changed {
            self.update_playlist(sis, si.clone(), likes);
        } else if old_song_id != si.id {
            self.switch_current_row(si.id);
        }

        let song_changed = old_song_id != si.id;
        self.update_now_playing(&si);
        if song_changed {
            self.restart_record_motion();
        }
        if song_changed || imp.comments_song_id.get() != si.id {
            self.prepare_comments_for_song(si.id);
        }
        imp.current_song_id.set(si.id);
        self.setup_scroll_controller();
        song_changed || imp.lyrics_song_id.get() != si.id
    }

    pub fn prepare_comments_for_song(&self, song_id: u64) {
        let imp = self.imp();
        if imp.comments_song_id.get() == song_id {
            self.queue_comments_visibility_check();
            return;
        }

        imp.comments_song_id.set(song_id);
        imp.comments_next_offset.set(0);
        imp.comments_loaded.set(false);
        imp.comments_loading.set(false);
        imp.comments_pending_offset.set(None);
        imp.comments_last_load_started_at.replace(None);
        imp.comments_exhausted.set(false);
        imp.comments_seen_ids.borrow_mut().clear();
        imp.comments_latest_header_added.set(false);
        imp.comments_hot_header_added.set(false);
        imp.comments_spinner.set_visible(false);
        imp.comments_spinner.set_spinning(false);
        imp.comments_count_label.set_label(&gettext("Not loaded"));
        imp.comments_status_label.set_visible(true);
        imp.comments_status_label
            .set_label(&gettext("Scroll down to view comments"));
        self.clear_comments();

        self.queue_comments_visibility_check();
    }

    pub fn begin_comments_update(&self, song_id: u64, offset: u32) -> bool {
        let imp = self.imp();
        if imp.comments_song_id.get() != song_id {
            self.prepare_comments_for_song(song_id);
        }
        if imp.comments_pending_offset.get() == Some(offset) {
            imp.comments_pending_offset.set(None);
        }
        if song_id == 0
            || imp.comments_loading.get()
            || imp.comments_exhausted.get()
            || offset != imp.comments_next_offset.get()
        {
            return false;
        }

        imp.comments_loading.set(true);
        imp.comments_last_load_started_at
            .replace(Some(Instant::now()));
        imp.comments_spinner.set_visible(true);
        imp.comments_spinner.set_spinning(true);
        imp.comments_status_label.set_visible(true);
        if offset == 0 {
            imp.comments_count_label.set_label(&gettext("Loading..."));
            imp.comments_status_label
                .set_label(&gettext("Loading comments..."));
        } else {
            imp.comments_status_label
                .set_label(&gettext("Loading more comments..."));
        }
        true
    }

    pub fn comments_update_failed(&self, song_id: u64, offset: u32) {
        let imp = self.imp();
        if imp.comments_song_id.get() != song_id || offset != imp.comments_next_offset.get() {
            return;
        }

        imp.comments_loading.set(false);
        imp.comments_pending_offset.set(None);
        imp.comments_spinner.set_spinning(false);
        imp.comments_spinner.set_visible(false);
        imp.comments_status_label.set_visible(true);
        if offset == 0 {
            imp.comments_count_label.set_label(&gettext("Unavailable"));
            imp.comments_status_label
                .set_label(&gettext("Failed to load comments"));
        } else {
            imp.comments_status_label.set_label(&gettext(
                "Failed to load more comments. Scroll down to retry",
            ));
        }
    }

    pub fn update_comments(&self, song_id: u64, offset: u32, comments: SongComments) {
        let imp = self.imp();
        if imp.comments_song_id.get() != song_id {
            return;
        }

        imp.comments_loading.set(false);
        imp.comments_pending_offset.set(None);
        imp.comments_spinner.set_spinning(false);
        imp.comments_spinner.set_visible(false);
        imp.comments_count_label
            .set_label(&format_comment_count(comments.total));

        if offset == 0 {
            self.clear_comments();
            imp.comments_seen_ids.borrow_mut().clear();
            imp.comments_latest_header_added.set(false);
            imp.comments_hot_header_added.set(false);
        }

        let has_hot = !comments.hot_comments.is_empty();
        let has_latest = !comments.comments.is_empty();
        if offset == 0 && has_hot && !imp.comments_hot_header_added.replace(true) {
            self.append_comment_header(&gettext("Hot Comments"));
            for comment in comments.hot_comments.iter().take(8) {
                if self.register_visible_comment(comment.comment_id) {
                    self.append_comment(comment);
                }
            }
        }
        if has_latest {
            if !imp.comments_latest_header_added.replace(true) {
                self.append_comment_header(&gettext("Latest Comments"));
            }
            for comment in comments.comments.iter() {
                if self.register_visible_comment(comment.comment_id) {
                    self.append_comment(comment);
                }
            }
        }

        let next_offset = offset.saturating_add(comments.comments.len() as u32);
        imp.comments_next_offset.set(next_offset);
        imp.comments_loaded.set(true);
        let loaded_all = comments.comments.is_empty()
            || comments.comments.len() < usize::from(crate::ncmapi::SONG_COMMENT_LIMIT)
            || (comments.total != 0 && u64::from(next_offset) >= comments.total);
        imp.comments_exhausted.set(loaded_all);

        let has_comments = !imp.comments_seen_ids.borrow().is_empty();
        imp.comments_status_label.set_visible(!has_comments);
        if !has_comments {
            imp.comments_status_label.set_label(&gettext("No comments"));
        }
    }

    pub fn update_comment_like(&self, song_id: u64, comment_id: u64, liked: bool) {
        let imp = self.imp();
        if imp.comments_song_id.get() != song_id {
            return;
        }

        let Some(liked_state) = imp.comment_liked_states.borrow().get(&comment_id).cloned() else {
            return;
        };
        let Some(count_state) = imp.comment_like_counts.borrow().get(&comment_id).cloned() else {
            return;
        };
        let old_liked = liked_state.replace(liked);
        if old_liked != liked {
            let count = count_state.get();
            count_state.set(if liked {
                count.saturating_add(1)
            } else {
                count.saturating_sub(1)
            });
        }

        if let Some(button) = imp.comment_like_buttons.borrow().get(&comment_id) {
            button.set_sensitive(true);
            if liked {
                button.add_css_class("liked");
            } else {
                button.remove_css_class("liked");
            }
        }
        if let Some(label) = imp.comment_like_labels.borrow().get(&comment_id) {
            if liked {
                label.add_css_class("liked");
            } else {
                label.remove_css_class("liked");
            }
            label.set_label(&format_short_count(count_state.get()));
        }
    }

    pub fn update_comment_replies(
        &self,
        song_id: u64,
        comment_id: u64,
        replies: SongCommentReplies,
    ) {
        let imp = self.imp();
        if imp.comments_song_id.get() != song_id {
            return;
        }

        if let Some(count_state) = imp.comment_reply_counts.borrow().get(&comment_id) {
            count_state.set(replies.total.max(replies.replies.len() as u64));
        }

        let list_box = imp.comment_reply_lists.borrow().get(&comment_id).cloned();
        let Some(list_box) = list_box else {
            return;
        };
        self.unregister_reply_rows(comment_id);
        while let Some(child) = list_box.first_child() {
            list_box.remove(&child);
        }
        if replies.replies.is_empty() {
            append_empty_reply_row(&list_box);
        } else {
            for reply in &replies.replies {
                self.append_comment_reply(&list_box, comment_id, reply);
            }
        }

        if let Some(loaded) = imp.comment_reply_loaded.borrow().get(&comment_id) {
            loaded.set(true);
        }
        if let Some(revealer) = imp.comment_reply_revealers.borrow().get(&comment_id) {
            revealer.set_reveal_child(true);
        }
        self.update_reply_button_label(comment_id);
    }

    pub fn dismiss_comment_context_menu(&self) {
        let imp = self.imp();
        app_menu::clear_overlay_menu(
            &imp.lyrics_overlay,
            &imp.active_comment_context_layer,
            &imp.active_comment_context_card,
        );
    }

    pub fn comment_context_contains_widget(&self, widget: &Widget) -> bool {
        app_menu::contains_widget(&self.imp().active_comment_context_card, widget)
    }

    pub fn update_comment_reply_count(&self, song_id: u64, comment_id: u64, count: u64) {
        let imp = self.imp();
        if imp.comments_song_id.get() != song_id {
            return;
        }
        if let Some(count_state) = imp.comment_reply_counts.borrow().get(&comment_id) {
            count_state.set(count);
        }
        self.update_reply_button_label(comment_id);
    }

    pub fn comment_reply_sent(&self, song_id: u64, comment_id: u64, reply: Option<CommentReply>) {
        let imp = self.imp();
        if imp.comments_song_id.get() != song_id {
            return;
        }
        if let Some(entry) = imp.comment_input_entries.borrow().get(&comment_id) {
            entry.set_text("");
            entry.set_sensitive(true);
        }
        if let Some(button) = imp.comment_send_buttons.borrow().get(&comment_id) {
            button.set_sensitive(true);
        }
        if let Some(revealer) = imp.comment_input_revealers.borrow().get(&comment_id) {
            revealer.set_reveal_child(false);
        }
        if let Some(reply) = reply {
            if let Some(revealer) = imp.comment_reply_revealers.borrow().get(&comment_id) {
                revealer.set_reveal_child(true);
            }
            let already_visible = imp
                .comment_row_widgets
                .borrow()
                .contains_key(&reply.comment_id);
            if !already_visible {
                if let Some(list_box) = imp.comment_reply_lists.borrow().get(&comment_id).cloned() {
                    if imp
                        .comment_reply_ids_by_parent
                        .borrow()
                        .get(&comment_id)
                        .map(|ids| ids.is_empty())
                        .unwrap_or(true)
                    {
                        while let Some(child) = list_box.first_child() {
                            list_box.remove(&child);
                        }
                    }
                    self.append_comment_reply(&list_box, comment_id, &reply);
                }
                if let Some(count_state) = imp.comment_reply_counts.borrow().get(&comment_id) {
                    count_state.set(count_state.get().saturating_add(1));
                }
            }
            if let Some(loaded) = imp.comment_reply_loaded.borrow().get(&comment_id) {
                loaded.set(true);
            }
        } else {
            if let Some(revealer) = imp.comment_reply_revealers.borrow().get(&comment_id) {
                revealer.set_reveal_child(false);
            }
            if let Some(count_state) = imp.comment_reply_counts.borrow().get(&comment_id) {
                count_state.set(count_state.get().saturating_add(1));
            }
            if let Some(loaded) = imp.comment_reply_loaded.borrow().get(&comment_id) {
                loaded.set(false);
            }
        }
        self.update_reply_button_label(comment_id);
    }

    pub fn update_now_playing(&self, si: &SongInfo) {
        let imp = self.imp();
        imp.lyrics_song_title_label.set_label(&si.name);
        imp.lyrics_song_title_label.set_tooltip_text(Some(&si.name));
        imp.lyrics_song_meta_label
            .set_label(&format!("{} · {}", si.album, si.singer));

        let cover_image = imp.record_cover_image.get();
        cover_image.set_text(Some(&si.name));
        let mut path_cover = CACHE.clone();
        path_cover.push(format!("{}-songlist.jpg", si.album_id));
        if path_cover.exists() {
            set_avatar_image(&cover_image, &path_cover);
        } else {
            cover_image.set_custom_image(None::<&gdk::Paintable>);
            cover_image.set_icon_name(Some("image-missing-symbolic"));
            if !si.pic_url.is_empty() {
                let sender = imp.sender.get().unwrap().clone();
                cover_image.set_from_net(si.pic_url.clone(), path_cover, (360, 360), &sender);
            }
        }
    }

    pub fn set_queue_revealed(&self, revealed: bool) {
        self.imp().queue_revealer.set_reveal_child(revealed);
    }

    pub fn toggle_queue_revealed(&self) {
        let queue_revealer = self.imp().queue_revealer.get();
        queue_revealer.set_reveal_child(!queue_revealer.reveals_child());
    }

    pub fn set_playback_active(&self, active: bool) {
        let imp = self.imp();
        if imp.playback_active.replace(active) == active {
            return;
        }
        self.sync_record_motion();
    }

    pub fn sync_record_motion(&self) {
        let imp = self.imp();
        let angle = self.current_record_angle();
        imp.record_angle.set(angle);

        let record_disc = imp.record_disc.get();
        let record_tonearm = imp.record_tonearm.get();
        record_disc.remove_css_class("spinning");
        record_disc.remove_css_class("paused");
        if imp.playback_active.get() {
            imp.record_started_at.replace(Some(Instant::now()));
            self.apply_record_spin_animation(angle);
            record_tonearm.add_css_class("playing");
        } else {
            imp.record_started_at.replace(None);
            self.apply_record_static_transform(angle);
            record_tonearm.remove_css_class("playing");
        }
    }

    pub fn queue_record_motion_sync(&self) {
        self.sync_record_motion();

        let obj_weak = self.downgrade();
        glib::timeout_add_local_once(Duration::from_millis(16), move || {
            let Some(obj) = obj_weak.upgrade() else {
                return;
            };
            obj.sync_record_motion();
        });
    }

    pub fn restart_record_motion(&self) {
        let imp = self.imp();
        imp.record_angle.set(0.0);
        imp.record_started_at.replace(None);
        if imp.playback_active.get() {
            imp.record_started_at.replace(Some(Instant::now()));
            self.apply_record_spin_animation(0.0);
            imp.record_tonearm.add_css_class("playing");
        } else {
            self.apply_record_static_transform(0.0);
            imp.record_tonearm.remove_css_class("playing");
        }
    }

    fn setup_record_animation_css(&self) {
        let imp = self.imp();
        imp.record_cover_image
            .set_widget_name("lyrics_record_spinner");

        let provider = CssProvider::new();
        provider.load_from_data(&Self::record_static_css(0.0));
        gtk::style_context_add_provider_for_display(
            &gdk::Display::default().expect("Could not connect to a display."),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
        imp.record_css_provider.replace(Some(provider));
    }

    fn current_record_angle(&self) -> f64 {
        let imp = self.imp();
        let angle = imp.record_angle.get();
        if let Some(started_at) = *imp.record_started_at.borrow() {
            (angle + started_at.elapsed().as_secs_f64() * RECORD_SPIN_DEGREES_PER_SECOND)
                .rem_euclid(360.0)
        } else {
            angle
        }
    }

    fn apply_record_static_transform(&self, angle: f64) {
        self.load_record_css(&Self::record_static_css(angle));
    }

    fn apply_record_spin_animation(&self, angle: f64) {
        let imp = self.imp();
        let generation = imp.record_animation_generation.get().wrapping_add(1);
        imp.record_animation_generation.set(generation);
        self.load_record_css(&format!(
            "@keyframes lyrics-record-spin-{generation} {{
                from {{ transform: rotate({angle:.4}deg); }}
                to {{ transform: rotate({end_angle:.4}deg); }}
            }}

            #lyrics_record_spinner {{
                transform: rotate({angle:.4}deg);
                animation: lyrics-record-spin-{generation} {duration:.4}s linear infinite;
            }}",
            end_angle = angle + 360.0,
            duration = RECORD_SPIN_DURATION_SECONDS,
        ));
    }

    fn load_record_css(&self, css: &str) {
        if let Some(provider) = self.imp().record_css_provider.borrow().as_ref() {
            provider.load_from_data(css);
        }
    }

    fn record_static_css(angle: f64) -> String {
        format!(
            "#lyrics_record_spinner {{
                transform: rotate({angle:.4}deg);
                animation: none;
            }}"
        )
    }

    fn setup_scroll_controller(&self) {
        if self.imp().scroll_controller_ready.replace(true) {
            return;
        }

        let lyrics_outer_scroll = self.imp().lyrics_outer_scroll.get();
        let outer_adjustment = lyrics_outer_scroll.vadjustment();
        let obj_weak = self.downgrade();
        outer_adjustment.connect_value_changed(move |_| {
            let Some(obj) = obj_weak.upgrade() else {
                return;
            };
            obj.maybe_load_comments_for_scroll();
        });

        let outer_scroll_controller =
            EventControllerScroll::new(EventControllerScrollFlags::VERTICAL);
        let obj_weak = self.downgrade();
        outer_scroll_controller.connect_scroll(move |_, _, _| {
            let Some(obj) = obj_weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            obj.maybe_load_comments_for_scroll();
            glib::Propagation::Proceed
        });
        lyrics_outer_scroll.add_controller(outer_scroll_controller);

        let scroll_controller = EventControllerScroll::new(EventControllerScrollFlags::VERTICAL);
        let obj_weak = self.downgrade();
        scroll_controller.connect_scroll(move |_, _, _| {
            let Some(obj) = obj_weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            obj.note_manual_lyrics_scroll();
            glib::Propagation::Proceed
        });
        let scroll_win = self.imp().scroll_lyrics_win.get();
        scroll_win.add_controller(scroll_controller);

        let lyrics_text_view = self.imp().lyrics_text_view.get();
        self.attach_lyrics_text_context_menu(&lyrics_text_view);
    }

    fn queue_comments_visibility_check(&self) {
        let obj_weak = self.downgrade();
        glib::timeout_add_local_once(Duration::from_millis(80), move || {
            let Some(obj) = obj_weak.upgrade() else {
                return;
            };
            obj.maybe_load_comments_for_scroll();
        });
    }

    fn maybe_load_comments_for_scroll(&self) {
        let imp = self.imp();
        if imp.comments_song_id.get() == 0 {
            return;
        }

        if !imp.comments_loaded.get() {
            if self.comments_section_near_viewport() {
                self.request_more_comments();
            }
            return;
        }

        if self.comments_bottom_near() {
            self.request_more_comments();
        }
    }

    fn comments_section_near_viewport(&self) -> bool {
        let imp = self.imp();
        let outer = imp.lyrics_outer_scroll.get();
        let section = imp.comments_section.get();
        let viewport_height = outer.allocated_height().max(1) as f64;
        section
            .compute_point(&outer, &gtk::graphene::Point::new(0.0, 0.0))
            .map(|point| f64::from(point.y()) <= viewport_height + COMMENTS_PRELOAD_MARGIN)
            .unwrap_or_default()
    }

    fn comments_bottom_near(&self) -> bool {
        let adjustment = self.imp().lyrics_outer_scroll.vadjustment();
        adjustment.value() + adjustment.page_size()
            >= adjustment.upper() - COMMENTS_BOTTOM_LOAD_MARGIN
    }

    fn request_more_comments(&self) {
        let imp = self.imp();
        if imp.comments_loading.get()
            || imp.comments_exhausted.get()
            || imp.comments_pending_offset.get().is_some()
        {
            return;
        }
        if imp
            .comments_last_load_started_at
            .borrow()
            .as_ref()
            .map(|started_at| started_at.elapsed() < COMMENTS_LOAD_COOLDOWN)
            .unwrap_or_default()
        {
            return;
        }
        let Some(sender) = imp.sender.get() else {
            return;
        };
        let offset = imp.comments_next_offset.get();
        imp.comments_pending_offset.set(Some(offset));
        let _ = sender.send_blocking(Action::LoadSongComments {
            song_id: imp.comments_song_id.get(),
            offset,
        });
    }

    fn note_manual_lyrics_scroll(&self) {
        let scrolled = self.imp().scrolled.clone();
        {
            let mut val = scrolled.lock().unwrap();
            *val += 1;
        }
        glib::timeout_add_seconds(3, move || {
            let mut val = scrolled.lock().unwrap();
            *val -= 1;
            glib::ControlFlow::Break
        });
    }

    pub fn update_playlist(&self, sis: &[SongInfo], current_song: SongInfo, likes: &[bool]) {
        let imp = self.imp();
        imp.playlist.replace(Clone::clone(&sis).to_vec());
        let sender = imp.sender.get().unwrap();
        let songs_list = imp.songs_list.get();
        songs_list.set_sender(sender.clone());
        songs_list.replace_list_if_changed(sis, likes);

        let i: i32 = {
            let mut i: i32 = 0;
            match sis.iter().find(|si| {
                i += 1;
                si.id == current_song.id
            }) {
                Some(_) => i - 1,
                _ => -1,
            }
        };
        self.switch_row(i);
    }

    fn switch_current_row(&self, song_id: u64) {
        let i = {
            let playlist = self.imp().playlist.borrow();
            playlist
                .iter()
                .position(|si| si.id == song_id)
                .map(|i| i as i32)
                .unwrap_or(-1)
        };
        self.switch_row(i);
    }

    pub fn begin_lyrics_update(&self, song_id: u64, text: &str) -> bool {
        let imp = self.imp();
        if song_id != 0 && imp.lyrics_song_id.get() == song_id {
            return false;
        }

        imp.lyrics_song_id.set(song_id);
        self.update_lyrics_text(text);
        true
    }

    pub fn lyrics_update_failed(&self, song_id: u64, text: &str) {
        let imp = self.imp();
        if imp.lyrics_song_id.get() == song_id {
            imp.lyrics_song_id.set(0);
            self.update_lyrics_text(text);
        }
    }

    pub fn update_lyrics_text(&self, text: &str) {
        let imp = self.imp();
        let buffer = imp.buffer.get();
        buffer.set_text(text);
        imp.highlighted_range.set(None);
        imp.lyrics_scroll_generation
            .set(imp.lyrics_scroll_generation.get().wrapping_add(1));
        imp.current_lyrics.write().unwrap().clear();
    }

    pub fn update_lyrics(&self, song_id: u64, lyrics: Vec<(u64, String)>) {
        let imp = self.imp();
        if imp.lyrics_song_id.get() != song_id {
            return;
        }

        let buffer = imp.buffer.get();
        buffer.set_text(
            &lyrics
                .iter()
                .map(|(_, x)| x.to_owned())
                .collect::<Vec<_>>()
                .join(""),
        );
        imp.highlighted_range.set(None);
        imp.lyrics_scroll_generation
            .set(imp.lyrics_scroll_generation.get().wrapping_add(1));
        let mut current_lyrics = imp.current_lyrics.write().unwrap();
        *current_lyrics = lyrics;
    }

    pub fn update_lyrics_highlight(&self, time: u64) {
        let playing_indexes = {
            let lyrics = self.imp().current_lyrics.read().unwrap();
            get_playing_indexes(&lyrics, time)
        };
        if playing_indexes.is_none() {
            // 没有行需要高亮
            return;
        }
        let (start, end) = playing_indexes.unwrap();
        let center_mark = self.set_lyrics_highlight(start as i32, end as i32);

        if let Some(mark) = center_mark
            && *(self.imp().scrolled.lock().unwrap()) == 0
        {
            self.queue_lyrics_scroll(mark, start as i32, end as i32);
        }
    }

    fn queue_lyrics_scroll(&self, mark: TextMark, line_start: i32, line_end: i32) {
        let imp = self.imp();
        let generation = imp.lyrics_scroll_generation.get().wrapping_add(1);
        imp.lyrics_scroll_generation.set(generation);

        let obj_weak = self.downgrade();
        glib::timeout_add_local_once(Duration::from_millis(16), move || {
            let Some(obj) = obj_weak.upgrade() else {
                return;
            };
            let imp = obj.imp();
            if imp.lyrics_scroll_generation.get() != generation {
                return;
            }
            if *(imp.scrolled.lock().unwrap()) != 0 {
                return;
            }

            let lyrics_text_view = imp.lyrics_text_view.get();
            let yalign = obj.lyrics_center_yalign(line_start, line_end);
            lyrics_text_view.scroll_to_mark(&mark, 0.0, true, 0.0, yalign);
        });
    }

    fn lyrics_center_yalign(&self, line_start: i32, line_end: i32) -> f64 {
        let imp = self.imp();
        let lyrics_text_view = imp.lyrics_text_view.get();
        let buffer = imp.buffer.get();
        let viewport_height = imp.scroll_lyrics_win.allocated_height().max(1) as f64;
        let Some(start_iter) = buffer.iter_at_line(line_start) else {
            return 0.46;
        };

        let start_rect = lyrics_text_view.iter_location(&start_iter);
        let start_y = start_rect.y();
        let bottom_y = buffer
            .iter_at_line(line_end.saturating_add(1))
            .map(|next_iter| lyrics_text_view.iter_location(&next_iter).y())
            .or_else(|| {
                buffer.iter_at_line(line_end).map(|end_iter| {
                    let end_rect = lyrics_text_view.iter_location(&end_iter);
                    end_rect.y() + end_rect.height()
                })
            })
            .unwrap_or(start_y + start_rect.height());
        let range_height = (bottom_y - start_y).max(start_rect.height()) as f64;

        (0.5 - range_height / (viewport_height * 2.0)).clamp(0.36, 0.5)
    }

    fn set_lyrics_highlight(&self, line_start: i32, line_end: i32) -> Option<TextMark> {
        let highlight_text_tag = self.imp().highlight_text_tag.get();
        let buffer = self.imp().buffer.get();

        let mut mark_to_return = None;
        let previous_range = self
            .imp()
            .highlighted_range
            .replace(Some((line_start, line_end)));
        if previous_range == Some((line_start, line_end)) {
            return None;
        }

        if let Some((previous_start, previous_end)) = previous_range {
            for i in previous_start..=previous_end {
                let Some(start) = buffer.iter_at_line(i) else {
                    continue;
                };
                let mut end = start;
                if !start.ends_line() {
                    end.forward_to_line_end();
                }
                buffer.remove_tag(&highlight_text_tag, &start, &end);
            }
        }

        // gtk doesn't seem to be happy to apply tags to a multi-line TextIter region after an immediate `remove_tag``, so we apply tags line by line
        for i in line_start..=line_end {
            let start = buffer.iter_at_line(i);
            if start.is_none() {
                continue;
            }
            let start = start.unwrap();
            if mark_to_return.is_none() {
                mark_to_return = Some(buffer.create_mark(None, &start, true))
            }
            let mut end = start;
            if !start.ends_line() {
                end.forward_to_line_end();
            }
            buffer.apply_tag(&highlight_text_tag, &start, &end);
        }

        mark_to_return
    }

    pub fn switch_row(&self, index: i32) {
        self.imp().songs_list.mark_new_row_playing(index, false);
    }

    fn clear_comments(&self) {
        let imp = self.imp();
        self.dismiss_comment_context_menu();
        imp.comment_like_buttons.borrow_mut().clear();
        imp.comment_like_labels.borrow_mut().clear();
        imp.comment_liked_states.borrow_mut().clear();
        imp.comment_like_counts.borrow_mut().clear();
        imp.comment_reply_buttons.borrow_mut().clear();
        imp.comment_reply_revealers.borrow_mut().clear();
        imp.comment_reply_lists.borrow_mut().clear();
        imp.comment_reply_loaded.borrow_mut().clear();
        imp.comment_reply_counts.borrow_mut().clear();
        imp.comment_input_entries.borrow_mut().clear();
        imp.comment_input_revealers.borrow_mut().clear();
        imp.comment_send_buttons.borrow_mut().clear();
        imp.comment_row_widgets.borrow_mut().clear();
        imp.comment_reply_parent_ids.borrow_mut().clear();
        imp.comment_reply_ids_by_parent.borrow_mut().clear();

        let list_box = imp.comments_list_box.get();
        while let Some(child) = list_box.first_child() {
            list_box.remove(&child);
        }
    }

    pub fn comment_deleted(&self, song_id: u64, parent_comment_id: u64, comment_id: u64) {
        let imp = self.imp();
        if imp.comments_song_id.get() != song_id {
            return;
        }

        if let Some(row) = imp.comment_row_widgets.borrow_mut().remove(&comment_id)
            && let Some(list_box) = row
                .parent()
                .and_then(|parent| parent.downcast::<ListBox>().ok())
        {
            list_box.remove(&row);
        }

        self.unregister_comment_controls(comment_id);
        if parent_comment_id == 0 {
            self.unregister_reply_rows(comment_id);
            return;
        }

        if let Some(ids) = imp
            .comment_reply_ids_by_parent
            .borrow_mut()
            .get_mut(&parent_comment_id)
        {
            ids.retain(|id| *id != comment_id);
        }
        if let Some(count_state) = imp.comment_reply_counts.borrow().get(&parent_comment_id) {
            count_state.set(count_state.get().saturating_sub(1));
        }
        if let Some(list_box) = imp
            .comment_reply_lists
            .borrow()
            .get(&parent_comment_id)
            .cloned()
            && list_box.first_child().is_none()
        {
            append_empty_reply_row(&list_box);
        }
        self.update_reply_button_label(parent_comment_id);
    }

    fn unregister_comment_controls(&self, comment_id: u64) {
        let imp = self.imp();
        imp.comment_like_buttons.borrow_mut().remove(&comment_id);
        imp.comment_like_labels.borrow_mut().remove(&comment_id);
        imp.comment_liked_states.borrow_mut().remove(&comment_id);
        imp.comment_like_counts.borrow_mut().remove(&comment_id);
        imp.comment_row_widgets.borrow_mut().remove(&comment_id);
        imp.comment_reply_parent_ids
            .borrow_mut()
            .remove(&comment_id);
    }

    fn unregister_reply_rows(&self, parent_comment_id: u64) {
        let Some(reply_ids) = self
            .imp()
            .comment_reply_ids_by_parent
            .borrow_mut()
            .remove(&parent_comment_id)
        else {
            return;
        };
        for reply_id in reply_ids {
            self.unregister_comment_controls(reply_id);
        }
    }

    fn append_comment_header(&self, title: &str) {
        let row = ListBoxRow::new();
        row.set_selectable(false);
        row.set_activatable(false);
        let label = Label::new(Some(title));
        label.set_halign(Align::Start);
        label.add_css_class("lyrics-comment-group-title");
        row.set_child(Some(&label));
        self.imp().comments_list_box.append(&row);
    }

    fn register_visible_comment(&self, comment_id: u64) -> bool {
        comment_id != 0 && self.imp().comments_seen_ids.borrow_mut().insert(comment_id)
    }

    fn append_comment(&self, comment: &SongComment) {
        let row = ListBoxRow::new();
        row.set_selectable(false);
        row.set_activatable(false);

        let row_box = Box::new(Orientation::Vertical, 0);

        let card = Box::new(Orientation::Horizontal, 12);
        card.add_css_class("lyrics-comment-card");

        let avatar = Image::from_icon_name("avatar-default-symbolic");
        avatar.set_pixel_size(40);
        avatar.set_width_request(40);
        avatar.set_height_request(40);
        avatar.set_valign(Align::Start);
        avatar.add_css_class("lyrics-comment-avatar");
        avatar.set_tooltip_text(
            (!comment.avatar_url.is_empty()).then_some(comment.avatar_url.as_str()),
        );
        self.set_comment_avatar(&avatar, comment);
        card.append(&avatar);

        let content_box = Box::new(Orientation::Vertical, 7);
        content_box.set_hexpand(true);

        let name = Label::new(Some(&comment.nickname));
        name.set_halign(Align::Start);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.add_css_class("lyrics-comment-name");
        content_box.append(&name);

        let content = Label::new(Some(&comment.content));
        content.set_halign(Align::Start);
        content.set_wrap(true);
        content.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        content.set_selectable(true);
        content.add_css_class("lyrics-comment-content");
        self.attach_comment_text_context_menu(&content);
        content_box.append(&content);

        let meta = Label::new(Some(&comment.time));
        meta.set_halign(Align::Start);
        meta.add_css_class("lyrics-comment-meta");
        content_box.append(&meta);

        let reply_count = comment.reply_count;
        let replies_button = Button::with_label("");
        replies_button.set_halign(Align::Start);
        replies_button.add_css_class("flat");
        replies_button.add_css_class("lyrics-comment-replies-button");
        replies_button.set_visible(false);
        content_box.append(&replies_button);

        card.append(&content_box);

        let replies_revealer = Revealer::new();
        replies_revealer.set_reveal_child(false);
        replies_revealer.set_transition_duration(180);
        replies_revealer.set_transition_type(RevealerTransitionType::SlideDown);

        let replies_box = Box::new(Orientation::Vertical, 8);
        replies_box.set_margin_start(52);
        replies_box.add_css_class("lyrics-comment-replies-drawer");
        let replies_list = ListBox::new();
        replies_list.set_selection_mode(SelectionMode::None);
        replies_list.add_css_class("lyrics-comment-replies-list");
        replies_box.append(&replies_list);
        replies_revealer.set_child(Some(&replies_box));

        let drawer = Revealer::new();
        drawer.set_reveal_child(false);
        drawer.set_transition_duration(180);
        drawer.set_transition_type(RevealerTransitionType::SlideDown);

        let input_box = Box::new(Orientation::Horizontal, 8);
        input_box.set_margin_start(52);
        input_box.add_css_class("lyrics-comment-input-drawer");

        let entry = Entry::new();
        entry.set_hexpand(true);
        entry.set_placeholder_text(Some(&format!(
            "{} @{}",
            gettext("Reply to"),
            comment.nickname
        )));
        entry.add_css_class("lyrics-comment-entry");
        self.attach_entry_context_menu(&entry);
        input_box.append(&entry);

        let send_button = Button::with_label(&gettext("Send"));
        send_button.add_css_class("suggested-action");
        send_button.add_css_class("lyrics-comment-send-button");
        input_box.append(&send_button);
        drawer.set_child(Some(&input_box));

        let actions_box = Box::new(Orientation::Horizontal, 6);
        actions_box.set_valign(Align::Start);
        actions_box.add_css_class("lyrics-comment-actions");

        let like_group = Box::new(Orientation::Horizontal, 3);
        like_group.set_valign(Align::Center);
        let like_button = comment_icon_button("emblem-favorite-symbolic", &gettext("Like"));
        let like_count = Label::new(Some(&format_short_count(comment.liked_count)));
        like_count.add_css_class("lyrics-comment-like-count");
        if comment.liked {
            like_button.add_css_class("liked");
            like_count.add_css_class("liked");
        }
        like_group.append(&like_button);
        like_group.append(&like_count);
        actions_box.append(&like_group);

        let comment_button = comment_icon_button("document-edit-symbolic", &gettext("Comment"));
        actions_box.append(&comment_button);
        card.append(&actions_box);

        let song_id = self.imp().comments_song_id.get();
        let comment_id = comment.comment_id;
        let can_comment = comment_id != 0 && song_id != 0;
        like_button.set_sensitive(can_comment);
        comment_button.set_sensitive(can_comment);
        replies_button.set_visible(can_comment && reply_count > 0);
        send_button.set_sensitive(can_comment);
        entry.set_sensitive(can_comment);

        let liked = Rc::new(Cell::new(comment.liked));
        let liked_count = Rc::new(Cell::new(comment.liked_count));
        if can_comment {
            let imp = self.imp();
            imp.comment_row_widgets
                .borrow_mut()
                .insert(comment_id, row.clone());
            imp.comment_like_buttons
                .borrow_mut()
                .insert(comment_id, like_button.clone());
            imp.comment_like_labels
                .borrow_mut()
                .insert(comment_id, like_count.clone());
            imp.comment_liked_states
                .borrow_mut()
                .insert(comment_id, liked.clone());
            imp.comment_like_counts
                .borrow_mut()
                .insert(comment_id, liked_count.clone());
            imp.comment_reply_buttons
                .borrow_mut()
                .insert(comment_id, replies_button.clone());
            imp.comment_reply_revealers
                .borrow_mut()
                .insert(comment_id, replies_revealer.clone());
            imp.comment_reply_lists
                .borrow_mut()
                .insert(comment_id, replies_list.clone());
            imp.comment_reply_loaded
                .borrow_mut()
                .insert(comment_id, Rc::new(Cell::new(false)));
            imp.comment_reply_counts
                .borrow_mut()
                .insert(comment_id, Rc::new(Cell::new(reply_count)));
            imp.comment_input_entries
                .borrow_mut()
                .insert(comment_id, entry.clone());
            imp.comment_input_revealers
                .borrow_mut()
                .insert(comment_id, drawer.clone());
            imp.comment_send_buttons
                .borrow_mut()
                .insert(comment_id, send_button.clone());
            self.update_reply_button_label(comment_id);
            self.attach_delete_context_menu(
                &card,
                song_id,
                0,
                comment_id,
                comment.user_id,
                &comment.nickname,
            );
        }

        let liked_state = liked.clone();
        let sender_for_like = self.imp().sender.get().cloned();
        like_button.connect_clicked(move |button| {
            if let Some(sender) = sender_for_like.as_ref() {
                button.set_sensitive(false);
                let sender = sender.clone();
                let liked_now = !liked_state.get();
                let _ = sender.send_blocking(Action::LikeSongComment {
                    song_id,
                    comment_id,
                    like: liked_now,
                });
            }
        });

        let sender_for_replies = self.imp().sender.get().cloned();
        let loaded_for_replies = self
            .imp()
            .comment_reply_loaded
            .borrow()
            .get(&comment_id)
            .cloned();
        let count_for_replies = self
            .imp()
            .comment_reply_counts
            .borrow()
            .get(&comment_id)
            .cloned();
        let replies_revealer_for_button = replies_revealer.clone();
        replies_button.connect_clicked(move |button| {
            if replies_revealer_for_button.reveals_child() {
                replies_revealer_for_button.set_reveal_child(false);
                if let Some(count) = count_for_replies.as_ref() {
                    button.set_label(&format_reply_button_label(count.get(), false));
                }
                return;
            }

            if loaded_for_replies
                .as_ref()
                .map(|loaded| loaded.get())
                .unwrap_or_default()
            {
                replies_revealer_for_button.set_reveal_child(true);
                if let Some(count) = count_for_replies.as_ref() {
                    button.set_label(&format_reply_button_label(count.get(), true));
                }
                return;
            }

            button.set_label(&gettext("Loading replies..."));
            button.set_sensitive(false);
            if let Some(sender) = sender_for_replies.as_ref() {
                let sender = sender.clone();
                let _ = sender.send_blocking(Action::LoadSongCommentReplies {
                    song_id,
                    comment_id,
                });
            }
        });

        let drawer_for_comment = drawer.clone();
        let entry_for_comment = entry.clone();
        comment_button.connect_clicked(move |_| {
            let revealed = !drawer_for_comment.reveals_child();
            drawer_for_comment.set_reveal_child(revealed);
            if revealed {
                entry_for_comment.grab_focus();
            }
        });

        let entry_for_send = entry.clone();
        let send_button_for_send = send_button.clone();
        let sender_for_send = self.imp().sender.get().cloned();
        send_button.connect_clicked(move |_| {
            if let Some(sender) = sender_for_send.as_ref() {
                submit_comment_reply(
                    sender,
                    song_id,
                    comment_id,
                    &entry_for_send,
                    &send_button_for_send,
                );
            }
        });

        let send_button_for_activate = send_button.clone();
        let sender_for_activate = self.imp().sender.get().cloned();
        entry.connect_activate(move |entry| {
            if let Some(sender) = sender_for_activate.as_ref() {
                submit_comment_reply(
                    sender,
                    song_id,
                    comment_id,
                    entry,
                    &send_button_for_activate,
                );
            }
        });

        row_box.append(&card);
        row_box.append(&replies_revealer);
        row_box.append(&drawer);
        row.set_child(Some(&row_box));
        self.imp().comments_list_box.append(&row);
    }

    pub fn comment_action_failed(&self, song_id: u64, comment_id: u64) {
        let imp = self.imp();
        if imp.comments_song_id.get() != song_id {
            return;
        }
        if let Some(button) = imp.comment_like_buttons.borrow().get(&comment_id) {
            button.set_sensitive(true);
        }
        if let Some(entry) = imp.comment_input_entries.borrow().get(&comment_id) {
            entry.set_sensitive(true);
        }
        if let Some(button) = imp.comment_send_buttons.borrow().get(&comment_id) {
            button.set_sensitive(true);
        }
        self.update_reply_button_label(comment_id);
    }

    fn update_reply_button_label(&self, comment_id: u64) {
        let imp = self.imp();
        let button = imp.comment_reply_buttons.borrow().get(&comment_id).cloned();
        let revealer = imp
            .comment_reply_revealers
            .borrow()
            .get(&comment_id)
            .cloned();
        let count = imp
            .comment_reply_counts
            .borrow()
            .get(&comment_id)
            .map(|count| count.get())
            .unwrap_or_default();
        let Some(button) = button else {
            return;
        };
        button.set_sensitive(true);
        button.set_visible(count > 0);
        let revealed = revealer
            .as_ref()
            .map(|revealer| revealer.reveals_child())
            .unwrap_or_default();
        button.set_label(&format_reply_button_label(count, revealed));
    }

    fn append_comment_reply(
        &self,
        list_box: &ListBox,
        parent_comment_id: u64,
        reply: &CommentReply,
    ) {
        let row = ListBoxRow::new();
        row.set_selectable(false);
        row.set_activatable(false);

        let card = Box::new(Orientation::Horizontal, 12);
        card.add_css_class("lyrics-comment-reply-card");

        let avatar = Image::from_icon_name("avatar-default-symbolic");
        avatar.set_pixel_size(34);
        avatar.set_width_request(34);
        avatar.set_height_request(34);
        avatar.set_valign(Align::Start);
        avatar.add_css_class("lyrics-comment-reply-avatar");
        self.set_reply_avatar(&avatar, reply);
        card.append(&avatar);

        let box_ = Box::new(Orientation::Vertical, 5);
        box_.set_hexpand(true);

        let display_name = self.comment_display_name(reply.user_id, &reply.nickname);
        let name = Label::new(Some(&display_name));
        name.set_halign(Align::Start);
        name.set_xalign(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.add_css_class("lyrics-comment-reply-name");
        box_.append(&name);

        let content = Label::new(Some(&reply.content));
        content.set_halign(Align::Start);
        content.set_xalign(0.0);
        content.set_wrap(true);
        content.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        content.set_selectable(true);
        content.add_css_class("lyrics-comment-reply-content");
        self.attach_comment_text_context_menu(&content);
        box_.append(&content);

        if !reply.time.is_empty() {
            let meta = Label::new(Some(&reply.time));
            meta.set_halign(Align::Start);
            meta.set_xalign(0.0);
            meta.add_css_class("lyrics-comment-reply-meta");
            box_.append(&meta);
        }

        card.append(&box_);

        let actions_box = Box::new(Orientation::Horizontal, 6);
        actions_box.set_valign(Align::Start);
        actions_box.add_css_class("lyrics-comment-actions");

        let like_group = Box::new(Orientation::Horizontal, 3);
        like_group.set_valign(Align::Center);
        let like_button = comment_icon_button("emblem-favorite-symbolic", &gettext("Like"));
        let like_count = Label::new(Some(&format_short_count(reply.liked_count)));
        like_count.add_css_class("lyrics-comment-like-count");
        if reply.liked {
            like_button.add_css_class("liked");
            like_count.add_css_class("liked");
        }
        like_group.append(&like_button);
        like_group.append(&like_count);
        actions_box.append(&like_group);
        card.append(&actions_box);

        let song_id = self.imp().comments_song_id.get();
        let comment_id = reply.comment_id;
        let can_like = song_id != 0 && comment_id != 0;
        like_button.set_sensitive(can_like);
        if can_like {
            let liked = Rc::new(Cell::new(reply.liked));
            self.imp()
                .comment_row_widgets
                .borrow_mut()
                .insert(comment_id, row.clone());
            self.imp()
                .comment_reply_parent_ids
                .borrow_mut()
                .insert(comment_id, parent_comment_id);
            self.imp()
                .comment_reply_ids_by_parent
                .borrow_mut()
                .entry(parent_comment_id)
                .or_default()
                .push(comment_id);
            self.imp()
                .comment_like_buttons
                .borrow_mut()
                .insert(comment_id, like_button.clone());
            self.imp()
                .comment_like_labels
                .borrow_mut()
                .insert(comment_id, like_count.clone());
            self.imp()
                .comment_liked_states
                .borrow_mut()
                .insert(comment_id, liked.clone());
            self.imp()
                .comment_like_counts
                .borrow_mut()
                .insert(comment_id, Rc::new(Cell::new(reply.liked_count)));

            let sender_for_like = self.imp().sender.get().cloned();
            like_button.connect_clicked(move |button| {
                if let Some(sender) = sender_for_like.as_ref() {
                    button.set_sensitive(false);
                    let sender = sender.clone();
                    let liked_now = !liked.get();
                    let _ = sender.send_blocking(Action::LikeSongComment {
                        song_id,
                        comment_id,
                        like: liked_now,
                    });
                }
            });
        }

        self.attach_delete_context_menu(
            &card,
            song_id,
            parent_comment_id,
            comment_id,
            reply.user_id,
            &reply.nickname,
        );

        row.set_child(Some(&card));
        list_box.append(&row);
    }

    fn comment_display_name(&self, user_id: u64, nickname: &str) -> String {
        if user_id != 0 && user_id == self.imp().current_user_id.get() {
            if nickname.is_empty() {
                let current_user_nickname = self.imp().current_user_nickname.borrow().clone();
                if current_user_nickname.is_empty() {
                    gettext("Me")
                } else {
                    current_user_nickname
                }
            } else {
                nickname.to_owned()
            }
        } else if nickname.is_empty() && user_id != 0 {
            gettext("User")
        } else {
            nickname.to_owned()
        }
    }

    fn attach_delete_context_menu(
        &self,
        widget: &impl IsA<Widget>,
        song_id: u64,
        parent_comment_id: u64,
        comment_id: u64,
        user_id: u64,
        nickname: &str,
    ) {
        let current_user_id = self.imp().current_user_id.get();
        let current_user_nickname = self.imp().current_user_nickname.borrow().clone();
        let is_current_user = user_id != 0 && user_id == current_user_id
            || (!nickname.is_empty()
                && !current_user_nickname.is_empty()
                && nickname == current_user_nickname);
        if song_id == 0 || comment_id == 0 || user_id == 0 || !is_current_user {
            return;
        }

        let Some(sender) = self.imp().sender.get().cloned() else {
            return;
        };
        let widget = widget.clone().upcast::<Widget>();
        let gesture = GestureClick::new();
        gesture.set_button(3);
        gesture.set_propagation_phase(PropagationPhase::Capture);
        let obj_weak = self.downgrade();
        let widget_for_menu = widget.clone();
        gesture.connect_pressed(move |gesture, _, x, y| {
            if gesture
                .widget()
                .and_then(|widget| widget.pick(x, y, gtk::PickFlags::DEFAULT))
                .as_ref()
                .map(is_comment_text_target)
                .unwrap_or_default()
            {
                return;
            }
            if let Some(obj) = obj_weak.upgrade() {
                obj.show_comment_context_menu(
                    &widget_for_menu,
                    x,
                    y,
                    sender.clone(),
                    CommentDeleteTarget {
                        song_id,
                        parent_comment_id,
                        comment_id,
                    },
                );
            }
            gesture.set_state(EventSequenceState::Claimed);
        });
        widget.add_controller(gesture);
    }

    fn attach_comment_text_context_menu(&self, label: &Label) {
        let label = label.clone();
        let gesture = GestureClick::new();
        gesture.set_button(3);
        gesture.set_propagation_phase(PropagationPhase::Capture);
        let obj_weak = self.downgrade();
        let label_for_menu = label.clone();
        gesture.connect_pressed(move |gesture, _, x, y| {
            if let Some(obj) = obj_weak.upgrade() {
                obj.show_text_context_menu(&label_for_menu, x, y);
            }
            gesture.set_state(EventSequenceState::Claimed);
        });
        label.add_controller(gesture);
    }

    fn attach_lyrics_text_context_menu(&self, text_view: &TextView) {
        let text_view = text_view.clone();
        let gesture = GestureClick::new();
        gesture.set_button(3);
        gesture.set_propagation_phase(PropagationPhase::Capture);
        let obj_weak = self.downgrade();
        let text_view_for_menu = text_view.clone();
        gesture.connect_pressed(move |gesture, _, x, y| {
            if let Some(obj) = obj_weak.upgrade() {
                obj.show_lyrics_text_context_menu(&text_view_for_menu, x, y);
            }
            gesture.set_state(EventSequenceState::Claimed);
        });
        text_view.add_controller(gesture);
    }

    fn attach_entry_context_menu(&self, entry: &Entry) {
        let entry = entry.clone();
        let gesture = GestureClick::new();
        gesture.set_button(3);
        gesture.set_propagation_phase(PropagationPhase::Capture);
        let obj_weak = self.downgrade();
        let entry_for_menu = entry.clone();
        gesture.connect_pressed(move |gesture, _, x, y| {
            if let Some(obj) = obj_weak.upgrade() {
                obj.show_entry_context_menu(&entry_for_menu, x, y);
            }
            gesture.set_state(EventSequenceState::Claimed);
        });
        entry.add_controller(gesture);
    }

    fn show_text_context_menu(&self, label: &Label, x: f64, y: f64) {
        self.dismiss_comment_context_menu();

        let imp = self.imp();
        let overlay = imp.lyrics_overlay.get();
        let menu = Box::new(Orientation::Vertical, 0);
        let copy_button = app_menu::text_row(&gettext("Copy"));
        let select_all_button = app_menu::text_row(&gettext("Select All"));
        menu.append(&copy_button);
        menu.append(&select_all_button);

        let obj_weak = self.downgrade();
        app_menu::show_point_menu(
            app_menu::OverlayMenuState {
                overlay: &overlay,
                layer_state: &imp.active_comment_context_layer,
                card_state: &imp.active_comment_context_card,
            },
            app_menu::PointMenuPlacement {
                anchor: label.upcast_ref(),
                width: 112,
                estimated_height: 80,
                x,
                y,
                extra_card_class: Some("lyrics-comment-context-card"),
            },
            &menu,
            move || {
                if let Some(obj) = obj_weak.upgrade() {
                    obj.dismiss_comment_context_menu();
                }
            },
        );

        let obj_weak = self.downgrade();
        let label_for_copy = label.clone();
        copy_button.connect_clicked(move |_| {
            if label_for_copy
                .selection_bounds()
                .map(|(start, end)| start != end)
                .unwrap_or_default()
            {
                label_for_copy.emit_copy_clipboard();
            } else {
                label_for_copy.clipboard().set_text(&label_for_copy.text());
            }
            if let Some(obj) = obj_weak.upgrade() {
                obj.dismiss_comment_context_menu();
            }
        });

        let label_for_select = label.clone();
        select_all_button.connect_clicked(move |_| {
            label_for_select.select_region(0, -1);
        });
    }

    fn show_lyrics_text_context_menu(&self, text_view: &TextView, x: f64, y: f64) {
        self.dismiss_comment_context_menu();

        let imp = self.imp();
        let overlay = imp.lyrics_overlay.get();
        let menu = Box::new(Orientation::Vertical, 0);
        let copy_button = app_menu::text_row(&gettext("Copy"));
        let select_all_button = app_menu::text_row(&gettext("Select All"));
        menu.append(&copy_button);
        menu.append(&select_all_button);

        let obj_weak = self.downgrade();
        app_menu::show_point_menu(
            app_menu::OverlayMenuState {
                overlay: &overlay,
                layer_state: &imp.active_comment_context_layer,
                card_state: &imp.active_comment_context_card,
            },
            app_menu::PointMenuPlacement {
                anchor: text_view.upcast_ref(),
                width: 112,
                estimated_height: 80,
                x,
                y,
                extra_card_class: Some("lyrics-comment-context-card"),
            },
            &menu,
            move || {
                if let Some(obj) = obj_weak.upgrade() {
                    obj.dismiss_comment_context_menu();
                }
            },
        );

        let obj_weak = self.downgrade();
        let text_view_for_copy = text_view.clone();
        copy_button.connect_clicked(move |_| {
            let buffer = text_view_for_copy.buffer();
            if buffer.selection_bounds().is_some() {
                buffer.copy_clipboard(&text_view_for_copy.clipboard());
            } else {
                let (start, end) = buffer.bounds();
                text_view_for_copy
                    .clipboard()
                    .set_text(&buffer.text(&start, &end, true));
            }
            if let Some(obj) = obj_weak.upgrade() {
                obj.dismiss_comment_context_menu();
            }
        });

        let text_view_for_select = text_view.clone();
        select_all_button.connect_clicked(move |_| {
            let buffer = text_view_for_select.buffer();
            let (start, end) = buffer.bounds();
            buffer.select_range(&start, &end);
        });
    }

    fn show_entry_context_menu(&self, entry: &Entry, x: f64, y: f64) {
        self.dismiss_comment_context_menu();

        let imp = self.imp();
        let overlay = imp.lyrics_overlay.get();
        let menu = Box::new(Orientation::Vertical, 0);
        let cut_button = app_menu::text_row(&gettext("Cut"));
        let copy_button = app_menu::text_row(&gettext("Copy"));
        let paste_button = app_menu::text_row(&gettext("Paste"));
        let select_all_button = app_menu::text_row(&gettext("Select All"));
        let has_selection = entry_selected_text(entry).is_some();
        let has_text = !entry.text().is_empty();
        let is_editable = entry.is_editable();
        cut_button.set_sensitive(has_selection && is_editable);
        copy_button.set_sensitive(has_selection || has_text);
        paste_button.set_sensitive(is_editable);
        select_all_button.set_sensitive(has_text);
        menu.append(&cut_button);
        menu.append(&copy_button);
        menu.append(&paste_button);
        menu.append(&select_all_button);

        let obj_weak = self.downgrade();
        app_menu::show_point_menu(
            app_menu::OverlayMenuState {
                overlay: &overlay,
                layer_state: &imp.active_comment_context_layer,
                card_state: &imp.active_comment_context_card,
            },
            app_menu::PointMenuPlacement {
                anchor: entry.upcast_ref(),
                width: 118,
                estimated_height: 152,
                x,
                y,
                extra_card_class: Some("lyrics-comment-context-card"),
            },
            &menu,
            move || {
                if let Some(obj) = obj_weak.upgrade() {
                    obj.dismiss_comment_context_menu();
                }
            },
        );

        let obj_weak = self.downgrade();
        let entry_for_cut = entry.clone();
        cut_button.connect_clicked(move |_| {
            if let Some(text) = entry_selected_text(&entry_for_cut) {
                entry_for_cut.clipboard().set_text(&text);
                entry_for_cut.delete_selection();
            }
            if let Some(obj) = obj_weak.upgrade() {
                obj.dismiss_comment_context_menu();
            }
        });

        let obj_weak = self.downgrade();
        let entry_for_copy = entry.clone();
        copy_button.connect_clicked(move |_| {
            let text = entry_selected_text(&entry_for_copy)
                .unwrap_or_else(|| entry_for_copy.text().to_string());
            entry_for_copy.clipboard().set_text(&text);
            if let Some(obj) = obj_weak.upgrade() {
                obj.dismiss_comment_context_menu();
            }
        });

        let obj_weak = self.downgrade();
        let entry_for_paste = entry.clone();
        paste_button.connect_clicked(move |_| {
            let clipboard = entry_for_paste.clipboard();
            let entry_for_paste = entry_for_paste.clone();
            clipboard.read_text_async(None::<&gtk::gio::Cancellable>, move |result| {
                if let Ok(Some(text)) = result {
                    entry_for_paste.grab_focus();
                    if entry_for_paste.selection_bounds().is_some() {
                        entry_for_paste.delete_selection();
                    }
                    let mut position = entry_for_paste.position();
                    entry_for_paste.insert_text(text.as_str(), &mut position);
                    entry_for_paste.set_position(position);
                }
            });
            if let Some(obj) = obj_weak.upgrade() {
                obj.dismiss_comment_context_menu();
            }
        });

        let obj_weak = self.downgrade();
        let entry_for_select = entry.clone();
        select_all_button.connect_clicked(move |_| {
            entry_for_select.grab_focus();
            entry_for_select.select_region(0, -1);
            if let Some(obj) = obj_weak.upgrade() {
                obj.dismiss_comment_context_menu();
            }
        });
    }

    fn show_comment_context_menu(
        &self,
        anchor: &Widget,
        x: f64,
        y: f64,
        sender: Sender<Action>,
        target: CommentDeleteTarget,
    ) {
        self.dismiss_comment_context_menu();

        let imp = self.imp();
        let overlay = imp.lyrics_overlay.get();
        let menu = Box::new(Orientation::Vertical, 0);

        let delete_button = Button::with_label(&gettext("Delete"));
        delete_button.add_css_class("flat");
        delete_button.add_css_class("destructive-action");
        delete_button.add_css_class("lyrics-comment-context-delete");
        menu.append(&delete_button);

        let obj_weak = self.downgrade();
        app_menu::show_point_menu(
            app_menu::OverlayMenuState {
                overlay: &overlay,
                layer_state: &imp.active_comment_context_layer,
                card_state: &imp.active_comment_context_card,
            },
            app_menu::PointMenuPlacement {
                anchor,
                width: 96,
                estimated_height: 44,
                x,
                y,
                extra_card_class: Some("lyrics-comment-context-card"),
            },
            &menu,
            move || {
                if let Some(obj) = obj_weak.upgrade() {
                    obj.dismiss_comment_context_menu();
                }
            },
        );

        let obj_weak = self.downgrade();
        delete_button.connect_clicked(move |_| {
            if let Some(obj) = obj_weak.upgrade() {
                obj.dismiss_comment_context_menu();
            }
            let _ = sender.send_blocking(Action::DeleteSongComment {
                song_id: target.song_id,
                parent_comment_id: target.parent_comment_id,
                comment_id: target.comment_id,
            });
        });
    }

    fn set_comment_avatar(&self, avatar: &Image, comment: &SongComment) {
        if comment.user_id == 0 || comment.avatar_url.is_empty() {
            return;
        }

        let mut path_avatar = CACHE.clone();
        path_avatar.push(format!("{}-comment-avatar.jpg", comment.user_id));
        if path_avatar.exists() {
            avatar.set_from_file(Some(&path_avatar));
        } else if let Some(sender) = self.imp().sender.get() {
            avatar.set_from_net(comment.avatar_url.clone(), path_avatar, (80, 80), sender);
        }
    }

    fn set_reply_avatar(&self, avatar: &Image, reply: &CommentReply) {
        if reply.user_id == 0 || reply.avatar_url.is_empty() {
            return;
        }

        let mut path_avatar = CACHE.clone();
        path_avatar.push(format!("{}-comment-reply-avatar.jpg", reply.user_id));
        if path_avatar.exists() {
            avatar.set_from_file(Some(&path_avatar));
        } else if let Some(sender) = self.imp().sender.get() {
            avatar.set_from_net(reply.avatar_url.clone(), path_avatar, (56, 56), sender);
        }
    }
}

impl Default for PlayListLyricsPage {
    fn default() -> Self {
        Self::new()
    }
}

fn set_avatar_image(avatar: &adw::Avatar, path: &Path) {
    if let Ok(pixbuf) = gdk_pixbuf::Pixbuf::from_file(path) {
        let image = Image::from_pixbuf(Some(&pixbuf));
        if let Some(paintable) = image.paintable() {
            avatar.set_custom_image(Some(&paintable));
        }
    }
}

fn format_comment_count(count: u64) -> String {
    format!("({count})")
}

fn format_reply_button_label(count: u64, revealed: bool) -> String {
    match (count, revealed) {
        (0, true) => gettext("Collapse replies"),
        (0, false) => gettext("View replies"),
        (_, true) => format!(
            "{}({})",
            gettext("Collapse replies"),
            format_short_count(count)
        ),
        (_, false) => format!("{}({})", gettext("More replies"), format_short_count(count)),
    }
}

fn format_short_count(count: u64) -> String {
    if count == 0 {
        String::new()
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

fn entry_selected_text(entry: &Entry) -> Option<String> {
    entry
        .selection_bounds()
        .and_then(|(start, end)| (start != end).then_some((start.min(end), start.max(end))))
        .map(|(start, end)| entry.chars(start, end).to_string())
}

fn is_comment_text_target(widget: &Widget) -> bool {
    let mut current = Some(widget.clone());
    while let Some(widget) = current {
        if widget.has_css_class("lyrics-comment-content")
            || widget.has_css_class("lyrics-comment-reply-content")
        {
            return true;
        }
        current = widget.parent();
    }
    false
}

fn comment_icon_button(icon_name: &str, tooltip: &str) -> Button {
    let button = Button::from_icon_name(icon_name);
    button.set_tooltip_text(Some(tooltip));
    button.add_css_class("flat");
    button.add_css_class("lyrics-comment-action-button");
    button
}

fn append_empty_reply_row(list_box: &ListBox) {
    let row = ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    let empty = Label::new(Some(&gettext("No replies")));
    empty.set_halign(Align::Start);
    empty.add_css_class("lyrics-comment-replies-empty");
    row.set_child(Some(&empty));
    list_box.append(&row);
}

fn submit_comment_reply(
    sender: &Sender<Action>,
    song_id: u64,
    comment_id: u64,
    entry: &Entry,
    send_button: &Button,
) {
    let content = entry.text().trim().to_owned();
    if song_id == 0 || comment_id == 0 || content.is_empty() {
        return;
    }
    entry.set_sensitive(false);
    send_button.set_sensitive(false);
    let _ = sender.send_blocking(Action::ReplySongComment {
        song_id,
        comment_id,
        content,
    });
}

mod imp {

    use std::sync::{Arc, Mutex, RwLock};

    use super::*;

    #[derive(Debug, Default, CompositeTemplate)]
    #[template(resource = "/io/github/b1ngggg/CloudMusicPlayer/gtk/playlist-lyrics-page.ui")]
    pub struct PlayListLyricsPage {
        #[template_child]
        pub lyrics_overlay: TemplateChild<Overlay>,
        #[template_child]
        pub lyrics_outer_scroll: TemplateChild<ScrolledWindow>,
        #[template_child]
        pub queue_revealer: TemplateChild<Revealer>,
        #[template_child]
        pub record_disc: TemplateChild<Overlay>,
        #[template_child]
        pub record_tonearm: TemplateChild<Overlay>,
        #[template_child]
        pub record_cover_image: TemplateChild<adw::Avatar>,
        #[template_child]
        pub lyrics_song_title_label: TemplateChild<Label>,
        #[template_child]
        pub lyrics_song_meta_label: TemplateChild<Label>,
        #[template_child]
        pub songs_list: TemplateChild<SongListView>,
        #[template_child]
        pub scroll_lyrics_win: TemplateChild<ScrolledWindow>,
        #[template_child]
        pub lyrics_text_view: TemplateChild<TextView>,
        #[template_child]
        pub buffer: TemplateChild<TextBuffer>,
        #[template_child]
        pub highlight_text_tag: TemplateChild<TextTag>,
        #[template_child]
        pub comments_count_label: TemplateChild<Label>,
        #[template_child]
        pub comments_spinner: TemplateChild<Spinner>,
        #[template_child]
        pub comments_status_label: TemplateChild<Label>,
        #[template_child]
        pub comments_section: TemplateChild<Box>,
        #[template_child]
        pub comments_list_box: TemplateChild<ListBox>,
        pub(crate) scrolled: Arc<Mutex<usize>>,
        pub playlist: Rc<RefCell<Vec<SongInfo>>>,
        pub sender: OnceCell<Sender<Action>>,
        pub current_song_id: Cell<u64>,
        pub lyrics_song_id: Cell<u64>,
        pub comments_song_id: Cell<u64>,
        pub comments_next_offset: Cell<u32>,
        pub comments_loaded: Cell<bool>,
        pub comments_loading: Cell<bool>,
        pub comments_pending_offset: Cell<Option<u32>>,
        pub comments_last_load_started_at: RefCell<Option<Instant>>,
        pub comments_exhausted: Cell<bool>,
        pub comments_hot_header_added: Cell<bool>,
        pub comments_latest_header_added: Cell<bool>,
        pub comments_seen_ids: RefCell<HashSet<u64>>,
        pub current_lyrics: Arc<RwLock<Vec<(u64, String)>>>,
        pub scroll_controller_ready: Cell<bool>,
        pub highlighted_range: Cell<Option<(i32, i32)>>,
        pub playback_active: Cell<bool>,
        pub record_css_provider: RefCell<Option<CssProvider>>,
        pub record_angle: Cell<f64>,
        pub record_animation_generation: Cell<u32>,
        pub record_started_at: RefCell<Option<Instant>>,
        pub lyrics_scroll_generation: Cell<u64>,
        pub comment_like_buttons: RefCell<HashMap<u64, Button>>,
        pub comment_like_labels: RefCell<HashMap<u64, Label>>,
        pub comment_liked_states: RefCell<HashMap<u64, Rc<Cell<bool>>>>,
        pub comment_like_counts: RefCell<HashMap<u64, Rc<Cell<u64>>>>,
        pub comment_reply_buttons: RefCell<HashMap<u64, Button>>,
        pub comment_reply_revealers: RefCell<HashMap<u64, Revealer>>,
        pub comment_reply_lists: RefCell<HashMap<u64, ListBox>>,
        pub comment_reply_loaded: RefCell<HashMap<u64, Rc<Cell<bool>>>>,
        pub comment_reply_counts: RefCell<HashMap<u64, Rc<Cell<u64>>>>,
        pub comment_input_entries: RefCell<HashMap<u64, Entry>>,
        pub comment_input_revealers: RefCell<HashMap<u64, Revealer>>,
        pub comment_send_buttons: RefCell<HashMap<u64, Button>>,
        pub comment_row_widgets: RefCell<HashMap<u64, ListBoxRow>>,
        pub comment_reply_parent_ids: RefCell<HashMap<u64, u64>>,
        pub comment_reply_ids_by_parent: RefCell<HashMap<u64, Vec<u64>>>,
        pub active_comment_context_layer: RefCell<Option<Widget>>,
        pub active_comment_context_card: RefCell<Option<Widget>>,
        pub current_user_id: Cell<u64>,
        pub current_user_nickname: RefCell<String>,
        pub current_user_avatar_url: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PlayListLyricsPage {
        const NAME: &'static str = "PlayListLyricsPage";
        type Type = super::PlayListLyricsPage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[gtk::template_callbacks]
    impl PlayListLyricsPage {
        #[template_callback]
        fn queue_close_cb(&self) {
            self.queue_revealer.set_reveal_child(false);
        }
    }

    impl ObjectImpl for PlayListLyricsPage {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup_record_animation_css();
        }

        fn properties() -> &'static [ParamSpec] {
            static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(std::vec::Vec::new);
            PROPERTIES.as_ref()
        }

        fn set_property(&self, _id: usize, _value: &Value, pspec: &ParamSpec) {
            pspec.name();
            unimplemented!()
        }

        fn property(&self, _id: usize, pspec: &ParamSpec) -> Value {
            pspec.name();
            unimplemented!()
        }
    }
    impl WidgetImpl for PlayListLyricsPage {}
    impl BinImpl for PlayListLyricsPage {}
}

fn get_playing_indexes(lyrics: &[(u64, String)], time: u64) -> Option<(usize, usize)> {
    for i in 0..lyrics.len() {
        let current = lyrics[i].0;
        let next = lyrics.get(i + 1).map(|lyr| lyr.0).unwrap_or(u64::MAX);
        let after_next = lyrics.get(i + 2).map(|lyr| lyr.0).unwrap_or(u64::MAX);

        if (time >= current && time < next)
            || current == next && time >= current && time < after_next
        {
            if current == next {
                return Some((i, i + 1));
            } else {
                return Some((i, i));
            }
        }
    }
    None
}
