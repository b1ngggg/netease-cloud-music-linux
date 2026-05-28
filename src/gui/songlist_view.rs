//
// songlist_view.rs
// Copyright (C) 2022 gmg137 <gmg137 AT live.com>
// Copyright (C) 2026 b1ngggg
// Distributed under terms of the GPL-3.0-or-later license.
//
use gtk::gio::{self, Settings};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{CompositeTemplate, glib, *};

use crate::{application::Action, gui::songlist_row::SonglistRow};
use async_channel::Sender;
use glib::{
    ParamSpec, ParamSpecBoolean, ParamSpecInt, RustClosure, SignalHandlerId, Value,
    subclass::Signal,
};
use ncm_api::SongInfo;
use once_cell::sync::{Lazy, OnceCell};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    time::Duration,
};

const SYNC_RENDER_LIMIT: usize = 80;
const RENDER_CHUNK_SIZE: usize = 96;
const RENDER_CHUNK_INTERVAL_MS: u64 = 8;

glib::wrapper! {
    pub struct SongListView(ObjectSubclass<imp::SongListView>)
        @extends Widget, Box,
        @implements Accessible, Actionable, Buildable, ConstraintTarget;
}

impl Default for SongListView {
    fn default() -> Self {
        Self::new()
    }
}

impl SongListView {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    pub fn set_sender(&self, _sender: Sender<Action>) {
        let sender = &self.imp().sender;
        if sender.get().is_none() {
            sender.set(_sender).unwrap();
        }
    }

    fn setup_settings(&self) {
        let settings = Settings::new(crate::APP_ID);

        self.imp()
            .settings
            .set(settings)
            .expect("Could not set `Settings`.");
    }

    fn setup_list_view(&self) {
        let imp = self.imp();
        let model = gio::ListStore::new::<SongListItem>();
        let selection = NoSelection::new(Some(model.clone()));
        imp.list_view.set_model(Some(&selection));
        imp.model.set(model).unwrap();

        let factory = SignalListItemFactory::new();
        let obj_weak = self.downgrade();
        factory.connect_setup(move |_, list_item| {
            let row = SonglistRow::empty();
            let obj_weak = obj_weak.clone();
            row.connect_activate(move |row| {
                let Some(obj) = obj_weak.upgrade() else {
                    return;
                };
                let Some(si) = row.get_song_info() else {
                    return;
                };
                if row.is_activatable() || row.not_ignore_grey() {
                    let index = row.model_index();
                    row.switch_image(true);
                    let obj_for_active = obj.downgrade();
                    glib::idle_add_local_once(move || {
                        if let Some(obj) = obj_for_active.upgrade() {
                            obj.set_active_row_index(index);
                        }
                    });
                    if let Some(sender) = obj.imp().sender.get() {
                        sender.send_blocking(Action::AddPlay(si.clone())).unwrap();
                    }
                    obj.emit_row_activated(row);
                }
            });
            list_item.set_child(Some(&row));
        });

        let obj_weak = self.downgrade();
        factory.connect_bind(move |_, list_item| {
            let Some(obj) = obj_weak.upgrade() else {
                return;
            };
            let Some(item) = list_item
                .item()
                .and_then(|item| item.downcast::<SongListItem>().ok())
            else {
                return;
            };
            let Some(row) = list_item
                .child()
                .and_then(|child| child.downcast::<SonglistRow>().ok())
            else {
                return;
            };

            if let Some(sender) = obj.imp().sender.get() {
                row.set_sender(sender.clone());
            }

            let si = item.song_info();
            row.set_model_index(list_item.position() as i32);
            row.set_from_song_info(&si);
            row.set_property("like", item.like());
            row.set_property("playing", item.playing());
            row.set_property(
                "not-ignore-grey",
                obj.imp().settings.get().unwrap().boolean("not-ignore-grey"),
            );
            row.set_like_button_visible(!obj.property::<bool>("no-act-like"));
            row.set_album_button_visible(!obj.property::<bool>("no-act-album"));
            row.set_remove_button_visible(!obj.property::<bool>("no-act-remove"));
        });

        factory.connect_unbind(move |_, list_item| {
            if let Some(row) = list_item
                .child()
                .and_then(|child| child.downcast::<SonglistRow>().ok())
            {
                row.set_model_index(-1);
                row.switch_image(false);
            }
        });

        imp.list_view.set_factory(Some(&factory));
    }

