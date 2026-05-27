//
// ncmapi.rs
// Copyright (C) 2022 gmg137 <gmg137 AT live.com>
// Distributed under terms of the GPL-3.0-or-later license.
//
use anyhow::{Context, Error, Result, bail};
use base64::{Engine as _, engine::general_purpose};
use cookie_store::{CookieStore, serde};
use isahc::{HttpClient, Request, prelude::*};
use ncm_api::{CookieBuilder, CookieJar, MusicApi, PlayListDetail, SongInfo, SongUrl};
use openssl::{
    rand::rand_bytes,
    rsa::{Padding, Rsa},
    symm::{Cipher, encrypt},
};
use serde_json::Value;
use urlqstring::QueryParams;

use crate::path::{CACHE, LYRICS};
use log::{debug, error, warn};
use std::{collections::HashMap, fs, io, path::PathBuf, time::Duration};

const COOKIE_FILE: &str = "cookies.json";
const MAX_CONS: usize = 32;
const TIMEOUT: u64 = 100;
const PLAYLIST_DETAIL_LIMIT: usize = 100_000;
const SONG_DETAIL_BATCH_SIZE: usize = 500;
const NCM_WEB_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/51.0.2704.103 Safari/537.36";
pub const SONG_COMMENT_LIMIT: u16 = 20;
const SONG_COMMENT_REPLY_LIMIT: u16 = 100;
const COMMENT_RESOURCE_PREFIX: &str = "R_SO_4_";
const WEAPI_IV: &[u8] = b"0102030405060708";
const WEAPI_PRESET_KEY: &[u8] = b"0CoJUm6Qyw8W8jud";
const WEAPI_BASE62: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const WEAPI_RSA_PUBLIC_KEY: &[u8] = b"-----BEGIN PUBLIC KEY-----\nMIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQDgtQn2JZ34ZC28NWYpAUd98iZ37BUrX/aKzmFbt7clFSs6sXqHauqKWqdtLkF2KexO40H1YTX8z2lSgBBOAxLsvaklV8k4cBFK9snQXE9/DDaFt6Rr7iVZMldczhC0JNgTz+SHXT6CBHuX3e9SdB1Ua44oncaTWz7OBGLbCiK45wIDAQAB\n-----END PUBLIC KEY-----";

pub const BASE_URL_LIST: [&str; 12] = [
    "https://music.163.com/",
    "https://music.163.com/eapi/clientlog",
    "https://music.163.com/eapi/feedback",
    "https://music.163.com/api/clientlog",
    "https://music.163.com/api/feedback",
    "https://music.163.com/neapi/clientlog",
    "https://music.163.com/neapi/feedback",
    "https://music.163.com/weapi/clientlog",
    "https://music.163.com/weapi/feedback",
    "https://music.163.com/wapi/clientlog",
    "https://music.163.com/wapi/feedback",
    "https://music.163.com/openapi/clientlog",
];

fn value_as_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

fn playlist_track_ids(value: &Value) -> Vec<u64> {
    value
        .get("playlist")
        .and_then(|playlist| playlist.get("trackIds"))
        .and_then(Value::as_array)
        .map(|track_ids| {
            track_ids
                .iter()
                .filter_map(|track| track.get("id").and_then(value_as_u64))
                .collect()
        })
        .unwrap_or_default()
}

fn value_as_string(value: &Value) -> String {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .or_else(|| value.as_u64().map(|value| value.to_string()))
        .or_else(|| value.as_i64().map(|value| value.to_string()))
        .unwrap_or_default()
}

fn value_child_string(value: &Value, key: &str) -> String {
    value.get(key).map(value_as_string).unwrap_or_default()
}

fn value_as_bool(value: &Value) -> bool {
    value
        .as_bool()
        .or_else(|| value.as_i64().map(|value| value != 0))
        .or_else(|| value.as_u64().map(|value| value != 0))
        .or_else(|| {
            value
                .as_str()
                .map(|value| value.eq_ignore_ascii_case("true") || value == "1" || value == "like")
        })
        .unwrap_or_default()
}

