//! Pictures, fetched and encoded for the terminal off the main thread: for the article, and for the viewer. Kitty,
//! iTerm2 and sixel terminals (foot, Ghostty, WezTerm, …) show the real image; the others get half-block cells.

use crate::backend::Backend;
use base64::Engine as _;
use image::DynamicImage;
use percent_encoding::percent_decode_str;
use ratatui::layout::Size;
use ratatui_image::Resize;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;
use ratatui_image::sliced::SlicedProtocol;
use std::sync::Arc;

/// Decoded images are kept no bigger than this, wide or high: a full-screen viewer, and not 48 MB of phone photo.
const LARGEST: u32 = 2400;

pub enum Job {
    /// An image for the article: how big it is decides how the article is laid out.
    Fetch { src: String },
    /// That image, encoded for the rows set aside for it.
    Slice { src: String, image: Arc<DynamicImage>, size: Size },
    /// The viewer's picture, not seen yet.
    Load { src: String, size: Size },
    /// The viewer's picture, for a new size or one the article already has.
    Fit { src: String, image: Arc<DynamicImage>, size: Size },
}

pub enum Answer {
    Fetched { src: String, result: Result<Arc<DynamicImage>, String> },
    Sliced { src: String, size: Size, result: Result<SlicedProtocol, String> },
    Viewer { src: String, size: Size, result: Result<(Arc<DynamicImage>, Protocol), String> },
}

fn decode_data_uri(src: &str) -> Option<Vec<u8>> {
    let (header, body) = src.strip_prefix("data:")?.split_once(',')?;
    if header.ends_with(";base64") {
        base64::engine::general_purpose::STANDARD.decode(body.trim()).ok()
    } else {
        Some(percent_decode_str(body).collect())
    }
}

async fn fetch(backend: &Backend, src: &str) -> Result<Vec<u8>, String> {
    if src.starts_with("data:") {
        return decode_data_uri(src).ok_or_else(|| "The image didn't load".to_string());
    }
    backend.fetch_image(src).await.map_err(|e| e.message())
}

fn decode(bytes: &[u8]) -> Result<Arc<DynamicImage>, String> {
    // SVG and AVIF end up here too.
    let image = image::load_from_memory(bytes).map_err(|_| "This image can't be shown here".to_string())?;
    let shrunk = image.width().max(image.height()) > LARGEST;
    Ok(Arc::new(if shrunk { image.resize(LARGEST, LARGEST, image::imageops::FilterType::Triangle) } else { image }))
}

fn encode(picker: &Picker, image: &DynamicImage, size: Size) -> Result<Protocol, String> {
    picker.new_protocol(image.clone(), size, Resize::Fit(None)).map_err(|e| e.to_string())
}

/// Off the runtime's few workers: decoding and encoding take a while.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tokio::task::spawn_blocking(work).await.unwrap_or_else(|e| Err(e.to_string()))
}

async fn fetched(backend: &Backend, src: &str) -> Result<Arc<DynamicImage>, String> {
    let bytes = fetch(backend, src).await?;
    blocking(move || decode(&bytes)).await
}

pub async fn run(job: Job, backend: Arc<Backend>, picker: Picker) -> Answer {
    match job {
        Job::Fetch { src } => {
            let result = fetched(&backend, &src).await;
            Answer::Fetched { src, result }
        }
        Job::Slice { src, image, size } => {
            let work =
                move || SlicedProtocol::new(&picker, image.as_ref().clone(), Some(size)).map_err(|e| e.to_string());
            Answer::Sliced { src, size, result: blocking(work).await }
        }
        Job::Load { src, size } => {
            let result = match fetched(&backend, &src).await {
                Ok(image) => {
                    let keep = Arc::clone(&image);
                    blocking(move || Ok((keep, encode(&picker, &image, size)?))).await
                }
                Err(message) => Err(message),
            };
            Answer::Viewer { src, size, result }
        }
        Job::Fit { src, image, size } => {
            let keep = Arc::clone(&image);
            let result = blocking(move || Ok((keep, encode(&picker, &image, size)?))).await;
            Answer::Viewer { src, size, result }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn data_uris_are_read_as_written() {
        assert_eq!(decode_data_uri("data:image/gif;base64,AQID"), Some(vec![1, 2, 3]));
        assert_eq!(decode_data_uri("data:image/svg+xml;utf8,%3Csvg%3E"), Some(b"<svg>".to_vec()));
        assert_eq!(decode_data_uri("data:nothing"), None);
    }
}
