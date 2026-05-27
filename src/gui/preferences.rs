//
// preferences.rs
// Copyright (C) 2022 gmg137 <gmg137 AT live.com>
// Distributed under terms of the GPL-3.0-or-later license.
//

use gio::Settings;
use gtk::gio::SettingsBindFlags;
use gtk::{CompositeTemplate, glib, prelude::*, subclass::prelude::*, *};
use once_cell::sync::OnceCell;

use crate::gui::app_menu;

glib::wrapper! {
    pub struct NeteaseCloudMusicLinuxPreferences(ObjectSubclass<imp::NeteaseCloudMusicLinuxPreferences>)
        @extends adw::PreferencesDialog, adw::Dialog, Widget,
        @implements Accessible, Buildable, ConstraintTarget, Native, Root, ShortcutManager;
}

fn entry_selected_text(entry: &Entry) -> Option<String> {
    entry
        .selection_bounds()
        .and_then(|(start, end)| (start != end).then_some((start.min(end), start.max(end))))
        .map(|(start, end)| entry.chars(start, end).to_string())
}

impl NeteaseCloudMusicLinuxPreferences {
    pub fn new() -> Self {
        glib::Object::new()
    }

    fn setup_settings(&self) {
        let settings = Settings::new(crate::APP_ID);
        self.imp()
            .settings
            .set(settings)
            .expect("Could not set `Settings`.");
    }

    fn settings(&self) -> &Settings {
        self.imp().settings.get().expect("Could not get settings.")
    }

    fn bind_settings(&self) {
        let switch = self.imp().exit_switch.get();
        self.settings()
            .bind("exit-switch", &switch, "active")
            .flags(SettingsBindFlags::DEFAULT)
            .build();

        let mute_start_switch = self.imp().mute_start_switch.get();
        self.settings()
            .bind("mute-start", &mute_start_switch, "active")
            .flags(SettingsBindFlags::DEFAULT)
            .build();

        let not_ignore_grey_switch = self.imp().not_ignore_grey_switch.get();
        self.settings()
            .bind("not-ignore-grey", &not_ignore_grey_switch, "active")
            .flags(SettingsBindFlags::DEFAULT)
            .build();

        let entry = self.imp().proxy_entry.get();
        self.settings()
            .bind("proxy-address", &entry, "text")
            .flags(SettingsBindFlags::DEFAULT)
            .build();

        let rate = self.imp().switch_rate.get();
        self.settings()
            .bind("music-rate", &rate, "selected")
            .flags(SettingsBindFlags::DEFAULT)
            .build();

        let cache_clear = self.imp().cache_clear.get();
        self.settings()
            .bind("cache-clear", &cache_clear, "selected")
            .flags(SettingsBindFlags::DEFAULT)
            .build();

        let desktop_lyrics = self.imp().desktop_lyrics.get();
        self.settings()
            .bind("desktop-lyrics", &desktop_lyrics, "active")
            .flags(SettingsBindFlags::DEFAULT)
            .build();
    }

    fn setup_entry_context_menu(&self) {
        let proxy_entry = self.imp().proxy_entry.get();
        self.attach_entry_context_menu(&proxy_entry);
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

    fn show_entry_context_menu(&self, entry: &Entry, x: f64, y: f64) {
        let popover = Popover::new();
        popover.set_has_arrow(false);
        popover.set_autohide(true);
        popover.add_css_class("app-entry-context-popover");
        popover.set_parent(entry);
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
            x.round() as i32,
            y.round() as i32,
            1,
            1,
        )));

        let card = Box::new(Orientation::Vertical, 0);
        card.set_width_request(118);
        card.add_css_class("app-menu-card");

        let cut_button = app_menu::text_row("剪切");
        let copy_button = app_menu::text_row("复制");
        let paste_button = app_menu::text_row("粘贴");
        let select_all_button = app_menu::text_row("全选");
        let has_selection = entry_selected_text(entry).is_some();
        let has_text = !entry.text().is_empty();
        let is_editable = entry.is_editable();
        cut_button.set_sensitive(has_selection && is_editable);
        copy_button.set_sensitive(has_selection || has_text);
        paste_button.set_sensitive(is_editable);
        select_all_button.set_sensitive(has_text);
        card.append(&cut_button);
        card.append(&copy_button);
        card.append(&paste_button);
        card.append(&select_all_button);
        popover.set_child(Some(&card));

        let popover_for_cut = popover.clone();
        let entry_for_cut = entry.clone();
        cut_button.connect_clicked(move |_| {
            if let Some(text) = entry_selected_text(&entry_for_cut) {
                entry_for_cut.clipboard().set_text(&text);
                entry_for_cut.delete_selection();
            }
            popover_for_cut.popdown();
        });

        let popover_for_copy = popover.clone();
        let entry_for_copy = entry.clone();
        copy_button.connect_clicked(move |_| {
            let text = entry_selected_text(&entry_for_copy)
                .unwrap_or_else(|| entry_for_copy.text().to_string());
            entry_for_copy.clipboard().set_text(&text);
            popover_for_copy.popdown();
        });

        let popover_for_paste = popover.clone();
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
            popover_for_paste.popdown();
        });

        let popover_for_select = popover.clone();
        let entry_for_select = entry.clone();
        select_all_button.connect_clicked(move |_| {
            entry_for_select.grab_focus();
            entry_for_select.select_region(0, -1);
            popover_for_select.popdown();
        });

        popover.connect_closed(|popover| {
            popover.unparent();
        });
        popover.popup();
    }

    pub fn set_cache_size_label(&self, size: f64, unit: String) {
        self.imp()
            .cache_clear
            .get()
            .set_property("subtitle", format!("{:.1} {}", size, unit));
    }
}

impl Default for NeteaseCloudMusicLinuxPreferences {
    fn default() -> Self {
        Self::new()
    }
}

mod imp {

    use adw::subclass::prelude::*;

    use super::*;

    #[derive(Debug, Default, CompositeTemplate)]
    #[template(resource = "/io/github/b1ngggg/netease_cloud_music_linux/gtk/preferences.ui")]
    pub struct NeteaseCloudMusicLinuxPreferences {
        pub settings: OnceCell<Settings>,
        #[template_child]
        pub exit_switch: TemplateChild<Switch>,
        #[template_child]
        pub mute_start_switch: TemplateChild<Switch>,
        #[template_child]
        pub not_ignore_grey_switch: TemplateChild<Switch>,
        #[template_child]
        pub proxy_entry: TemplateChild<Entry>,
        #[template_child]
        pub switch_rate: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub cache_clear: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub desktop_lyrics: TemplateChild<Switch>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for NeteaseCloudMusicLinuxPreferences {
        const NAME: &'static str = "NeteaseCloudMusicLinuxPreferences";
        type Type = super::NeteaseCloudMusicLinuxPreferences;
        type ParentType = adw::PreferencesDialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for NeteaseCloudMusicLinuxPreferences {
        fn constructed(&self) {
            let obj = self.obj();
            self.parent_constructed();

            obj.setup_settings();
            obj.bind_settings();
            obj.setup_entry_context_menu();
        }
    }
    impl WidgetImpl for NeteaseCloudMusicLinuxPreferences {}
    impl AdwDialogImpl for NeteaseCloudMusicLinuxPreferences {}
    impl PreferencesDialogImpl for NeteaseCloudMusicLinuxPreferences {}
}