fn parse_comment_reply(value: &Value) -> CommentReply {
    let user = value.get("user").unwrap_or(&Value::Null);
    let replied_to = value
        .get("beReplied")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .unwrap_or(&Value::Null);
    let replied_to_user = replied_to.get("user").unwrap_or(&Value::Null);
    CommentReply {
        comment_id: value
            .get("commentId")
            .or_else(|| value.get("beRepliedCommentId"))
            .and_then(value_as_u64)
            .unwrap_or_default(),
        user_id: user
            .get("userId")
            .and_then(value_as_u64)
            .unwrap_or_default(),
        nickname: value_child_string(user, "nickname"),
        avatar_url: value_child_string(user, "avatarUrl"),
        replied_to_nickname: value_child_string(replied_to_user, "nickname"),
        replied_to_content: value_child_string(replied_to, "content"),
        content: value_child_string(value, "content"),
        time: value_child_string(value, "timeStr"),
        liked: value.get("liked").map(value_as_bool).unwrap_or_default(),
        liked_count: value
            .get("likedCount")
            .and_then(value_as_u64)
            .unwrap_or_default(),
    }
}

fn parse_sent_comment_reply(value: &Value) -> Option<CommentReply> {
    let comment = value
        .get("comment")
        .or_else(|| value.get("data").and_then(|data| data.get("comment")))?;
    let reply = parse_comment_reply(comment);
    (!reply.content.is_empty() && reply.comment_id != 0).then_some(reply)
}

fn parse_comment(value: &Value) -> SongComment {
    let user = value.get("user").unwrap_or(&Value::Null);
    let replied: Vec<CommentReply> = value
        .get("beReplied")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(parse_comment_reply).collect())
        .unwrap_or_default();
    let reply_count = parse_comment_reply_count(value, 0);

    SongComment {
        comment_id: value
            .get("commentId")
            .and_then(value_as_u64)
            .unwrap_or_default(),
        user_id: user
            .get("userId")
            .and_then(value_as_u64)
            .unwrap_or_default(),
        nickname: value_child_string(user, "nickname"),
        avatar_url: value_child_string(user, "avatarUrl"),
        content: value_child_string(value, "content"),
        time: value_child_string(value, "timeStr"),
        liked: value.get("liked").map(value_as_bool).unwrap_or_default(),
        liked_count: value
            .get("likedCount")
            .and_then(value_as_u64)
            .unwrap_or_default(),
        reply_count,
        replied,
    }
}

fn parse_comment_reply_count(value: &Value, fallback: u64) -> u64 {
    let direct = first_u64_child(
        value,
        &[
            "replyCount",
            "totalCount",
            "commentCount",
            "showReplyCount",
            "floorCommentCount",
        ],
    );
    let floor = value.get("showFloorComment").unwrap_or(&Value::Null);
    let floor_count = first_u64_child(
        floor,
        &[
            "replyCount",
            "totalCount",
            "commentCount",
            "showReplyCount",
            "floorCommentCount",
        ],
    )
    .or_else(|| {
        floor
            .get("comments")
            .and_then(Value::as_array)
            .map(|items| items.len() as u64)
    });

    direct
        .or(floor_count)
        .unwrap_or_else(|| if !floor.is_null() { 1 } else { fallback })
        .max(fallback)
}

fn first_u64_child(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(value_as_u64))
}

fn parse_comments(value: &Value, key: &str) -> Vec<SongComment> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| items.iter().map(parse_comment).collect())
        .unwrap_or_default()
}

fn parse_song_comments(value: &Value) -> SongComments {
    SongComments {
        total: value
            .get("total")
            .and_then(value_as_u64)
            .unwrap_or_default(),
        hot_comments: parse_comments(value, "hotComments"),
        comments: parse_comments(value, "comments"),
    }
}

fn song_comment_thread_id(song_id: u64) -> String {
    format!("{COMMENT_RESOURCE_PREFIX}{song_id}")
}

