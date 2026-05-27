use adw::{prelude::AdwDialogExt, subclass::prelude::*};
use async_channel::{Receiver, Sender, unbounded};
use gettextrs::gettext;
use gio::Settings;
use glib::{WeakRef, clone, source::Priority, timeout_future, timeout_future_seconds};
use gtk::{gio, glib, prelude::*};
use log::*;
use ncm_api::{
    AlbumDetailDynamic, BannersInfo, CookieJar, LoginInfo, PlayListDetailDynamic, SingerInfo,
    SongInfo, SongList, TargetType, TopList,
};
use once_cell::sync::OnceCell;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use crate::{
    MAINCONTEXT, NeteaseCloudMusicLinuxWindow, audio::MprisController, config::VERSION,
    gui::NeteaseCloudMusicLinuxPreferences, model::*, ncmapi::*, path::CACHE, utils::*,
};

// implements Debug for Fn(Targ) using "blanket implementations"
pub trait ActionCallbackTr<TArg>: Fn(TArg) + Sync + Send {}
impl<Targ, Tr: Fn(Targ) + Sync + Send> ActionCallbackTr<Targ> for Tr {}
impl<Targ> std::fmt::Debug for dyn ActionCallbackTr<Targ> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ActionCallback")
    }
}
// wrapper dyn Fn(Targ) => ActionCallback<Targ>
// Note: we can capture glib object with glib::SendWeakRef, but only valied in MainContext thread

// callback is needed as there is no way to lookup the sender object
// alternative methods:
//   unique id for sender object, and store a map
//   sender object create new (sender, receiver) and attach, then action send back
pub type ActionCallback<Targ = ()> = Arc<dyn ActionCallbackTr<Targ>>;

const SONG_DETAIL_BATCH_SIZE: usize = 500;
const RADIO_PROGRAM_BATCH_SIZE: u16 = 500;

fn empty_search_result(search_type: SearchType) -> SearchResult {
    match search_type {
        SearchType::Singer => SearchResult::Singers(Vec::new()),
        SearchType::Album
        | SearchType::SongList
        | SearchType::TopPicks
        | SearchType::AllAlbums
        | SearchType::Radio
        | SearchType::LikeAlbums
        | SearchType::LikeSongList => SearchResult::SongLists(Vec::new()),
        _ => SearchResult::Songs(Vec::new(), Vec::new()),
    }
}

async fn fetch_all_radio_programs(ncmapi: &NcmClient, rid: u64) -> anyhow::Result<Vec<SongInfo>> {
    let mut songs = Vec::new();
    let mut offset = 0u32;

    loop {
        let chunk = ncmapi
            .client
            .radio_program(rid, offset as u16, RADIO_PROGRAM_BATCH_SIZE)
            .await?;
        let finished = chunk.len() < usize::from(RADIO_PROGRAM_BATCH_SIZE);
        songs.extend(chunk);

        if finished {
            break;
        }

        offset += u32::from(RADIO_PROGRAM_BATCH_SIZE);
        if offset > u32::from(u16::MAX) {
            break;
        }
    }

    Ok(songs)
}

fn song_id_order_score<'a>(ids: impl Iterator<Item = &'a u64>, playlist_ids: &[u64]) -> usize {
    ids.zip(playlist_ids.iter())
        .take(64)
        .filter(|(id, playlist_id)| *id == *playlist_id)
        .count()
}

fn should_reverse_song_id_order(ids: &[u64], playlist_ids: &[u64]) -> bool {
    if playlist_ids.is_empty() {
        return false;
    }

    let forward_score = song_id_order_score(ids.iter(), playlist_ids);
    let reverse_score = song_id_order_score(ids.iter().rev(), playlist_ids);
    reverse_score > forward_score
}

fn missing_comment_reply_count_ids(comments: &SongComments) -> Vec<u64> {
    let mut seen = HashSet::new();
    comments
        .hot_comments
        .iter()
        .chain(comments.comments.iter())
        .filter(|comment| comment.comment_id != 0 && comment.reply_count == 0)
        .filter_map(|comment| {
            seen.insert(comment.comment_id)
                .then_some(comment.comment_id)
        })
        .collect()
}

async fn update_comment_reply_counts_after_render(
    ncmapi: NcmClient,
    window: NeteaseCloudMusicLinuxWindow,
    song_id: u64,
    comment_ids: Vec<u64>,
) {
    if comment_ids.is_empty() {
        return;
    }

    timeout_future(Duration::from_millis(80)).await;
    for (index, comment_id) in comment_ids.into_iter().enumerate() {
        if index > 0 {
            timeout_future(Duration::from_millis(35)).await;
        }
        match ncmapi.song_comment_reply_count(song_id, comment_id).await {
            Ok(count) if count > 0 => {
                window.update_comment_reply_count(song_id, comment_id, count);
            }
            Ok(_) => {}
            Err(err) => {
                debug!("获取评论回复数失败 {}: {:?}", comment_id, err);
            }
        }
        if index % 4 == 3 {
            timeout_future(Duration::from_millis(120)).await;
        }
    }
}

#[derive(Debug, Clone)]
pub enum Action {
    AddToast(String),

    // (关键字，搜索类型，起始点，数量)
    Search(String, SearchType, u16, u16, ActionCallback<SearchResult>),
    // (url,path,width,height)
    DownloadImage(String, PathBuf, u16, u16, Option<ActionCallback>),
    LikeSongList(u64, bool, Option<ActionCallback>),
    LikeAlbum(u64, bool, Option<ActionCallback>),
    LikeSong(u64, bool, Option<ActionCallback>),
    Moved(SongInfo),

    // play
    AddPlay(SongInfo),
    PlayNextSong,
    Play(SongInfo),
    PlayStart(SongInfo),
    // (歌单, 是否立即播放)
    AddPlayList(Vec<SongInfo>, bool),
    PlayListStart,
    PersistVolume(f64),
    GetSongUrl(SongInfo),
    SetSongUrl(SongInfo),

    // login
    CheckLogin(UserMenuChild, CookieJar),
    Logout,
    InitUserInfo(LoginInfo),
    SwitchUserMenuToPhone,
    SwitchUserMenuToQr,
    SwitchUserMenuToUser(LoginInfo, UserMenuChild),
    GetCaptcha(String, String),
    CaptchaLogin(String, String, String),

    // Qr
    TryUpdateQrCode,
    SetQrImage(PathBuf),
    CheckQrTimeout(String),
    CheckQrTimeoutCb(String),
    SetQrImageTimeout,

    // discover
    InitCarousel,
    InitTopPicks,
    SetupTopPicks(Vec<SongList>),
    InitNewAlbums,
    SetupNewAlbums(Vec<SongList>),
    BannerTo(BannersInfo),

    // toplist
    GetToplist,
    GetToplistSongsList(u64),
    InitTopList(Vec<TopList>),
    UpdateTopList(Vec<SongInfo>),

    // my
    InitMyPage,
    InitMyPageRecSongList(Vec<SongList>),