    pub fn init_new_list(&self, sis: &[SongInfo], likes: &[bool]) {
        let imp = self.imp();
        if sis.is_empty() {
            if imp.pending_rows.borrow().is_empty() {
                self.set_loading(false);
            }
            return;
        }

        imp.row_ids.borrow_mut().extend(sis.iter().map(|si| si.id));
        imp.row_songs.borrow_mut().extend(sis.iter().cloned());
        imp.row_likes
            .borrow_mut()
            .extend((0..sis.len()).map(|index| likes.get(index).copied().unwrap_or(false)));

        if sis.len() <= SYNC_RENDER_LIMIT && imp.pending_rows.borrow().is_empty() {
            for (index, si) in sis.iter().enumerate() {
                let like = likes.get(index).copied().unwrap_or(false);
                self.append_song_item(si.clone(), like);
            }
            self.set_loading(false);
            return;
        }

        self.set_loading(true);
        imp.pending_rows.borrow_mut().extend(
            sis.iter()
                .enumerate()
                .map(|(index, si)| (si.clone(), likes.get(index).copied().unwrap_or_default())),
        );
        self.ensure_render_source();
    }

    pub fn replace_list_if_changed(&self, sis: &[SongInfo], likes: &[bool]) -> bool {
        if self.has_same_song_ids(sis) {
            self.sync_likes(likes);
            return false;
        }

        self.clear_list();
        self.init_new_list(sis, likes);
        true
    }

    pub fn has_same_song_ids(&self, sis: &[SongInfo]) -> bool {
        let row_ids = self.imp().row_ids.borrow();
        row_ids.len() == sis.len() && row_ids.iter().zip(sis.iter()).all(|(id, si)| *id == si.id)
    }

    pub fn sync_likes(&self, likes: &[bool]) {
        let imp = self.imp();
        let mut changed_indices = Vec::new();
        {
            let mut row_likes = imp.row_likes.borrow_mut();
            let len = row_likes.len();
            for index in 0..len {
                let like = likes.get(index).copied().unwrap_or(false);
                if row_likes[index] == like {
                    continue;
                }

                row_likes[index] = like;
                changed_indices.push(index as i32);
            }
        }

        for index in changed_indices {
            if let Some(item) = self.item_at_index(index) {
                item.set_like(likes.get(index as usize).copied().unwrap_or(false));
                self.refresh_item(index);
            }
        }
    }

    pub fn set_loading(&self, loading: bool) {
        let imp = self.imp();
        imp.loading_spinner.set_spinning(loading);
        imp.loading_revealer.set_reveal_child(loading);
    }