fn should_skip_comment_write_fallback(err: &Error) -> bool {
    let message = format!("{err:#}");
    message.contains("code 250")
        || message.contains("操作过于频繁")
        || message.contains("为了您的安全")
        || message.contains("切换设备")
}

fn weapi_aes_base64(data: &str, key: &[u8]) -> Result<String> {
    let cipher_text = encrypt(Cipher::aes_128_cbc(), key, Some(WEAPI_IV), data.as_bytes())?;
    Ok(general_purpose::STANDARD.encode(cipher_text))
}

fn weapi_rsa_encrypt(data: &str) -> Result<String> {
    let rsa = Rsa::public_key_from_pem(WEAPI_RSA_PUBLIC_KEY)?;
    let prefix = vec![0u8; 128usize.saturating_sub(data.len())];
    let data = [&prefix[..], data.as_bytes()].concat();
    let mut buf = vec![0; rsa.size() as usize];
    let size = rsa.public_encrypt(&data, &mut buf, Padding::NONE)?;
    buf.truncate(size);
    Ok(hex::encode(buf))
}

fn weapi_body(text: &str) -> Result<String> {
    let mut secret_key = [0u8; 16];
    rand_bytes(&mut secret_key)?;
    let key: Vec<u8> = secret_key
        .iter()
        .map(|value| WEAPI_BASE62[usize::from(value % 62)])
        .collect();

    let first = weapi_aes_base64(text, WEAPI_PRESET_KEY)?;
    let params = weapi_aes_base64(&first, &key)?;
    let reversed_key: Vec<u8> = key.iter().rev().copied().collect();
    let enc_sec_key = weapi_rsa_encrypt(std::str::from_utf8(&reversed_key)?)?;

    Ok(QueryParams::from(vec![
        ("params", params.as_str()),
        ("encSecKey", enc_sec_key.as_str()),
    ])
    .stringify())
}

#[derive(Debug, Clone, Default)]
pub struct CommentReply {
    pub comment_id: u64,
    pub user_id: u64,
    pub nickname: String,
    pub avatar_url: String,
    pub replied_to_nickname: String,
    pub replied_to_content: String,
    pub content: String,
    pub time: String,
    pub liked: bool,
    pub liked_count: u64,
}

#[derive(Debug, Clone, Default)]
pub struct SongComment {
    pub comment_id: u64,
    pub user_id: u64,
    pub nickname: String,
    pub avatar_url: String,
    pub content: String,
    pub time: String,
    pub liked: bool,
    pub liked_count: u64,
    pub reply_count: u64,
    pub replied: Vec<CommentReply>,
}

#[derive(Debug, Clone, Default)]
pub struct SongComments {
    pub total: u64,
    pub hot_comments: Vec<SongComment>,
    pub comments: Vec<SongComment>,
}

#[derive(Debug, Clone, Default)]
pub struct SongCommentReplies {
    pub total: u64,
    pub replies: Vec<CommentReply>,
}

#[derive(Clone)]
pub struct NcmClient {
    pub client: MusicApi,
}

impl NcmClient {
    pub fn new() -> Self {
        Self {
            client: MusicApi::new(MAX_CONS),
        }
    }

    pub fn from_cookie_jar(cookie_jar: CookieJar) -> Self {
        Self {
            client: MusicApi::from_cookie_jar(cookie_jar, MAX_CONS),
        }
    }

    pub fn set_proxy(&mut self, proxy: String) -> Result<()> {
        self.client.set_proxy(&proxy)
    }

    fn http_client(&self) -> Result<HttpClient> {
        let mut client_builder = HttpClient::builder()
            .timeout(Duration::from_secs(TIMEOUT))
            .max_connections(MAX_CONS)
            .cookies();
        if let Some(cookie_jar) = self.client.cookie_jar() {
            client_builder = client_builder.cookie_jar(cookie_jar.clone());
        }
        Ok(client_builder.build()?)
    }

