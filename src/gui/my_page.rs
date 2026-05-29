//
// my_page.rs
// Copyright (C) 2022 gmg137 <gmg137 AT live.com>
// Copyright (C) 2026 b1ngggg
// Distributed under terms of the GPL-3.0-or-later license.
//

use async_channel::Sender;
use gtk::{CompositeTemplate, glib, prelude::*, subclass::prelude::*};
use ncm_api::SongList;
use once_cell::sync::OnceCell;

use crate::{application::Action, gui::SongListGridItem};

glib::wrapper! {
    pub struct MyPage(ObjectSubclass<imp::MyPage>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable,gtk::ConstraintTarget, gtk::Orientable;
}

impl MyPage {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn set_sender(&self, sender: Sender<Action>) {
        self.imp().sender.set(sender).unwrap();
    }

    pub fn init_page(&self, song_list: Vec<SongList>) {
        let imp = self.imp();
        let rec_grid = imp.rec_grid.get();
        SongListGridItem::box_clear(rec_grid);
        self.setup_rec_grid(song_list);
    }

    fn setup_rec_grid(&self, song_list: Vec<SongList>) {
        let sender = self.imp().sender.get().unwrap().clone();
        let top_picks = self.imp().rec_grid.get();

        SongListGridItem::box_update_songlist(top_picks.clone(), &song_list, 140, false, &sender);

        top_picks.connect_child_activated(move |_, child| {
            let index = child.index() as usize;
            if let Some(sl) = song_list.get(index) {
                sender
                    .send_blocking(Action::ToSongListPage(sl.clone()))
                    .unwrap();
            }
        });
    }
}

impl Default for MyPage {
    fn default() -> Self {
        Self::new()
    }
}

mod imp {

    use super::*;

    #[derive(Debug, Default, CompositeTemplate)]
    #[template(resource = "/io/github/b1ngggg/CloudMusicPlayer/gtk/my-page.ui")]
    pub struct MyPage {
        #[template_child]
        pub rec_grid: TemplateChild<gtk::FlowBox>,

        pub sender: OnceCell<Sender<Action>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MyPage {
        const NAME: &'static str = "MyPage";
        type Type = super::MyPage;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for MyPage {
        fn constructed(&self) {
            self.parent_constructed();
        }
    }
    impl WidgetImpl for MyPage {}
    impl BoxImpl for MyPage {}
}