    fn ensure_render_source(&self) {
        if self.imp().render_source.borrow().is_some() {
            return;
        }

        let obj_weak = self.downgrade();
        let source_id =
            glib::timeout_add_local(Duration::from_millis(RENDER_CHUNK_INTERVAL_MS), move || {
                let Some(obj) = obj_weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };

                for _ in 0..RENDER_CHUNK_SIZE {
                    let Some((song, like)) = obj.imp().pending_rows.borrow_mut().pop_front() else {
                        obj.imp().render_source.borrow_mut().take();
                        obj.set_loading(false);
                        return glib::ControlFlow::Break;
                    };
                    obj.append_song_item(song, like);
                }

                glib::ControlFlow::Continue
            });
        self.imp().render_source.replace(Some(source_id));
    }

    fn append_song_item(&self, si: SongInfo, like: bool) {
        let imp = self.imp();
        let row_index = imp.loaded_rows.get();
        let like = imp
            .row_likes
            .borrow()
            .get(row_index)
            .copied()
            .unwrap_or(like);
        let playing = row_index as i32 == imp.playing_index.get();

        let item = SongListItem::new(si, like, playing);
        imp.model.get().unwrap().append(&item);
        if playing {
            imp.active_row_index.set(row_index as i32);
        }
        imp.loaded_rows.set(row_index + 1);
    }

    fn cancel_render(&self) {
        let imp = self.imp();
        imp.pending_rows.borrow_mut().clear();
        if let Some(source_id) = imp.render_source.borrow_mut().take() {
            source_id.remove();
        }
    }

    fn sync_not_ignore_grey(&self) {
        self.refresh_loaded_items();
    }

    pub fn get_songinfo_list(&self) -> Vec<SongInfo> {
        self.imp().row_songs.borrow().clone()
    }

    pub fn clear_list(&self) {
        let imp = self.imp();
        self.cancel_render();
        imp.row_ids.borrow_mut().clear();
        imp.row_songs.borrow_mut().clear();
        imp.row_likes.borrow_mut().clear();
        imp.loaded_rows.set(0);
        imp.active_row_index.set(-1);
        imp.playing_index.set(-1);
        self.set_loading(false);

        if let Some(model) = imp.model.get() {
            model.remove_all();
        }
    }

    pub fn mark_new_row_playing(&self, index: i32, do_active: bool) {
        self.set_active_row_index(index);
        if do_active
            && let Some(item) = self.item_at_index(index)
            && let Some(sender) = self.imp().sender.get()
        {
            sender
                .send_blocking(Action::AddPlay(item.song_info()))
                .unwrap();
        }
    }

    fn set_active_row_index(&self, index: i32) {
        if index < 0 {
            return;
        }

        let imp = self.imp();
        let old_index = imp.active_row_index.replace(index);
        imp.playing_index.set(index);

        if old_index != index {
            self.set_item_playing(old_index, false);
        }
        self.set_item_playing(index, true);
    }

    fn item_at_index(&self, index: i32) -> Option<SongListItem> {
        if index < 0 {
            return None;
        }
        self.imp()
            .model
            .get()?
            .item(index as u32)?
            .downcast::<SongListItem>()
            .ok()
    }

    fn set_item_playing(&self, index: i32, playing: bool) {
        let Some(item) = self.item_at_index(index) else {
            return;
        };
        if item.playing() == playing {
            return;
        }
        item.set_playing(playing);
        self.refresh_item(index);
    }

    fn refresh_item(&self, index: i32) {
        let Some(item) = self.item_at_index(index) else {
            return;
        };
        let model = self.imp().model.get().unwrap();
        let replacement = item.clone_item();
        model.splice(index as u32, 1, &[replacement]);
    }

    fn refresh_loaded_items(&self) {
        let imp = self.imp();
        let Some(model) = imp.model.get() else {
            return;
        };
        let loaded_rows = imp.loaded_rows.get();
        if loaded_rows == 0 {
            return;
        }

        let row_songs = imp.row_songs.borrow();
        let row_likes = imp.row_likes.borrow();
        let playing_index = imp.playing_index.get();
        let items = (0..loaded_rows)
            .filter_map(|index| {
                row_songs.get(index).map(|si| {
                    SongListItem::new(
                        si.clone(),
                        row_likes.get(index).copied().unwrap_or(false),
                        index as i32 == playing_index,
                    )
                })
            })
            .collect::<Vec<_>>();

        model.splice(0, model.n_items(), &items);
    }

    pub fn emit_row_activated(&self, row: &SonglistRow) {
        self.emit_by_name::<()>("row-activated", &[&row]);
    }

    pub fn connect_row_activated(&self, f: RustClosure) -> SignalHandlerId {
        self.connect_closure("row-activated", false, f)
    }
}

#[gtk::template_callbacks]
impl SongListView {}

glib::wrapper! {
    pub struct SongListItem(ObjectSubclass<item_imp::SongListItem>);
}

impl SongListItem {
    fn new(song_info: SongInfo, like: bool, playing: bool) -> Self {
        let obj: Self = glib::Object::new();
        let imp = obj.imp();
        imp.song_info.replace(Some(song_info));
        imp.like.set(like);
        imp.playing.set(playing);
        obj
    }

    fn song_info(&self) -> SongInfo {
        self.imp().song_info.borrow().as_ref().unwrap().clone()
    }

    fn like(&self) -> bool {
        self.imp().like.get()
    }

    fn set_like(&self, like: bool) {
        self.imp().like.set(like);
    }

    fn playing(&self) -> bool {
        self.imp().playing.get()
    }

    fn set_playing(&self, playing: bool) {
        self.imp().playing.set(playing);
    }

    fn clone_item(&self) -> Self {
        Self::new(self.song_info(), self.like(), self.playing())
    }
}

mod item_imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct SongListItem {
        pub song_info: RefCell<Option<SongInfo>>,
        pub like: Cell<bool>,
        pub playing: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SongListItem {
        const NAME: &'static str = "SongListItem";
        type Type = super::SongListItem;
    }

    impl ObjectImpl for SongListItem {}
}

mod imp {

    use super::*;

    #[derive(Debug, Default, CompositeTemplate)]
    #[template(resource = "/io/github/b1ngggg/netease_cloud_music_linux/gtk/songlist-view.ui")]
    pub struct SongListView {
        #[template_child]
        pub scroll_win: TemplateChild<ScrolledWindow>,
        #[template_child]
        pub adw_clamp: TemplateChild<adw::Clamp>,
        #[template_child]
        pub list_view: TemplateChild<ListView>,
        #[template_child]
        pub loading_revealer: TemplateChild<Revealer>,
        #[template_child]
        pub loading_spinner: TemplateChild<Spinner>,

