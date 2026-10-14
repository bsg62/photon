//! The system's video backends (`photon_ui::video`), over three small files: `h264.mp4` and
//! `hevc.mp4`, 320 by 240, four seconds of red, green, blue and white, a second each, with
//! sound - so a frame's colour says which second it came from; and `rotated.mp4`, stored
//! 320 by 240 with its left half red and its right half blue, and a matrix that turns it a
//! quarter anticlockwise as it is shown: blue over red, 240 by 320.
//!
//! `odd.mp4` is `h264.mp4`'s colours without sound at 320 by 184, which a decoder pads to
//! 192.
//!
//! A binary of its own, with no test harness of libtest's: every test runs on the main
//! thread, which AVFoundation's player wants (`libtest-mimic`, one thread). A system that
//! cannot decode what a test needs - a Linux without GStreamer's plugins, a Windows without
//! Microsoft's HEVC extension - is said so in the output, and the test passes over itself.
//! A file the system calls broken is a failure. **Where `PHOTON_VIDEO_EXPECTS` names the
//! codec, passing over is a failure too** (CI's runners set it): nextest shows nothing of a
//! test that passed, and a runner whose decoders went missing would pass every test here.

use libtest_mimic::{Arguments, Failed, Trial};
use photon_ui::video::{self, Failure, Picture, Player};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn main() {
    let mut args = Arguments::from_args();
    args.test_threads = Some(1);
    let tests = vec![
        Trial::test("h264_posters_show_the_second_asked_for", || {
            posters("h264.mp4")
        }),
        Trial::test(
            "hevc_posters_show_the_second_asked_for_where_hevc_is_decoded",
            || posters("hevc.mp4"),
        ),
        Trial::test(
            "a_video_whose_height_is_no_multiple_of_sixteen_is_its_own_size",
            odd,
        ),
        Trial::test("a_turned_video_is_turned_as_it_is_seen", turned),
        Trial::test("a_video_plays_its_seconds_in_order", in_order),
        Trial::test("a_paused_video_stands_still", paused),
        Trial::test("a_seek_lands_on_the_second_asked_for", seek),
        Trial::test(
            "a_seek_while_paused_shows_the_frame_it_landed_on",
            paused_seek,
        ),
        Trial::test(
            "an_hevc_video_plays_or_the_system_says_it_cannot",
            hevc_player,
        ),
        Trial::test("a_video_knows_its_length_and_its_end", length_and_end),
        Trial::test("volume_and_mute_are_set_without_a_failure", sound),
        Trial::test("a_file_that_is_no_video_is_a_failure_not_a_panic", no_video),
        #[cfg(target_os = "linux")]
        Trial::test(
            "a_video_whose_picture_has_no_decoder_is_unsupported_though_its_sound_has",
            linux::no_picture_decoder,
        ),
    ];
    libtest_mimic::run(&args, tests).exit();
}

fn media(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/video")
        .join(name)
}

/// What the system cannot do with `codec`, said and passed over - unless this machine is
/// one that is expected to.
fn cannot(codec: &str, why: impl std::fmt::Display) -> Result<(), Failed> {
    let expects = std::env::var("PHOTON_VIDEO_EXPECTS").unwrap_or_default();
    if expects.split([' ', ',']).any(|expected| expected == codec) {
        return Err(format!("{codec} is expected to play here: {why}").into());
    }
    println!("passed over: {why}");
    Ok(())
}

/// Time passing while a test waits on a player. On a Mac the main thread's run loop is
/// turned through it, as winit turns it in the window: AVFoundation tells a player's state
/// there, and nothing else turns it in a test.
fn idle(long: Duration) {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSDate, NSRunLoop};
        NSRunLoop::currentRunLoop()
            .runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(long.as_secs_f64()));
    }
    #[cfg(not(target_os = "macos"))]
    std::thread::sleep(long);
}

/// The colour of a pixel, named by which channels are lit.
fn named(px: [u8; 4]) -> &'static str {
    match [px[0], px[1], px[2]].map(|c| c > 128) {
        [true, false, false] => "red",
        [false, true, false] => "green",
        [false, false, true] => "blue",
        [true, true, true] => "white",
        _ => "other",
    }
}

fn centre(picture: &Picture) -> &'static str {
    named(picture.pixel(picture.width / 2, picture.height / 2))
}

