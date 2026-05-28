use crate::{
    application::{Action, NeteaseCloudMusicLinuxApplication},
    audio::{LoopsState, MprisController},
    gui::*,
    model::*,
    ncmapi::NcmClient,
};
use adw::{ColorScheme, StyleManager, Toast};
use async_channel::Sender;
use gettextrs::gettext;
use gio::{Settings, SimpleAction};
use glib::{
    ParamSpec, ParamSpecEnum, ParamSpecObject, ParamSpecUInt64, Value, clone, source::Priority,
};
use gtk::{
    CompositeTemplate, CssProvider, gdk,
    gio::{self, SettingsBindFlags},
    glib, style_context_add_provider_for_display,
};
use log::*;
use ncm_api::{BannersInfo, LoginInfo, SongInfo, SongList, TopList};
use once_cell::sync::{Lazy, OnceCell};
use std::{
    cell::{Cell, RefCell},
    collections::LinkedList,
    path::PathBuf,
    sync::{Arc, Mutex},
};

fn empty_song_info() -> SongInfo {
    SongInfo {
        id: 0,
        name: String::new(),
        singer: String::new(),
        album: String::new(),
        album_id: 0,
        pic_url: String::new(),
        duration: 0,
        song_url: String::new(),
        copyright: ncm_api::SongCopyright::Unknown,
    }
}

fn editable_selected_text(editable: &Editable) -> Option<String> {
    editable
        .selection_bounds()
        .and_then(|(start, end)| (start != end).then_some((start.min(end), start.max(end))))
        .map(|(start, end)| editable.chars(start, end).to_string())
}

mod imp {

    use super::*;

    #[derive(Default, CompositeTemplate)]
    #[template(resource = "/io/github/b1ngggg/netease_cloud_music_linux/gtk/window.ui")]
    pub struct NeteaseCloudMusicLinuxWindow {
        #[template_child]
        pub header_bar: TemplateChild<adw::HeaderBar>,
        #[template_child]
        pub main_overlay: TemplateChild<Overlay>,
        #[template_child]
        pub gbox: TemplateChild<Box>,
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub fullscreen_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub global_queue_revealer: TemplateChild<Revealer>,
        #[template_child]
        pub global_queue_songs_list: TemplateChild<SongListView>,
        #[template_child]
        pub base_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub stack: TemplateChild<adw::ViewStack>,
        #[template_child]
        pub back_button: TemplateChild<Button>,
        #[template_child]
        pub search_button: TemplateChild<ToggleButton>,
        #[template_child]
        pub search_bar: TemplateChild<SearchBar>,
        #[template_child]
        pub search_entry: TemplateChild<SearchEntry>,
        #[template_child]
        pub search_menu: TemplateChild<MenuButton>,
        #[template_child]
        pub primary_menu_button: TemplateChild<MenuButton>,
        #[template_child]
        pub switcher_title: TemplateChild<adw::ViewSwitcher>,
        #[template_child]
        pub label_title: TemplateChild<Label>,
        #[template_child]
        pub user_button: TemplateChild<MenuButton>,
        #[template_child]
        pub nav_discover_button: TemplateChild<Button>,
        #[template_child]
        pub nav_toplist_button: TemplateChild<Button>,
        #[template_child]
        pub nav_my_button: TemplateChild<Button>,
        #[template_child]
        pub nav_daily_rec_button: TemplateChild<Button>,
        #[template_child]
        pub nav_favorite_songs_button: TemplateChild<Button>,
        #[template_child]
        pub nav_radio_button: TemplateChild<Button>,
        #[template_child]
        pub nav_cloud_music_button: TemplateChild<Button>,
        #[template_child]
        pub nav_favorite_albums_button: TemplateChild<Button>,
        #[template_child]
        pub nav_favorite_songlists_button: TemplateChild<Button>,
        #[template_child]
        pub player_revealer: TemplateChild<Revealer>,
        #[template_child]
        pub player_controls: TemplateChild<PlayerControls>,
        #[template_child]
        pub toplist: TemplateChild<TopListView>,
        #[template_child]
        pub discover: TemplateChild<Discover>,
        #[template_child]
        pub my_stack: TemplateChild<adw::ViewStack>,
        #[template_child]
        pub my_page: TemplateChild<MyPage>,

        pub playlist_lyrics_page: OnceCell<PlayListLyricsPage>,

        pub user_menus: OnceCell<UserMenus>,
        pub settings: OnceCell<Settings>,
        pub sender: OnceCell<Sender<Action>>,
        pub stack_child: Arc<Mutex<LinkedList<(String, String)>>>,
        pub page_stack: OnceCell<PageStack>,
        pub fullscreen_page_stack: OnceCell<PageStack>,

        search_type: Cell<SearchType>,
        pub theme_css_provider: RefCell<Option<CssProvider>>,
        pub active_app_menu_layer: RefCell<Option<Widget>>,
        pub active_app_menu_card: RefCell<Option<Widget>>,
        pub active_text_context_layer: RefCell<Option<Widget>>,
        pub active_text_context_card: RefCell<Option<Widget>>,
        toast: RefCell<Option<Toast>>,
        user_info: RefCell<UserInfo>,
    }