    fn csrf_token(&self) -> String {
        let Some(cookie_jar) = self.client.cookie_jar() else {
            return String::new();
        };
        let Ok(uri) = "https://music.163.com/".parse() else {
            return String::new();
        };
        cookie_jar
            .get_by_name(&uri, "__csrf")
            .map(|cookie| cookie.value().to_owned())
            .unwrap_or_default()
    }

    fn cookie_header(&self, os: &str) -> String {
        let mut parts = vec![
            format!("os={os}"),
            "appver=2.7.1.198277".to_owned(),
            "__remember_me=true".to_owned(),
        ];
        let mut cookie_values = HashMap::new();
        if let Some(cookie_jar) = self.client.cookie_jar() {
            for base_url in BASE_URL_LIST {
                if let Ok(uri) = base_url.parse() {
                    for cookie in cookie_jar.get_for_uri(&uri) {
                        if matches!(cookie.name(), "os" | "appver" | "__remember_me") {
                            continue;
                        }
                        cookie_values
                            .entry(cookie.name().to_owned())
                            .or_insert_with(|| cookie.value().to_owned());
                    }
                }
            }
        }
        for name in ["MUSIC_U", "MUSIC_A_T", "MUSIC_R_T", "__csrf", "NMTID"] {
            if let Some(value) = cookie_values.remove(name) {
                parts.push(format!("{name}={value}"));
            }
        }
        let mut remaining = cookie_values.into_iter().collect::<Vec<_>>();
        remaining.sort_by(|left, right| left.0.cmp(&right.0));
        for (name, value) in remaining {
            parts.push(format!("{name}={value}"));
        }
        parts.join("; ")
    }

    async fn weapi_post_json(
        &self,
        path: &str,
        mut params: HashMap<String, String>,
        os: &str,
    ) -> Result<Value> {
        let csrf = self.csrf_token();
        params.insert("csrf_token".to_owned(), csrf.clone());
        let params_json = serde_json::to_string(&params)?;
        let body = weapi_body(&params_json)?;
        let url = format!("https://music.163.com{path}?csrf_token={csrf}");
        let request = Request::post(&url)
            .header("Cookie", self.cookie_header(os))
            .header("Accept", "*/*")
            .header("Accept-Language", "en-US,en;q=0.5")
            .header("Connection", "keep-alive")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Host", "music.163.com")
            .header("Referer", "https://music.163.com")
            .header("User-Agent", NCM_WEB_USER_AGENT)
            .body(body)?;
        let mut response = self.http_client()?.send_async(request).await?;
        let text = response.text().await?;
        let json = serde_json::from_str::<Value>(&text)
            .with_context(|| format!("网易云接口返回非 JSON: {text}"))?;
        let code = json.get("code").and_then(value_as_u64).unwrap_or(200);
        if code != 200 {
            let message = value_child_string(&json, "message");
            let message = if message.is_empty() {
                value_child_string(&json, "msg")
            } else {
                message
            };
            bail!("网易云接口返回 code {code}: {message}");
        }
        Ok(json)
    }

    pub fn get_api_rate(item: u32) -> u32 {
        match item {
            0 => 128000,
            1 => 192000,
            2 => 320000,
            3 => 999000,
            4 => 1900000,
            _ => 320000,
        }
    }

    /*
    pub fn set_cookie_jar_to_global(&self) {
        if let Some(cookie_jar) = self.client.cookie_jar() {
            match COOKIE_JAR.get() {
                Some(global_jar) => {
                    for base_url in BASE_URL_LIST {
                        let url = base_url.parse().unwrap();
                        cookie_jar.get_for_uri(&url).into_iter().for_each(|c| {
                            global_jar.set(c, &url).unwrap();
                        });
                    }
                }
                None => {
                    COOKIE_JAR.set(cookie_jar.to_owned()).unwrap();
                }
            }
        }
    }
    */

    pub fn cookie_file_path() -> PathBuf {
        crate::path::DATA.clone().join(COOKIE_FILE)
    }

