//! What one photo costs to draw: the thumbnail pipeline (decode, fit to the preview) and the
//! full-size render of an edited photo (decode, orient/turn/crop, encode), stage by stage, on
//! a 24 MP camera-sized JPEG.
//!
//! The picture is noise over a gradient rather than a flat colour: a flat frame decodes and
//! encodes in a fraction of a real photo's time, because nearly every block is empty.

use criterion::{Criterion, criterion_group, criterion_main};
use image::{DynamicImage, RgbImage, imageops::FilterType};
use photon_core::{
    decode,
    edit::{self, Crop, Edit},
};
use std::{hint::black_box, path::Path, time::Duration};

const W: u32 = 6000;
const H: u32 = 4000;
const PREVIEW_EDGE: u32 = 1600;

/// A deterministic 24 MP photo stand-in: a gradient with per-pixel noise, saved as a q90
/// JPEG, which is about what a camera writes.
fn noisy_jpeg(path: &Path) {
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let img = RgbImage::from_fn(W, H, |x, y| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let n = (seed & 0x3F) as u32;
        image::Rgb([
            ((x * 200 / W) + n) as u8,
            ((y * 200 / H) + n) as u8,
            (((x + y) * 100 / (W + H)) + n) as u8,
        ])
    });
    let file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(file, 90);
    img.write_with_encoder(encoder).unwrap();
}

/// A quarter turn and a crop of the middle two thirds: the edit whose render does the most.
fn edited() -> Edit {
    let third = (edit::CROP_UNIT / 6) as u16;
    Edit::new(
        1,
        Some(Crop {
            left: third,
            top: third,
            right: edit::CROP_UNIT as u16 - third,
            bottom: edit::CROP_UNIT as u16 - third,
        }),
    )
    .unwrap()
}

fn bench_render(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("24mp.jpg");
    noisy_jpeg(&path);
    let decoded = decode::decode_image(&path).unwrap();

    // `image`'s resize and `thumbnail` are here as the references `fit_within` replaced.
    let mut g = c.benchmark_group("thumbnail_24mp");
    g.sample_size(10).measurement_time(Duration::from_secs(8));
    g.bench_function("decode", |b| {
        b.iter(|| black_box(decode::decode_image(&path).unwrap()))
    });
    g.bench_function("fit_image_triangle", |b| {
        b.iter(|| black_box(decoded.resize(PREVIEW_EDGE, PREVIEW_EDGE, FilterType::Triangle)))
    });
    g.bench_function("fit_image_thumbnail", |b| {
        b.iter(|| black_box(decoded.thumbnail(PREVIEW_EDGE, PREVIEW_EDGE)))
    });
    g.bench_function("fit_within", |b| {
        b.iter_batched(
            || decoded.clone(),
            |img| black_box(decode::fit_within(img, PREVIEW_EDGE)),
            criterion::BatchSize::PerIteration,
        )
    });
    g.bench_function("decode_oriented", |b| {
        b.iter(|| black_box(decode::decode_oriented(&path, 6, PREVIEW_EDGE).unwrap()))
    });
    // The two preview paths side by side, at orientation 1 so nothing but the decode and the
    // fit differs: zune's full decode then `fit_within`, and `decode_oriented`, which takes
    // libjpeg-turbo's scaled decode for this JPEG.
    g.bench_function("preview_zune", |b| {
        b.iter(|| {
            black_box(decode::fit_within(
                decode::decode_image(&path).unwrap(),
                PREVIEW_EDGE,
            ))
        })
    });
    g.bench_function("preview_turbo", |b| {
        b.iter(|| black_box(decode::decode_oriented(&path, 1, PREVIEW_EDGE).unwrap()))
    });
    g.finish();

    let edit = edited();
    let mut g = c.benchmark_group("render_full_24mp");
    g.sample_size(10).measurement_time(Duration::from_secs(10));
    g.bench_function("place", |b| {
        b.iter_batched(
            || decoded.clone(),
            |img| black_box(edit.place(img, 6)),
            criterion::BatchSize::PerIteration,
        )
    });
    let picture = edit::render_picture(&path, 6, edit).unwrap();
    let rgb = picture.to_rgb8();
    for (name, chroma) in [
        ("encode_jpeg_420", edit::Chroma::Half),
        ("encode_jpeg_444", edit::Chroma::Full),
    ] {
        g.bench_function(name, |b| {
            b.iter(|| {
                let mut bytes = Vec::new();
                edit::encode_jpeg(&rgb, edit::FULL_QUALITY, chroma, &mut bytes).unwrap();
                black_box(bytes)
            })
        });
    }
    // What `render_full` encoded with before: `image`'s own encoder, which writes 4:4:4.
    g.bench_function("encode_image_jpeg", |b| {
        b.iter(|| {
            let mut bytes = Vec::new();
            let encoder =
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, edit::FULL_QUALITY);
            picture.write_with_encoder(encoder).unwrap();
            black_box(bytes)
        })
    });
    g.bench_function("render_full", |b| {
        b.iter(|| {
            black_box(
                edit::render_full(&path, 6, edit, edit::FULL_QUALITY, edit::Chroma::Half).unwrap(),
            )
        })
    });
    g.finish();
    drop::<DynamicImage>(picture);
}

criterion_group!(benches, bench_render);
criterion_main!(benches);