    impl NeteaseCloudMusicLinuxWindow {
        pub fn user_like_song_contains(&self, id: &u64) -> bool {
            self.user_info.borrow().like_songs.contains(id)
        }
        pub fn user_like_song_add(&self, id: u64) {
            self.user_info.borrow_mut().like_songs.insert(id);
        }
        pub fn user_like_song_remove(&self, id: &u64) {
            self.user_info.borrow_mut().like_songs.remove(id);
        }
        pub fn set_user_uid(&self, uid: u64) {
            self.user_info.borrow_mut().uid = uid;
        }
        pub fn set_user_profile(&self, uid: u64, nickname: String, avatar_url: String) {
            let mut user_info = self.user_info.borrow_mut();
            user_info.uid = uid;
            user_info.nickname = nickname;
            user_info.avatar_url = avatar_url;
        }
        pub fn clear_user_info(&self) {
            self.user_info.take();
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for NeteaseCloudMusicLinuxWindow {
        const NAME: &'static str = "NeteaseCloudMusicLinuxWindow";
        type Type = super::NeteaseCloudMusicLinuxWindow;
        type ParentType = ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_instance_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for NeteaseCloudMusicLinuxWindow {
        fn constructed(&self) {
            let obj = self.obj();
            self.parent_constructed();

            self.page_stack
                .set(PageStack::new(self.base_stack.get()))
                .unwrap();
            self.fullscreen_page_stack
                .set(PageStack::new(self.fullscreen_stack.get()))
                .unwrap();

            self.playlist_lyrics_page
                .set(PlayListLyricsPage::new())
                .unwrap();

            if let Ok(mut stack_child) = self.stack_child.lock() {
                stack_child.push_back(("discover".to_owned(), "".to_owned()));
            }

            self.toast.replace(Some(Toast::new("")));

            obj.setup_settings();
            obj.bind_settings();
        }

        fn properties() -> &'static [ParamSpec] {
            static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(|| {
                vec![
                    ParamSpecEnum::builder::<SearchType>("search-type")
                        .explicit_notify()
                        .build(),
                    ParamSpecObject::builder::<Toast>("toast").build(),
                    ParamSpecUInt64::builder("uid").build(),
                ]
            });
            PROPERTIES.as_ref()
        }

        fn set_property(&self, _id: usize, value: &Value, pspec: &ParamSpec) {
            match pspec.name() {
                "toast" => {
                    let toast = value.get().unwrap();
                    self.toast.replace(toast);
                }
                "search-type" => {
                    let input_type = value
                        .get()
                        .expect("The value needs to be of type `SearchType`.");
                    self.search_type.replace(input_type);
                }
                "uid" => {
                    let uid = value.get().unwrap();
                    self.user_info.borrow_mut().uid = uid;
                }
                _ => unimplemented!(),
            }
        }

        fn property(&self, _id: usize, pspec: &ParamSpec) -> Value {
            match pspec.name() {
                "toast" => self.toast.borrow().to_value(),
                "search-type" => self.search_type.get().to_value(),
                "uid" => self.user_info.borrow().uid.to_value(),
                _ => unimplemented!(),
            }
        }
    }
    impl WidgetImpl for NeteaseCloudMusicLinuxWindow {}
    impl WindowImpl for NeteaseCloudMusicLinuxWindow {}
    impl ApplicationWindowImpl for NeteaseCloudMusicLinuxWindow {}
}

glib::wrapper! {
    pub struct NeteaseCloudMusicLinuxWindow(ObjectSubclass<imp::NeteaseCloudMusicLinuxWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl NeteaseCloudMusicLinuxWindow {
    pub fn new<P: glib::object::IsA<gtk::Application>>(
        application: &P,
        sender: Sender<Action>,
    ) -> Self {
        let window: NeteaseCloudMusicLinuxWindow = glib::Object::builder()
            .property("application", application)
            .build();

        window.imp().sender.set(sender).unwrap();
        window.setup_widget();
        window.setup_action();
        window.init_page_data();
        window
    }

    fn setup_settings(&self) {
        let settings = Settings::new(crate::APP_ID);
        self.imp()
            .settings
            .set(settings)
            .expect("Could not set `Settings`.");
    }

    pub fn settings(&self) -> &Settings {
        self.imp().settings.get().expect("Could not get settings.")
    }

    fn setup_action(&self) {
        let imp = self.imp();
        let sender_ = imp.sender.get().unwrap().clone();

        // 绑定设置与主题
        let action_style = self.settings().create_action("style-variant");
        self.add_action(&action_style);

        // 绑定搜索按钮和搜索栏
        let search_button = imp.search_button.get();
        // let search_bar = imp.search_bar.get();
        // search_button
        //     .bind_property("active", &search_bar, "search-mode-enabled")
        //     .flags(BindingFlags::BIDIRECTIONAL)
        //     .build();
        let search_entry = imp.search_entry.get();

        // 设置搜索动作
        let action_search = SimpleAction::new("search-button", None);
        action_search.connect_activate(clone!(
            #[weak]
            search_button,
            move |_, _| {
                search_button.emit_clicked();
            }
        ));
        self.add_action(&action_search);

        let search_bar = imp.search_bar.get();
        search_bar.connect_search_mode_enabled_notify(clone!(
            #[weak]
            search_entry,
            move |bar| {
                if bar.is_search_mode() {
                    // 清空搜索框
                    search_entry.set_text("");
                    // 使搜索框获取输入焦点
                    search_entry.grab_focus();
                }
            }
        ));

        // 设置返回键功能
        let action_back = SimpleAction::new("back-button", None);
        self.add_action(&action_back);

        let sender = sender_;
        action_back.connect_activate(move |_, _| {
            sender.send_blocking(Action::PageBack).unwrap();
        });
    }

    fn bind_settings(&self) {
        let style = StyleManager::default();
        self.settings()
            .bind("style-variant", &style, "color-scheme")
            .mapping(|themes, _| {
                let themes = themes
                    .get::<String>()
                    .expect("The variant needs to be of type `String`.");
                let scheme = match themes.as_str() {
                    "system" => ColorScheme::Default,
                    "light" => ColorScheme::ForceLight,
                    "dark" => ColorScheme::ForceDark,
                    _ => ColorScheme::Default,
                };
                Some(scheme.to_value())
            })
            .build();

        self.setup_app_theme_css();

        self.settings()
            .bind("exit-switch", self, "hide-on-close")
            .flags(SettingsBindFlags::DEFAULT)
            .build();
    }

    fn setup_app_theme_css(&self) {
        let provider = CssProvider::new();
        style_context_add_provider_for_display(
            &gdk::Display::default().expect("Could not connect to a display."),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 3,
        );
        self.imp().theme_css_provider.replace(Some(provider));
        self.update_app_theme_css();

        self.settings().connect_changed(
            Some("style-variant"),
            clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _| {
                    window.update_app_theme_css();
                }
            ),
        );

        StyleManager::default().connect_dark_notify(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window.update_app_theme_css();
            }
        ));
    }

    fn update_app_theme_css(&self) {
        let theme = self.settings().string("style-variant");
        let dark = match theme.as_str() {
            "light" => false,
            "dark" => true,
            _ => StyleManager::default().is_dark(),
        };
        let css = crate::app_theme::css(dark);
        if let Some(provider) = self.imp().theme_css_provider.borrow().as_ref() {
            provider.load_from_data(css);
        }
    }

    fn setup_widget(&self) {
        let imp = self.imp();
        let display = gdk::Display::default().expect("Could not connect to a display.");
        gtk::IconTheme::for_display(&display)
            .add_resource_path("/io/github/b1ngggg/netease_cloud_music_linux/icons");

        let sender = imp.sender.get().unwrap();
        let user_menus = UserMenus::new(sender.clone());

        imp.user_menus.set(user_menus).unwrap();

        self.setup_sidebar_navigation();
        self.setup_builtin_menus();
        self.setup_app_click_dismissal();
        self.update_sidebar_selection("discover");
    }

    fn setup_app_click_dismissal(&self) {
        let gesture = gtk::GestureClick::new();
        gesture.set_button(0);
        gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
        let obj_weak = self.downgrade();
        gesture.connect_pressed(move |gesture, _, x, y| {
            let Some(obj) = obj_weak.upgrade() else {
                return;
            };
            let target = gesture
                .widget()
                .and_then(|widget| widget.pick(x, y, gtk::PickFlags::DEFAULT));
            obj.dismiss_app_menus_if_outside(target.as_ref());
            obj.dismiss_window_text_context_menu_if_outside(target.as_ref());
            obj.dismiss_comment_context_menu(target.as_ref());
        });
        self.add_controller(gesture);

        let gesture = gtk::GestureClick::new();
        gesture.set_button(0);
        gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
        let obj_weak = self.downgrade();
        gesture.connect_pressed(move |gesture, _, x, y| {
            let Some(obj) = obj_weak.upgrade() else {
                return;
            };
            let target = gesture
                .widget()
                .and_then(|widget| widget.pick(x, y, gtk::PickFlags::DEFAULT));
            obj.dismiss_app_menus_if_outside(target.as_ref());
            obj.dismiss_window_text_context_menu_if_outside(target.as_ref());
            obj.dismiss_comment_context_menu(target.as_ref());
        });
        self.imp().header_bar.add_controller(gesture);
    }

    fn setup_builtin_menus(&self) {
        let imp = self.imp();
        let primary_button = imp.primary_menu_button.get();
        self.install_app_menu_button(&primary_button, |obj, button| {
            obj.show_primary_app_menu(button);
        });

        let user_button = imp.user_button.get();
        self.install_app_menu_button(&user_button, |obj, button| {
            obj.show_user_app_menu(button);
        });

        let search_button = imp.search_menu.get();
        self.install_app_menu_button(&search_button, |obj, button| {
            obj.show_search_app_menu(button);
        });

        imp.player_controls.set_repeat_menu_handler(clone!(
            #[weak(rename_to = obj)]
            self,
            move |button| {
                obj.show_repeat_app_menu(button);
            }
        ));

        self.attach_window_editable_context_menu(&imp.search_entry.get());
        let user_menus = imp.user_menus.get().unwrap();
        self.attach_window_editable_context_menu(&user_menus.ctcode_entry);
        self.attach_window_editable_context_menu(&user_menus.phone_entry);
        self.attach_window_editable_context_menu(&user_menus.captcha_entry);
    }

    fn install_app_menu_button<F>(&self, button: &MenuButton, handler: F)
    where
        F: Fn(&Self, &MenuButton) + 'static,
    {
        let gesture = gtk::GestureClick::new();
        gesture.set_button(1);
        gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
        let obj_weak = self.downgrade();
        let button = button.clone();
        let button_for_handler = button.clone();
        gesture.connect_pressed(move |gesture, _, _, _| {
            let Some(obj) = obj_weak.upgrade() else {
                return;
            };
            handler(&obj, &button_for_handler);
            gesture.set_state(gtk::EventSequenceState::Claimed);
        });
        button.add_controller(gesture);
    }