    pub fn load_cookie_jar_from_file() -> Option<CookieJar> {
        match fs::File::open(Self::cookie_file_path()) {
            Err(err) => match err.kind() {
                io::ErrorKind::NotFound => (),
                other => error!("{:?}", other),
            },
            Ok(file) => match serde::json::load(io::BufReader::new(file)) {
                Err(err) => error!("{:?}", err),
                Ok(cookie_store) => {
                    let cookie_jar = CookieJar::default();
                    for base_url in BASE_URL_LIST {
                        let url = base_url.parse().unwrap();
                        for c in cookie_store.matches(&url) {
                            let cookie = CookieBuilder::new(c.name(), c.value())
                                .domain("music.163.com")
                                .path(c.path().unwrap_or("/"))
                                .build()
                                .unwrap();
                            cookie_jar.set(cookie, &base_url.parse().unwrap()).unwrap();
                        }
                    }
                    return Some(cookie_jar);
                }
            },
        };
        None
    }

    pub fn save_cookie_jar_to_file(&self) {
        if let Some(cookie_jar) = self.client.cookie_jar() {
            match fs::File::create(Self::cookie_file_path()) {
                Err(err) => error!("{:?}", err),
                Ok(mut file) => {
                    let mut cookie_store = CookieStore::default();
                    for base_url in BASE_URL_LIST {
                        let uri = &base_url.parse().unwrap();
                        let url = &base_url.parse().unwrap();
                        for c in cookie_jar.get_for_uri(url) {
                            let cookie = cookie_store::Cookie::parse(
                                format!(
                                    "{}={}; Path={}; Domain=music.163.com; Max-Age=31536000",
                                    c.name(),
                                    c.value(),
                                    url.path()
                                ),
                                uri,
                            )
                            .unwrap();
                            cookie_store.insert(cookie, uri).unwrap();
                        }
                    }
                    serde::json::save(&cookie_store, &mut file).unwrap();
                }
            }
        }
    }

    pub fn clean_cookie_file() {
        if let Err(err) = fs::remove_file(crate::path::DATA.clone().join(COOKIE_FILE)) {
            match err.kind() {
                io::ErrorKind::NotFound => (),
                other => error!("{:?}", other),
            }
        }
    }

    pub async fn create_qrcode(&self) -> Result<(PathBuf, String)> {
        let (qr_url, unikey) = self.client.login_qr_create().await?;
        let mut path = CACHE.clone();
        path.push("qrimage.png");
        qrcode_generator::to_png_to_file(qr_url, qrcode_generator::QrCodeEcc::Low, 140, &path)?;
        Ok((path, unikey))
    }

    pub async fn songs_url(&self, ids: &[u64], rate: u32) -> Result<Vec<SongUrl>> {
        self.client
            .songs_url(ids, &Self::get_api_rate(rate).to_string())
            .await
    }

    pub async fn song_comments(&self, song_id: u64, offset: u32) -> Result<SongComments> {
        match self.song_comments_weapi(song_id, offset).await {
            Ok(comments) => Ok(comments),
            Err(err) => {
                debug!("新版评论列表接口失败，回退旧接口: {:?}", err);
                self.song_comments_legacy(song_id, offset).await
            }
        }
    }

    async fn song_comments_weapi(&self, song_id: u64, offset: u32) -> Result<SongComments> {
        let thread_id = song_comment_thread_id(song_id);
        let mut params = HashMap::new();
        params.insert("rid".to_owned(), thread_id.clone());
        params.insert("threadId".to_owned(), thread_id.clone());
        params.insert("limit".to_owned(), SONG_COMMENT_LIMIT.to_string());
        params.insert("offset".to_owned(), offset.to_string());
        params.insert("beforeTime".to_owned(), "0".to_owned());
        params.insert("commentId".to_owned(), "0".to_owned());
        params.insert("showInner".to_owned(), "true".to_owned());
        params.insert("markReplied".to_owned(), "true".to_owned());
        params.insert("forceFlatComment".to_owned(), "false".to_owned());
        params.insert("compareUserLocation".to_owned(), "false".to_owned());
        params.insert("total".to_owned(), (offset == 0).to_string());
        let json = self
            .weapi_post_json(
                &format!("/weapi/v1/resource/comments/{thread_id}"),
                params,
                "pc",
            )
            .await?;
        Ok(parse_song_comments(&json))
    }

