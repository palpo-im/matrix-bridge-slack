use std::path::Path;

use anyhow::{Result, anyhow};
use reqwest::Client;
use tracing::{debug, warn};

const MAX_SLACK_FILE_SIZE: usize = 8 * 1024 * 1024;
const MAX_MATRIX_FILE_SIZE: usize = 50 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct MediaInfo {
    pub data: Vec<u8>,
    pub content_type: String,
    pub filename: String,
    pub size: usize,
}

pub struct MediaHandler {
    client: Client,
    homeserver_url: String,
}

impl MediaHandler {
    pub fn new(homeserver_url: &str) -> Self {
        Self {
            client: Client::new(),
            homeserver_url: homeserver_url.to_string(),
        }
    }

    pub async fn download_from_url(&self, url: &str) -> Result<MediaInfo> {
        debug!("downloading media from {}", url);

        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| anyhow!("failed to download from {}: {}", url, e))?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "failed to download from {}: status {}",
                url,
                response.status()
            ));
        }

        let headers = response.headers().clone();
        let raw_content_type = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned);
        let content_disposition = headers
            .get("content-disposition")
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned);

        let data = response
            .bytes()
            .await
            .map_err(|e| anyhow!("failed to read response body: {}", e))?
            .to_vec();

        let size = data.len();
        let mut filename = content_disposition
            .as_deref()
            .and_then(filename_from_content_disposition)
            .or_else(|| filename_from_url(url))
            .unwrap_or_else(|| "attachment".to_string());
        let content_type = normalize_content_type(raw_content_type.as_deref(), &filename, &data);
        filename = ensure_filename_extension(&filename, &content_type);

        debug!("downloaded {} bytes from {}", size, url);

        Ok(MediaInfo {
            data,
            content_type,
            filename,
            size,
        })
    }

    pub async fn download_matrix_media(&self, mxc_url: &str) -> Result<MediaInfo> {
        if !mxc_url.starts_with("mxc://") {
            return Err(anyhow!("invalid mxc URL: {}", mxc_url));
        }

        let mxc_path = mxc_url.trim_start_matches("mxc://");
        let download_url = format!(
            "{}/_matrix/media/v3/download/{}",
            self.homeserver_url.trim_end_matches('/'),
            mxc_path
        );

        self.download_from_url(&download_url).await
    }

    pub async fn upload_to_matrix(&self, media: &MediaInfo, access_token: &str) -> Result<String> {
        if media.size > MAX_MATRIX_FILE_SIZE {
            return Err(anyhow!(
                "file too large for Matrix: {} bytes (max {})",
                media.size,
                MAX_MATRIX_FILE_SIZE
            ));
        }

        let upload_url = format!(
            "{}/_matrix/media/v3/upload?filename={}",
            self.homeserver_url.trim_end_matches('/'),
            urlencoding::encode(&media.filename)
        );

        debug!("uploading {} to Matrix", media.filename);

        let response = self
            .client
            .post(&upload_url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", &media.content_type)
            .body(media.data.clone())
            .send()
            .await
            .map_err(|e| anyhow!("failed to upload to Matrix: {}", e))?;

        let status = response.status();

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("failed to upload to Matrix: {} - {}", status, body));
        }

        let body_bytes = response
            .bytes()
            .await
            .map_err(|e| anyhow!("failed to read response body: {}", e))?;
        let json: serde_json::Value = serde_json::from_slice(&body_bytes)
            .map_err(|e| anyhow!("failed to parse upload response: {}", e))?;

        let content_uri = json
            .get("content_uri")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("no content_uri in upload response"))?
            .to_string();

        debug!("uploaded to Matrix: {}", content_uri);
        Ok(content_uri)
    }

    pub fn check_slack_file_size(size: usize) -> Result<()> {
        if size > MAX_SLACK_FILE_SIZE {
            warn!(
                "file too large for Slack: {} bytes (max {})",
                size, MAX_SLACK_FILE_SIZE
            );
            Err(anyhow!(
                "file too large for Slack: {} bytes (max {})",
                size,
                MAX_SLACK_FILE_SIZE
            ))
        } else {
            Ok(())
        }
    }
}

fn filename_from_content_disposition(value: &str) -> Option<String> {
    for part in value.split(';').map(str::trim) {
        if let Some(raw) = part.strip_prefix("filename*=") {
            let raw = trim_wrapping_quotes(raw.trim());
            let encoded = raw.rsplit("''").next().unwrap_or(raw);
            if let Some(name) = percent_decode(encoded)
                .as_deref()
                .and_then(sanitize_filename)
            {
                return Some(name);
            }
        }
    }

    for part in value.split(';').map(str::trim) {
        if let Some(raw) = part.strip_prefix("filename=")
            && let Some(name) = sanitize_filename(trim_wrapping_quotes(raw.trim()))
        {
            return Some(name);
        }
    }

    None
}