    fn attach_window_editable_context_menu<W>(&self, editable: &W)
    where
        W: IsA<Widget> + IsA<Editable> + Clone + 'static,
    {
        let gesture = gtk::GestureClick::new();
        gesture.set_button(3);
        gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
        let obj_weak = self.downgrade();
        let editable_for_menu = editable.clone().upcast::<Editable>();
        let anchor_for_menu = editable.clone().upcast::<Widget>();
        gesture.connect_pressed(move |gesture, _, x, y| {
            if let Some(obj) = obj_weak.upgrade() {
                obj.show_window_editable_context_menu(&editable_for_menu, &anchor_for_menu, x, y);
            }
            gesture.set_state(gtk::EventSequenceState::Claimed);
        });
        editable.add_controller(gesture);
    }

    fn dismiss_window_text_context_menu_if_outside(&self, target: Option<&Widget>) {
        if target
            .map(|target| self.window_text_context_contains_widget(target))
            .unwrap_or_default()
        {
            return;
        }
        self.dismiss_window_text_context_menu();
    }

    fn window_text_context_contains_widget(&self, widget: &Widget) -> bool {
        app_menu::contains_widget(&self.imp().active_text_context_card, widget)
    }

    fn dismiss_window_text_context_menu(&self) {
        let imp = self.imp();
        app_menu::clear_overlay_menu(
            &imp.main_overlay,
            &imp.active_text_context_layer,
            &imp.active_text_context_card,
        );
    }

    fn show_window_editable_context_menu(
        &self,
        editable: &Editable,
        anchor: &Widget,
        x: f64,
        y: f64,
    ) {
        if self.app_menu_contains_widget(anchor) {
            self.dismiss_window_text_context_menu();
        } else {
            self.dismiss_app_menus();
        }

        let imp = self.imp();
        let overlay = imp.main_overlay.get();
        let menu = Box::new(Orientation::Vertical, 0);
        let cut_button = app_menu::text_row(&gettext("Cut"));
        let copy_button = app_menu::text_row(&gettext("Copy"));
        let paste_button = app_menu::text_row(&gettext("Paste"));
        let select_all_button = app_menu::text_row(&gettext("Select All"));
        let has_selection = editable_selected_text(editable).is_some();
        let has_text = !editable.text().is_empty();
        let is_editable = editable.is_editable();
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
                layer_state: &imp.active_text_context_layer,
                card_state: &imp.active_text_context_card,
            },
            app_menu::PointMenuPlacement {
                anchor,
                width: 118,
                estimated_height: 152,
                x,
                y,
                extra_card_class: None,
            },
            &menu,
            move || {
                if let Some(obj) = obj_weak.upgrade() {
                    obj.dismiss_window_text_context_menu();
                }
            },
        );

        let obj_weak = self.downgrade();
        let editable_for_cut = editable.clone();
        let anchor_for_cut = anchor.clone();
        cut_button.connect_clicked(move |_| {
            if let Some(text) = editable_selected_text(&editable_for_cut) {
                anchor_for_cut.clipboard().set_text(&text);
                editable_for_cut.delete_selection();
            }
            if let Some(obj) = obj_weak.upgrade() {
                obj.dismiss_window_text_context_menu();
            }
        });

        let obj_weak = self.downgrade();
        let editable_for_copy = editable.clone();
        let anchor_for_copy = anchor.clone();
        copy_button.connect_clicked(move |_| {
            let text = editable_selected_text(&editable_for_copy)
                .unwrap_or_else(|| editable_for_copy.text().to_string());
            anchor_for_copy.clipboard().set_text(&text);
            if let Some(obj) = obj_weak.upgrade() {
                obj.dismiss_window_text_context_menu();
            }
        });

        let obj_weak = self.downgrade();
        let editable_for_paste = editable.clone();
        let anchor_for_paste = anchor.clone();
        paste_button.connect_clicked(move |_| {
            let clipboard = anchor_for_paste.clipboard();
            let editable_for_paste = editable_for_paste.clone();
            let anchor_for_focus = anchor_for_paste.clone();
            clipboard.read_text_async(None::<&gio::Cancellable>, move |result| {
                if let Ok(Some(text)) = result {
                    anchor_for_focus.grab_focus();
                    if editable_for_paste.selection_bounds().is_some() {
                        editable_for_paste.delete_selection();
                    }
                    let mut position = editable_for_paste.position();
                    editable_for_paste.insert_text(text.as_str(), &mut position);
                    editable_for_paste.set_position(position);
                }
            });
            if let Some(obj) = obj_weak.upgrade() {
                obj.dismiss_window_text_context_menu();
            }
        });