/// What the coloured files show at `at` seconds.
fn expected(at: f64) -> &'static str {
    ["red", "green", "blue", "white"][(at.max(0.0) as usize).min(3)]
}

/// A frame shown at `at` with `colour` is the second it is from - or, within a tenth of a
/// second after a second's edge, the second before: the frame's time is the player's own,
/// and GStreamer's runs two frames behind the picture in a file with an edit list.
fn in_its_second(at: f64, colour: &str) -> bool {
    colour == expected(at) || (at.fract() < 0.1 && at >= 1.0 && colour == expected(at - 1.0))
}

fn check(ok: bool, said: impl FnOnce() -> String) -> Result<(), Failed> {
    if ok { Ok(()) } else { Err(said().into()) }
}

fn posters(file: &str) -> Result<(), Failed> {
    if !video::can_play() {
        return cannot("h264", "the system has no H.264 decoder");
    }
    for at in [0.4, 2.5] {
        match video::poster(&media(file), at) {
            Ok(poster) => check(
                (poster.width, poster.height) == (320, 240) && centre(&poster) == expected(at),
                || format!("{file} at {at} s: {poster:?}, {}", centre(&poster)),
            )?,
            Err(Failure::Unsupported(why)) if file == "hevc.mp4" => return cannot("hevc", why),
            Err(failure) => return Err(format!("{file} at {at} s: {failure:?}").into()),
        }
    }
    Ok(())
}

fn opened(file: &str) -> Result<Option<Player>, Failed> {
    if !video::can_play() {
        cannot("h264", "the system has no H.264 decoder")?;
        return Ok(None);
    }
    Player::open(&media(file))
        .map(Some)
        .map_err(|failure| format!("{file}: {failure:?}").into())
}

/// How long a player is given to hand over its first frame: the system's own time to begin,
/// which on macOS' runner was more than half a second once.
const BEGUN: Duration = Duration::from_secs(5);

/// The frames a player hands over for `long`, each asked for every five milliseconds.
fn frames_for(player: &Player, long: Duration) -> Vec<Picture> {
    let until = Instant::now() + long;
    let mut frames = Vec::new();
    while Instant::now() < until {
        frames.extend(player.frame());
        idle(Duration::from_millis(5));
    }
    frames
}

/// The first frame handed over at or after `at` seconds, within `long`.
fn frame_from(player: &Player, at: f64, long: Duration) -> Option<Picture> {
    let until = Instant::now() + long;
    while Instant::now() < until {
        if let Some(frame) = player.frame().filter(|frame| frame.at >= at) {
            return Some(frame);
        }
        idle(Duration::from_millis(5));
    }
    None
}

fn ends(picture: &Picture) -> (&'static str, &'static str) {
    let x = picture.width / 2;
    (
        named(picture.pixel(x, picture.height / 4)),
        named(picture.pixel(x, picture.height * 3 / 4)),
    )
}

/// The pixel colours down the middle of a picture, its last row's among them: the rows a
/// decoder pads a picture with are black or green, never the picture's.
fn bottom(picture: &Picture) -> &'static str {
    named(picture.pixel(picture.width / 2, picture.height - 1))
}

fn odd() -> Result<(), Failed> {
    if !video::can_play() {
        return cannot("h264", "the system has no H.264 decoder");
    }
    let poster =
        video::poster(&media("odd.mp4"), 0.4).map_err(|failure| format!("poster: {failure:?}"))?;
    check(
        (poster.width, poster.height) == (320, 184) && bottom(&poster) == "red",
        || format!("poster {poster:?}, its last row {}", bottom(&poster)),
    )?;
    let Some(player) = opened("odd.mp4")? else {
        return Ok(());
    };
    player.play();
    let frame = frame_from(&player, 0.0, BEGUN).ok_or("no frame played")?;
    check(
        (frame.width, frame.height) == (320, 184) && bottom(&frame) == "red",
        || format!("played {frame:?}, its last row {}", bottom(&frame)),
    )
}