        pub sender: OnceCell<Sender<Action>>,
        pub settings: OnceCell<Settings>,
        pub model: OnceCell<gio::ListStore>,
        pub row_ids: RefCell<Vec<u64>>,
        pub row_songs: RefCell<Vec<SongInfo>>,
        pub row_likes: RefCell<Vec<bool>>,
        pub pending_rows: RefCell<VecDeque<(SongInfo, bool)>>,
        pub render_source: RefCell<Option<glib::SourceId>>,
        pub loaded_rows: Cell<usize>,
        pub active_row_index: Cell<i32>,
        pub playing_index: Cell<i32>,

        no_act_like: Cell<bool>,
        no_act_album: Cell<bool>,
        no_act_remove: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SongListView {
        const NAME: &'static str = "SongListView";
        type Type = super::SongListView;
        type ParentType = Box;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            klass.bind_template_callbacks();
            klass.bind_template_instance_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[gtk::template_callbacks]
    impl SongListView {}

    impl ObjectImpl for SongListView {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj().clone();

            obj.setup_settings();
            obj.setup_list_view();
            let settings = self.settings.get().unwrap();
            let obj_for_settings = obj.downgrade();
            settings.connect_changed(Some("not-ignore-grey"), move |_, _| {
                if let Some(obj) = obj_for_settings.upgrade() {
                    obj.sync_not_ignore_grey();
                }
            });
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: Lazy<Vec<Signal>> = Lazy::new(|| {
                vec![
                    Signal::builder("row-activated")
                        .param_types([SonglistRow::static_type()])
                        .build(),
                ]
            });
            SIGNALS.as_ref()
        }

        fn properties() -> &'static [ParamSpec] {
            static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(|| {
                vec![
                    ParamSpecBoolean::builder("no-act-like").build(),
                    ParamSpecBoolean::builder("no-act-album").build(),
                    ParamSpecBoolean::builder("no-act-remove").build(),
                    ParamSpecInt::builder("clamp-margin-top").build(),
                    ParamSpecInt::builder("clamp-margin-bottom").build(),
                    ParamSpecInt::builder("clamp-maximum-size").build(),
                    ParamSpecInt::builder("clamp-tightening-threshold").build(),
                ]
            });
            PROPERTIES.as_ref()
        }

        fn set_property(&self, _id: usize, value: &Value, pspec: &ParamSpec) {
            match pspec.name() {
                "no-act-like" => {
                    let val = value.get().unwrap();
                    if self.no_act_like.replace(val) != val {
                        self.obj().refresh_loaded_items();
                    }
                }
                "no-act-album" => {
                    let val = value.get().unwrap();
                    if self.no_act_album.replace(val) != val {
                        self.obj().refresh_loaded_items();
                    }
                }
                "no-act-remove" => {
                    let val = value.get().unwrap();
                    if self.no_act_remove.replace(val) != val {
                        self.obj().refresh_loaded_items();
                    }
                }
                "clamp-margin-top" => {
                    let val = value.get().unwrap();
                    self.adw_clamp.set_margin_top(val);
                }
                "clamp-margin-bottom" => {
                    let val = value.get().unwrap();
                    self.adw_clamp.set_margin_bottom(val);
                }
                "clamp-maximum-size" => {
                    let val = value.get().unwrap();
                    self.adw_clamp.set_maximum_size(val);
                }
                "clamp-tightening-threshold" => {
                    let val = value.get().unwrap();
                    self.adw_clamp.set_tightening_threshold(val);
                }
                n => unimplemented!("{}", n),
            }
        }

        fn property(&self, _id: usize, pspec: &ParamSpec) -> Value {
            match pspec.name() {
                "no-act-like" => self.no_act_like.get().to_value(),
                "no-act-album" => self.no_act_album.get().to_value(),
                "no-act-remove" => self.no_act_remove.get().to_value(),
                "clamp-margin-top" => self.adw_clamp.margin_top().to_value(),
                "clamp-margin-bottom" => self.adw_clamp.margin_bottom().to_value(),
                "clamp-maximum-size" => self.adw_clamp.maximum_size().to_value(),
                "clamp-tightening-threshold" => self.adw_clamp.tightening_threshold().to_value(),
                n => unimplemented!("{}", n),
            }
        }
    }
    impl WidgetImpl for SongListView {}
    impl BoxImpl for SongListView {}
}