        let obj_weak = self.downgrade();
        let editable_for_select = editable.clone();
        let anchor_for_select = anchor.clone();
        select_all_button.connect_clicked(move |_| {
            anchor_for_select.grab_focus();
            editable_for_select.select_region(0, -1);
            if let Some(obj) = obj_weak.upgrade() {
                obj.dismiss_window_text_context_menu();
            }
        });
    }

    fn dismiss_app_menus_if_outside(&self, target: Option<&Widget>) {
        if target
            .map(|target| {
                self.app_menu_contains_widget(target)
                    || self.window_text_context_contains_widget(target)
            })
            .unwrap_or_default()
        {
            return;
        }
        self.dismiss_app_menus();
    }

    fn app_menu_contains_widget(&self, widget: &Widget) -> bool {
        app_menu::contains_widget(&self.imp().active_app_menu_card, widget)
    }

    fn dismiss_app_menus(&self) {
        self.dismiss_window_text_context_menu();
        let imp = self.imp();
        app_menu::clear_overlay_menu(
            &imp.main_overlay,
            &imp.active_app_menu_layer,
            &imp.active_app_menu_card,
        );
    }

    fn show_primary_app_menu(&self, anchor: &impl IsA<Widget>) {
        let menu = Box::new(Orientation::Vertical, 8);
        menu.add_css_class("app-menu-content");
        menu.append(&crate::gui::ThemeSelector::new());
        menu.append(&app_menu::separator());
        menu.append(&app_menu::action_row(
            "emblem-system-symbolic",
            &gettext("Preferences"),
            {
                let obj = self.downgrade();
                move || {
                    if let Some(obj) = obj.upgrade() {
                        obj.dismiss_app_menus();
                        let _ =
                            gtk::prelude::WidgetExt::activate_action(&obj, "app.preferences", None);
                    }
                }
            },
        ));
        menu.append(&app_menu::action_row(
            "preferences-desktop-keyboard-symbolic",
            &gettext("Keyboard Shortcuts"),
            {
                let obj = self.downgrade();
                move || {
                    if let Some(obj) = obj.upgrade() {
                        obj.dismiss_app_menus();
                        let _ = gtk::prelude::WidgetExt::activate_action(
                            &obj,
                            "win.show-help-overlay",
                            None,
                        );
                    }
                }
            },
        ));
        menu.append(&app_menu::action_row(
            "help-about-symbolic",
            &gettext("About"),
            {
                let obj = self.downgrade();
                move || {
                    if let Some(obj) = obj.upgrade() {
                        obj.dismiss_app_menus();
                        let _ = gtk::prelude::WidgetExt::activate_action(&obj, "app.about", None);
                    }
                }
            },
        ));

        self.show_app_menu_at_anchor(anchor, &menu, 286, 260, false, 18);
    }

    fn show_user_app_menu(&self, anchor: &impl IsA<Widget>) {
        self.dismiss_app_menus();
        if let Some(sender) = self.imp().sender.get() {
            let _ = sender.send_blocking(Action::TryUpdateQrCode);
        }
        let user_menus = self.imp().user_menus.get().unwrap();
        if user_menus.container.parent().is_some() {
            user_menus.container.unparent();
        }
        user_menus.container.set_width_request(318);
        self.show_app_menu_at_anchor(anchor, &user_menus.container, 318, 360, false, 64);
    }

    fn show_search_app_menu(&self, anchor: &impl IsA<Widget>) {
        let menu = Box::new(Orientation::Vertical, 4);
        menu.add_css_class("app-menu-content");
        let current = self.property::<SearchType>("search-type");
        for (label, search_type) in [
            ("Songs", SearchType::Song),
            ("Artists", SearchType::Singer),
            ("Albums", SearchType::Album),
            ("Lyrics", SearchType::Lyrics),
            ("Playlists", SearchType::SongList),
        ] {
            let translated_label = gettext(label);
            let row = app_menu::choice_row(&translated_label, current == search_type);
            let obj = self.downgrade();
            row.connect_clicked(move |_| {
                if let Some(obj) = obj.upgrade() {
                    obj.imp().search_menu.set_label(&gettext(label));
                    obj.set_property("search-type", search_type);
                    obj.dismiss_app_menus();
                }
            });
            menu.append(&row);
        }

        self.show_app_menu_at_anchor(anchor, &menu, 132, 196, false, 320);
    }

    fn show_repeat_app_menu(&self, anchor: &impl IsA<Widget>) {
        let menu = Box::new(Orientation::Vertical, 4);
        menu.add_css_class("app-menu-content");
        let player_controls = self.imp().player_controls.get();
        let current = player_controls.property::<LoopsState>("loops");
        for (label, icon, state) in [
            (
                "Sequential",
                "media-playlist-consecutive-symbolic",
                LoopsState::None,
            ),
            (
                "Repeat One",
                "media-playlist-repeat-song-symbolic",
                LoopsState::Track,
            ),
            (
                "Repeat All",
                "media-playlist-repeat-symbolic",
                LoopsState::Playlist,
            ),
            (
                "Shuffle",
                "media-playlist-shuffle-symbolic",
                LoopsState::Shuffle,
            ),
        ] {
            let translated_label = gettext(label);
            let row = app_menu::icon_choice_row(icon, &translated_label, current == state);
            let obj = self.downgrade();
            row.connect_clicked(move |_| {
                if let Some(obj) = obj.upgrade() {
                    obj.imp().player_controls.set_loops(state);
                    obj.dismiss_app_menus();
                }
            });
            menu.append(&row);
        }

        self.show_app_menu_at_anchor(anchor, &menu, 172, 180, true, 70);
    }

    fn show_app_menu_at_anchor(
        &self,
        anchor: &impl IsA<Widget>,
        content: &impl IsA<Widget>,
        width: i32,
        estimated_height: i32,
        above: bool,
        fallback_end_margin: i32,
    ) {
        self.dismiss_window_text_context_menu();
        self.dismiss_app_menus();

        let imp = self.imp();
        let overlay = imp.main_overlay.get();
        let obj_weak = self.downgrade();
        app_menu::show_anchor_menu(
            app_menu::OverlayMenuState {
                overlay: &overlay,
                layer_state: &imp.active_app_menu_layer,
                card_state: &imp.active_app_menu_card,
            },
            app_menu::AnchorMenuPlacement {
                anchor: anchor.upcast_ref(),
                width,
                estimated_height,
                above,
                fallback_end_margin,
            },
            content,
            move || {
                if let Some(obj) = obj_weak.upgrade() {
                    obj.dismiss_app_menus();
                }
            },
        );
    }

    fn setup_sidebar_navigation(&self) {
        let imp = self.imp();
        let nav_discover_button = imp.nav_discover_button.get();
        let nav_toplist_button = imp.nav_toplist_button.get();
        let nav_my_button = imp.nav_my_button.get();
        let nav_daily_rec_button = imp.nav_daily_rec_button.get();
        let nav_favorite_songs_button = imp.nav_favorite_songs_button.get();
        let nav_radio_button = imp.nav_radio_button.get();
        let nav_cloud_music_button = imp.nav_cloud_music_button.get();
        let nav_favorite_albums_button = imp.nav_favorite_albums_button.get();
        let nav_favorite_songlists_button = imp.nav_favorite_songlists_button.get();

        let weak_window = self.downgrade();
        nav_discover_button.connect_clicked(move |_| {
            if let Some(window) = weak_window.upgrade() {
                window.switch_root_page("discover");
            }
        });

        let weak_window = self.downgrade();
        nav_toplist_button.connect_clicked(move |_| {
            if let Some(window) = weak_window.upgrade() {
                window.switch_root_page("toplist");
            }
        });

        let weak_window = self.downgrade();
        nav_my_button.connect_clicked(move |_| {
            if let Some(window) = weak_window.upgrade() {
                window.switch_root_page("my");
            }
        });

        let sender = imp.sender.get().unwrap().clone();
        nav_daily_rec_button.connect_clicked(move |_| {
            sender.send_blocking(Action::ToMyPageDailyRec).unwrap();
        });

        let sender = imp.sender.get().unwrap().clone();
        nav_favorite_songs_button.connect_clicked(move |_| {
            sender.send_blocking(Action::ToMyPageHeartbeat).unwrap();
        });

        let sender = imp.sender.get().unwrap().clone();
        nav_radio_button.connect_clicked(move |_| {
            sender.send_blocking(Action::ToMyPageRadio).unwrap();
        });

        let sender = imp.sender.get().unwrap().clone();
        nav_cloud_music_button.connect_clicked(move |_| {
            sender.send_blocking(Action::ToMyPageCloudDisk).unwrap();
        });

        let sender = imp.sender.get().unwrap().clone();
        nav_favorite_albums_button.connect_clicked(move |_| {
            sender.send_blocking(Action::ToMyPageAlbums).unwrap();
        });

        let sender = imp.sender.get().unwrap().clone();
        nav_favorite_songlists_button.connect_clicked(move |_| {
            sender.send_blocking(Action::ToMyPageSonglist).unwrap();
        });
    }

    fn switch_root_page(&self, name: &str) {
        let imp = self.imp();
        let page_stack = imp.page_stack.get().unwrap();

        self.set_global_queue_revealed(false);

        while page_stack.len() > 1 {
            page_stack.back_page();
        }

        imp.stack.set_visible_child_name(name);
        self.page_widget_switch(false);
        self.update_sidebar_selection(name);
    }

    fn update_sidebar_selection(&self, name: &str) {
        let known_sidebar_target = matches!(
            name,
            "discover"
                | "toplist"
                | "my"
                | "ToMyPageDailyRec"
                | "ToMyPageHeartbeat"
                | "ToMyPageRadio"
                | "ToMyPageCloudDisk"
                | "ToMyPageAlbums"
                | "ToMyPageSonglist"
        );
        if !known_sidebar_target {
            return;
        }

        let imp = self.imp();
        let set_active = |button: Button, active: bool| {
            if active {
                button.add_css_class("active");
            } else {
                button.remove_css_class("active");
            }
        };

        set_active(imp.nav_discover_button.get(), name == "discover");
        set_active(imp.nav_toplist_button.get(), name == "toplist");
        set_active(imp.nav_my_button.get(), name == "my");
        set_active(imp.nav_daily_rec_button.get(), name == "ToMyPageDailyRec");
        set_active(
            imp.nav_favorite_songs_button.get(),
            name == "ToMyPageHeartbeat",
        );
        set_active(imp.nav_radio_button.get(), name == "ToMyPageRadio");
        set_active(
            imp.nav_cloud_music_button.get(),
            name == "ToMyPageCloudDisk",
        );
        set_active(
            imp.nav_favorite_albums_button.get(),
            name == "ToMyPageAlbums",
        );
        set_active(
            imp.nav_favorite_songlists_button.get(),
            name == "ToMyPageSonglist",
        );
    }

    pub fn get_uid(&self) -> u64 {
        self.property::<u64>("uid")
    }

    pub fn set_uid(&self, val: u64) {
        self.set_property("uid", val);
        self.imp().set_user_uid(val);
    }

    pub fn is_logined(&self) -> bool {
        self.get_uid() != 0u64
    }

    pub fn logout(&self) {
        self.imp().clear_user_info();
        if let Some(page) = self.imp().playlist_lyrics_page.get() {
            page.set_comment_user_info(0, String::new(), String::new());
        }
    }

    pub fn get_song_likes(&self, sis: &[SongInfo]) -> Vec<bool> {
        sis.iter()
            .map(|si| self.imp().user_like_song_contains(&si.id))
            .collect()
    }

    pub fn set_like_song(&self, id: u64, val: bool) {
        let imp = self.imp();
        if let Some(song) = imp.player_controls.get().get_current_song()
            && song.id == id
        {
            imp.player_controls.get().set_property("like", val);
        }

        if val {
            imp.user_like_song_add(id);
        } else {
            imp.user_like_song_remove(&id);
        }
    }

    pub fn set_user_like_songs(&self, song_ids: &[u64]) {
        song_ids
            .iter()
            .for_each(|id| self.imp().user_like_song_add(id.to_owned()));
    }

    pub fn set_user_qrimage(&self, path: PathBuf) {
        let user_menus = self.imp().user_menus.get().unwrap();
        user_menus.set_qrimage(path);
    }

    pub fn set_user_qrimage_timeout(&self) {
        let user_menus = self.imp().user_menus.get().unwrap();
        user_menus.set_qrimage_timeout();
    }

    pub fn is_user_menu_active(&self, menu: UserMenuChild) -> bool {
        self.imp().user_menus.get().unwrap().is_menu_active(menu)
    }

    pub fn switch_user_menu_to_phone(&self) {
        let user_menus = self.imp().user_menus.get().unwrap();
        user_menus.switch_menu(UserMenuChild::Phone);
    }

    pub fn switch_user_menu_to_qr(&self) {
        let user_menus = self.imp().user_menus.get().unwrap();
        user_menus.switch_menu(UserMenuChild::Qr);
    }

    pub fn switch_user_menu_to_user(&self, login_info: LoginInfo, _menu: UserMenuChild) {
        let user_menus = self.imp().user_menus.get().unwrap();
        user_menus.switch_menu(UserMenuChild::User);
        let uid = login_info.uid;
        let nickname = login_info.nickname;
        let avatar_url = login_info.avatar_url;
        if login_info.vip_type == 0 {
            user_menus.set_user_name(nickname.clone());
        } else {
            user_menus.set_user_name(format!("👑{}", nickname));
        }

        self.imp()
            .set_user_profile(uid, nickname.clone(), avatar_url.clone());
        if let Some(page) = self.imp().playlist_lyrics_page.get() {
            page.set_comment_user_info(uid, nickname, avatar_url);
        }
    }

    pub fn set_avatar(&self, url: String, path: PathBuf) {
        self.imp()
            .user_menus
            .get()
            .unwrap()
            .set_user_avatar(url, path);
    }

    pub fn add_toast(&self, mes: String) {
        let pre = self.property::<Toast>("toast");

        let toast = Toast::builder()
            .title(glib::markup_escape_text(&mes))
            .priority(adw::ToastPriority::High)
            .build();
        self.set_property("toast", &toast);
        self.imp().toast_overlay.add_toast(toast);

        // seems that dismiss will clear something used by animation
        // cause adw_animation_skip emit 'done' segfault on closure(https://github.com/gmg137/netease-cloud-music-gtk/issues/202)
        // delay to wait for animation skipped/done
        crate::MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
            glib::timeout_future(std::time::Duration::from_millis(500)).await;
            // removed from overlay toast queue by signal
            pre.dismiss();
        });
    }

    pub fn add_carousel(&self, banner: BannersInfo) {
        let discover = self.imp().discover.get();
        discover.add_carousel(banner);
    }

    pub fn setup_top_picks(&self, song_list: Vec<SongList>) {
        let discover = self.imp().discover.get();
        discover.setup_top_picks(song_list);
    }

    pub fn setup_new_albums(&self, song_list: Vec<SongList>) {
        let discover = self.imp().discover.get();
        discover.setup_new_albums(song_list);
    }

    pub fn add_play(&self, song_info: SongInfo) {
        let player_controls = self.imp().player_controls.get();
        player_controls.add_song(song_info);
    }

    pub fn remove_from_playlist(&self, song_info: SongInfo) {
        let player_controls = self.imp().player_controls.get();
        let lyrics_page_open = self.page_cur_playlist_lyrics_page();
        let global_queue_open = self.imp().global_queue_revealer.reveals_child();
        player_controls.remove_song(song_info);

        let refresh_result = player_controls.with_playlist(|playlist| {
            playlist.current_song().cloned().map(|si| {
                let should_update_lyrics = if lyrics_page_open {
                    self.refresh_playlist_lyrics_page(playlist.songs(), si.to_owned())
                } else {
                    false
                };
                if global_queue_open {
                    self.refresh_global_queue_drawer(playlist.songs(), &si);
                }
                (si, should_update_lyrics)
            })
        });

        if let Some(Some((si, should_update_lyrics))) = refresh_result {
            if should_update_lyrics {
                let sender = self.imp().sender.get().unwrap();
                sender
                    .send_blocking(Action::UpdateLyrics(si.to_owned(), 0))
                    .unwrap();
            }
        } else {
            self.set_global_queue_revealed(false);
            let player_revealer = self.imp().player_revealer.get();
            player_revealer.set_reveal_child(false);
            if lyrics_page_open {
                let sender = self.imp().sender.get().unwrap();
                sender.send_blocking(Action::PageBack).unwrap();
            }
        }
    }

    pub fn add_playlist(&self, sis: Vec<SongInfo>, is_play: bool) {
        let player_controls = self.imp().player_controls.get();
        player_controls.add_list(sis);
        let sender = self.imp().sender.get().unwrap();
        if is_play {
            sender.send_blocking(Action::PlayListStart).unwrap();
        }
    }

    pub fn playlist_start(&self) {
        let sender = self.imp().sender.get().unwrap();
        let player_controls = self.imp().player_controls.get();
        if let Some(song_info) = player_controls.get_current_song() {
            sender.send_blocking(Action::Play(song_info)).unwrap();
            return;
        }
        sender
            .send_blocking(Action::AddToast(gettext("No playable songs found！")))
            .unwrap();
    }

    pub fn play_next(&self) {
        let player_controls = self.imp().player_controls.get();
        player_controls.next_song();
    }

    pub fn play(&self, song_info: SongInfo) -> bool {
        let player_controls = self.imp().player_controls.get();
        player_controls.set_property("like", self.imp().user_like_song_contains(&song_info.id));
        player_controls.play(song_info.clone());

        let should_update_lyrics = if self.page_cur_playlist_lyrics_page() {
            player_controls
                .with_playlist(|playlist| {
                    self.refresh_playlist_lyrics_page(playlist.songs(), song_info.clone())
                })
                .unwrap_or(false)
        } else if let Some(song_info) = player_controls.get_current_song() {
            let page = self.imp().playlist_lyrics_page.get().unwrap();
            page.update_now_playing(&song_info);
            false
        } else {
            false
        };

        self.imp()
            .playlist_lyrics_page
            .get()
            .unwrap()
            .set_playback_active(true);
        let player_revealer = self.imp().player_revealer.get();
        if !player_revealer.reveals_child() {
            player_revealer.set_visible(true);
            player_revealer.set_reveal_child(true);
        }
        should_update_lyrics
    }

    pub fn init_page_data(&self) {
        let imp = self.imp();
        let sender = imp.sender.get().unwrap();

        // 初始化我的页面
        let my_page = imp.my_page.get();
        my_page.set_sender(sender.clone());

        // 初始化播放栏
        let player_controls = imp.player_controls.get();
        player_controls.set_sender(sender.clone());

        // 初始化发现页
        let discover = imp.discover.get();
        discover.set_sender(sender.clone());
        discover.init_page();

        // 初始化榜单
        sender.send_blocking(Action::GetToplist).unwrap();
        let toplist = imp.toplist.get();
        toplist.set_sender(sender.clone());

        // 初始化播放列表页
        let playlist_lyrics_page = imp.playlist_lyrics_page.get().unwrap();
        playlist_lyrics_page.set_sender(sender.clone());
        let global_queue_songs_list = imp.global_queue_songs_list.get();
        global_queue_songs_list.set_sender(sender.clone());

        let page_stack = imp.page_stack.get().unwrap();
        page_stack.set_transition_type(StackTransitionType::Crossfade);
        page_stack.set_transition_duration(100); // default 200
        let fullscreen_page_stack = imp.fullscreen_page_stack.get().unwrap();
        fullscreen_page_stack.set_transition_type(StackTransitionType::OverUp);
        fullscreen_page_stack.set_transition_duration(260);
    }

    pub fn init_toplist(&self, list: Vec<TopList>) {
        let toplist = self.imp().toplist.get();
        toplist.init_sidebar(list);
    }

    pub fn update_toplist(&self, list: Vec<SongInfo>) {
        let toplist = self.imp().toplist.get();
        toplist.update_songs_list(
            &list,
            &list
                .iter()
                .map(|si| self.imp().user_like_song_contains(&si.id))
                .collect::<Vec<bool>>(),
        );
    }

    // page routing
    fn page_widget_switch(&self, need_back: bool) {
        let imp = self.imp();
        let switcher_title = imp.switcher_title.get();
        let label_title = imp.label_title.get();
        let back_button = imp.back_button.get();

        let visible = need_back;
        let player_view_open = visible && self.page_cur_playlist_lyrics_page();
        back_button.set_visible(visible);
        if player_view_open {
            back_button.set_icon_name("pan-down-symbolic");
            back_button.set_tooltip_text(Some(&gettext("Collapse player view")));
        } else {
            back_button.set_icon_name("go-previous-symbolic");
            back_button.set_tooltip_text(Some(&gettext("Back")));
        }
        imp.player_controls
            .get()
            .set_queue_view_open(player_view_open);
        label_title.set_visible(visible);
        switcher_title.set_visible(false);
    }
    pub fn page_set_info(&self, title: &str) {
        let imp = self.imp();
        let label_title = imp.label_title.get();

        label_title.set_label(title);
    }
    // same name will clear old page
    pub fn page_new_with_name(
        &self,
        name: &str,
        page: &impl glib::object::IsA<Widget>,
        title: &str,
    ) {
        let imp = self.imp();
        let stack = imp.page_stack.get().unwrap();
        stack.set_transition_type(StackTransitionType::Crossfade);
        stack.set_transition_duration(100);
        let stack_page = stack.new_page_with_name(page, name);
        stack_page.set_title(title);
        self.page_set_info(title);
        self.page_widget_switch(true);
        self.update_sidebar_selection(name);
    }
    pub fn page_new(&self, page: &impl glib::object::IsA<Widget>, title: &str, name: &str) {
        let imp = self.imp();
        let stack = imp.page_stack.get().unwrap();
        if stack.len() > 1 {
            let top_page = stack.top_page();
            if top_page.title().unwrap() == title {
                if let Some(n) = top_page.name() {
                    if n == name {
                        return;
                    }
                } else {
                    return;
                }
            }
        }
        if name == "Lyrics & Queue" {
            stack.set_transition_type(StackTransitionType::OverUp);
            stack.set_transition_duration(260);
        } else {
            stack.set_transition_type(StackTransitionType::Crossfade);
            stack.set_transition_duration(100);
        }
        let stack_page = stack.new_page(page);
        stack_page.set_title(title);
        stack_page.set_name(name);
        self.page_set_info(title);
        self.page_widget_switch(true);
        self.update_sidebar_selection(name);
    }
    pub fn fullscreen_page_new(
        &self,
        page: &impl glib::object::IsA<Widget>,
        title: &str,
        name: &str,
    ) {
        let imp = self.imp();
        let stack = imp.fullscreen_page_stack.get().unwrap();
        if stack.len() > 1 {
            let top_page = stack.top_page();
            if top_page.title().unwrap() == title {
                if let Some(n) = top_page.name() {
                    if n == name {
                        self.page_set_info(title);
                        self.page_widget_switch(true);
                        return;
                    }
                } else {
                    self.page_set_info(title);
                    self.page_widget_switch(true);
                    return;
                }
            }
        }
        stack.set_transition_type(StackTransitionType::OverUp);
        stack.set_transition_duration(260);
        let stack_page = stack.new_page(page);
        stack_page.set_title(title);
        stack_page.set_name(name);
        self.page_set_info(title);
        self.page_widget_switch(true);
    }
    pub fn page_back(&self) -> Option<Widget> {
        let imp = self.imp();
        let stack = imp.page_stack.get().unwrap();

        if self.page_cur_playlist_lyrics_page() {
            let fullscreen_stack = imp.fullscreen_page_stack.get().unwrap();
            fullscreen_stack.set_transition_type(StackTransitionType::UnderDown);
            fullscreen_stack.set_transition_duration(260);
            fullscreen_stack.back_page();

            if stack.len() > 1 {
                let top_page = stack.top_page();
                self.page_set_info(top_page.title().unwrap().to_string().as_str());
                if let Some(name) = top_page.name() {
                    self.update_sidebar_selection(name.as_str());
                }
                self.page_widget_switch(true);
            } else {
                if let Some(name) = imp.stack.visible_child_name() {
                    self.update_sidebar_selection(name.as_str());
                }
                self.page_widget_switch(false);
            }
            return None;
        }

        stack.set_transition_type(StackTransitionType::Crossfade);
        stack.set_transition_duration(100);
        stack.back_page();

        if stack.len() > 1 {
            let top_page = stack.top_page();
            self.page_set_info(top_page.title().unwrap().to_string().as_str());
            if let Some(name) = top_page.name() {
                self.update_sidebar_selection(name.as_str());
            }
            self.page_widget_switch(true);
        } else {
            if let Some(name) = imp.stack.visible_child_name() {
                self.update_sidebar_selection(name.as_str());
            }
            self.page_widget_switch(false);
        }
        None
    }
    pub fn persist_volume(&self, value: f64) {
        let imp = self.imp();
        imp.player_controls.persist_volume(value);
    }
    pub fn page_cur_playlist_lyrics_page(&self) -> bool {
        let imp = self.imp();
        let page = imp.playlist_lyrics_page.get().unwrap();
        let fullscreen_stack = imp.fullscreen_page_stack.get().unwrap();
        if fullscreen_stack.len() <= 1 {
            return false;
        }
        let cur = &fullscreen_stack.top_page().child();
        cur == page
    }

    pub fn init_picks_songlist(&self) -> SearchSongListPage {
        let imp = self.imp();
        let sender = imp.sender.get().unwrap().clone();
        let page = SearchSongListPage::new();
        page.set_sender(sender);
        page.init_page(&gettext("All Playlists"), SearchType::TopPicks);
        page
    }

    pub fn init_all_albums(&self) -> SearchSongListPage {
        let imp = self.imp();
        let sender = imp.sender.get().unwrap().clone();
        let page = SearchSongListPage::new();
        page.set_sender(sender);
        page.init_page(&gettext("All Albums"), SearchType::AllAlbums);
        page
    }

    pub fn init_search_song_page(&self, text: &str, search_type: SearchType) -> SearchSongPage {
        let imp = self.imp();
        let sender = imp.sender.get().unwrap().clone();
        let page = SearchSongPage::new();
        page.set_sender(sender);
        page.init_page(text, search_type);
        page
    }

    pub fn init_search_songlist_page(
        &self,
        text: &str,
        search_type: SearchType,
    ) -> SearchSongListPage {
        let imp = self.imp();
        let sender = imp.sender.get().unwrap().clone();
        let page = SearchSongListPage::new();
        page.set_sender(sender);
        page.init_page(text, search_type);
        page
    }
    pub fn init_search_singer_page(&self, text: &str) -> SearchSingerPage {
        let imp = self.imp();
        let sender = imp.sender.get().unwrap().clone();
        let page = SearchSingerPage::new();
        page.set_sender(sender);
        page.init_page(text.to_string());
        page
    }

    pub fn init_songlist_page(&self, songlist: &SongList, is_album: bool) -> SonglistPage {
        let sender = self.imp().sender.get().unwrap().clone();
        let page = SonglistPage::new();
        page.set_sender(sender);
        page.init_songlist_info(songlist, is_album, self.is_logined());
        page
    }

    pub fn update_search_song_page(&self, page: SearchSongPage, sis: Vec<SongInfo>) {
        page.update_songs(&sis, &self.get_song_likes(&sis));
    }

    pub fn update_songlist_page(&self, page: SonglistPage, detail: &SongListDetail) {
        page.init_songlist(detail, &self.get_song_likes(detail.sis()));
    }

    pub fn switch_my_page_to_login(&self) {
        let imp = self.imp();
        imp.my_stack.set_visible_child_name("my_login");
    }

    pub fn switch_my_page_to_logout(&self) {
        let imp = self.imp();
        imp.my_stack.set_visible_child_name("my_no_login");
    }

    pub fn init_my_page(&self, sls: Vec<SongList>) {
        self.imp().my_page.init_page(sls);
    }

    pub fn init_playlist_lyrics_page(&self) -> Option<(SongInfo, bool)> {
        let imp = self.imp();
        let page = imp.playlist_lyrics_page.get().unwrap();
        let (si, should_update_lyrics) = imp.player_controls.get().with_playlist(|playlist| {
            let si = playlist
                .current_song()
                .cloned()
                .unwrap_or_else(empty_song_info);
            let should_update_lyrics =
                self.refresh_playlist_lyrics_page(playlist.songs(), si.clone());
            (si, should_update_lyrics)
        })?;

        self.set_global_queue_revealed(false);
        self.fullscreen_page_new(page, &gettext("Now Playing"), "Lyrics & Queue");
        Some((si, should_update_lyrics))
    }

    fn refresh_playlist_lyrics_page(&self, sis: &[SongInfo], si: SongInfo) -> bool {
        let page = self.imp().playlist_lyrics_page.get().unwrap();
        let playlist_changed = page.playlist_changed(sis);
        let likes = if playlist_changed {
            self.get_song_likes(sis)
        } else {
            Vec::new()
        };
        let should_update_lyrics = page.init_page(sis, si, &likes, playlist_changed);
        if !should_update_lyrics {
            page.queue_record_motion_sync();
        }
        should_update_lyrics
    }

    pub fn set_global_queue_revealed(&self, revealed: bool) {
        self.imp().global_queue_revealer.set_reveal_child(revealed);
    }

    pub fn toggle_global_queue_drawer(&self) {
        let imp = self.imp();
        let revealer = imp.global_queue_revealer.get();
        if revealer.reveals_child() {
            revealer.set_reveal_child(false);
            return;
        }

        imp.player_controls.get().with_playlist(|playlist| {
            let si = playlist
                .current_song()
                .cloned()
                .unwrap_or_else(empty_song_info);
            self.refresh_global_queue_drawer(playlist.songs(), &si);
        });
        revealer.set_reveal_child(true);
    }

    fn refresh_global_queue_drawer(&self, sis: &[SongInfo], si: &SongInfo) {
        let imp = self.imp();
        let songs_list = imp.global_queue_songs_list.get();
        songs_list.replace_list_if_changed(sis, &self.get_song_likes(sis));
        if let Some(index) = sis.iter().position(|song| song.id == si.id) {
            songs_list.mark_new_row_playing(index as i32, false);
        }
    }

    pub fn set_playlist_queue_revealed(&self, revealed: bool) {
        let imp = self.imp();
        let page = imp.playlist_lyrics_page.get().unwrap();
        page.set_queue_revealed(revealed);
    }

    pub fn toggle_playlist_queue_revealed(&self) {
        let imp = self.imp();
        let page = imp.playlist_lyrics_page.get().unwrap();
        page.toggle_queue_revealed();
    }

    pub fn begin_lyrics_update(&self, song_id: u64) -> bool {
        let imp = self.imp();
        let page = imp.playlist_lyrics_page.get().unwrap();
        page.begin_lyrics_update(song_id, &gettext("Loading lyrics..."))
    }

    pub fn lyrics_update_failed(&self, song_id: u64) {
        let imp = self.imp();
        let page = imp.playlist_lyrics_page.get().unwrap();
        page.lyrics_update_failed(song_id, &gettext("Lyrics unavailable"))
    }

    pub fn begin_comments_update(&self, song_id: u64, offset: u32) -> bool {
        let imp = self.imp();
        let page = imp.playlist_lyrics_page.get().unwrap();
        page.begin_comments_update(song_id, offset)
    }

    pub fn comments_update_failed(&self, song_id: u64, offset: u32) {
        let imp = self.imp();
        let page = imp.playlist_lyrics_page.get().unwrap();
        page.comments_update_failed(song_id, offset);
    }

    pub fn update_comments(
        &self,
        song_id: u64,
        offset: u32,
        comments: crate::ncmapi::SongComments,
    ) {
        let imp = self.imp();
        let page = imp.playlist_lyrics_page.get().unwrap();
        page.update_comments(song_id, offset, comments);
    }

    pub fn update_comment_like(&self, song_id: u64, comment_id: u64, liked: bool) {
        let imp = self.imp();
        let page = imp.playlist_lyrics_page.get().unwrap();
        page.update_comment_like(song_id, comment_id, liked);
    }

    pub fn update_comment_replies(
        &self,
        song_id: u64,
        comment_id: u64,
        replies: crate::ncmapi::SongCommentReplies,
    ) {
        let imp = self.imp();
        let page = imp.playlist_lyrics_page.get().unwrap();
        page.update_comment_replies(song_id, comment_id, replies);
    }

    pub fn update_comment_reply_count(&self, song_id: u64, comment_id: u64, count: u64) {
        let imp = self.imp();
        let page = imp.playlist_lyrics_page.get().unwrap();
        page.update_comment_reply_count(song_id, comment_id, count);
    }

    pub fn comment_reply_sent(
        &self,
        song_id: u64,
        comment_id: u64,
        reply: Option<crate::ncmapi::CommentReply>,
    ) {
        let imp = self.imp();
        let page = imp.playlist_lyrics_page.get().unwrap();
        page.comment_reply_sent(song_id, comment_id, reply);
    }

    pub fn dismiss_comment_context_menu(&self, target: Option<&Widget>) {
        let imp = self.imp();
        let Some(page) = imp.playlist_lyrics_page.get() else {
            return;
        };
        if target
            .map(|target| page.comment_context_contains_widget(target))
            .unwrap_or_default()
        {
            return;
        }
        page.dismiss_comment_context_menu();
    }

    pub fn comment_deleted(&self, song_id: u64, parent_comment_id: u64, comment_id: u64) {
        let imp = self.imp();
        let page = imp.playlist_lyrics_page.get().unwrap();
        page.comment_deleted(song_id, parent_comment_id, comment_id);
    }

    pub fn comment_action_failed(&self, song_id: u64, comment_id: u64) {
        let imp = self.imp();
        let page = imp.playlist_lyrics_page.get().unwrap();
        page.comment_action_failed(song_id, comment_id);
    }

    /// 更新歌词内容，不调整位置
    pub fn update_lyrics(&self, song_id: u64, lrc: Vec<(u64, String)>) {
        let imp = self.imp();
        let page = imp.playlist_lyrics_page.get().unwrap();
        page.update_lyrics(song_id, lrc);
    }

    /// 强行更新歌词区文字，用于显示歌词加载提示
    pub fn update_lyrics_text(&self, text: &str) {
        let imp = self.imp();
        let page = imp.playlist_lyrics_page.get().unwrap();
        page.update_lyrics_text(text);
    }

    // 更新歌词高亮位置
    pub fn update_lyrics_timestamp(&self, time: u64) {
        let imp = self.imp();
        let page = imp.playlist_lyrics_page.get().unwrap();
        if self.page_cur_playlist_lyrics_page() {
            page.update_lyrics_highlight(time);
        }
    }

    pub fn update_playlist_status(&self, index: usize) {
        let imp = self.imp();
        let page = imp.playlist_lyrics_page.get().unwrap();
        if self.page_cur_playlist_lyrics_page() {
            page.switch_row(index as i32);
        }
        if imp.global_queue_revealer.reveals_child() {
            imp.global_queue_songs_list
                .get()
                .mark_new_row_playing(index as i32, false);
        }
    }

    pub fn set_song_url(&self, si: SongInfo) {
        self.imp().player_controls.get().set_song_url(si);
    }
    pub fn gst_duration_changed(&self, sec: u64) {
        self.imp().player_controls.get().gst_duration_changed(sec);
    }
    pub fn gst_state_changed(&self, state: gstreamer_play::PlayState) {
        self.imp().player_controls.get().gst_state_changed(state);
        self.imp()
            .playlist_lyrics_page
            .get()
            .unwrap()
            .set_playback_active(matches!(state, gstreamer_play::PlayState::Playing));
    }
    pub fn gst_volume_changed(&self, volume: f64) {
        self.imp().player_controls.get().gst_volume_changed(volume);
    }
    pub fn gst_cache_download_complete(&self, loc: String) {
        self.imp()
            .player_controls
            .get()
            .gst_cache_download_complete(loc);
    }
    pub fn scale_seek_update(&self, sec: u64) {
        self.imp().player_controls.get().scale_seek_update(sec);
    }
    pub fn scale_value_update(&self) {
        self.imp().player_controls.get().scale_value_update();
    }
    pub fn init_mpris(&self, mpris: MprisController) {
        self.imp().player_controls.get().init_mpris(mpris);
    }

    pub async fn action_search(
        &self,
        ncmapi: NcmClient,
        text: String,
        search_type: SearchType,
        offset: u16,
        limit: u16,
    ) -> Option<SearchResult> {
        let imp = self.imp();
        let sender = imp.sender.get().unwrap().clone();
        let window = self;

        let res = match search_type {
            SearchType::Song => ncmapi
                .client
                .search_song(text, offset, limit)
                .await
                .map(|res| {
                    debug!("搜索歌曲：{:?}", res);
                    let likes = window.get_song_likes(&res);
                    SearchResult::Songs(res, likes)
                }),
            SearchType::Singer => {
                ncmapi
                    .client
                    .search_singer(text, offset, limit)
                    .await
                    .map(|res| {
                        debug!("搜索歌手：{:?}", res);
                        SearchResult::Singers(res)
                    })
            }
            SearchType::Album => ncmapi
                .client
                .search_album(text, offset, limit)
                .await
                .map(|res| {
                    debug!("搜索专辑：{:?}", res);
                    SearchResult::SongLists(res)
                }),
            SearchType::Lyrics => {
                ncmapi
                    .client
                    .search_lyrics(text, offset, limit)
                    .await
                    .map(|res| {
                        debug!("搜索歌词：{:?}", res);
                        let likes = window.get_song_likes(&res);
                        SearchResult::Songs(res, likes)
                    })
            }
            SearchType::SongList => ncmapi
                .client
                .search_songlist(text, offset, limit)
                .await
                .map(|res| {
                    debug!("搜索歌单：{:?}", res);
                    SearchResult::SongLists(res)
                }),
            SearchType::TopPicks => ncmapi
                .client
                .top_song_list("全部", "hot", offset, limit)
                .await
                .map(|res| {
                    debug!("获取歌单：{:?}", res);
                    SearchResult::SongLists(res)
                }),
            SearchType::AllAlbums => {
                ncmapi
                    .client
                    .new_albums("ALL", offset, limit)
                    .await
                    .map(|res| {
                        debug!("获取专辑：{:?}", res);
                        SearchResult::SongLists(res)
                    })
            }
            SearchType::Radio => ncmapi
                .client
                .user_radio_sublist(offset, limit)
                .await
                .map(|res| {
                    debug!("获取电台：{:?}", res);
                    SearchResult::SongLists(res)
                }),
            SearchType::LikeAlbums => ncmapi.client.album_sublist(offset, limit).await.map(|res| {
                debug!("获取收藏的专辑：{:?}", res);
                SearchResult::SongLists(res)
            }),
            SearchType::LikeSongList => {
                let uid = window.get_uid();
                ncmapi
                    .client
                    .user_song_list(uid, offset, limit)
                    .await
                    .map(|res| {
                        debug!("获取收藏的歌单：{:?}", res);
                        SearchResult::SongLists(res)
                    })
            }
            _ => Err(anyhow::anyhow!("")),
        };
        if let Err(err) = &res {
            error!("{:?}", err);
            sender
                .send_blocking(Action::AddToast(gettext(
                    "Request for interface failed, please try again!",
                )))
                .unwrap();
        }
        res.ok()
    }
}