fn turned() -> Result<(), Failed> {
    if !video::can_play() {
        return cannot("h264", "the system has no H.264 decoder");
    }
    let poster = video::poster(&media("rotated.mp4"), 1.0)
        .map_err(|failure| format!("poster: {failure:?}"))?;
    check(
        (poster.width, poster.height) == (240, 320) && ends(&poster) == ("blue", "red"),
        || format!("poster {poster:?}, top and bottom {:?}", ends(&poster)),
    )?;
    let Some(player) = opened("rotated.mp4")? else {
        return Ok(());
    };
    player.set_muted(true);
    player.play();
    let first = frame_from(&player, 0.0, BEGUN).ok_or("no frame played")?;
    let frames = frames_for(&player, Duration::from_millis(300));
    let last = frames.last().unwrap_or(&first);
    check(
        (last.width, last.height) == (240, 320) && ends(last) == ("blue", "red"),
        || format!("played {last:?}, top and bottom {:?}", ends(last)),
    )
}

fn in_order() -> Result<(), Failed> {
    let Some(player) = opened("h264.mp4")? else {
        return Ok(());
    };
    player.set_muted(true);
    player.play();
    let first = frame_from(&player, 0.0, BEGUN).ok_or("no frame played")?;
    let mut frames = vec![first];
    frames.extend(frames_for(&player, Duration::from_millis(2500)));
    let seen: Vec<_> = frames
        .iter()
        .map(|frame| (frame.at, centre(frame)))
        .collect();
    let wrong: Vec<_> = seen
        .iter()
        .filter(|(at, colour)| !in_its_second(*at, colour))
        .collect();
    let first = seen.first().copied();
    check(
        frames.len() >= 20
            && wrong.is_empty()
            && first.is_some_and(|(at, colour)| at >= 0.0 && colour == "red")
            && seen.iter().any(|(_, colour)| *colour == "green")
            && seen.windows(2).all(|pair| pair[0].0 < pair[1].0)
            && frames
                .iter()
                .all(|frame| (frame.width, frame.height) == (320, 240)),
        || format!("{} frames, first {first:?}, wrong {wrong:?}", frames.len()),
    )
}

fn paused() -> Result<(), Failed> {
    let Some(player) = opened("h264.mp4")? else {
        return Ok(());
    };
    player.set_muted(true);
    player.play();
    // Until it is well under way, not for a time: see `BEGUN`.
    let until = Instant::now() + BEGUN;
    let playing = loop {
        let at = player.position().unwrap_or(0.0);
        if at > 0.3 {
            break at;
        }
        if Instant::now() > until {
            return Err(format!("never under way, at {at}").into());
        }
        frames_for(&player, Duration::from_millis(20));
    };
    player.pause();
    frames_for(&player, Duration::from_millis(100));
    let stopped = player.position().ok_or("no position once paused")?;
    frames_for(&player, Duration::from_millis(400));
    let later = player.position().ok_or("no position later")?;
    check(stopped >= playing && (later - stopped).abs() < 0.05, || {
        format!("playing at {playing}, paused at {stopped}, then {later}")
    })
}

fn seek() -> Result<(), Failed> {
    let Some(player) = opened("h264.mp4")? else {
        return Ok(());
    };
    player.set_muted(true);
    player.play();
    player.seek(3.5);
    let frame = frame_from(&player, 3.4, BEGUN).ok_or("no frame after 3.4 s")?;
    check(centre(&frame) == "white", || {
        format!("{frame:?} is {}", centre(&frame))
    })
}

/// Scrubbing a paused video, and Shift with an arrow on one: the picture is the frame the
/// seek landed on, though nothing plays.
fn paused_seek() -> Result<(), Failed> {
    let Some(player) = opened("h264.mp4")? else {
        return Ok(());
    };
    player.seek(2.5);
    let frame = frame_from(&player, 2.4, BEGUN).ok_or("no frame after 2.4 s")?;
    check(centre(&frame) == "blue", || {
        format!("{frame:?} is {}", centre(&frame))
    })
}

fn hevc_player() -> Result<(), Failed> {
    if !video::can_play() {
        return cannot("h264", "the system has no H.264 decoder");
    }
    let player = match Player::open(&media("hevc.mp4")) {
        Ok(player) => player,
        Err(Failure::Unsupported(why)) => return cannot("hevc", why),
        Err(failure) => return Err(format!("{failure:?}").into()),
    };
    player.set_muted(true);
    player.play();
    match frame_from(&player, 0.0, BEGUN) {
        Some(frame) => check(centre(&frame) == "red", || {
            format!("{frame:?} is {}", centre(&frame))
        }),
        None => match player.failure() {
            Some(Failure::Unsupported(why)) => cannot("hevc", why),
            failure => Err(format!("no frame, and {failure:?}").into()),
        },
    }
}

