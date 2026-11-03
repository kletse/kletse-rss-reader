//! Pictures, fetched and encoded for the terminal off the main thread: for the article, and for the viewer. Kitty,
//! iTerm2 and sixel terminals (foot, Ghostty, WezTerm, …) show the real image; the others get half-block cells.

use crate::backend::Backend;
use base64::Engine as _;
use image::DynamicImage;
use percent_encoding::percent_decode_str;
use ratatui::layout::Size;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;
use ratatui_image::sliced::SlicedProtocol;
use ratatui_image::{FontSize, Resize};
use std::sync::Arc;

/// Decoded images are kept no bigger than this, wide or high: a full-screen viewer, and not 48 MB of phone photo.
const LARGEST: u32 = 2400;

/// An image as it came, still encoded. What the window keeps: decoded, a photo is 20 MB, and an article can have
/// forty; encoded it is what the server sent. It is decoded again, off the main thread, each time it is encoded for
/// the terminal.
pub type Bytes = Arc<Vec<u8>>;
/// Width and height in pixels, after the `LARGEST` cap.
pub type Dimensions = (u32, u32);

pub enum Job {
    /// An image for the article: how big it is decides how the article is laid out.
    Fetch { src: String },
    /// That image, encoded for the rows set aside for it.
    Slice { src: String, bytes: Bytes, size: Size },
    /// The viewer's picture, not seen yet.
    Load { src: String, size: Size },
    /// The viewer's picture, for a new size or one the article already has.
    Fit { src: String, bytes: Bytes, size: Size },
}

pub enum Answer {
    Fetched { src: String, result: Result<(Bytes, Dimensions), String> },
    Sliced { src: String, size: Size, result: Result<SlicedProtocol, String> },
    Viewer { src: String, size: Size, result: Result<(Bytes, Protocol), String> },
}

/// The cells a picture of `dimensions` takes when it is fitted into `available`, as `Resize::Fit` sizes it
/// (`Resize::size_for` wants the decoded image, which the window no longer keeps): shrunk to fit, never grown,
/// its shape kept, in whole cells.
pub fn fit(dimensions: Dimensions, font: FontSize, available: Size) -> Size {
    let (width, height) = dimensions;
    let room =
        (u32::from(available.width) * u32::from(font.width), u32::from(available.height) * u32::from(font.height));
    let ratio =
        f64::min(f64::from(room.0.min(width)) / f64::from(width), f64::from(room.1.min(height)) / f64::from(height));
    let scaled = |px: u32| (f64::from(px) * ratio).round().max(1.0);
    let cells = |px: f64, font: u16| u16::try_from((px / f64::from(font)).ceil() as u32).unwrap_or(u16::MAX);
    Size::new(cells(scaled(width), font.width), cells(scaled(height), font.height))
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

fn decode(bytes: &[u8]) -> Result<DynamicImage, String> {
    // SVG and AVIF end up here too.
    let image = image::load_from_memory(bytes).map_err(|_| "This image can't be shown here".to_string())?;
    let shrunk = image.width().max(image.height()) > LARGEST;
    Ok(if shrunk { image.resize(LARGEST, LARGEST, image::imageops::FilterType::Triangle) } else { image })
}

fn encode(picker: &Picker, image: &DynamicImage, size: Size) -> Result<Protocol, String> {
    picker.new_protocol(image.clone(), size, Resize::Fit(None)).map_err(|e| e.to_string())
}

/// Off the runtime's few workers: decoding and encoding take a while.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tokio::task::spawn_blocking(work).await.unwrap_or_else(|e| Err(e.to_string()))
}

/// The image at `src`, and its size: decoded once to make sure it can be, and to measure it.
async fn fetched(backend: &Backend, src: &str) -> Result<(Bytes, Dimensions), String> {
    let bytes = Arc::new(fetch(backend, src).await?);
    let measured = Arc::clone(&bytes);
    let dimensions = blocking(move || decode(&measured).map(|i| (i.width(), i.height()))).await?;
    Ok((bytes, dimensions))
}

/// The viewer's picture: decoded and encoded for `size`, with its bytes for the next size.
async fn fitted(bytes: Bytes, picker: Picker, size: Size) -> Result<(Bytes, Protocol), String> {
    let keep = Arc::clone(&bytes);
    blocking(move || Ok((keep, encode(&picker, &decode(&bytes)?, size)?))).await
}

pub async fn run(job: Job, backend: Arc<Backend>, picker: Picker) -> Answer {
    match job {
        Job::Fetch { src } => {
            let result = fetched(&backend, &src).await;
            Answer::Fetched { src, result }
        }
        Job::Slice { src, bytes, size } => {
            let work = move || SlicedProtocol::new(&picker, decode(&bytes)?, Some(size)).map_err(|e| e.to_string());
            Answer::Sliced { src, size, result: blocking(work).await }
        }
        Job::Load { src, size } => {
            let result = match fetched(&backend, &src).await {
                Ok((bytes, _)) => fitted(bytes, picker, size).await,
                Err(message) => Err(message),
            };
            Answer::Viewer { src, size, result }
        }
        Job::Fit { src, bytes, size } => Answer::Viewer { src, size, result: fitted(bytes, picker, size).await },
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// `fit` must give the rows a picture is encoded for, or it gets rows it doesn't fill.
    #[test]
    fn fit_sizes_a_picture_as_ratatui_image_does() {
        let font = FontSize { width: 10, height: 20 };
        for (w, h) in [(200, 200), (1, 1), (2400, 300), (33, 1999), (7, 7), (1000, 10)] {
            let image = DynamicImage::new_rgb8(w, h);
            for area in [Size::new(80, 24), Size::new(1, 1), Size::new(300, 5), Size::new(20, 200)] {
                let theirs = Resize::Fit(None).size_for(&image, font, area);
                assert_eq!(fit((w, h), font, area), theirs, "{w}×{h} in {area}");
            }
        }
    }

    #[test]
    fn data_uris_are_read_as_written() {
        assert_eq!(decode_data_uri("data:image/gif;base64,AQID"), Some(vec![1, 2, 3]));
        assert_eq!(decode_data_uri("data:image/svg+xml;utf8,%3Csvg%3E"), Some(b"<svg>".to_vec()));
        assert_eq!(decode_data_uri("data:nothing"), None);
    }
}