    // playlist
    ToPlayListLyricsPage,
    TogglePlayListQueueDrawer,
    UpdateLyrics(SongInfo, u64),
    LoadSongComments {
        song_id: u64,
        offset: u32,
    },
    LikeSongComment {
        song_id: u64,
        comment_id: u64,
        like: bool,
    },
    LoadSongCommentReplies {
        song_id: u64,
        comment_id: u64,
    },
    ReplySongComment {
        song_id: u64,
        comment_id: u64,
        content: String,
    },
    DeleteSongComment {
        song_id: u64,
        parent_comment_id: u64,
        comment_id: u64,
    },
    UpdatePlayListStatus(usize),
    RemoveFromPlayList(SongInfo),

    // page routing
    ToTopPicksPage,
    ToAllAlbumsPage,
    ToSongListPage(SongList),
    ToAlbumPage(SongList),
    ToRadioPage(SongList),
    ToSingerSongsPage(SingerInfo),
    ToMyPageDailyRec,
    ToMyPageHeartbeat,
    ToMyPageRadio,
    ToMyPageCloudDisk,
    ToMyPageAlbums,
    ToMyPageSonglist,
    PageBack,

    // gst
    GstDurationChanged(u64),
    GstStateChanged(gstreamer_play::PlayState),
    GstVolumeChanged(f64),
    GstCacheDownloadComplete(String),
    ScaleSeekUpdate(u64),
    ScaleValueUpdate,

    InitMpris(MprisController),
}

mod imp {

    use std::sync::{Arc, RwLock};

    use super::*;

    pub struct NeteaseCloudMusicLinuxApplication {
        pub window: OnceCell<WeakRef<NeteaseCloudMusicLinuxWindow>>,
        pub sender: Sender<Action>,
        pub receiver: RefCell<Option<Receiver<Action>>>,
        pub unikey: Arc<RwLock<String>>,
        pub ncmapi: RefCell<Option<NcmClient>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for NeteaseCloudMusicLinuxApplication {
        const NAME: &'static str = "NeteaseCloudMusicLinuxApplication";
        type Type = super::NeteaseCloudMusicLinuxApplication;
        type ParentType = adw::Application;
        fn new() -> Self {
            let (sender, r) = unbounded();
            let receiver = RefCell::new(Some(r));
            let window = OnceCell::new();
            let unikey = Arc::new(RwLock::new(String::new()));
            let ncmapi = RefCell::new(None);

            Self {
                window,
                sender,
                receiver,
                unikey,
                ncmapi,
            }
        }
    }

    impl ObjectImpl for NeteaseCloudMusicLinuxApplication {
        fn constructed(&self) {
            let obj = self.obj();
            self.parent_constructed();

            obj.setup_gactions();
            obj.setup_cache_clear();
            obj.set_accels_for_action("app.quit", &["<primary>q"]);
            obj.set_accels_for_action("win.search-button", &["<primary>f", "slash"]);
            obj.set_accels_for_action("win.back-button", &["<primary>BackSpace"]);
        }
    }

    impl ApplicationImpl for NeteaseCloudMusicLinuxApplication {
        // We connect to the activate callback to create a window when the application
        // has been launched. Additionally, this callback notifies us when the user
        // tries to launch a "second instance" of the application. When they try
        // to do that, we'll just present any existing window.
        fn activate(&self) {
            let obj = self.obj();
            let app = obj
                .downcast_ref::<super::NeteaseCloudMusicLinuxApplication>()
                .unwrap();

            if let Some(weak_window) = self.window.get() {
                weak_window.upgrade().unwrap().present();
                return;
            }

            let window = app.create_window();
            let _ = self.window.set(window.downgrade());

            // Setup action channel
            let receiver = self.receiver.borrow_mut().take().unwrap();
            MAINCONTEXT.spawn_local_with_priority(
                Priority::HIGH,
                clone!(
                    #[strong]
                    app,
                    async move {
                        while let Ok(action) = receiver.recv().await {
                            app.process_action(action);
                        }
                    }
                ),
            );

            // Ask the window manager/compositor to present the window
            window.present();
        }
    }