#[gtk::template_callbacks]
impl NeteaseCloudMusicLinuxWindow {
    #[template_callback]
    fn global_queue_close_cb(&self) {
        self.set_global_queue_revealed(false);
    }

    #[template_callback]
    fn stack_visible_child_cb(&self) {
        let imp = self.imp();
        let stack = imp.stack.get();
        let label = imp.label_title.get();
        if let Some(visible_child_name) = stack.visible_child_name() {
            self.update_sidebar_selection(visible_child_name.as_str());
            let mut stack_child = LinkedList::new();
            if let Ok(sc) = imp.stack_child.lock() {
                stack_child = (*sc).clone();
            }
            if let Some(child) = stack_child.back()
                && visible_child_name == child.0
            {
                return;
            }
            if stack_child.len() == 1 {
                if visible_child_name == "discover"
                    || visible_child_name == "toplist"
                    || visible_child_name == "my"
                {
                    if let Ok(mut sc) = imp.stack_child.lock() {
                        sc.pop_back();
                        sc.push_back((visible_child_name.to_string(), "".to_owned()));
                    }
                } else if let Ok(mut sc) = imp.stack_child.lock() {
                    sc.push_back((visible_child_name.to_string(), label.text().to_string()));
                }
            } else if visible_child_name == "discover"
                || visible_child_name == "toplist"
                || visible_child_name == "my"
            {
                if let Ok(mut sc) = imp.stack_child.lock() {
                    sc.clear();
                    sc.push_back((visible_child_name.to_string(), "".to_owned()));
                }
            } else if let Ok(mut sc) = imp.stack_child.lock() {
                sc.push_back((visible_child_name.to_string(), label.text().to_string()));
            }
        }
    }