fn length_and_end() -> Result<(), Failed> {
    let Some(player) = opened("h264.mp4")? else {
        return Ok(());
    };
    let whole = player.duration().ok_or("no duration")?;
    check((whole - 4.0).abs() < 0.1, || format!("{whole} s long"))?;
    check(!player.ended(), || "ended before it played".into())?;
    player.set_muted(true);
    player.play();
    player.seek(3.7);
    let until = Instant::now() + Duration::from_secs(3);
    while !player.ended() {
        if Instant::now() > until {
            return Err(format!("not ended, at {:?}", player.position()).into());
        }
        player.frame();
        idle(Duration::from_millis(10));
    }
    check(player.failure().is_none(), || {
        format!("ended with {:?}", player.failure())
    })?;
    // Played to its end and taken back: it has not ended any more, nor does the end it
    // had reached come back to say it has (a loop goes round this way).
    player.seek(1.0);
    check(!player.ended(), || "still ended after a seek back".into())?;
    frames_for(&player, Duration::from_millis(300));
    check(!player.ended(), || {
        format!("ended again at {:?}", player.position())
    })
}

fn sound() -> Result<(), Failed> {
    let Some(player) = opened("h264.mp4")? else {
        return Ok(());
    };
    player.set_volume(0.3);
    player.set_muted(true);
    player.play();
    let frames: Vec<_> = frame_from(&player, 0.0, BEGUN).into_iter().collect();
    player.set_muted(false);
    player.set_volume(0.0);
    frames_for(&player, Duration::from_millis(100));
    check(!frames.is_empty() && player.failure().is_none(), || {
        format!("{} frames, {:?}", frames.len(), player.failure())
    })
}

fn no_video() -> Result<(), Failed> {
    let dir = tempfile::tempdir()?;
    let fake = dir.path().join("fake.mp4");
    std::fs::write(&fake, b"this is no video, whatever its name says")?;
    let missing = dir.path().join("missing.mp4");
    for path in [&fake, &missing] {
        check(video::poster(path, 0.0).is_err(), || {
            format!("a poster of {}", path.display())
        })?;
        check(Player::open(path).is_err(), || {
            format!("a player of {}", path.display())
        })?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use gstreamer::{self as gst, prelude::*};

    /// The decoders of HEVC taken out of the registry's choice for as long as this lives,
    /// and put back as they were - also when the test fails, as it runs in the same process
    /// as the others under `cargo test`.
    struct Hidden(Vec<(gst::ElementFactory, gst::Rank)>);

    impl Drop for Hidden {
        fn drop(&mut self) {
            for (factory, rank) in &self.0 {
                factory.set_rank(*rank);
            }
        }
    }

    /// A Linux with no HEVC decoder - Fedora's own packages - and an iPhone's video, whose
    /// sound is AAC, which it does decode.
    pub fn no_picture_decoder() -> Result<(), Failed> {
        if !video::can_play() {
            return cannot("h264", "the system has no H.264 decoder");
        }
        let hevc = gst::Caps::builder("video/x-h265").build();
        let decoders: Vec<_> = gst::ElementFactory::factories_with_type(
            gst::ElementFactoryType::DECODER,
            gst::Rank::NONE,
        )
        .into_iter()
        .filter(|factory| factory.rank() > gst::Rank::NONE && factory.can_sink_any_caps(&hevc))
        .collect();
        if decoders.is_empty() {
            return cannot("hevc", "the system has no HEVC decoder to take away");
        }
        let _hidden = Hidden(
            decoders
                .into_iter()
                .map(|factory| {
                    let rank = factory.rank();
                    factory.set_rank(gst::Rank::NONE);
                    (factory, rank)
                })
                .collect(),
        );
        let poster = video::poster(&media("hevc.mp4"), 0.4);
        check(matches!(poster, Err(Failure::Unsupported(_))), || {
            format!("a poster: {poster:?}")
        })?;
        let player = Player::open(&media("hevc.mp4")).map(|_| ());
        check(matches!(player, Err(Failure::Unsupported(_))), || {
            format!("a player: {player:?}")
        })
    }
}
