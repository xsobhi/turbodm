//! A torrent's details: from its .torrent file (behind a link, or on this computer), or from
//! peers for a magnet link. Nothing is downloaded or written yet.

use super::is_magnet;
use crate::engine::http::{self, Headers};
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, ListOnlyResponse, Session};
use std::net::SocketAddr;
use std::sync::Arc;

/// .torrent files are kilobytes; even one listing many thousands of files is a few MB.
const MAX_TORRENT_FILE: u64 = 32 << 20;

/// What a torrent is.
pub struct Meta {
    pub bytes: Vec<u8>, // its .torrent file
    pub info_hash: String,
    pub name: String,      // its single file, or the folder for its files
    pub main_file: String, // its biggest file's name: what kind of download it is
    pub size: u64,
    pub files: usize,
    pub peers: Vec<SocketAddr>, // met while asking for the details: asked first for the data
}

/// Get a torrent's details from wherever `url` points.
pub async fn fetch(session: &Arc<Session>, url: &str, headers: &Headers, client: &reqwest::Client)
                   -> Result<Meta, String> {
    let url = url.trim();
    let add = if is_magnet(url) {
        AddTorrent::from_url(url.to_string())
    } else if url.starts_with("http://") || url.starts_with("https://") {
        AddTorrent::from_bytes(download(client, url, headers).await?)
    } else {
        AddTorrent::from_bytes(read(url).await?)
    };
    let only_look = AddTorrentOptions { list_only: true, ..Default::default() };
    match session.add_torrent(add, Some(only_look)).await {
        Ok(AddTorrentResponse::ListOnly(listed)) => meta(listed),
        Ok(_) => Err("The torrent was added instead of looked at".into()), // not with list_only
        Err(err) => Err(explain(&format!("{err:#}"))),
    }
}

/// A link to a .torrent file, fetched like any download (with the browser's cookies, which
/// sites that need a login want).
async fn download(client: &reqwest::Client, url: &str, headers: &Headers) -> Result<Vec<u8>, String> {
    let resp = http::open(client, url, headers, None).await.map_err(|e| format!("Cannot get the torrent file: {e}"))?;
    if resp.content_length().is_some_and(|len| len > MAX_TORRENT_FILE) {
        return Err("That's not a torrent file (it's too big)".into());
    }
    let bytes = resp.bytes().await.map_err(|e| format!("Cannot get the torrent file: {}", http::FetchError::from(e)))?;
    Ok(bytes.to_vec())
}

async fn read(path: &str) -> Result<Vec<u8>, String> {
    let size = tokio::fs::metadata(path).await.map_err(|e| format!("Cannot read the torrent file: {e}"))?.len();
    if size > MAX_TORRENT_FILE {
        return Err("That's not a torrent file (it's too big)".into());
    }
    tokio::fs::read(path).await.map_err(|e| format!("Cannot read the torrent file: {e}"))
}

fn meta(listed: ListOnlyResponse) -> Result<Meta, String> {
    let info = &listed.info;
    let files: Vec<(String, u64)> = info.iter_file_details()
        .filter(|file| !file.attrs().padding) // filler between files, never written
        .map(|file| (file.filename.to_pathbuf().to_string_lossy().into_owned(), file.len))
        .collect();
    let (biggest, _) = files.iter().max_by_key(|(_, len)| *len).ok_or("The torrent has no files")?;
    let main_file = biggest.rsplit(['/', '\\']).next().unwrap_or(biggest).to_string();
    // one file: the name it's written under; more: the folder they go in
    let name = match (files.len(), info.name()) {
        (1, _) => files[0].0.clone(),
        (_, Some(name)) => name.into_owned(),
        _ => listed.info_hash.as_string(),
    };
    Ok(Meta {
        bytes: listed.torrent_bytes.to_vec(),
        info_hash: listed.info_hash.as_string(),
        name,
        main_file,
        size: info.lengths().total_length(),
        files: files.len(),
        peers: listed.seen_peers,
    })
}

/// librqbit's words, in the user's.
fn explain(err: &str) -> String {
    if err.contains("no way to discover torrent metainfo") || err.contains("no known way to resolve peers") {
        "No peers found for this magnet link (it may have no trackers, or nobody shares it)".into()
    } else if err.contains("error decoding torrent") {
        "That's not a torrent file (it can't be read as one)".into()
    } else if err.contains("not a valid magnet") || err.contains("infohash") || err.contains("info hash") {
        "That magnet link isn't valid".into()
    } else {
        err.lines().next().unwrap_or(err).to_string()
    }
}