    async fn song_comments_legacy(&self, song_id: u64, offset: u32) -> Result<SongComments> {
        let url = format!(
            "https://music.163.com/api/v1/resource/comments/R_SO_4_{song_id}?limit={SONG_COMMENT_LIMIT}&offset={offset}"
        );
        let client = self.http_client()?;

        let request = Request::get(&url)
            .header("Accept", "*/*")
            .header("Accept-Language", "en-US,en;q=0.5")
            .header("Connection", "keep-alive")
            .header("Host", "music.163.com")
            .header("Referer", "https://music.163.com")
            .header("User-Agent", NCM_WEB_USER_AGENT)
            .body(())?;
        let mut response = client.send_async(request).await?;
        let text = response.text().await?;
        let json = serde_json::from_str::<Value>(&text)?;
        Ok(parse_song_comments(&json))
    }

    pub async fn song_comment_like(&self, song_id: u64, comment_id: u64, like: bool) -> Result<()> {
        let mut params = HashMap::new();
        params.insert("threadId".to_owned(), song_comment_thread_id(song_id));
        params.insert("commentId".to_owned(), comment_id.to_string());
        let action = if like { "like" } else { "unlike" };
        self.weapi_post_json(&format!("/weapi/v1/comment/{action}"), params, "pc")
            .await?;
        Ok(())
    }

    pub async fn reply_song_comment(
        &self,
        song_id: u64,
        comment_id: u64,
        content: &str,
    ) -> Result<Option<CommentReply>> {
        let mut params = HashMap::new();
        params.insert("threadId".to_owned(), song_comment_thread_id(song_id));
        params.insert("commentId".to_owned(), comment_id.to_string());
        params.insert("content".to_owned(), content.to_owned());
        let json = self
            .weapi_post_json("/weapi/resource/comments/reply", params, "pc")
            .await?;
        Ok(parse_sent_comment_reply(&json))
    }

    pub async fn delete_song_comment(&self, song_id: u64, comment_id: u64) -> Result<()> {
        let mut params = HashMap::new();
        params.insert("threadId".to_owned(), song_comment_thread_id(song_id));
        params.insert("commentId".to_owned(), comment_id.to_string());
        match self
            .weapi_post_json("/weapi/resource/comments/delete", params.clone(), "pc")
            .await
        {
            Ok(_) => Ok(()),
            Err(first_err) if should_skip_comment_write_fallback(&first_err) => Err(first_err),
            Err(first_err) => {
                match self
                    .weapi_post_json("/weapi/resource/comments/delete", params, "android")
                    .await
                {
                    Ok(_) => Ok(()),
                    Err(second_err) => bail!("{first_err}; Android 备用请求失败: {second_err}"),
                }
            }
        }
    }

    pub async fn song_comment_replies(
        &self,
        song_id: u64,
        comment_id: u64,
    ) -> Result<SongCommentReplies> {
        self.song_comment_replies_with_limit(song_id, comment_id, SONG_COMMENT_REPLY_LIMIT)
            .await
    }

    pub async fn song_comment_reply_count(&self, song_id: u64, comment_id: u64) -> Result<u64> {
        self.song_comment_replies_with_limit(song_id, comment_id, 1)
            .await
            .map(|replies| replies.total)
    }