    #[template_callback]
    fn search_song_cb(&self, check: CheckButton) {
        if !check.is_active() {
            return;
        }
        let menu = self.imp().search_menu.get();
        menu.set_label(&check.label().unwrap());
        menu.popdown();
        self.set_property("search-type", SearchType::Song);
    }

    #[template_callback]
    fn search_singer_cb(&self, check: CheckButton) {
        if !check.is_active() {
            return;
        }
        let menu = self.imp().search_menu.get();
        menu.set_label(&check.label().unwrap());
        menu.popdown();
        self.set_property("search-type", SearchType::Singer);
    }

    #[template_callback]
    fn search_album_cb(&self, check: CheckButton) {
        if !check.is_active() {
            return;
        }
        let menu = self.imp().search_menu.get();
        menu.set_label(&check.label().unwrap());
        menu.popdown();
        self.set_property("search-type", SearchType::Album);
    }

    #[template_callback]
    fn search_lyrics_cb(&self, check: CheckButton) {
        if !check.is_active() {
            return;
        }
        let menu = self.imp().search_menu.get();
        menu.set_label(&check.label().unwrap());
        menu.popdown();
        self.set_property("search-type", SearchType::Lyrics);
    }

    #[template_callback]
    fn search_songlist_cb(&self, check: CheckButton) {
        if !check.is_active() {
            return;
        }
        let menu = self.imp().search_menu.get();
        menu.set_label(&check.label().unwrap());
        menu.popdown();
        self.set_property("search-type", SearchType::SongList);
    }