fn filename_from_url(url: &str) -> Option<String> {
    if let Ok(parsed) = reqwest::Url::parse(url)
        && let Some(segment) = parsed.path_segments().and_then(|mut s| s.next_back())
        && let Some(name) = sanitize_filename(segment)
    {
        return Some(name);
    }

    let without_query = url.split('?').next().unwrap_or(url);
    let tail = without_query.rsplit('/').next().unwrap_or(without_query);
    sanitize_filename(tail)
}

fn sanitize_filename(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let basename = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    let basename = basename.trim();
    if basename.is_empty() {
        return None;
    }

    let cleaned: String = basename.chars().filter(|c| !c.is_control()).collect();

    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn trim_wrapping_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
}

fn percent_decode(value: &str) -> Option<String> {
    let mut out = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &value[i + 1..i + 3];
            let parsed = u8::from_str_radix(hex, 16).ok()?;
            out.push(parsed);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }

    String::from_utf8(out).ok()
}

fn normalize_content_type(header_value: Option<&str>, filename: &str, data: &[u8]) -> String {
    let header_value = header_value
        .and_then(|v| v.split(';').next())
        .map(str::trim)
        .unwrap_or("application/octet-stream");

    if !header_value.is_empty() && header_value != "application/octet-stream" {
        return header_value.to_string();
    }

    guess_mime_from_filename(filename)
        .or_else(|| sniff_mime(data))
        .unwrap_or("application/octet-stream")
        .to_string()
}

fn ensure_filename_extension(filename: &str, content_type: &str) -> String {
    if Path::new(filename).extension().is_some() {
        return filename.to_string();
    }

    if let Some(ext) = extension_from_mime(content_type) {
        return format!("{}.{}", filename, ext);
    }

    filename.to_string()
}

fn guess_mime_from_filename(filename: &str) -> Option<&'static str> {
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())?;
    extension_to_mime(&ext)
}

fn extension_to_mime(ext: &str) -> Option<&'static str> {
    match ext {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "svg" => Some("image/svg+xml"),
        "mp4" => Some("video/mp4"),
        "mov" => Some("video/quicktime"),
        "mp3" => Some("audio/mpeg"),
        "ogg" => Some("audio/ogg"),
        "wav" => Some("audio/wav"),
        "pdf" => Some("application/pdf"),
        _ => None,
    }
}

fn extension_from_mime(content_type: &str) -> Option<&'static str> {
    match content_type {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/bmp" => Some("bmp"),
        "image/svg+xml" => Some("svg"),
        "video/mp4" => Some("mp4"),
        "video/quicktime" => Some("mov"),
        "audio/mpeg" => Some("mp3"),
        "audio/ogg" => Some("ogg"),
        "audio/wav" => Some("wav"),
        "application/pdf" => Some("pdf"),
        _ => None,
    }
}

fn sniff_mime(data: &[u8]) -> Option<&'static str> {
    if data.len() >= 8 && data[..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        return Some("image/png");
    }
    if data.len() >= 3 && data[..3] == [0xFF, 0xD8, 0xFF] {
        return Some("image/jpeg");
    }
    if data.len() >= 6 && (&data[..6] == b"GIF87a" || &data[..6] == b"GIF89a") {
        return Some("image/gif");
    }
    if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return Some("image/webp");
    }

    None
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ensure_filename_extension, filename_from_content_disposition, filename_from_url,
        normalize_content_type,
    };

    #[test]
    fn picks_filename_from_content_disposition_filename_star() {
        let header = "attachment; filename*=UTF-8''outfox-board.png";
        let filename = filename_from_content_disposition(header).unwrap();
        assert_eq!(filename, "outfox-board.png");
    }

    #[test]
    fn strips_query_from_url_filename() {
        let url = "https://cdn.slackapp.com/attachments/1/2/outfox...png?ex=abc&is=def";
        let filename = filename_from_url(url).unwrap();
        assert_eq!(filename, "outfox...png");
    }

    #[test]
    fn infers_png_type_and_extension_when_header_is_octet_stream() {
        let body = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0];
        let content_type =
            normalize_content_type(Some("application/octet-stream"), "attachment", &body);
        assert_eq!(content_type, "image/png");

        let filename = ensure_filename_extension("attachment", &content_type);
        assert_eq!(filename, "attachment.png");
    }
}