    async fn song_comment_replies_with_limit(
        &self,
        song_id: u64,
        comment_id: u64,
        limit: u16,
    ) -> Result<SongCommentReplies> {
        let mut params = HashMap::new();
        params.insert("parentCommentId".to_owned(), comment_id.to_string());
        params.insert("threadId".to_owned(), song_comment_thread_id(song_id));
        params.insert("time".to_owned(), "-1".to_owned());
        params.insert("limit".to_owned(), limit.to_string());
        let json = self
            .weapi_post_json("/weapi/resource/comment/floor/get", params, "pc")
            .await?;
        let data = json.get("data").unwrap_or(&json);
        let replies = data
            .get("comments")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(parse_comment_reply)
                    .filter(|reply| !reply.content.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let total = data
            .get("totalCount")
            .or_else(|| data.get("total"))
            .and_then(value_as_u64)
            .unwrap_or(replies.len() as u64);
        Ok(SongCommentReplies { total, replies })
    }

    pub async fn song_list_detail_unlimited(&self, songlist_id: u64) -> Result<PlayListDetail> {
        let url = format!(
            "https://music.163.com/api/v6/playlist/detail?id={songlist_id}&offset=0&total=true&limit={PLAYLIST_DETAIL_LIMIT}&n={PLAYLIST_DETAIL_LIMIT}&s=8"
        );
        let mut client_builder = HttpClient::builder()
            .timeout(Duration::from_secs(TIMEOUT))
            .max_connections(MAX_CONS)
            .cookies();
        if let Some(cookie_jar) = self.client.cookie_jar() {
            client_builder = client_builder.cookie_jar(cookie_jar.clone());
        }
        let client = client_builder.build()?;

        let request = Request::get(&url)
            .header("Accept", "*/*")
            .header("Accept-Language", "en-US,en;q=0.5")
            .header("Connection", "keep-alive")
            .header("Host", "music.163.com")
            .header("Referer", "https://music.163.com")
            .header("User-Agent", NCM_WEB_USER_AGENT)
            .body(())?;
        let mut response = client.send_async(request).await?;
        let text = response.text().await?;
        let json = serde_json::from_str::<Value>(&text)?;
        let track_ids = playlist_track_ids(&json);
        let mut detail = ncm_api::to_mix_detail(&json)?;
        self.complete_playlist_tracks(&mut detail, track_ids)
            .await?;
        Ok(detail)
    }

    pub async fn song_list_detail_complete(&self, songlist_id: u64) -> Result<PlayListDetail> {
        match self.song_list_detail_unlimited(songlist_id).await {
            Ok(detail) => Ok(detail),
            Err(err) => {
                warn!(
                    "获取完整歌单详情失败，回退到默认详情接口，歌单 {}: {:?}",
                    songlist_id, err
                );
                self.client.song_list_detail(songlist_id).await
            }
        }
    }

    async fn complete_playlist_tracks(
        &self,
        detail: &mut PlayListDetail,
        track_ids: Vec<u64>,
    ) -> Result<()> {
        if track_ids.is_empty() {
            return Ok(());
        }

        let mut songs = detail
            .songs
            .drain(..)
            .map(|song| (song.id, song))
            .collect::<HashMap<_, _>>();
        let missing_ids = track_ids
            .iter()
            .copied()
            .filter(|id| !songs.contains_key(id))
            .collect::<Vec<_>>();

        for chunk in missing_ids.chunks(SONG_DETAIL_BATCH_SIZE) {
            for song in self.client.songs_detail(chunk).await? {
                songs.insert(song.id, song);
            }
        }

        detail.songs = track_ids
            .into_iter()
            .filter_map(|id| songs.remove(&id))
            .collect();
        Ok(())
    }

    pub async fn get_lyrics(&self, si: SongInfo) -> Result<Vec<(u64, String)>> {
        // 歌词文件位置
        let mut lyric_path = LYRICS.clone();
        lyric_path.push(format!(
            "{}-{}-{}.lrc",
            si.name.replace('/', "／"),
            si.singer,
            si.album
        ));
        // 翻译歌词文件位置
        let mut tlyric_path = LYRICS.clone();
        tlyric_path.push(format!("{}.tlrc", si.id));
        // 替换歌词时间
        let re = regex::Regex::new(r"\[(?P<min>\d+):(?P<sec>\d{1,2})(?:[.:](?P<frac>\d{1,3}))?\]")?;
        // 修正不正常的时间戳 [00:11:22]
        let re_abnormal_ts = regex::Regex::new(r"^\[(\d+):(\d+):(\d+)\]")?;
        if !lyric_path.exists() {
            if let Ok(lyr) = self.client.song_lyric(si.id).await {
                debug!("歌词: {:?}", lyr);
                // 添加歌词翻译
                let lt = merge_lyrics_with_translation(&lyr.lyric, &lyr.tlyric, &re);
                // 保存歌词文件
                let lyric = lyr
                    .lyric
                    .into_iter()
                    .map(|x| re_abnormal_ts.replace_all(&x, "[$1:$2.$3]").to_string())
                    .collect::<Vec<String>>()
                    .join("\n");
                fs::write(&lyric_path, lyric)?;
                if !lyr.tlyric.is_empty() {
                    // 保存翻译歌词文件
                    let tlyric = lyr
                        .tlyric
                        .into_iter()
                        .map(|x| re_abnormal_ts.replace_all(&x, "[$1:$2.$3]").to_string())
                        .collect::<Vec<String>>()
                        .join("\n");
                    fs::write(&tlyric_path, tlyric)?;
                }
                // 组织歌词+翻译
                Ok(lt)
            } else {
                anyhow::bail!("No lyrics found!")
            }
        } else {
            let lyric = fs::read_to_string(&lyric_path)?;
            let lyrics: Vec<String> = lyric
                .split('\n')
                .collect::<Vec<&str>>()
                .iter()
                .map(|s| s.to_string())
                .collect();
            let mut tlyrics = vec![];
            if tlyric_path.exists() {
                let tlyric = fs::read_to_string(&tlyric_path)?;
                tlyrics = tlyric
                    .split('\n')
                    .collect::<Vec<&str>>()
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
            }
            // 添加歌词翻译
            let lt = merge_lyrics_with_translation(&lyrics, &tlyrics, &re);
            // 组织歌词+翻译
            Ok(lt)
        }
    }
}

fn merge_lyrics_with_translation(
    lyrics: &[String],
    translations: &[String],
    timestamp_re: &regex::Regex,
) -> Vec<(u64, String)> {
    let mut merged = Vec::new();
    for lyric in lyrics {
        let Some((time, text)) = parse_lrc_line(lyric, timestamp_re) else {
            continue;
        };
        merged.push((time, text));

        for translation in translations {
            if parse_lrc_timestamp(translation, timestamp_re) == Some(time)
                && let Some((_, text)) = parse_lrc_line(translation, timestamp_re)
            {
                merged.push((time, text));
            }
        }
    }
    merged
}

fn parse_lrc_line(line: &str, timestamp_re: &regex::Regex) -> Option<(u64, String)> {
    let time = parse_lrc_timestamp(line, timestamp_re)?;
    let mut text = timestamp_re.replace_all(line, "").to_string();
    text.push('\n');
    Some((time, text))
}

fn parse_lrc_timestamp(line: &str, timestamp_re: &regex::Regex) -> Option<u64> {
    let captures = timestamp_re.captures(line)?;
    let minutes = captures.name("min")?.as_str().parse::<u64>().ok()?;
    let seconds = captures.name("sec")?.as_str().parse::<u64>().ok()?;
    let fraction = captures
        .name("frac")
        .map(|m| lrc_fraction_to_millis(m.as_str()))
        .unwrap_or(0);

    Some((minutes * 60 + seconds) * 1000 + fraction)
}

fn lrc_fraction_to_millis(fraction: &str) -> u64 {
    let mut digits = fraction.chars().take(3).collect::<String>();
    while digits.len() < 3 {
        digits.push('0');
    }
    digits.parse::<u64>().unwrap_or(0)
}

impl Default for NcmClient {
    fn default() -> Self {
        Self::new()
    }
}