    impl GtkApplicationImpl for NeteaseCloudMusicLinuxApplication {}
    impl AdwApplicationImpl for NeteaseCloudMusicLinuxApplication {}
}

glib::wrapper! {
    pub struct NeteaseCloudMusicLinuxApplication(ObjectSubclass<imp::NeteaseCloudMusicLinuxApplication>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl NeteaseCloudMusicLinuxApplication {
    pub fn new(application_id: &str, flags: &gio::ApplicationFlags) -> Self {
        glib::Object::builder()
            .property("application-id", application_id)
            .property("flags", flags)
            .build()
    }

    fn create_window(&self) -> NeteaseCloudMusicLinuxWindow {
        let imp = self.imp();
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::IconTheme::for_display(&display)
                .add_resource_path("/io/github/b1ngggg/netease_cloud_music_linux/icons");
        }
        gtk::Window::set_default_icon_name(crate::APP_ICON);
        let window = NeteaseCloudMusicLinuxWindow::new(&self.clone(), imp.sender.clone());
        window.set_icon_name(Some(crate::APP_ICON));

        window.present();
        window
    }

    fn init_ncmapi(&self, cli: NcmClient) -> NcmClient {
        let window = self.imp().window.get().unwrap().upgrade().unwrap();
        let mut ncmapi = cli;
        let proxy_address = window.settings().string("proxy-address").to_string();
        if !proxy_address.is_empty() && ncmapi.set_proxy(proxy_address).is_err() {
            // do nothing
        }
        ncmapi
    }

    fn process_action(&self, action: Action) -> glib::ControlFlow {
        let imp = self.imp();
        if self.active_window().is_none() {
            return glib::ControlFlow::Continue;
        }

        let window = imp.window.get().unwrap().upgrade().unwrap();
        let ncmapi = {
            let ncmapi_opt = { imp.ncmapi.borrow().as_ref().cloned() };
            if let Some(ncmapi) = ncmapi_opt {
                ncmapi
            } else {
                let ncmapi = self.init_ncmapi(NcmClient::new());
                imp.ncmapi.replace(Some(ncmapi.clone()));
                ncmapi
            }
        };

        match action {
            Action::CheckLogin(user_menu, logined_cookie_jar) => {
                let sender = imp.sender.clone();
                let ncmapi = self.init_ncmapi(NcmClient::from_cookie_jar(logined_cookie_jar));
                let s = self.clone();

                MAINCONTEXT.spawn_local_with_priority(Priority::HIGH_IDLE, async move {
                    if !window.is_logined() {
                        match ncmapi.client.login_status().await {
                            Ok(login_info) => {
                                debug!("获取用户信息成功: {:?}", login_info);
                                window.set_uid(login_info.uid);

                                ncmapi.save_cookie_jar_to_file();
                                s.imp().ncmapi.replace(Some(ncmapi));

                                sender
                                    .send(Action::InitUserInfo(login_info.to_owned()))
                                    .await
                                    .unwrap();
                                sender
                                    .send(Action::SwitchUserMenuToUser(login_info, user_menu))
                                    .await
                                    .unwrap();
                                sender.send(Action::InitMyPage).await.unwrap();
                                sender
                                    .send(Action::AddToast(gettext("Login successful!")))
                                    .await
                                    .unwrap();
                            }
                            Err(err) => {
                                error!("获取用户信息失败！{:?}", err);
                                sender
                                    .send(Action::AddToast(gettext("Login failed!")))
                                    .await
                                    .unwrap();

                                s.imp().ncmapi.replace(None);
                                NcmClient::clean_cookie_file();
                            }
                        }
                    }
                });
            }
            Action::Logout => {
                let sender = imp.sender.clone();
                let s = self.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    ncmapi.client.logout().await;

                    s.imp().ncmapi.replace(None);
                    NcmClient::clean_cookie_file();

                    window.logout();
                    window.switch_my_page_to_logout();
                    sender.send(Action::SwitchUserMenuToQr).await.unwrap();
                    sender
                        .send(Action::AddToast(gettext("Logout!")))
                        .await
                        .unwrap();
                });
            }
            Action::InitUserInfo(login_info) => {
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    match ncmapi.client.user_song_id_list(login_info.uid).await {
                        Ok(song_ids) => {
                            window.set_user_like_songs(&song_ids);
                        }
                        Err(err) => error!("{:?}", err),
                    }
                });
            }
            Action::TryUpdateQrCode => {
                if !window.is_logined() && window.is_user_menu_active(UserMenuChild::Qr) {
                    let sender = imp.sender.clone();
                    MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                        if let Ok((path, unikey)) = ncmapi.create_qrcode().await {
                            sender.send(Action::SetQrImage(path)).await.unwrap();
                            sender.send(Action::CheckQrTimeout(unikey)).await.unwrap();
                        }
                    });
                }
            }
            Action::SetQrImage(path) => {
                window.set_user_qrimage(path);
            }
            Action::CheckQrTimeout(unikey) => {
                if let Ok(key) = imp.unikey.read()
                    && unikey != *key
                {
                    let sender = imp.sender.clone();
                    sender
                        .send_blocking(Action::CheckQrTimeoutCb(unikey))
                        .unwrap();
                }
            }
            Action::CheckQrTimeoutCb(unikey) => {
                debug!("检查登录二维码状态，unikey={}", unikey);
                {
                    let mut key = imp.unikey.write().unwrap();
                    *key = unikey.clone();
                }
                let sender = imp.sender.clone();
                let key = imp.unikey.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    let mut send_toast = true;
                    loop {
                        {
                            let key = key.read().unwrap();
                            if *key != unikey {
                                warn!("unikey 已失效，unikey={}", unikey);
                                break;
                            }
                        }
                        match  ncmapi.client.login_qr_check(unikey.to_owned()).await {
                            Ok(msg) => {
                                match msg.code {
                                    // 已过期
                                    800 => {
                                        debug!("二维码已过期，unikey={}", unikey);
                                        sender.send(Action::SetQrImageTimeout).await.unwrap();
                                        break;
                                    }
                                    // 等待扫码
                                    801 => {
                                        debug!("等待扫码，unikey={}", unikey);
                                    },
                                    // 等待确认
                                    802 => {
                                        debug!("等待app端确认，unikey={}", unikey);
                                        if send_toast {
                                            sender
                                                .send(Action::AddToast(gettext("Have scanned the QR code, waiting for confirmation!")))
                                                .await.unwrap();
                                            send_toast = false;
                                        }
                                    }
                                    // 登录成功
                                    803 => {
                                        debug!("扫码登录成功，unikey={}", unikey);
                                        let cookie_jar = ncmapi.client.cookie_jar().cloned().unwrap_or_else(|| {
                                            error!("No login cookie found");
                                            CookieJar::new()
                                        });
                                        sender.send(Action::CheckLogin(UserMenuChild::Qr, cookie_jar)).await.unwrap();
                                        break;
                                    }
                                    _ => break,
                                }
                            },
                            Err(err) => error!("{:?}", err),
                        }
                        timeout_future_seconds(1).await;
                    }
                });
            }
            Action::SetQrImageTimeout => {
                window.set_user_qrimage_timeout();
            }
            Action::SwitchUserMenuToPhone => {
                window.switch_user_menu_to_phone();
            }
            Action::SwitchUserMenuToQr => {
                window.switch_user_menu_to_qr();
            }
            Action::GetCaptcha(ctcode, phone) => {
                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    match ncmapi.client.captcha(ctcode, phone).await {
                        Ok(..) => {
                            debug!("发送获取验证码请求...");
                            sender
                            .send(Action::AddToast(gettext(
                                "Please pay attention to check the cell phone verification code!",
                            )))
                            .await.unwrap();
                        }
                        Err(err) => {
                            warn!("获取验证码失败! {:?}", err);
                            sender
                                .send(Action::AddToast(gettext(
                                    "Failed to get verification code!",
                                )))
                                .await
                                .unwrap();
                        }
                    }
                });
            }
            Action::CaptchaLogin(ctcode, phone, captcha) => {
                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    debug!("使用验证码登录：{}", captcha);
                    if let Ok(_login_info) =
                        ncmapi.client.login_cellphone(ctcode, phone, captcha).await
                    {
                        let cookie_jar = ncmapi.client.cookie_jar().cloned().unwrap_or_else(|| {
                            error!("No login cookie found");
                            CookieJar::new()
                        });
                        sender
                            .send(Action::CheckLogin(UserMenuChild::Phone, cookie_jar))
                            .await
                            .unwrap();
                    } else {
                        error!("登录失败！");
                        sender
                            .send(Action::AddToast(gettext("Login failed!")))
                            .await
                            .unwrap();
                    }
                });
            }
            Action::SwitchUserMenuToUser(login_info, menu) => {
                window.switch_user_menu_to_user(login_info.clone(), menu);
                let avatar_url = login_info.avatar_url;
                let mut path = CACHE.clone();
                path.push("avatar.jpg");
                window.set_avatar(avatar_url, path);
            }
            Action::AddToast(mes) => {
                window.add_toast(mes);
            }
            Action::InitCarousel => {
                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    match ncmapi.client.banners().await {
                        Ok(banners) => {
                            debug!("获取轮播信息: {:?}", banners);
                            for banner in banners {
                                window.add_carousel(banner);
                            }

                            // auto check login after banners
                            // https://github.com/Binaryify/NeteaseCloudMusicApi/issues/1217
                            if let Some(cookie_jar) = NcmClient::load_cookie_jar_from_file() {
                                sender
                                    .send(Action::CheckLogin(UserMenuChild::Qr, cookie_jar))
                                    .await
                                    .unwrap();
                            }
                        }
                        Err(err) => {
                            error!("获取首页轮播信息失败！{:?}", err);
                            timeout_future(Duration::from_millis(500)).await;
                            sender.send(Action::InitCarousel).await.unwrap();
                        }
                    }
                });
            }
            Action::InitTopPicks => {
                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    match ncmapi.client.top_song_list("全部", "hot", 0, 8).await {
                        Ok(song_list) => {
                            debug!("获取热门推荐信息：{:?}", song_list);
                            sender.send(Action::SetupTopPicks(song_list)).await.unwrap();
                        }
                        Err(err) => {
                            error!("获取热门推荐信息失败！{:?}", err);
                            timeout_future(Duration::from_millis(500)).await;
                            sender.send(Action::InitTopPicks).await.unwrap();
                        }
                    }
                });
            }
            Action::ToTopPicksPage => {
                let page = window.init_picks_songlist();
                let title = "全部热门推荐";
                window.page_new(&page, title, "ToTopPicksPage");
                let page = page.downgrade();

                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    if let Some(SearchResult::SongLists(sls)) = window
                        .action_search(ncmapi, String::new(), SearchType::TopPicks, 0, 50)
                        .await
                        && let Some(page) = page.upgrade()
                    {
                        page.update_songlist(&sls);
                    }
                });
            }
            Action::DownloadImage(url, path, width, height, callback) => {
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    if ncmapi
                        .client
                        .download_img(url, path, width, height)
                        .await
                        .is_ok()
                        && let Some(cb) = callback
                    {
                        cb(());
                    }
                });
            }
            Action::SetupTopPicks(song_list) => {
                window.setup_top_picks(song_list);
            }
            Action::InitNewAlbums => {
                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    match ncmapi.client.new_albums("ALL", 0, 8).await {
                        Ok(song_list) => {
                            debug!("获取新碟上架信息：{:?}", song_list);
                            sender
                                .send(Action::SetupNewAlbums(song_list))
                                .await
                                .unwrap();
                        }
                        Err(err) => {
                            error!("获取新碟上架信息失败！{:?}", err);
                            timeout_future(Duration::from_millis(500)).await;
                            sender.send(Action::InitNewAlbums).await.unwrap();
                        }
                    }
                });
            }
            Action::ToAllAlbumsPage => {
                let page = window.init_all_albums();

                let title = "全部专辑";
                window.page_new(&page, title, "ToAllAlbumsPage");
                let page = page.downgrade();

                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    if let Some(SearchResult::SongLists(sls)) = window
                        .action_search(ncmapi, String::new(), SearchType::AllAlbums, 0, 50)
                        .await
                        && let Some(page) = page.upgrade()
                    {
                        page.update_songlist(&sls);
                    }
                });
            }
            Action::SetupNewAlbums(song_list) => {
                window.setup_new_albums(song_list);
            }
            Action::BannerTo(banner) => {
                let sender = imp.sender.clone();
                match banner.target_type {
                    TargetType::Song => {
                        MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                            match ncmapi.client.songs_detail(&[banner.target_id]).await {
                                Ok(songs) => {
                                    debug!("获取轮播歌曲信息：{:?}", songs);
                                    if let Some(song) = songs.first() {
                                        sender.send(Action::AddPlay(song.clone())).await.unwrap();
                                    }
                                }
                                Err(err) => {
                                    error!("获取轮播歌曲信息失败！{:?}", err);
                                }
                            }
                        });
                    }
                    TargetType::Album => {
                        MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                            match ncmapi.client.album(banner.target_id).await {
                                Ok(album) => {
                                    debug!("获取轮播专辑信息：{:?}", album);
                                    let page = window.init_songlist_page(
                                        &SongList {
                                            id: album.id,
                                            name: album.name.to_owned(),
                                            cover_img_url: album.pic_url.to_owned(),
                                            author: album.artist_name.to_owned(),
                                        },
                                        true,
                                    );
                                    window.page_new(&page, &album.name, "Album");
                                    let page = page.downgrade();
                                    let detal_dynamic_as =
                                        ncmapi.client.album_detail_dynamic(album.id);
                                    let dy = detal_dynamic_as.await.unwrap_or_else(|err| {
                                        error!("{:?}", err);
                                        AlbumDetailDynamic::default()
                                    });
                                    let detail = SongListDetail::Album(album, dy);
                                    if let Some(page) = page.upgrade() {
                                        window.update_songlist_page(page, &detail);
                                    }
                                }
                                Err(err) => {
                                    error!("获取轮播专辑信息失败！{:?}", err);
                                }
                            }
                        });
                    }
                    TargetType::Unknown => (),
                }
            }
            Action::AddPlay(song_info) => {
                window.add_play(song_info.clone());
                let sender = imp.sender.clone();
                sender.send_blocking(Action::Play(song_info)).unwrap();
            }
            Action::PlayNextSong => {
                window.play_next();
            }
            Action::Play(song_info) => {
                let sender = imp.sender.clone();
                let music_rate = window.settings().uint("music-rate");
                let path = crate::path::get_music_cache_path(song_info.id, music_rate);

                if !path.exists() {
                    MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                        if song_info.song_url.is_empty() {
                            if let Ok(song_url) =
                                ncmapi.songs_url(&[song_info.id], music_rate).await
                            {
                                debug!("获取歌曲播放链接: {:?}", song_url);
                                if let Some(song_url) = song_url.first() {
                                    let song_info = SongInfo {
                                        song_url: song_url.url.to_owned(),
                                        ..song_info
                                    };
                                    sender.send(Action::PlayStart(song_info)).await.unwrap();
                                } else {
                                    error!("获取歌曲播放链接失败: {:?}", &[song_info.id]);
                                    sender
                                        .send(Action::AddToast(gettext_f(
                                            "Get [{name}] Playback link failed!",
                                            &[("name", &song_info.name)],
                                        )))
                                        .await
                                        .unwrap();
                                    timeout_future_seconds(2).await;
                                    sender.send(Action::PlayNextSong).await.unwrap();
                                }
                            } else {
                                error!("获取歌曲播放链接失败: {:?}", &[song_info.id]);
                                sender
                                    .send(Action::AddToast(gettext_f(
                                        "Get [{name}] Playback link failed!",
                                        &[("name", &song_info.name)],
                                    )))
                                    .await
                                    .unwrap();
                                timeout_future_seconds(2).await;
                                sender.send(Action::PlayNextSong).await.unwrap();
                            }
                        } else {
                            sender.send(Action::PlayStart(song_info)).await.unwrap();
                        }
                    });
                } else {
                    let song_info = SongInfo {
                        song_url: format!("file://{}", path.to_str().unwrap().to_owned()),
                        ..song_info
                    };
                    sender.send_blocking(Action::PlayStart(song_info)).unwrap();
                }
            }
            Action::PlayStart(song_info) => {
                let should_update_lyrics_page = window.play(song_info.clone());
                if window.settings().boolean("desktop-lyrics") || should_update_lyrics_page {
                    let sender = imp.sender.clone();
                    sender
                        .send_blocking(Action::UpdateLyrics(song_info.to_owned(), 0))
                        .unwrap();
                };
                debug!("播放歌曲: {:?}", song_info);
            }
            Action::ToSongListPage(songlist) => {
                let page = window.init_songlist_page(&songlist, false);
                window.page_new(&page, &songlist.name, "ToSongListPage");
                let page = page.downgrade();

                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    let detal_dynamic_as = ncmapi.client.songlist_detail_dynamic(songlist.id);
                    match ncmapi.song_list_detail_complete(songlist.id).await {
                        Ok(detail) => {
                            debug!("获取歌单详情: {:?}", detail);
                            let dy = detal_dynamic_as.await.unwrap_or_else(|err| {
                                error!("{:?}", err);
                                PlayListDetailDynamic::default()
                            });
                            let detail = SongListDetail::PlayList(detail, dy);
                            if let Some(page) = page.upgrade() {
                                window.update_songlist_page(page, &detail);
                            }
                        }
                        Err(err) => {
                            error!("获取歌单详情失败: {:?}", err);
                            if let Some(page) = page.upgrade() {
                                page.set_loading(false);
                            }
                            sender
                                .send(Action::AddToast(gettext(
                                    "Failed to get song list details!",
                                )))
                                .await
                                .unwrap();
                        }
                    }
                });
            }
            Action::PersistVolume(value) => {
                window.persist_volume(value);
            }
            Action::GetSongUrl(song_info) => {
                let sender = imp.sender.clone();
                let music_rate = window.settings().uint("music-rate");
                let path = crate::path::get_music_cache_path(song_info.id, music_rate);

                if !path.exists() {
                    MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                        if let Ok(song_url) = ncmapi.songs_url(&[song_info.id], music_rate).await {
                            debug!("获取歌曲播放链接: {:?}", song_url);
                            if let Some(song_url) = song_url.first() {
                                let song_info = SongInfo {
                                    song_url: song_url.url.to_owned(),
                                    ..song_info
                                };
                                sender.send(Action::SetSongUrl(song_info)).await.unwrap();
                            }
                        }
                    });
                } else {
                    let song_info = SongInfo {
                        song_url: format!("file://{}", path.to_str().unwrap().to_owned()),
                        ..song_info
                    };
                    window.set_song_url(song_info);
                }
            }
            Action::SetSongUrl(song_info) => {
                window.set_song_url(song_info);
            }
            Action::ToAlbumPage(songlist) => {
                let page = window.init_songlist_page(&songlist, true);
                window.page_new(&page, &songlist.name, "ToAlbumPage");
                let page = page.downgrade();

                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    let detal_dynamic_as = ncmapi.client.album_detail_dynamic(songlist.id);
                    match ncmapi.client.album(songlist.id).await {
                        Ok(detail) => {
                            debug!("获取专辑详情: {:?}", detail);
                            let dy = detal_dynamic_as.await.unwrap_or_else(|err| {
                                error!("{:?}", err);
                                AlbumDetailDynamic::default()
                            });
                            let detail = SongListDetail::Album(detail, dy);
                            if let Some(page) = page.upgrade() {
                                window.update_songlist_page(page, &detail);
                            }
                        }
                        Err(err) => {
                            error!("获取专辑详情失败: {:?}", err);
                            if let Some(page) = page.upgrade() {
                                page.set_loading(false);
                            }
                            sender
                                .send(Action::AddToast(gettext("Failed to get album details!")))
                                .await
                                .unwrap();
                        }
                    }
                });
            }
            Action::ToRadioPage(songlist) => {
                let page = window.init_songlist_page(&songlist, true);
                window.page_new(&page, &songlist.name, "ToRadioPage");
                let page = page.downgrade();

                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    match fetch_all_radio_programs(&ncmapi, songlist.id).await {
                        Ok(detail) => {
                            debug!("获取电台详情: {:?}", detail);
                            let detail = SongListDetail::Radio(detail);
                            if let Some(page) = page.upgrade() {
                                window.update_songlist_page(page, &detail);
                            }
                        }
                        Err(err) => {
                            error!("获取电台详情失败: {:?}", err);
                            if let Some(page) = page.upgrade() {
                                page.set_loading(false);
                            }
                            sender
                                .send(Action::AddToast(gettext("Failed to get radio details!")))
                                .await
                                .unwrap();
                        }
                    }
                });
            }
            Action::AddPlayList(sis, is_play) => {
                window.add_playlist(sis, is_play);
            }
            Action::PlayListStart => {
                window.playlist_start();
            }
            Action::LikeSongList(id, is_like, callback) => {
                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    if ncmapi.client.song_list_like(is_like, id).await {
                        debug!("收藏/取消收藏歌单: {:?}", id);
                        if let Some(callback) = callback {
                            callback(());
                        }
                        sender
                            .send(Action::AddToast(if is_like {
                                gettext("Song list have been collected!")
                            } else {
                                gettext("Song list have been uncollected!")
                            }))
                            .await
                            .unwrap();
                    } else {
                        error!("收藏/取消收藏歌单失败: {:?}", id);
                        sender
                            .send(Action::AddToast(if is_like {
                                gettext("Failed to collect song list!")
                            } else {
                                gettext("Failed to uncollect song list!")
                            }))
                            .await
                            .unwrap();
                    }
                });
            }
            Action::LikeAlbum(id, is_like, callback) => {
                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    if ncmapi.client.album_like(is_like, id).await {
                        debug!("收藏/取消收藏专辑: {:?}", id);
                        if let Some(callback) = callback {
                            callback(());
                        }
                        sender
                            .send(Action::AddToast(if is_like {
                                gettext("Album have been collected!")
                            } else {
                                gettext("Album have been uncollected!")
                            }))
                            .await
                            .unwrap();
                    } else {
                        error!("收藏/取消收藏专辑失败: {:?}", id);
                        sender
                            .send(Action::AddToast(if is_like {
                                gettext("Failed to collect album!")
                            } else {
                                gettext("Failed to uncollect album!")
                            }))
                            .await
                            .unwrap();
                    }
                });
            }
            Action::LikeSong(id, is_like, callback) => {
                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    if ncmapi.client.like(is_like, id).await {
                        debug!("收藏/取消收藏歌曲: {:?}", id);
                        window.set_like_song(id, is_like);
                        sender
                            .send(Action::AddToast(if is_like {
                                gettext("Songs have been collected!")
                            } else {
                                gettext("Songs have been uncollected!")
                            }))
                            .await
                            .unwrap();
                        if let Some(callback) = callback {
                            callback(());
                        }
                    } else {
                        error!("收藏/取消收藏歌曲失败: {:?}", id);
                        sender
                            .send(Action::AddToast(if is_like {
                                gettext("Failed to collect songs!")
                            } else {
                                gettext("Failed to uncollect songs!")
                            }))
                            .await
                            .unwrap();
                    }
                });
            }
            Action::Moved(si) => {
                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    match ncmapi
                        .client
                        .playmode_intelligence_list(si.id, si.album_id)
                        .await
                    {
                        Ok(mut pl) => {
                            debug!("获取心动歌曲：{:?}", pl);
                            let mut pla = vec![si];
                            pla.append(&mut pl);

                            sender.send(Action::AddPlayList(pla, false)).await.unwrap();
                            sender
                                .send(Action::AddToast(gettext("Intelligent Mode")))
                                .await
                                .unwrap();
                        }
                        Err(err) => {
                            error!("获取心动歌曲 {} 失败! {:?}", si.name, err);
                            sender
                                .send(Action::AddToast(gettext("Intelligent mode failed!")))
                                .await
                                .unwrap();
                        }
                    }
                });
            }
            Action::GetToplist => {
                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    match ncmapi.client.toplist().await {
                        Ok(toplist) => {
                            debug!("获取排行榜: {:?}", toplist);
                            sender.send(Action::InitTopList(toplist)).await.unwrap();
                        }
                        Err(err) => {
                            error!("获取排行榜失败! {:?}", err);
                            timeout_future(Duration::from_millis(500)).await;
                            sender.send(Action::GetToplist).await.unwrap();
                        }
                    }
                });
            }
            Action::GetToplistSongsList(id) => {
                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    match ncmapi.song_list_detail_complete(id).await {
                        Ok(detail) => {
                            debug!("获取榜单 {} 详情：{:?}", id, detail);
                            sender
                                .send(Action::UpdateTopList(detail.songs))
                                .await
                                .unwrap();
                        }
                        Err(err) => {
                            error!("获取榜单 {} 失败! {:?}", id, err);
                            sender
                                .send(Action::UpdateTopList(Vec::new()))
                                .await
                                .unwrap();
                            sender
                                .send(Action::AddToast(gettext(
                                    "Request for interface failed, please try again!",
                                )))
                                .await
                                .unwrap();
                        }
                    }
                });
            }
            Action::InitTopList(toplist) => {
                window.init_toplist(toplist);
            }
            Action::UpdateTopList(sis) => {
                window.update_toplist(sis);
            }
            Action::Search(text, search_type, offset, limit, callback) => {
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    let res = window
                        .action_search(ncmapi, text, search_type, offset, limit)
                        .await;
                    callback(res.unwrap_or_else(|| empty_search_result(search_type)));
                });
            }
            Action::ToSingerSongsPage(singer) => {
                let title = &singer.name;
                let page = window.init_search_song_page(title, SearchType::SingerSongs);
                window.page_new(&page, title, "ToSingerSongsPage");
                let page = page.downgrade();

                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    match ncmapi.client.singer_songs(singer.id).await {
                        Ok(sis) => {
                            debug!("获取歌手单曲：{:?}", sis);
                            if let Some(page) = page.upgrade() {
                                window.update_search_song_page(page, sis);
                            }
                        }
                        Err(err) => {
                            error!("{:?}", err);
                            if let Some(page) = page.upgrade() {
                                page.set_loading(false);
                            }
                            sender
                                .send(Action::AddToast(gettext(
                                    "Request for interface failed, please try again!",
                                )))
                                .await
                                .unwrap();
                        }
                    }
                });
            }
            Action::ToMyPageDailyRec => {
                let title = "每日推荐";
                let page = window.init_search_song_page(title, SearchType::DailyRec);
                window.page_new(&page, title, "ToMyPageDailyRec");
                let page = page.downgrade();

                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    match ncmapi.client.recommend_songs().await {
                        Ok(sis) => {
                            debug!("获取每日推荐：{:?}", sis);
                            if let Some(page) = page.upgrade() {
                                window.update_search_song_page(page, sis);
                            }
                        }
                        Err(err) => {
                            error!("{:?}", err);
                            if let Some(page) = page.upgrade() {
                                page.set_loading(false);
                            }
                            sender
                                .send(Action::AddToast(gettext(
                                    "Request for interface failed, please try again!",
                                )))
                                .await
                                .unwrap();
                        }
                    }
                });
            }
            Action::ToMyPageHeartbeat => {
                let title = "收藏单曲";
                let page = window.init_search_song_page(title, SearchType::Heartbeat);
                window.page_new(&page, title, "ToMyPageHeartbeat");
                let page = page.downgrade();

                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    let uid = window.get_uid();
                    match ncmapi.client.user_song_id_list(uid).await {
                        Ok(ids) => {
                            debug!("获取收藏单曲 id：{}", ids.len());
                            window.set_user_like_songs(&ids);
                            if ids.is_empty() {
                                if let Some(page) = page.upgrade() {
                                    page.set_loading(false);
                                }
                                return;
                            }

                            let mut loaded_song_ids = HashSet::new();
                            let mut loaded_playlist_ids = Vec::new();
                            match ncmapi.client.user_song_list(uid, 0, 1).await {
                                Ok(songlists) => {
                                    if let Some(songlist) = songlists.first() {
                                        match ncmapi.song_list_detail_complete(songlist.id).await {
                                            Ok(detail) => {
                                                for chunk in
                                                    detail.songs.chunks(SONG_DETAIL_BATCH_SIZE)
                                                {
                                                    loaded_song_ids
                                                        .extend(chunk.iter().map(|song| song.id));
                                                    loaded_playlist_ids
                                                        .extend(chunk.iter().map(|song| song.id));
                                                    if let Some(page) = page.upgrade() {
                                                        window.update_search_song_page(
                                                            page,
                                                            chunk.to_vec(),
                                                        );
                                                        timeout_future(Duration::from_millis(1))
                                                            .await;
                                                    } else {
                                                        return;
                                                    }
                                                }
                                            }
                                            Err(err) => {
                                                warn!(
                                                    "获取收藏歌单详情失败，将按 id 列表补齐：{:?}",
                                                    err
                                                );
                                            }
                                        }
                                    }
                                }
                                Err(err) => {
                                    warn!("获取用户歌单失败，将按 id 列表补齐：{:?}", err);
                                }
                            }

                            let ordered_ids =
                                if should_reverse_song_id_order(&ids, &loaded_playlist_ids) {
                                    ids.iter().rev().copied().collect::<Vec<_>>()
                                } else {
                                    ids.clone()
                                };
                            let remaining_ids = ordered_ids
                                .into_iter()
                                .filter(|id| !loaded_song_ids.contains(id))
                                .collect::<Vec<_>>();
                            for chunk in remaining_ids.chunks(SONG_DETAIL_BATCH_SIZE) {
                                match ncmapi.client.songs_detail(chunk).await {
                                    Ok(mut songs) => {
                                        let order = chunk
                                            .iter()
                                            .enumerate()
                                            .map(|(index, id)| (*id, index))
                                            .collect::<HashMap<_, _>>();
                                        songs.sort_by_key(|song| {
                                            order.get(&song.id).copied().unwrap_or(usize::MAX)
                                        });
                                        if let Some(page) = page.upgrade() {
                                            window.update_search_song_page(page, songs);
                                        } else {
                                            return;
                                        }
                                    }
                                    Err(err) => {
                                        error!("{:?}", err);
                                        if let Some(page) = page.upgrade() {
                                            page.set_loading(false);
                                        }
                                        sender
                                            .send(Action::AddToast(gettext(
                                                "Failed to get song list details!",
                                            )))
                                            .await
                                            .unwrap();
                                        return;
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            error!("{:?}", err);
                            if let Some(page) = page.upgrade() {
                                page.set_loading(false);
                            }
                            sender
                                .send(Action::AddToast(gettext(
                                    "Request for interface failed, please try again!",
                                )))
                                .await
                                .unwrap();
                        }
                    }
                });
            }
            Action::ToMyPageCloudDisk => {
                let title = "云盘音乐";
                let page = window.init_search_song_page(title, SearchType::CloudDisk);
                window.page_new(&page, title, "ToMyPageCloudDisk");
                let page = page.downgrade();

                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    match ncmapi.client.user_cloud_disk().await {
                        Ok(sis) => {
                            debug!("获取云盘音乐：{:?}", sis);
                            if let Some(page) = page.upgrade() {
                                window.update_search_song_page(page, sis);
                            }
                        }
                        Err(err) => {
                            error!("{:?}", err);
                            if let Some(page) = page.upgrade() {
                                page.set_loading(false);
                            }
                            sender
                                .send(Action::AddToast(gettext(
                                    "Request for interface failed, please try again!",
                                )))
                                .await
                                .unwrap();
                        }
                    }
                });
            }
            Action::ToMyPageRadio => {
                let title = "我的电台";
                let page = window.init_search_songlist_page(title, SearchType::Radio);
                window.page_new(&page, title, "ToMyPageRadio");
                let page = page.downgrade();

                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    let res = window
                        .action_search(ncmapi, String::new(), SearchType::Radio, 0, 50)
                        .await;
                    if let Some(page) = page.upgrade()
                        && let Some(SearchResult::SongLists(sls)) = res
                    {
                        page.update_songlist(&sls);
                    }
                });
            }
            Action::ToMyPageAlbums => {
                let title = "收藏专辑";
                let page = window.init_search_songlist_page(title, SearchType::LikeAlbums);
                window.page_new(&page, title, "ToMyPageAlbums");
                let page = page.downgrade();

                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    let res = window
                        .action_search(ncmapi, String::new(), SearchType::LikeAlbums, 0, 50)
                        .await;
                    if let Some(page) = page.upgrade()
                        && let Some(SearchResult::SongLists(sls)) = res
                    {
                        page.update_songlist(&sls);
                    }
                });
            }
            Action::ToMyPageSonglist => {
                let title = "收藏歌单";
                let page = window.init_search_songlist_page(title, SearchType::LikeSongList);
                window.page_new(&page, title, "ToMyPageSonglist");
                let page = page.downgrade();

                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    let res = window
                        .action_search(ncmapi, String::new(), SearchType::LikeSongList, 0, 50)
                        .await;
                    if let Some(page) = page.upgrade()
                        && let Some(SearchResult::SongLists(sls)) = res
                    {
                        page.update_songlist(sls.get(1..).unwrap_or(&[]));
                    }
                });
            }
            Action::InitMyPage => {
                window.switch_my_page_to_login();
                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    match ncmapi.client.recommend_resource().await {
                        Ok(sls) => {
                            debug!("获取推荐歌单：{:?}", sls);
                            sender
                                .send(Action::InitMyPageRecSongList(sls))
                                .await
                                .unwrap();
                        }
                        Err(err) => {
                            error!("{:?}", err);
                            sender.send(Action::InitMyPage).await.unwrap();
                        }
                    }
                });
            }
            Action::InitMyPageRecSongList(sls) => {
                window.init_my_page(sls);
            }
            Action::ToPlayListLyricsPage => {
                let sender = imp.sender.clone();
                if !window.page_cur_playlist_lyrics_page() {
                    if let Some((si, should_update_lyrics)) = window.init_playlist_lyrics_page() {
                        window.set_playlist_queue_revealed(false);
                        if si.id != 0 && should_update_lyrics {
                            sender
                                .send_blocking(Action::UpdateLyrics(si.to_owned(), 0))
                                .unwrap();
                        }
                    }
                } else {
                    sender.send_blocking(Action::PageBack).unwrap();
                }
            }
            Action::TogglePlayListQueueDrawer => {
                if window.page_cur_playlist_lyrics_page() {
                    window.toggle_playlist_queue_revealed();
                } else {
                    window.toggle_global_queue_drawer();
                }
            }
            Action::UpdateLyrics(si, time) => {
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    let song_id = si.id;
                    if time == 0 {
                        // 当新曲目播放时，写入歌词内容
                        if window.begin_lyrics_update(song_id) {
                            match ncmapi.get_lyrics(si).await {
                                Ok(lrc) => {
                                    debug!("获取歌词：{:?}", lrc);
                                    window.update_lyrics(song_id, lrc);
                                }
                                Err(e) => {
                                    debug!("{}", e);
                                    window.lyrics_update_failed(song_id);
                                }
                            }
                        }
                    }
                    // 更新歌词高亮位置
                    window.update_lyrics_timestamp(time);
                });
            }
            Action::LoadSongComments { song_id, offset } => {
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    if song_id == 0 || !window.begin_comments_update(song_id, offset) {
                        return;
                    }

                    match ncmapi.song_comments(song_id, offset).await {
                        Ok(comments) => {
                            let missing_reply_count_ids =
                                missing_comment_reply_count_ids(&comments);
                            window.update_comments(song_id, offset, comments);
                            update_comment_reply_counts_after_render(
                                ncmapi,
                                window,
                                song_id,
                                missing_reply_count_ids,
                            )
                            .await;
                        }
                        Err(err) => {
                            debug!("获取评论失败: {:?}", err);
                            window.comments_update_failed(song_id, offset);
                        }
                    }
                });
            }
            Action::LikeSongComment {
                song_id,
                comment_id,
                like,
            } => {
                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    match ncmapi.song_comment_like(song_id, comment_id, like).await {
                        Ok(()) => {
                            window.update_comment_like(song_id, comment_id, like);
                        }
                        Err(err) => {
                            debug!("评论点赞失败: {:?}", err);
                            window.comment_action_failed(song_id, comment_id);
                            sender
                                .send(Action::AddToast(if like {
                                    gettext("Comment like failed!")
                                } else {
                                    gettext("Comment unlike failed!")
                                }))
                                .await
                                .unwrap();
                        }
                    }
                });
            }
            Action::LoadSongCommentReplies {
                song_id,
                comment_id,
            } => {
                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    match ncmapi.song_comment_replies(song_id, comment_id).await {
                        Ok(replies) => {
                            window.update_comment_replies(song_id, comment_id, replies);
                        }
                        Err(err) => {
                            debug!("加载评论回复失败: {:?}", err);
                            window.comment_action_failed(song_id, comment_id);
                            sender
                                .send(Action::AddToast(gettext("Failed to load comment replies!")))
                                .await
                                .unwrap();
                        }
                    }
                });
            }
            Action::ReplySongComment {
                song_id,
                comment_id,
                content,
            } => {
                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    match ncmapi
                        .reply_song_comment(song_id, comment_id, &content)
                        .await
                    {
                        Ok(sent_reply) => {
                            window.comment_reply_sent(song_id, comment_id, sent_reply);
                            sender
                                .send(Action::AddToast(gettext("Comment sent!")))
                                .await
                                .unwrap();
                        }
                        Err(err) => {
                            debug!("发送评论回复失败: {:?}", err);
                            window.comment_action_failed(song_id, comment_id);
                            sender
                                .send(Action::AddToast(format!("发送评论失败：{err}")))
                                .await
                                .unwrap();
                        }
                    }
                });
            }
            Action::DeleteSongComment {
                song_id,
                parent_comment_id,
                comment_id,
            } => {
                let sender = imp.sender.clone();
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    match ncmapi.delete_song_comment(song_id, comment_id).await {
                        Ok(()) => {
                            window.comment_deleted(song_id, parent_comment_id, comment_id);
                            sender
                                .send(Action::AddToast(gettext("Comment deleted!")))
                                .await
                                .unwrap();
                        }
                        Err(err) => {
                            debug!("删除评论失败: {:?}", err);
                            window.comment_action_failed(song_id, comment_id);
                            sender
                                .send(Action::AddToast(format!("删除评论失败：{err}")))
                                .await
                                .unwrap();
                        }
                    }
                });
            }
            Action::UpdatePlayListStatus(index) => {
                window.update_playlist_status(index);
            }
            Action::RemoveFromPlayList(song_info) => {
                MAINCONTEXT.spawn_local_with_priority(Priority::DEFAULT_IDLE, async move {
                    window.remove_from_playlist(song_info);
                });
            }
            Action::GstDurationChanged(sec) => {
                window.gst_duration_changed(sec);
            }
            Action::GstStateChanged(state) => {
                window.gst_state_changed(state);
            }
            Action::GstVolumeChanged(volume) => {
                window.gst_volume_changed(volume);
            }
            Action::GstCacheDownloadComplete(loc) => {
                window.gst_cache_download_complete(loc);
            }
            Action::ScaleSeekUpdate(sec) => {
                window.scale_seek_update(sec);
            }
            Action::ScaleValueUpdate => {
                window.scale_value_update();
            }

            Action::PageBack => {
                window.page_back();
            }
            Action::InitMpris(mpris) => {
                window.init_mpris(mpris);
            }
        }
        glib::ControlFlow::Continue
    }

    fn setup_gactions(&self) {
        let preferences_action = gio::SimpleAction::new("preferences", None);
        preferences_action.connect_activate(clone!(
            #[weak(rename_to = app)]
            self,
            move |_, _| {
                app.show_prefrerences();
            }
        ));
        self.add_action(&preferences_action);

        let quit_action = gio::SimpleAction::new("quit", None);
        quit_action.connect_activate(clone!(
            #[weak(rename_to = app)]
            self,
            move |_, _| {
                app.quit();
            }
        ));
        self.add_action(&quit_action);

        let about_action = gio::SimpleAction::new("about", None);
        about_action.connect_activate(clone!(
            #[weak(rename_to = app)]
            self,
            move |_, _| {
                app.show_about();
            }
        ));
        self.add_action(&about_action);
    }

    fn show_prefrerences(&self) {
        let window = self.active_window().unwrap();
        let preferences = NeteaseCloudMusicLinuxPreferences::new();

        let (size, unit) = crate::path::get_cache_size();
        preferences.set_cache_size_label(size, unit);

        preferences.present(Some(&window));
    }

    fn show_about(&self) {
        let window = self.active_window().unwrap();
        let dialog = adw::AboutDialog::builder()
            .application_name(crate::APP_NAME)
            .application_icon(crate::APP_ICON)
            .version(VERSION)
            .developer_name("b1ngggg")
            .comments(
                "基于 Rust + GTK 和 NetEase Cloud Music Gtk 二次开发。\n\n\
                 重构界面 UI，新增评论区，并优化歌词页、播放队列、播放条和大列表性能。",
            )
            .build();
        dialog.add_link(
            "项目主页",
            "https://github.com/b1ngggg/netease-cloud-music-linux",
        );
        dialog.add_link(
            "原项目",
            "https://github.com/gmg137/netease-cloud-music-gtk",
        );
        dialog.add_acknowledgement_section(
            Some("鸣谢"),
            &[
                "b1ngggg",
                "gmg137",
                "catsout",
                "fplust",
                "outloudvi",
                "CyrusYip",
                "Integral-Tech",
                "NOBLES5E",
                "atzlinux",
                "mokurin000",
                "onlymash",
                "An-n-ya",
                "MarvelousBlack",
                "langzime",
                "h0cheung",
                "weilinfox",
                "xyzhou-1",
                "liuyujielol",
                "heddxh",
                "fungaren",
                "xen0n",
                "arkuna23",
                "wngtk",
                "zyw271828",
            ],
        );

        dialog.present(Some(&window));
    }

    fn setup_cache_clear(&self) {
        let sender = self.imp().sender.clone();
        let settings = Settings::new(crate::APP_ID);
        let cache_clear = settings.uint("cache-clear");
        let flag = settings.boolean("cache-clear-flag");
        let cache_path = CACHE.clone();
        MAINCONTEXT.spawn_local_with_priority(Priority::LOW, async move {
            match cache_clear {
                1 => {
                    if remove_all_file(cache_path).is_ok() {
                        sender
                            .send(Action::AddToast(gettext("Cache cleared.")))
                            .await
                            .unwrap();
                    }
                }
                2 => {
                    if let Ok(datetime) = glib::DateTime::now_local() {
                        if datetime.day_of_week() == 1 && !flag {
                            if remove_all_file(cache_path).is_ok() {
                                sender
                                    .send(Action::AddToast(gettext("Cache cleared.")))
                                    .await
                                    .unwrap();
                            }
                            settings.set_boolean("cache-clear-flag", true).unwrap();
                        } else if datetime.day_of_week() != 1 {
                            settings.set_boolean("cache-clear-flag", false).unwrap();
                        }
                    }
                }
                3 => {
                    if let Ok(datetime) = glib::DateTime::now_local() {
                        if datetime.day_of_month() == 1 && !flag {
                            if remove_all_file(cache_path).is_ok() {
                                sender
                                    .send(Action::AddToast(gettext("Cache cleared.")))
                                    .await
                                    .unwrap();
                            }
                            settings.set_boolean("cache-clear-flag", true).unwrap();
                        } else if datetime.day_of_month() != 1 {
                            settings.set_boolean("cache-clear-flag", false).unwrap();
                        }
                    }
                }
                _ => {
                    settings.set_boolean("cache-clear-flag", false).unwrap();
                }
            }
        });
    }
}

impl Default for NeteaseCloudMusicLinuxApplication {
    fn default() -> Self {
        gio::Application::default()
            .expect("Could not get default GApplication")
            .downcast()
            .unwrap()
    }
}

fn remove_all_file(path: PathBuf) -> anyhow::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}