    #[template_callback]
    fn search_entry_cb(&self, entry: SearchEntry) {
        let imp = self.imp();
        let sender = imp.sender.get().unwrap();
        let text = entry.text().to_string();
        imp.label_title.set_label(&text);
        imp.switcher_title.set_visible(false);
        imp.label_title.set_visible(true);
        imp.back_button.set_visible(true);

        let search_type = self.property::<SearchType>("search-type");

        let page = match search_type {
            SearchType::Lyrics | SearchType::Song => {
                let page = self.init_search_song_page(&text, search_type);
                Some(page.upcast::<Widget>())
            }
            SearchType::Singer => {
                let page = self.init_search_singer_page(&text);
                Some(page.upcast::<Widget>())
            }
            SearchType::Album | SearchType::SongList => {
                let page = self.init_search_songlist_page(&text, search_type);
                Some(page.upcast::<Widget>())
            }
            _ => None,
        };
        if let Some(page) = page {
            self.page_new_with_name("search", &page, text.as_str());
            let page = glib::SendWeakRef::from(page.downgrade());
            sender
                .send_blocking(Action::Search(
                    text,
                    search_type,
                    0,
                    50,
                    Arc::new(move |res| {
                        if let Some(page) = page.upgrade() {
                            match res {
                                SearchResult::Songs(sis, likes) => {
                                    page.downcast::<SearchSongPage>()
                                        .unwrap()
                                        .update_songs(&sis, &likes);
                                }
                                SearchResult::Singers(sgs) => {
                                    page.downcast::<SearchSingerPage>()
                                        .unwrap()
                                        .update_singer(sgs);
                                }
                                SearchResult::SongLists(sls) => {
                                    page.downcast::<SearchSongListPage>()
                                        .unwrap()
                                        .update_songlist(&sls);
                                }
                            };
                        }
                    }),
                ))
                .unwrap();
        }
    }
}

impl Default for NeteaseCloudMusicLinuxWindow {
    fn default() -> Self {
        NeteaseCloudMusicLinuxApplication::default()
            .active_window()
            .unwrap()
            .downcast()
            .unwrap()
    }
}
